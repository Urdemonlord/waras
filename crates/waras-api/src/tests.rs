//! Test API end-to-end di atas SQLite in-memory yang di-seed
//! (KONTRAK §5). Semua lewat `Router::oneshot`, tanpa server HTTP.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use waras_db::Db;

use crate::{router, AppState};

async fn seeded_app() -> Router {
    let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
    db.migrate().await.expect("migrasi");
    assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
    router(AppState { db: Arc::new(db) })
}

async fn call(app: Router, req: Request<Body>) -> (StatusCode, Value, String) {
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let _content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("baca body");
    let text = String::from_utf8_lossy(&bytes).to_string();
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v, text)
}

fn req(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    b.header("content-type", "application/json")
        .body(match body {
            Some(v) => Body::from(v.to_string()),
            None => Body::empty(),
        })
        .unwrap()
}

async fn login_as(app: &Router, username: &str) -> String {
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"username": username, "password": "waras123"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "login {username} gagal: {v}");
    v["token"].as_str().expect("token ada").to_string()
}

const ENC_SITI: &str = "ENC-2026-11-19-014";

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_success_returns_token_and_user_without_password_hash() {
    let app = seeded_app().await;
    let (s, v, raw) = call(
        app,
        req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"username": "ahmad", "password": "waras123"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["token"].as_str().map(|t| t.len() == 64).unwrap_or(false));
    assert_eq!(v["user"]["username"], "ahmad");
    assert_eq!(v["user"]["name"], "dr. Ahmad Hidayat");
    assert_eq!(v["user"]["role"], "dokter_umum");
    assert!(raw.find("password_hash").is_none(), "hash tidak boleh bocor: {raw}");
    assert!(raw.find("waras123").is_none(), "password tidak boleh bocor");
}

#[tokio::test]
async fn login_by_nip_works() {
    let app = seeded_app().await;
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"username": "198505122010011001", "password": "waras123"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["user"]["username"], "ahmad");
}

#[tokio::test]
async fn login_wrong_password_is_401() {
    let app = seeded_app().await;
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"username": "ahmad", "password": "salah"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    assert_eq!(v["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn login_unknown_user_is_401() {
    let app = seeded_app().await;
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"username": "tidakada", "password": "waras123"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
}

#[tokio::test]
async fn unauthenticated_request_is_401() {
    let app = seeded_app().await;
    let (s, v, _) = call(app, req("GET", "/api/patients", None, None)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    assert_eq!(v["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn auth_me_returns_user_from_token() {
    let app = seeded_app().await;
    let token = login_as(&app, "sari").await;
    let (s, v, _) = call(app, req("GET", "/api/auth/me", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["user"]["username"], "sari");
    assert_eq!(v["user"]["role"], "perawat");
}

#[tokio::test]
async fn health_is_public() {
    let app = seeded_app().await;
    let (s, v, _) = call(app, req("GET", "/api/health", None, None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["version"], "0.1.0");
    assert_eq!(v["mode"], "sqlite");
    assert_eq!(v["today"], "2026-11-19");
}

// ---------------------------------------------------------------------------
// RBAC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kasir_cannot_prescribe() {
    let app = seeded_app().await;
    let token = login_as(&app, "sinta").await; // Kasir
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/prescriptions",
            Some(&token),
            Some(json!({
                "encounter_id": ENC_SITI,
                "drug_name": "Azithromycin 500 mg",
                "drug_class": "macrolide",
                "dosage": "1x1",
                "duration": "3 hari",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["code"], "FORBIDDEN");
}

#[tokio::test]
async fn perawat_cannot_prescribe() {
    let app = seeded_app().await;
    let token = login_as(&app, "sari").await; // Perawat
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/prescriptions",
            Some(&token),
            Some(json!({
                "encounter_id": ENC_SITI,
                "drug_name": "Azithromycin 500 mg",
                "drug_class": "macrolide",
                "dosage": "1x1",
                "duration": "3 hari",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
}

#[tokio::test]
async fn dokter_cannot_dispense_apoteker_can() {
    let app = seeded_app().await;
    let dokter = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/prescriptions/RX-1042/dispense", Some(&dokter), None),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");

    let apoteker = login_as(&app, "dina").await;
    let (s, v, _) = call(
        app,
        req("POST", "/api/prescriptions/RX-1042/dispense", Some(&apoteker), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "diberikan");
}

#[tokio::test]
async fn dokter_cannot_create_invoice_or_receive_payment() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/invoices",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000812", "payer": "bpjs",
                "service": "Poli Umum", "amount": 100000,
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/invoices/INV-2026-1183/pay",
            Some(&token),
            Some(json!({"method": "tunai", "amount": 450000})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
}

// ---------------------------------------------------------------------------
// Aturan keamanan obat
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prescribe_amoxicillin_for_siti_blocked_422_and_stored() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/prescriptions",
            Some(&token),
            Some(json!({
                "encounter_id": ENC_SITI,
                "drug_name": "Amoxicillin 500 mg",
                "drug_class": "penicillin",
                "dosage": "1x1",
                "duration": "3 hari",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "BLOCKED");
    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("DIBLOKIR"), "pesan harus berisi DIBLOKIR: {msg}");

    // Resep yang diblokir tetap tersimpan (seed RX-1040 + yang baru).
    let (s, v, _) = call(
        app,
        req(
            "GET",
            "/api/prescriptions?patient_id=RM-000812&status=diblokir",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(2), "{v}");
    // Semua baris yang diblokir punya alasan; yang baru diblokir lewat
    // API memakai pesan aturan keamanan ("DIBLOKIR: ..."), sedangkan
    // RX-1040 seed menyimpan alasan desain ("alergi Penisilin").
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["blocked_reason"].as_str().map(|m| !m.is_empty()).unwrap_or(false)));
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["blocked_reason"]
            .as_str()
            .map(|m| m.contains("DIBLOKIR"))
            .unwrap_or(false)));
}

#[tokio::test]
async fn prescribe_azithromycin_for_siti_succeeds() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/prescriptions",
            Some(&token),
            Some(json!({
                "encounter_id": ENC_SITI,
                "drug_name": "Azithromycin 500 mg",
                "drug_class": "macrolide",
                // Sengaja kirim format lama: server harus menormalisasi
                // dosage jadi frekuensi saja ("1x1"), bukan menyimpan
                // "500 mg 1x1" yang mengulang kekuatan dari drug_name.
                "dosage": "500 mg 1x1",
                "duration": "3 hari",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "aktif");
    assert_eq!(v["drug_name"], "Azithromycin 500 mg");
    assert_eq!(v["dosage"], "1x1", "dosage ternormalisasi, tanpa kekuatan dobel: {v}");
}

#[tokio::test]
async fn confirm_waiting_prescription() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req("POST", "/api/prescriptions/RX-1041/confirm", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "aktif");
}

// ---------------------------------------------------------------------------
// Pasien, antrean, encounter, SOAP, dasbor (aliran penuh)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn patient_detail_shape() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req("GET", "/api/patients/RM-000812", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "Siti Rahmawati");
    assert_eq!(v["allergies"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(v["allergies"][0]["substance"], "Penisilin");
    assert_eq!(v["conditions"].as_array().map(|a| a.len()), Some(7));
    assert_eq!(v["last_encounter"]["id"], ENC_SITI);
}

#[tokio::test]
async fn patient_not_found_is_404() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req("GET", "/api/patients/RM-999999", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn create_patient_validates_nik() {
    let app = seeded_app().await;
    let token = login_as(&app, "sari").await;
    let (s, v, _) = call(
        app,
        req(
            "POST",
            "/api/patients",
            Some(&token),
            Some(json!({
                "nik": "123", "name": "Uji", "birth_date": "1990-01-01",
                "gender": "laki_laki", "address": "Jl. Uji", "insurance": "umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "VALIDATION");
}

#[tokio::test]
async fn dashboard_kpis_match_seed() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(app, req("GET", "/api/dashboard", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["queue_total"], 44);
    assert_eq!(v["queue_active"], 32);
    assert_eq!(v["queue_waiting"], 15);
    assert_eq!(v["queue_in_consultation"], 17);
    assert_eq!(v["queue_done"], 10);
    assert_eq!(v["queue_no_show"], 2);
    assert_eq!(v["followups_7d"], 31);
    assert_eq!(v["patients_today"], 248);
    assert_eq!(v["visits_avg_7d"], 178);
    assert_eq!(v["visits_peak_7d"], 210);
}

#[tokio::test]
async fn full_flow_patient_queue_encounter_vital_soap() {
    let app = seeded_app().await;
    let dokter = login_as(&app, "ahmad").await;

    // 1. Pasien baru -> RM-000822.
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/patients",
            Some(&dokter),
            Some(json!({
                "nik": "3374010101900001", "name": "Uji Coba",
                "birth_date": "1990-01-01", "gender": "laki_laki",
                "address": "Jl. Uji No. 1", "insurance": "umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "RM-000822");

    // 2. Ambil antrean -> A-045.
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/queue/take",
            Some(&dokter),
            Some(json!({
                "patient_id": "RM-000822",
                "poli": "Poli Umum",
                "doctor": "dr. Ahmad Hidayat",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["number"], "A-045");
    let queue_id = v["id"].as_str().unwrap().to_string();

    // 2b. Panggil antrean.
    let (s, v, _) = call(
        app.clone(),
        req("POST", &format!("/api/queue/{queue_id}/call"), Some(&dokter), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "dipanggil");
    assert!(v["called_at"].is_string());

    // 3. Encounter baru.
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/encounters",
            Some(&dokter),
            Some(json!({
                "patient_id": "RM-000822",
                "poli": "Poli Umum",
                "doctor_name": "dr. Ahmad Hidayat",
                "doctor_id": "U-001",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let enc_id = v["id"].as_str().unwrap().to_string();
    assert!(enc_id.starts_with("ENC-2026-11-19-"), "{enc_id}");
    assert_eq!(v["status"], "aktif");

    // 4. Tanda vital (unit diisi dari VitalCode).
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            &format!("/api/encounters/{enc_id}/vitals"),
            Some(&dokter),
            Some(json!({
                "code": "tekanan_darah", "value": "120/80",
                "measured_by": "Ns. Sari Wijaya",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["unit"], "mmHg");

    // 5. SOAP v1 (lengkap) lalu v2 (plan kosong -> Draft); versi lama kept.
    let soap_v1 = json!({
        "subjective": "Batuk 2 hari.",
        "objective": "Paru bersih.",
        "assessment": "ISPA atas (J06.9).",
        "plan": "Paracetamol 500 mg 3x1, 3 hari.",
    });
    let (s, v, _) = call(
        app.clone(),
        req(
            "PUT",
            &format!("/api/encounters/{enc_id}/soap"),
            Some(&dokter),
            Some(soap_v1),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version"], 1);
    assert_eq!(v["status"], "lengkap");

    let (s, v, _) = call(
        app.clone(),
        req(
            "PUT",
            &format!("/api/encounters/{enc_id}/soap"),
            Some(&dokter),
            Some(json!({
                "subjective": "Batuk 2 hari.",
                "objective": "Paru bersih.",
                "assessment": "ISPA atas (J06.9).",
                "plan": "",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version"], 2);
    assert_eq!(v["status"], "draft");

    let (s, v, _) = call(
        app.clone(),
        req(
            "GET",
            &format!("/api/encounters/{enc_id}/soap"),
            Some(&dokter),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["version"], 2);
    assert_eq!(v["versions"].as_array().map(|a| a.len()), Some(2));
    assert_eq!(v["versions"][0]["version"], 1);
    assert_eq!(v["versions"][0]["plan"], "Paracetamol 500 mg 3x1, 3 hari.");
    assert_eq!(v["versions"][1]["version"], 2);

    // 6. Detail encounter memuat SOAP + vital + bundle FHIR.
    let (s, v, _) = call(
        app,
        req("GET", &format!("/api/encounters/{enc_id}"), Some(&dokter), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["vitals"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(v["fhir_bundle"]["resourceType"], "Bundle");
}

// ---------------------------------------------------------------------------
// Tagihan, FHIR, audit, statis
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pay_invoice_flips_to_lunas() {
    let app = seeded_app().await;
    let kasir = login_as(&app, "sinta").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/invoices/INV-2026-1183/pay",
            Some(&kasir),
            Some(json!({"method": "tunai", "amount": 450000})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "lunas");
    assert_eq!(v["payments"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(v["payments"][0]["amount"], 450000);
}

#[tokio::test]
async fn fhir_bundle_has_all_resource_types() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req("GET", &format!("/api/encounters/{ENC_SITI}/fhir"), Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["resourceType"], "Bundle");
    let types: Vec<&str> = v["entry"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["resource"]["resourceType"].as_str())
        .collect();
    assert!(types.contains(&"Patient"), "{types:?}");
    assert!(types.contains(&"Encounter"), "{types:?}");
    assert!(types.contains(&"Observation"), "{types:?}");
    assert!(types.contains(&"Condition"), "{types:?}");
    assert!(types.contains(&"MedicationRequest"), "{types:?}");
    // Patient/812 (bukan RM-000812).
    let patient = v["entry"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["resource"]["resourceType"] == "Patient")
        .unwrap();
    assert_eq!(patient["resource"]["id"], "812");
}

#[tokio::test]
async fn patient_fhir_endpoint() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app,
        req("GET", "/api/patients/RM-000812/fhir", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["resourceType"], "Patient");
    assert_eq!(v["id"], "812");
}

#[tokio::test]
async fn audit_list_and_drugs() {
    let app = seeded_app().await;
    let token = login_as(&app, "admin").await;
    let (s, v, _) = call(app.clone(), req("GET", "/api/audit", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v.as_array().map(|a| a.len() >= 6).unwrap_or(false), "{v}");

    let (s, v, _) = call(app, req("GET", "/api/drugs", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(9));
    let amox = v
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kfa_code"] == "KFA-9201")
        .unwrap();
    assert_eq!(amox["stock"], 12);
    assert_eq!(amox["stock_status"], "kritis");
}

#[tokio::test]
async fn static_pages_and_fallback() {
    let app = seeded_app().await;
    // Halaman login publik, tanpa token.
    let (s, _v, text) = call(app.clone(), req("GET", "/", None, None)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(text.contains("WARAS") || text.contains("Waras"), "index.html: {text:.200}");

    let (s, _v, _t) = call(app.clone(), req("GET", "/dashboard", None, None)).await;
    assert_eq!(s, StatusCode::OK);

    let (s, _v, _t) = call(app.clone(), req("GET", "/css/app.css", None, None)).await;
    assert_eq!(s, StatusCode::OK);

    // API tidak dikenal -> JSON 404 (dengan token).
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/tidak-ada", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"]["code"], "NOT_FOUND");

    // Path non-API tidak dikenal -> redirect ke `/`.
    let resp = app
        .oneshot(req("GET", "/jalan-salah", None, None))
        .await
        .expect("oneshot");
    assert!(
        resp.status().is_redirection(),
        "status: {}",
        resp.status()
    );
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/")
    );
}

#[tokio::test]
async fn queue_and_notifications_lists() {
    let app = seeded_app().await;
    let token = login_as(&app, "sari").await;
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/queue?date=2026-11-19", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(44));

    let (s, v, _) = call(app, req("GET", "/api/notifications", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(3));
}
