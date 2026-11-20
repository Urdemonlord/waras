//! API publik eksternal, read-only (FASE4.md §5.2).
//!
//! Router terpisah `public_router()` — TANPA auth staf. Tiap request
//! wajib header `X-Api-Key: wrs_...`. Verifikasi: SHA-256(kunci yang
//! ditunjukkan) == `key_hash` tersimpan, `revoked = 0`, scope cocok.
//! Gagal -> 401 (kunci salah / dicabut / tidak ada) atau 403 (scope
//! tidak cukup). `last_used_at` diperbarui tiap panggilan sukses
//! (best-effort). Rate limit di memori per kunci: 100 request/menit,
//! lewat -> 429 (hilang saat restart — cukup untuk demo, FASE4.md §8.5).
//!
//! NIK tidak pernah dikembalikan lengkap di respons publik — selalu
//! disensor tengah (`portal_handlers::mask_nik`). Endpoint FHIR memakai
//! `fhir::patient_resource` yang sudah ada, lalu nilai identifier NIK
//! diganti dengan versi tersensor (helper itu sendiri menulis NIK
//! lengkap, jadi tanpa langkah ini endpoint akan membocorkan NIK).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use waras_core::fhir;
use waras_db::clinic_today;

use crate::{
    api_key_handlers::sha256_hex_str,
    error::ApiError,
    portal_handlers::{mask_nik, patient_masked_json},
    AppState,
};

/// Batas rate limit: 100 request per 60 detik per kunci.
pub const RATE_LIMIT_PER_MINUTE: u32 = 100;
const RATE_WINDOW: Duration = Duration::from_secs(60);

/// State rate limit di memori: kunci API -> (awal jendela, hitungan).
/// Struktur persis seperti kontrak (`Mutex<HashMap<String,
/// (Instant, u32)>>`); dibungkus `Arc` supaya bisa dibagi lewat
/// `Extension` ke semua handler router publik.
pub type RateLimiter = Arc<Mutex<HashMap<String, (Instant, u32)>>>;

/// Cek + hitung satu request untuk `key` pada `now`. `true` = masih
/// dalam batas (hitungan ditambah), `false` = lewat batas (429).
/// Jendela 60 detik dihitung dari request pertama di jendela itu.
pub fn rate_limit_allowed(
    map: &mut HashMap<String, (Instant, u32)>,
    key: &str,
    now: Instant,
) -> bool {
    match map.get_mut(key) {
        Some((start, count)) => {
            if now.duration_since(*start) >= RATE_WINDOW {
                *start = now;
                *count = 1;
                true
            } else if *count < RATE_LIMIT_PER_MINUTE {
                *count += 1;
                true
            } else {
                false
            }
        }
        None => {
            map.insert(key.to_string(), (now, 1));
            true
        }
    }
}

fn err_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn api_err_response(e: ApiError) -> Response {
    e.into_response()
}

/// Verifikasi `X-Api-Key` + scope, lalu rate limit. Mengembalikan
/// record kunci bila lolos; bila tidak, respons error siap kirim
/// (401 / 403 / 429). Urutan: kunci -> dicabut -> scope -> rate limit
/// (rate limit dihitung hanya untuk kunci valid + scope cukup).
async fn authenticate(
    state: &AppState,
    limiter: &RateLimiter,
    headers: &HeaderMap,
    scope: &str,
) -> Result<waras_core::types::ApiKey, Response> {
    let presented = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            err_response(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "header X-Api-Key wajib diisi",
            )
        })?;
    let key = state
        .db
        .find_api_key_by_hash(&sha256_hex_str(presented))
        .await
        .map_err(db_err)?;
    let key = match key {
        Some(k) if !k.revoked => k,
        _ => {
            return Err(err_response(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "kunci API tidak valid",
            ));
        }
    };
    let scopes: Vec<&str> = key
        .scopes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if !scopes.contains(&scope) {
        return Err(err_response(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            &format!("scope {scope} tidak dimiliki kunci ini"),
        ));
    }
    let allowed = {
        let mut map = limiter.lock().unwrap_or_else(|e| e.into_inner());
        rate_limit_allowed(&mut map, &key.id, Instant::now())
    };
    if !allowed {
        return Err(err_response(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "batas 100 request per menit terlampaui",
        ));
    }
    // Best-effort: jangan gagalkan request bila update gagal.
    let _ = state.db.touch_api_key(&key.id).await;
    Ok(key)
}

/// Hasil handler publik: sukses -> JSON, gagal -> respons error yang
/// sudah berbentuk `{ "error": { "code", "message" } }`.
type PubResult = Result<Json<Value>, Response>;

fn db_err(e: impl Into<ApiError>) -> Response {
    api_err_response(e.into())
}

pub async fn list_public_patients(
    State(state): State<AppState>,
    Extension(limiter): Extension<RateLimiter>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> PubResult {
    authenticate(&state, &limiter, &headers, "patients:read").await?;
    let patients = state
        .db
        .list_patients(q.get("search").map(String::as_str))
        .await
        .map_err(db_err)?;
    let mut out = Vec::with_capacity(patients.len());
    for p in &patients {
        out.push(patient_masked_json(p).map_err(db_err)?);
    }
    Ok(Json(json!(out)))
}

pub async fn get_public_patient(
    State(state): State<AppState>,
    Extension(limiter): Extension<RateLimiter>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PubResult {
    authenticate(&state, &limiter, &headers, "patients:read").await?;
    let patient = state
        .db
        .get_patient(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            api_err_response(ApiError::not_found(format!(
                "pasien {id} tidak ditemukan"
            )))
        })?;
    let mut v = patient_masked_json(&patient).map_err(db_err)?;
    v["allergies"] =
        serde_json::to_value(state.db.get_allergies(&id).await.map_err(db_err)?)
            .map_err(db_err)?;
    Ok(Json(v))
}

pub async fn list_public_queue(
    State(state): State<AppState>,
    Extension(limiter): Extension<RateLimiter>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> PubResult {
    authenticate(&state, &limiter, &headers, "queue:read").await?;
    let date = match q.get("date") {
        Some(s) if !s.trim().is_empty() => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| {
                api_err_response(ApiError::validation(format!(
                    "tanggal tidak valid: {s}"
                )))
            })?,
        _ => clinic_today(),
    };
    let queue = state.db.list_queue(date).await.map_err(db_err)?;
    Ok(Json(json!(queue)))
}

pub async fn list_public_encounters(
    State(state): State<AppState>,
    Extension(limiter): Extension<RateLimiter>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> PubResult {
    authenticate(&state, &limiter, &headers, "encounters:read").await?;
    let patient_id = q
        .get("patient_id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            api_err_response(ApiError::validation("patient_id wajib diisi"))
        })?;
    let encounters = state
        .db
        .list_encounters(&patient_id)
        .await
        .map_err(db_err)?;
    Ok(Json(json!(encounters)))
}

pub async fn get_public_fhir_patient(
    State(state): State<AppState>,
    Extension(limiter): Extension<RateLimiter>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PubResult {
    authenticate(&state, &limiter, &headers, "fhir:read").await?;
    let patient = state
        .db
        .get_patient(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            api_err_response(ApiError::not_found(format!(
                "pasien {id} tidak ditemukan"
            )))
        })?;
    let mut resource = fhir::patient_resource(&patient);
    // `patient_resource` menulis NIK lengkap di identifier[0]; ganti
    // dengan versi tersensor sebelum dikirim ke pemanggil eksternal.
    if let Some(identifiers) = resource
        .get_mut("identifier")
        .and_then(|v| v.as_array_mut())
        .and_then(|a| a.first_mut())
    {
        identifiers["value"] = json!(mask_nik(&patient.nik));
    }
    Ok(Json(resource))
}

/// Router publik (nama + signature persis seperti kontrak; agen B
/// me-merge-nya di `lib.rs`: `.merge(public_api::public_router())`).
/// Rate limiter dibuat sekali di sini dan dibagi ke semua handler
/// lewat `Extension`; router dibangun sekali saat startup, jadi satu
/// instance limiter dipakai semua request.
pub fn public_router() -> Router<AppState> {
    let limiter: RateLimiter = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        .route("/public/v1/patients", get(list_public_patients))
        .route("/public/v1/patients/{id}", get(get_public_patient))
        .route("/public/v1/queue", get(list_public_queue))
        .route("/public/v1/encounters", get(list_public_encounters))
        .route(
            "/public/v1/fhir/Patient/{id}",
            get(get_public_fhir_patient),
        )
        .layer(Extension(limiter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;

    use axum::{body::Body, http::Request};
    use serde_json::Value;
    use tower::ServiceExt;
    use waras_bridge::{build_registry, BpjsConfig, BridgeConfig, SatusehatConfig};
    use waras_db::Db;

    use crate::api_key_handlers::{generate_api_key, key_prefix};
    #[test]
    fn rate_limit_100_lolos_101_ditolak_lalu_reset_setelah_60_detik() {
        let mut map: HashMap<String, (Instant, u32)> = HashMap::new();
        let now = Instant::now();
        for i in 1..=100 {
            assert!(
                rate_limit_allowed(&mut map, "AK-000001", now),
                "request ke-{i} harus lolos"
            );
        }
        assert!(
            !rate_limit_allowed(&mut map, "AK-000001", now),
            "request ke-101 harus 429"
        );
        // Kunci lain tidak kena batas kunci ini.
        assert!(rate_limit_allowed(&mut map, "AK-000002", now));
        // Setelah 60 detik jendela baru: hitungan mulai lagi dari 1.
        let later = now + StdDuration::from_secs(61);
        assert!(rate_limit_allowed(&mut map, "AK-000001", later));
        for _ in 0..99 {
            assert!(rate_limit_allowed(&mut map, "AK-000001", later));
        }
        assert!(!rate_limit_allowed(&mut map, "AK-000001", later));
    }

    fn disabled_config() -> BridgeConfig {
        BridgeConfig {
            satusehat: SatusehatConfig {
                enabled: false,
                base_url: "http://127.0.0.1:9/fhir-r4/v1".into(),
                auth_url: "http://127.0.0.1:9/oauth2/v1".into(),
                client_id: String::new(),
                client_secret: String::new(),
                org_id: String::new(),
            },
            bpjs: BpjsConfig {
                enabled: false,
                cons_id: String::new(),
                secret_key: String::new(),
                user_key: String::new(),
                vclaim_url: "http://127.0.0.1:9/vclaim-rest/".into(),
                pcare_url: "http://127.0.0.1:9/pcare-rest-v3.0/".into(),
                antrean_url: "http://127.0.0.1:9/antrean-rs/".into(),
                aplicares_url: "http://127.0.0.1:9/aplicaresws/".into(),
                kode_ppk: String::new(),
                eklaim_url: "http://127.0.0.1:9/eklaim/".into(),
            },
            worker_enabled: false,
            worker_interval_secs: 10,
            worker_batch: 10,
            outbox_max_attempts: 5,
        }
    }

    /// Router publik + satu kunci `patients:read,queue:read` dan satu
    /// kunci `patients:read` saja. Mengembalikan (app, db, kunci penuh,
    /// kunci sempit, id kunci penuh).
    async fn public_app() -> (Router, Arc<Db>, String, String, String) {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
        let key = generate_api_key();
        let record = db
            .create_api_key(
                "Tes Publik",
                key_prefix(&key),
                &sha256_hex_str(&key),
                "patients:read,queue:read,encounters:read,fhir:read",
                "Admin Sistem",
            )
            .await
            .expect("buat kunci");
        let narrow = generate_api_key();
        db.create_api_key(
            "Tes Sempit",
            key_prefix(&narrow),
            &sha256_hex_str(&narrow),
            "patients:read",
            "Admin Sistem",
        )
        .await
        .expect("buat kunci sempit");
        let db = Arc::new(db);
        let app = public_router().with_state(AppState {
            db: db.clone(),
            bridges: Arc::new(build_registry(&disabled_config())),
            bridge_config: disabled_config(),
        });
        (app, db, key, narrow, record.id)
    }

    async fn call(
        app: Router,
        req: Request<Body>,
    ) -> (axum::http::StatusCode, Value, String) {
        let resp = app.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("baca body");
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let value: Value =
            serde_json::from_str(&raw).unwrap_or(Value::String(raw.clone()));
        (status, value, raw)
    }

    fn pub_req(uri: &str, key: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().method("GET").uri(uri);
        if let Some(k) = key {
            b = b.header("x-api-key", k);
        }
        b.body(Body::empty()).expect("request")
    }

    /// Auth + scope + sensor NIK + FHIR tersensor, end-to-end.
    #[tokio::test]
    async fn public_auth_scope_dan_nik_tersensor() {
        let (app, _db, key, narrow, _id) = public_app().await;
        // Tanpa header -> 401; kunci salah -> 401.
        let (status, _, _) = call(app.clone(), pub_req("/public/v1/patients", None)).await;
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
        let (status, _, _) = call(
            app.clone(),
            pub_req("/public/v1/patients", Some("wrs_00000000000000000000000000000000")),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
        // Scope tidak cukup: kunci sempit (patients:read saja) ke
        // /queue -> 403.
        let (status, body, _) = call(
            app.clone(),
            pub_req("/public/v1/queue", Some(&narrow)),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
        // Pasien: NIK tersensor, tidak ada NIK lengkap di body mentah.
        let (status, body, raw) = call(
            app.clone(),
            pub_req("/public/v1/patients?search=Siti", Some(&key)),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert_eq!(body[0]["nik"], "337402******0001");
        assert!(!raw.contains("3374021205850001"), "NIK lengkap bocor: {raw}");
        // FHIR Patient: identifier NIK ikut tersensor.
        let (status, body, raw) = call(
            app.clone(),
            pub_req("/public/v1/fhir/Patient/RM-000812", Some(&key)),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert_eq!(body["resourceType"], "Patient");
        assert_eq!(body["identifier"][0]["value"], "337402******0001");
        assert!(!raw.contains("3374021205850001"), "NIK lengkap bocor di FHIR");
        // Encounters butuh patient_id -> 400 bila tidak ada.
        let (status, _, _) = call(
            app.clone(),
            pub_req("/public/v1/encounters", Some(&key)),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        let (status, body, _) = call(
            app,
            pub_req("/public/v1/encounters?patient_id=RM-000812", Some(&key)),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert!(body.as_array().map(|a| !a.is_empty()).unwrap_or(false));
    }

    /// 100 request lolos, ke-101 -> 429 (router + limiter sungguhan).
    /// Kunci baru dipakai supaya hitungan mulai dari nol.
    #[tokio::test]
    async fn public_rate_limit_429_setelah_100() {
        let (app, _db, key, _, _) = public_app().await;
        for i in 1..=100 {
            let (status, _, _) = call(app.clone(), pub_req("/public/v1/queue", Some(&key))).await;
            assert_eq!(
                status,
                axum::http::StatusCode::OK,
                "request ke-{i} harus OK"
            );
        }
        let (status, body, _) =
            call(app, pub_req("/public/v1/queue", Some(&key))).await;
        assert_eq!(status, axum::http::StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(body["error"]["code"], "RATE_LIMITED");
    }

    /// Kunci yang dicabut -> 401; `last_used_at` terisi setelah
    /// panggilan sukses (bukti `touch_api_key` jalan).
    #[tokio::test]
    async fn public_kunci_dicabut_ditolak() {
        let (app, db, key, _, id) = public_app().await;
        // Sebelum dipakai: last_used_at masih kosong.
        let before = db.get_api_key(&id).await.expect("baca kunci").expect("ada");
        assert!(before.last_used_at.is_none());
        // Satu panggilan sukses -> last_used_at terisi.
        let (status, _, _) = call(app.clone(), pub_req("/public/v1/queue", Some(&key))).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        let after = db.get_api_key(&id).await.expect("baca kunci").expect("ada");
        assert!(after.last_used_at.is_some());
        // Cabut -> panggilan berikut ditolak 401.
        db.revoke_api_key(&id).await.expect("cabut kunci");
        let (status, body, _) = call(app, pub_req("/public/v1/queue", Some(&key))).await;
        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED, "{body}");
    }
}
