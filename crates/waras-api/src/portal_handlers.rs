//! Portal pasien (FASE4.md §5.2 + §7).
//!
//! Login memakai NIK + tanggal lahir — kredensial kelas demo, BUKAN
//! autentikasi kuat (NIK + tanggal lahir bisa diketahui orang lain).
//! Untuk produksi, ganti dengan OTP (SMS/WA) atau kata sandi portal
//! (FASE4.md §7). Token pasien (sesi 12 jam, `sessions.patient_id`
//! terisi, `user_id` NULL) DITOLAK oleh extractor staf `AuthUser`
//! karena JOIN ke `users` tidak menemukan baris.
//!
//! Aturan keras: semua query portal memfilter
//! `patient_id = token.patient_id`. NIK tidak pernah dikembalikan
//! lengkap — selalu disensor tengah (`mask_nik`).

use axum::{
    extract::{FromRequestParts, State},
    http::{header, request::Parts},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use waras_core::types::Patient;
use waras_db::{clinic_today, PrescriptionFilter};

use crate::{
    auth::JsonBody,
    error::ApiError,
    AppState,
};

type ApiResult<T> = Result<T, ApiError>;

/// Sensor NIK di tengah: 6 digit pertama + `*` + 4 digit terakhir,
/// mis. `3374021205850001` -> `337402******0001` (FASE4.md §5.2).
/// NIK pendek (< 10 digit) disensor seluruhnya kecuali 2 terakhir.
pub fn mask_nik(nik: &str) -> String {
    let n = nik.chars().count();
    if n >= 10 {
        let head: String = nik.chars().take(6).collect();
        let tail: String = nik.chars().skip(n - 4).collect();
        format!("{head}{}{tail}", "*".repeat(n - 10))
    } else if n > 2 {
        let tail: String = nik.chars().skip(n - 2).collect();
        format!("{}{}", "*".repeat(n - 2), tail)
    } else {
        "*".repeat(n)
    }
}

/// JSON pasien dengan NIK tersensor (dipakai portal + API publik).
pub fn patient_masked_json(patient: &Patient) -> ApiResult<Value> {
    let mut v = serde_json::to_value(patient)?;
    v["nik"] = json!(mask_nik(&patient.nik));
    Ok(v)
}

/// Pasien terautentikasi lewat token portal
/// (`Authorization: Bearer <token>` -> `Db::find_patient_by_token`).
pub struct PortalPatient(pub Patient);

impl FromRequestParts<AppState> for PortalPatient {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(ApiError::unauthorized)?;
        let patient = state
            .db
            .find_patient_by_token(token)
            .await
            .map_err(ApiError::from)?;
        match patient {
            Some(p) => Ok(PortalPatient(p)),
            None => Err(ApiError::unauthorized()),
        }
    }
}

#[derive(Deserialize)]
pub struct PatientLoginDto {
    pub nik: String,
    pub birth_date: NaiveDate,
}

/// `POST /api/auth/patient-login` (publik). NIK/tanggal tidak cocok
/// -> 401 dengan pesan generik (jangan bocorkan mana yang salah).
pub async fn patient_login(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<PatientLoginDto>,
) -> ApiResult<Json<Value>> {
    let patient = state.db.find_patient_by_nik(body.nik.trim()).await?;
    let patient = match patient {
        Some(p) if p.birth_date == body.birth_date => p,
        _ => return Err(ApiError::unauthorized()),
    };
    let token = state.db.create_patient_session(&patient.id).await?;
    Ok(Json(json!({
        "token": token,
        "patient": { "id": patient.id, "name": patient.name, "rm": patient.id },
    })))
}

/// `GET /api/portal/me` — data pasien pemilik token (NIK tersensor).
pub async fn portal_me(
    State(state): State<AppState>,
    PortalPatient(patient): PortalPatient,
) -> ApiResult<Json<Value>> {
    let mut v = patient_masked_json(&patient)?;
    v["allergies"] = serde_json::to_value(state.db.get_allergies(&patient.id).await?)?;
    Ok(Json(v))
}

/// `GET /api/portal/appointments` — janji milik pasien pemilik token.
pub async fn portal_appointments(
    State(state): State<AppState>,
    PortalPatient(patient): PortalPatient,
) -> ApiResult<Json<Vec<waras_core::types::Appointment>>> {
    let all = state.db.list_appointments(None).await?;
    Ok(Json(
        all.into_iter()
            .filter(|a| a.patient_id == patient.id)
            .collect(),
    ))
}

/// `GET /api/portal/queue` — antrean milik pasien hari ini.
pub async fn portal_queue(
    State(state): State<AppState>,
    PortalPatient(patient): PortalPatient,
) -> ApiResult<Json<Vec<waras_core::types::QueueEntry>>> {
    let all = state.db.list_queue(clinic_today()).await?;
    Ok(Json(
        all.into_iter()
            .filter(|q| q.patient_id == patient.id)
            .collect(),
    ))
}

/// `GET /api/portal/lab` — hasil lab milik pasien.
///
/// PENYIMPANGAN dari FASE4.md §5.2 (dicatat): kontrak tidak membatasi
/// tanggal, tetapi `Db` hari ini tidak punya method daftar lab per
/// pasien — `list_lab_orders` hanya berfilter tanggal. Endpoint ini
/// memakai order hari klinik ini saja. Butuh tindak lanjut agen A /
/// koordinator: tambah `Db::list_lab_orders_by_patient(patient_id)`
/// (atau `list_lab_orders` dengan filter pasien) supaya riwayat lab
/// lengkap tampil; sampai saat itu hasil lab lama tidak terlihat di
/// portal.
pub async fn portal_lab(
    State(state): State<AppState>,
    PortalPatient(patient): PortalPatient,
) -> ApiResult<Json<Vec<Value>>> {
    let orders = state
        .db
        .list_lab_orders_by_patient(&patient.id)
        .await?;
    let mut out = Vec::new();
    for order in orders.into_iter().filter(|o| o.patient_id == patient.id) {
        let results = state.db.list_lab_results(&order.id).await?;
        out.push(json!({ "order": order, "results": results }));
    }
    Ok(Json(out))
}

/// `GET /api/portal/prescriptions` — e-resep milik pasien pemilik token.
pub async fn portal_prescriptions(
    State(state): State<AppState>,
    PortalPatient(patient): PortalPatient,
) -> ApiResult<Json<Vec<waras_core::types::MedicationRequest>>> {
    Ok(Json(
        state
            .db
            .list_prescriptions(PrescriptionFilter {
                status: None,
                patient_id: Some(patient.id.clone()),
            })
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use serde_json::json;
    use tower::ServiceExt;
    use waras_bridge::{build_registry, BpjsConfig, BridgeConfig, SatusehatConfig};
    use waras_db::Db;

    #[test]
    fn mask_nik_menyensor_tengah_sesuai_contoh_kontrak() {
        assert_eq!(mask_nik("3374021205850001"), "337402******0001");
        assert_eq!(mask_nik("3374031005720013"), "337403******0013");
        // NIK tidak pernah kembali lengkap.
        let m = mask_nik("3374021205850001");
        assert!(!m.contains("120585"));
        assert!(m.starts_with("337402"));
        assert!(m.ends_with("0001"));
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

    async fn portal_app() -> Router {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
        let db = Arc::new(db);
        Router::new()
            .route("/api/auth/patient-login", post(patient_login))
            .route("/api/portal/me", get(portal_me))
            .route("/api/portal/appointments", get(portal_appointments))
            .route("/api/portal/queue", get(portal_queue))
            .route("/api/portal/lab", get(portal_lab))
            .route("/api/portal/prescriptions", get(portal_prescriptions))
            .with_state(AppState {
                db,
                bridges: Arc::new(build_registry(&disabled_config())),
                bridge_config: disabled_config(),
            })
    }

    async fn call(app: Router, req: Request<Body>) -> (StatusCode, Value) {
        let resp = app.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("baca body");
        let value: Value =
            serde_json::from_slice(&bytes).unwrap_or(Value::String(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        (status, value)
    }

    fn login_req(nik: &str, birth_date: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/auth/patient-login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "nik": nik, "birth_date": birth_date }).to_string(),
            ))
            .expect("request login")
    }

    /// Login Budi Santoso (seed: NIK 3374011503900002, lahir
    /// 1990-03-15) -> token pasien.
    async fn login_budi(app: &Router) -> String {
        let (status, body) = call(
            app.clone(),
            login_req("3374011503900002", "1990-03-15"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "login Budi harus OK: {body}");
        body["token"].as_str().expect("token").to_string()
    }

    #[tokio::test]
    async fn patient_login_tanggal_lahir_salah_401() {
        let app = portal_app().await;
        // NIK benar, tanggal lahir salah -> 401 generik.
        let (status, body) = call(
            app.clone(),
            login_req("3374011503900002", "1990-03-16"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["error"]["code"].is_string());
        // NIK tidak ada -> 401 juga (bukan 404: jangan bocorkan).
        let (status, _) = call(app, login_req("3374011503900099", "1990-03-15")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn portal_hanya_mengembalikan_data_pemilik_token() {
        let app = portal_app().await;
        let token = login_budi(&app).await;
        // Janji: hanya milik Budi (RM-000813); janji Siti (RM-000812)
        // tidak boleh bocor.
        let (status, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri("/api/portal/appointments")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request janji"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let list = body.as_array().expect("array janji");
        assert!(!list.is_empty(), "Budi punya janji di seed");
        assert!(
            list.iter().all(|a| a["patient_id"] == "RM-000813"),
            "tidak boleh ada janji pasien lain: {body}"
        );
        // /me: NIK tersensor, bukan lengkap.
        let (status, body) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri("/api/portal/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request me"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["nik"], "337401******0002");
        assert_eq!(body["id"], "RM-000813");
        // Token pasien DITOLAK endpoint staf (konstruksi AuthUser:
        // JOIN users tidak menemukan baris). Diuji di sini lewat
        // /api/portal/* tanpa token -> 401.
        let (status, _) = call(
            app,
            Request::builder()
                .method("GET")
                .uri("/api/portal/me")
                .body(Body::empty())
                .expect("request tanpa token"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
