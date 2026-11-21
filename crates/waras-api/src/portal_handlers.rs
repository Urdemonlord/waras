//! Portal pasien (FASE4.md §5.2 + §7; kata sandi portal Fase A/A4).
//!
//! Login memakai NIK + **kata sandi portal** (hash Argon2 di
//! `patients.portal_password_hash`, seed demo `waras123` — sama
//! seperti kata sandi seed staf, lihat README). NIK + tanggal lahir
//! saja TIDAK PERNAH cukup: payload lama `{nik, birth_date}`
//! ditolak (400, field `password` wajib) dan tanggal lahir sebagai
//! kata sandi ditolak 401. Tidak ada kanal OTP (tidak ada pengirim
//! SMS/WA) — kata sandi adalah mekanisme yang dipilih, lihat
//! `docs/FASE-A-A4.md`.
//! Token pasien (sesi 12 jam, `sessions.patient_id`
//! terisi, `user_id` NULL) DITOLAK oleh extractor staf `AuthUser`
//! karena JOIN ke `users` tidak menemukan baris.
//!
//! Aturan keras: semua query portal memfilter
//! `patient_id = token.patient_id`. NIK tidak pernah dikembalikan
//! lengkap — selalu disensor tengah (`mask_nik`).

use axum::{
    extract::{FromRequestParts, Path, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use waras_core::{rbac::Action, types::Patient};
use waras_db::{clinic_today, Db, PrescriptionFilter};

use crate::{
    auth::{require, AuthUser, ClientIp, JsonBody},
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
    pub password: String,
}

/// `POST /api/auth/patient-login` (publik). NIK + kata sandi portal
/// (Argon2, pola yang sama dengan login staf). NIK/tanggal lahir
/// saja TIDAK cukup: field `birth_date` tidak ada lagi di DTO, jadi
/// payload lama gagal validasi (400); tanggal lahir yang dipakai
/// sebagai kata sandi gagal verifikasi (401).
/// Brute-force sama seperti login staf: 5 gagal dalam 15 menit untuk
/// pasangan IP + NIK yang sama -> 429 + `Retry-After`; satu login
/// sukses menghapus hitungan. Pesan 401 tetap generik.
pub async fn patient_login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    JsonBody(body): JsonBody<PatientLoginDto>,
) -> ApiResult<Response> {
    let nik = body.nik.trim().to_string();
    if nik.is_empty() || body.password.is_empty() {
        return Err(ApiError::validation("NIK dan kata sandi wajib diisi"));
    }
    let attempt_key = format!("patient:{ip}:{nik}");
    if let Some(retry) = state.db.check_login_lockout(&attempt_key).await? {
        return Ok(ApiError::rate_limited_response(
            retry as u64,
            "terlalu banyak percobaan login portal, coba lagi nanti",
        ));
    }
    let patient = state.db.find_patient_by_nik(&nik).await?;
    let ok = match &patient {
        Some(p) => match state.db.get_portal_password_hash(&p.id).await? {
            Some(hash) => Db::verify_password(&hash, &body.password),
            // Pasien tanpa kata sandi portal (mis. pasien baru dari
            // registrasi) tidak bisa login portal.
            None => false,
        },
        None => false,
    };
    if !ok {
        if let Some(retry) = state.db.record_login_failure(&attempt_key).await? {
            return Ok(ApiError::rate_limited_response(
                retry as u64,
                "terlalu banyak percobaan login portal, coba lagi nanti",
            ));
        }
        return Err(ApiError::unauthorized());
    }
    let patient = patient.expect("pasien ada bila verifikasi lolos");
    let _ = state.db.clear_login_attempts(&attempt_key).await;
    let token = state.db.create_patient_session(&patient.id).await?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "token": token,
            "patient": { "id": patient.id, "name": patient.name, "rm": patient.id },
        })),
    )
        .into_response())
}

/// Panjang minimum kata sandi portal baru (set staf maupun ganti
/// pasien). Aturan baru item 1 follow-up Fase A: tidak ada aturan
/// panjang di codebase sebelumnya; 8 dipilih karena sama dengan kata
/// sandi seed `waras123` — pasien tidak dipaksa lebih ketat dari demo
/// yang sudah berjalan.
pub const MIN_PORTAL_PASSWORD_LEN: usize = 8;

fn valid_new_password(password: &str) -> ApiResult<()> {
    if password.is_empty() {
        return Err(ApiError::validation("kata sandi baru wajib diisi"));
    }
    if password.chars().count() < MIN_PORTAL_PASSWORD_LEN {
        return Err(ApiError::validation(format!(
            "kata sandi portal minimal {MIN_PORTAL_PASSWORD_LEN} karakter"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct SetPortalPasswordDto {
    pub password: String,
}

/// `POST /api/patients/{id}/portal-password` (staf, Admin saja).
/// Set awal SEKALIGUS reset: menimpa hash yang ada (atau NULL) dengan
/// hash Argon2 kata sandi baru via `Db::set_portal_password`. Tidak
/// butuh kata sandi lama — pasien baru dari registrasi belum punya
/// kata sandi sama sekali, jadi tidak ada yang bisa diverifikasi.
/// RBAC `ManageUsers` (hanya Admin di matriks `waras_core::rbac`;
/// tidak ada aksi baru di core supaya core tetap sesuai kontrak
/// "selesai kecuali bug"). Pasien tidak ditemukan -> 404; bukan Admin
/// -> 403 (dicek sebelum keberadaan pasien, supaya tidak membocorkan
/// mana RM yang ada). Dicatat ke audit (`portal.password_set`).
/// Tidak ada lockout brute-force di sini: tidak ada rahasia yang
/// ditebak — endpoint sudah di balik auth staf + RBAC Admin; token
/// yang tidak sah ditolak extractor `AuthUser` (401) sebelum DB
/// disentuh. Lihat `docs/FASE-A-FOLLOWUP.md` §1 untuk alasan tidak
/// memakai jalur self-service NIK + tanggal lahir.
pub async fn set_portal_password_staff(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<SetPortalPasswordDto>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ManageUsers)?;
    valid_new_password(&body.password)?;
    state
        .db
        .get_patient(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("pasien {id} tidak ditemukan")))?;
    state.db.set_portal_password(&id, &body.password).await?;
    let _ = state
        .db
        .log_audit(
            &user.name,
            "portal.password_set",
            "patient",
            &id,
            json!({"via": "admin"}),
        )
        .await;
    Ok(Json(json!({ "status": "ok", "patient_id": id })))
}

#[derive(Deserialize)]
pub struct ChangePortalPasswordDto {
    pub current_password: String,
    pub new_password: String,
}

/// `POST /api/portal/change-password` (pasien, token portal).
/// Ganti kata sandi sendiri: wajib kata sandi lama (verifikasi Argon2
/// terhadap hash tersimpan). Pasien yang hash-nya masih NULL (belum
/// pernah di-set staf) selalu 401 di sini — jalur mereka adalah set
/// staf, bukan ganti. Brute-force sama seperti login portal: 5 kata
/// sandi lama salah dalam 15 menit untuk pasangan IP + pasien yang
/// sama -> 429 + `Retry-After` (kunci `portal-change:<ip>:<id>`,
/// terpisah dari kunci login supaya percobaan ganti tidak mengunci
/// login dan sebaliknya); satu ganti sukses menghapus hitungan ganti
/// dan hitungan login portal IP + NIK yang sama, karena pasien baru
/// saja membuktikan ia tahu kata sandinya. Dicatat ke audit
/// (`portal.password_changed`).
pub async fn change_portal_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    PortalPatient(patient): PortalPatient,
    JsonBody(body): JsonBody<ChangePortalPasswordDto>,
) -> ApiResult<Response> {
    if body.current_password.is_empty() {
        return Err(ApiError::validation("kata sandi lama wajib diisi"));
    }
    valid_new_password(&body.new_password)?;
    let attempt_key = format!("portal-change:{ip}:{}", patient.id);
    if let Some(retry) = state.db.check_login_lockout(&attempt_key).await? {
        return Ok(ApiError::rate_limited_response(
            retry as u64,
            "terlalu banyak percobaan ganti kata sandi, coba lagi nanti",
        ));
    }
    let ok = match state.db.get_portal_password_hash(&patient.id).await? {
        Some(hash) => Db::verify_password(&hash, &body.current_password),
        // Pasien tanpa kata sandi portal tidak bisa ganti — jalurnya
        // set staf.
        None => false,
    };
    if !ok {
        if let Some(retry) = state.db.record_login_failure(&attempt_key).await? {
            return Ok(ApiError::rate_limited_response(
                retry as u64,
                "terlalu banyak percobaan ganti kata sandi, coba lagi nanti",
            ));
        }
        return Err(ApiError::unauthorized());
    }
    state
        .db
        .set_portal_password(&patient.id, &body.new_password)
        .await?;
    let _ = state.db.clear_login_attempts(&attempt_key).await;
    let _ = state
        .db
        .clear_login_attempts(&format!("patient:{ip}:{}", patient.nik))
        .await;
    let _ = state
        .db
        .log_audit(
            &patient.name,
            "portal.password_changed",
            "patient",
            &patient.id,
            json!({"via": "portal"}),
        )
        .await;
    Ok((StatusCode::OK, Json(json!({ "status": "ok" }))).into_response())
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
            .route("/api/auth/login", post(crate::handlers::login))
            .route(
                "/api/patients",
                post(crate::handlers::create_patient),
            )
            .route(
                "/api/patients/{id}/portal-password",
                post(set_portal_password_staff),
            )
            .route("/api/auth/patient-login", post(patient_login))
            .route("/api/portal/change-password", post(change_portal_password))
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

    fn login_req(nik: &str, password: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/auth/patient-login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "nik": nik, "password": password }).to_string(),
            ))
            .expect("request login")
    }

    /// Login Budi Santoso (seed: NIK 3374011503900002, kata sandi
    /// portal `waras123`) -> token pasien.
    async fn login_budi(app: &Router) -> String {
        let (status, body) = call(app.clone(), login_req("3374011503900002", "waras123")).await;
        assert_eq!(status, StatusCode::OK, "login Budi harus OK: {body}");
        body["token"].as_str().expect("token").to_string()
    }

    /// Login staf (`waras123`) -> token staf.
    async fn login_staff(app: &Router, username: &str) -> String {
        let (status, body) = call(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "username": username, "password": "waras123" }).to_string(),
                ))
                .expect("request login staf"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "login staf {username} harus OK: {body}");
        body["token"].as_str().expect("token").to_string()
    }

    /// Daftarkan pasien baru lewat API (hash portal NULL — celah yang
    /// ditutup item ini). Mengembalikan id RM baru.
    async fn register_patient(app: &Router, admin_token: &str) -> String {
        let (status, body) = call(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/patients")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "nik": "3374010101900001",
                        "name": "Pasien Baru Tes",
                        "birth_date": "1990-01-01",
                        "gender": "L",
                        "address": "Jl. Tes 1, Semarang",
                        "phone": null,
                        "blood_type": "O+",
                        "bpjs_number": null,
                        "insurance": "Umum",
                    })
                    .to_string(),
                ))
                .expect("request registrasi"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "registrasi harus 201: {body}");
        body["id"].as_str().expect("id RM").to_string()
    }

    fn set_password_req(
        token: Option<&str>,
        patient_id: &str,
        password: &str,
    ) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri(format!("/api/patients/{patient_id}/portal-password"))
            .header("content-type", "application/json");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::from(json!({ "password": password }).to_string()))
            .expect("request set password")
    }

    fn change_password_req(
        token: Option<&str>,
        current: &str,
        new: &str,
    ) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri("/api/portal/change-password")
            .header("content-type", "application/json");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        b.body(Body::from(
            json!({ "current_password": current, "new_password": new }).to_string(),
        ))
        .expect("request ganti password")
    }

    #[tokio::test]
    async fn pasien_baru_dari_registrasi_bisa_login_setelah_set_staf() {
        let app = portal_app().await;
        let admin = login_staff(&app, "admin").await;
        let rm = register_patient(&app, &admin).await;
        // Pasien baru: hash NULL -> login portal 401.
        let (status, _) = call(
            app.clone(),
            login_req("3374010101900001", "apaaja123"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "pasien baru tanpa kata sandi tidak boleh login");
        // Admin set kata sandi awal -> 200.
        let (status, body) = call(app.clone(), set_password_req(Some(&admin), &rm, "barupass1")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["patient_id"], rm);
        // Login portal dengan kata sandi baru -> 200 + /me jalan.
        let (status, body) = call(app.clone(), login_req("3374010101900001", "barupass1")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let token = body["token"].as_str().expect("token").to_string();
        let (status, body) = call(
            app,
            Request::builder()
                .method("GET")
                .uri("/api/portal/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request me"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["id"], rm);
    }

    #[tokio::test]
    async fn staff_set_tanpa_token_401_bukan_admin_403_tidak_ada_404_kata_sandi_pendek_400() {
        let app = portal_app().await;
        // Tanpa token -> 401 (extractor AuthUser).
        let (status, _) = call(app.clone(), set_password_req(None, "RM-000813", "barupass1")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // Dokter (bukan Admin) -> 403.
        let dokter = login_staff(&app, "ahmad").await;
        let (status, body) = call(app.clone(), set_password_req(Some(&dokter), "RM-000813", "barupass1")).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "FORBIDDEN");
        // Admin, pasien tidak ada -> 404.
        let admin = login_staff(&app, "admin").await;
        let (status, body) = call(app.clone(), set_password_req(Some(&admin), "RM-999999", "barupass1")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        // Admin, kata sandi < 8 karakter -> 400.
        let (status, body) = call(app.clone(), set_password_req(Some(&admin), "RM-000813", "pendek")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "VALIDATION");
        // Token pasien (bukan staf) -> 401, bukan 403.
        let budi = login_budi(&app).await;
        let (status, _) = call(app, set_password_req(Some(&budi), "RM-000813", "barupass1")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn staff_set_mereset_kata_sandi_lama() {
        let app = portal_app().await;
        let admin = login_staff(&app, "admin").await;
        let (status, _) = call(app.clone(), set_password_req(Some(&admin), "RM-000813", "resetpass1")).await;
        assert_eq!(status, StatusCode::OK);
        // Kata sandi lama tidak lagi berlaku, yang baru berlaku.
        let (status, _) = call(app.clone(), login_req("3374011503900002", "waras123")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "kata sandi lama harus mati setelah reset");
        let (status, _) = call(app, login_req("3374011503900002", "resetpass1")).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn portal_change_ganti_kata_sandi_lalu_login_baru_jalan_lama_mati() {
        let app = portal_app().await;
        let token = login_budi(&app).await;
        let (status, body) = call(
            app.clone(),
            change_password_req(Some(&token), "waras123", "gantipass1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "ok");
        let (status, _) = call(app.clone(), login_req("3374011503900002", "waras123")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "kata sandi lama harus mati setelah ganti");
        let (status, _) = call(app, login_req("3374011503900002", "gantipass1")).await;
        assert_eq!(status, StatusCode::OK, "kata sandi baru harus bisa login");
    }

    #[tokio::test]
    async fn portal_change_tanpa_token_401_kata_sandi_lama_salah_401_baru_pendek_400() {
        let app = portal_app().await;
        // Tanpa token -> 401.
        let (status, _) = call(app.clone(), change_password_req(None, "waras123", "gantipass1")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let token = login_budi(&app).await;
        // Kata sandi lama salah -> 401 generik.
        let (status, body) = call(
            app.clone(),
            change_password_req(Some(&token), "salah123", "gantipass1"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        // Kata sandi baru < 8 karakter -> 400 (tidak menghabiskan
        // hitungan lockout: validasi jalan sebelum verifikasi).
        let (status, body) = call(
            app.clone(),
            change_password_req(Some(&token), "waras123", "pendek"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        // Kata sandi lama tetap berlaku setelah dua penolakan itu.
        let (status, _) = call(app, login_req("3374011503900002", "waras123")).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn portal_change_lima_gagal_mengunci_429() {
        let app = portal_app().await;
        let token = login_budi(&app).await;
        // 4 gagal pertama -> 401; ke-5 -> 429.
        for i in 1..5 {
            let (status, _) = call(
                app.clone(),
                change_password_req(Some(&token), "salah123", "gantipass1"),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "gagal ke-{i}");
        }
        let (status, body) = call(
            app.clone(),
            change_password_req(Some(&token), "salah123", "gantipass1"),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(body["error"]["code"], "RATE_LIMITED");
        // Kata sandi lama yang benar pun ditolak selama terkunci.
        let (status, _) = call(
            app,
            change_password_req(Some(&token), "waras123", "gantipass1"),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn patient_login_kata_sandi_salah_401() {
        let app = portal_app().await;
        // NIK benar, kata sandi salah -> 401 generik.
        let (status, body) = call(app.clone(), login_req("3374011503900002", "salah")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["error"]["code"].is_string());
        // NIK tidak ada -> 401 juga (bukan 404: jangan bocorkan).
        let (status, _) = call(app, login_req("3374011503900099", "waras123")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patient_login_jalur_lama_nik_tanggal_lahir_ditolak() {
        let app = portal_app().await;
        // Payload lama {nik, birth_date} (tanpa password): field
        // `password` wajib -> 400, BUKAN login sukses.
        let (status, body) = call(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/auth/patient-login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "nik": "3374011503900002", "birth_date": "1990-03-15" })
                        .to_string(),
                ))
                .expect("request login lama"),
        )
        .await;
        assert_ne!(status, StatusCode::OK, "jalur lama tidak boleh login: {body}");
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        // Tanggal lahir yang dipakai SEBAGAI kata sandi -> 401.
        let (status, _) = call(
            app,
            login_req("3374011503900002", "1990-03-15"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patient_login_lima_gagal_mengunci() {
        let app = portal_app().await;
        // 4 gagal pertama -> 401; ke-5 -> 429 (terkunci 15 menit).
        for i in 1..5 {
            let (status, _) = call(
                app.clone(),
                login_req("3374011503900002", "salah"),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "gagal ke-{i}");
        }
        let (status, body) = call(app.clone(), login_req("3374011503900002", "salah")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(body["error"]["code"], "RATE_LIMITED");
        assert!(
            body["error"]["message"].as_str().unwrap_or("").contains("coba lagi"),
            "{body}"
        );
        // Kata sandi benar pun ditolak selama terkunci.
        let (status, _) = call(app, login_req("3374011503900002", "waras123")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
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
