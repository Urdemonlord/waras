//! Test API end-to-end di atas SQLite in-memory yang di-seed
//! (KONTRAK §5). Semua lewat `Router::oneshot`, tanpa server HTTP.

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Json, Router,
};
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::{json, Value};
use tower::ServiceExt;
use waras_bridge::{build_registry, BpjsConfig, BridgeConfig, SatusehatConfig};
use waras_db::Db;

use crate::{router, worker, AppState};

/// Konfigurasi jembatan serba-nonaktif (dipakai test Fase 1 yang lama).
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

async fn seeded_app() -> Router {
    seeded_app_with_config(disabled_config()).await.0
}

/// Seperti `seeded_app`, tapi dengan config jembatan pilihan pemanggil
/// (stub SATUSEHAT/BPJS). Mengembalikan juga `Db` supaya test bisa
/// menjalankan `worker::run_once` langsung.
async fn seeded_app_with_config(config: BridgeConfig) -> (Router, Arc<Db>) {
    let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
    db.migrate().await.expect("migrasi");
    assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
    let db = Arc::new(db);
    let bridges = Arc::new(build_registry(&config));
    let app = router(AppState {
        db: db.clone(),
        bridges,
        bridge_config: config,
    });
    (app, db)
}

fn worker_now() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, 19)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
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

// ---------------------------------------------------------------------------
// Fase 2: stub SATUSEHAT + BPJS + test integrasi (FASE2.md §3.4/§5)
// ---------------------------------------------------------------------------

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::Method,
    routing::{any, post},
};
use std::net::SocketAddr;

/// Keadaan stub yang bisa diubah test di tengah jalan (gagal N kali,
/// timpa envelope). Merekam semua request yang diterima.
#[derive(Default)]
struct StubInner {
    fail_times: usize,
    /// Gagalkan sekali tiap request yang path-nya mengandung salah satu
    /// string ini (dipakai menguji jalur gagal yang spesifik, mis. hanya
    /// insert SEP sementara eligibilitas tetap sukses).
    fail_once_paths: Vec<String>,
    envelope_override: Option<(String, String)>,
    requests: Vec<(String, String, Value)>,
}

#[derive(Clone, Default)]
struct StubState {
    inner: Arc<Mutex<StubInner>>,
}

impl StubState {
    fn requests(&self) -> Vec<(String, String, Value)> {
        self.inner.lock().unwrap().requests.clone()
    }

    fn set_fail_times(&self, n: usize) {
        self.inner.lock().unwrap().fail_times = n;
    }

    /// Gagalkan satu kali request yang path-nya mengandung tiap string.
    fn set_fail_once_paths(&self, paths: &[&str]) {
        self.inner.lock().unwrap().fail_once_paths =
            paths.iter().map(|s| s.to_string()).collect();
    }

    fn set_envelope_override(&self, code: &str, message: &str) {
        self.inner.lock().unwrap().envelope_override =
            Some((code.to_string(), message.to_string()));
    }

    fn record(&self, method: &str, path: &str, body: Value) {
        self.inner.lock().unwrap().requests.push((
            method.to_string(),
            path.to_string(),
            body,
        ));
    }

    /// `true` bila panggilan ini harus gagal 500 (dan kurangi hitungan).
    /// Cek dulu `fail_once_paths` (cocok substring path, sekali pakai),
    /// lalu hitungan `fail_times` global.
    fn should_fail(&self, path: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if let Some(i) = inner
            .fail_once_paths
            .iter()
            .position(|p| path.contains(p.as_str()))
        {
            inner.fail_once_paths.remove(i);
            return true;
        }
        if inner.fail_times > 0 {
            inner.fail_times -= 1;
            true
        } else {
            false
        }
    }
}

async fn satusehat_token_handler(
    State(state): State<StubState>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    state.record("POST", "/oauth2/v1/accesstoken", Value::Null);
    let _ = body;
    (
        StatusCode::OK,
        Json(json!({
            "access_token": "stub-token",
            "expires_in": 3600,
            "token_type": "Bearer",
        })),
    )
}

async fn satusehat_fhir_handler(
    State(state): State<StubState>,
    Path(path): Path<String>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let body_json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.record("POST", &format!("/fhir-r4/v1/{path}"), body_json);
    if state.should_fail(&format!("/fhir-r4/v1/{path}")) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "error", "diagnostics": "stub gagal 500"}],
            })),
        );
    }
    let resource = path.split('/').next().unwrap_or("Resource");
    (
        StatusCode::CREATED,
        Json(json!({"resourceType": resource, "id": "IHS-CONTOH-001"})),
    )
}

async fn spawn_satusehat_stub() -> (StubState, SocketAddr) {
    let state = StubState::default();
    let app = Router::new()
        .route("/oauth2/v1/accesstoken", post(satusehat_token_handler))
        .route("/fhir-r4/v1/{*path}", post(satusehat_fhir_handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (state, addr)
}

/// Respons kaleng per path (bentuk `response` envelope BPJS).
fn bpjs_canned_response(path: &str, body: &Value) -> Value {
    if path.contains("SEP/2.0/insert") {
        json!({"sep": {"noSep": "SEP-CONTOH-001"}})
    } else if path.contains("SEP/delete") {
        json!({"noSep": body["request"]["t_sep"]["noSep"].as_str().unwrap_or("SEP-CONTOH-001")})
    } else if path.contains("Peserta/nik/") {
        json!({"peserta": {"noKartu": "0001234567890", "nama": "Siti Rahmawati"}})
    } else if path.contains("Rujukan/insert") {
        json!({"rujukan": {"noKunjungan": "0301R0010126B000001"}})
    } else if path.contains("kunjungan") {
        json!({"noKunjungan": "KUNJ-CONTOH-001"})
    } else if path.contains("peserta/") {
        json!({"noKartu": "0001234567890", "nama": "Siti Rahmawati"})
    } else if path.contains("antrean/add") || path.contains("antrean/batal") {
        json!({"kodebooking": "JKN-CONTOH-001"})
    } else if path.contains("rest/bed/read/") {
        // Bentuk respons Aplicares asli (field huruf kecil).
        json!({"list": [
            {"kodekelas": "1", "koderuang": "R001", "namaruang": "Mawar", "kapasitas": 16, "tersedia": 12, "tersediapria": 6, "tersediawanita": 6, "tersediapriawanita": 0},
            {"kodekelas": "2", "koderuang": "R002", "namaruang": "Melati", "kapasitas": 32, "tersedia": 24, "tersediapria": 12, "tersediawanita": 12, "tersediapriawanita": 0},
        ]})
    } else if path.contains("klaim/submit") {
        json!({"claim_id": "CLM-CONTOH-001"})
    } else {
        json!({})
    }
}

async fn bpjs_handler(
    State(state): State<StubState>,
    method: Method,
    Path(path): Path<String>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let body_json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.record(&method.to_string(), &format!("/{path}"), body_json.clone());
    if state.should_fail(&path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"metaData": {"code": "500", "message": "stub gagal 500"}, "response": null})),
        );
    }
    let (code, message) = state
        .inner
        .lock()
        .unwrap()
        .envelope_override
        .clone()
        .unwrap_or_else(|| ("200".to_string(), "OK".to_string()));
    (
        StatusCode::OK,
        Json(json!({
            "metaData": {"code": code, "message": message},
            "response": bpjs_canned_response(&path, &body_json),
        })),
    )
}

async fn spawn_bpjs_stub() -> (StubState, SocketAddr) {
    let state = StubState::default();
    let app = Router::new()
        .route("/{*path}", any(bpjs_handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (state, addr)
}

fn satusehat_stub_config(addr: SocketAddr) -> BridgeConfig {
    let mut c = disabled_config();
    c.satusehat = SatusehatConfig {
        enabled: true,
        base_url: format!("http://{addr}/fhir-r4/v1"),
        auth_url: format!("http://{addr}/oauth2/v1"),
        client_id: "CONTOH-CLIENT-ID".into(),
        client_secret: "CONTOH-CLIENT-SECRET".into(),
        org_id: "CONTOH-ORG-ID".into(),
    };
    c
}

fn bpjs_stub_config(addr: SocketAddr) -> BridgeConfig {
    let mut c = disabled_config();
    c.bpjs = BpjsConfig {
        enabled: true,
        cons_id: "12345".into(),
        secret_key: "CONTOH-SECRET-KEY".into(),
        user_key: "CONTOH-USER-KEY".into(),
        vclaim_url: format!("http://{addr}/vclaim-rest/"),
        pcare_url: format!("http://{addr}/pcare-rest-v3.0/"),
        antrean_url: format!("http://{addr}/antrean-rs/"),
        aplicares_url: format!("http://{addr}/aplicaresws/"),
        kode_ppk: "1234567".into(),
        eklaim_url: format!("http://{addr}/eklaim/"),
    };
    c
}

async fn outbox_jobs(app: &Router, token: &str, query: &str) -> Value {
    let (s, v, _) = call(
        app.clone(),
        req(
            "GET",
            &format!("/api/outbox{query}"),
            Some(token),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

// ---------------------------------------------------------------------------
// Fase 2: test HTTP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bridges_unconfigured_lists_six_and_seps_seeded() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/bridges", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let bridges = v.as_array().expect("array 6 bridge");
    assert_eq!(bridges.len(), 6);
    for b in bridges {
        assert_eq!(b["health"], "belum_dikonfigurasi", "{b}");
        assert_eq!(b["configured"], false);
        assert_eq!(b["counts"]["pending"], 0);
        assert_eq!(b["counts"]["acked"], 0);
        assert!(b["last_success_at"].is_null());
    }

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/bridges/satusehat", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["destination"], "satusehat");
    assert_eq!(v["jobs"].as_array().map(|a| a.len()), Some(0));

    // Ejaan serde (`v_claim`) dan ejaan DB (`vclaim`) sama-sama diterima.
    for dest in ["v_claim", "vclaim", "p_care", "e_klaim"] {
        let (s, v, _) = call(
            app.clone(),
            req("GET", &format!("/api/bridges/{dest}"), Some(&token), None),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{dest}: {v}");
    }

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/bridges/tidak-ada", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/seps", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(8));

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/beds", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(5));

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/satusehat/resources", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let resources = v.as_array().expect("8 resource");
    assert_eq!(resources.len(), 8);
    assert!(resources.iter().all(|r| r["sent"] == 0 && r["failed"] == 0 && r["pending"] == 0));
}

#[tokio::test]
async fn fase1_handlers_enqueue_satusehat_jobs() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    // Pasien baru -> Patient.
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/patients",
            Some(&token),
            Some(json!({
                "nik": "3374010101900001", "name": "Test Pasien",
                "birth_date": "1990-01-01", "gender": "laki_laki",
                "address": "Jl. Uji No. 1", "insurance": "umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "RM-000822");
    // Encounter + vital + diagnosis + resep (lolos safety) -> 3 job lagi.
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/encounters",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000822", "poli": "Poli Umum",
                "doctor_name": "dr. Ahmad Hidayat",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let enc = v["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(
        app.clone(),
        req(
            "POST",
            &format!("/api/encounters/{enc}/vitals"),
            Some(&token),
            Some(json!({"code": "nadi", "value": "80", "measured_by": "Ns. Sari Wijaya"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _, _) = call(
        app.clone(),
        req(
            "POST",
            &format!("/api/encounters/{enc}/conditions"),
            Some(&token),
            Some(json!({
                "group": "utama", "code": "J06.9", "display": "ISPA atas",
                "status": "aktif", "clinician": "dr. Ahmad Hidayat",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/prescriptions",
            Some(&token),
            Some(json!({
                "encounter_id": enc, "drug_name": "Azithromycin 500 mg",
                "drug_class": "macrolide", "dosage": "1x1", "duration": "3 hari",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    let jobs = outbox_jobs(&app, &token, "?destination=satusehat&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox");
    assert_eq!(jobs.len(), 4, "{jobs:?}");
    let types: Vec<&str> = jobs
        .iter()
        .map(|j| j["resource_type"].as_str().unwrap())
        .collect();
    for t in ["Patient", "Observation", "Condition", "MedicationRequest"] {
        assert!(types.contains(&t), "kurang {t}: {types:?}");
    }
    assert!(jobs.iter().all(|j| j["status"] == "pending"));
}

#[tokio::test]
async fn close_encounter_enqueues_and_worker_acks_via_stub_satusehat() {
    let (stub, addr) = spawn_satusehat_stub().await;
    let config = satusehat_stub_config(addr);
    let (app, db) = seeded_app_with_config(config.clone()).await;
    let token = login_as(&app, "ahmad").await;

    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/encounters/ENC-2026-11-19-014/close",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "selesai");

    let jobs = outbox_jobs(&app, &token, "?destination=satusehat&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    let enc_job = jobs
        .iter()
        .find(|j| j["resource_type"] == "Encounter")
        .expect("job Encounter");
    assert_eq!(enc_job["status"], "pending");
    assert_eq!(
        enc_job["idempotency_key"],
        "satusehat:Encounter:ENC-2026-11-19-014:closed"
    );
    assert!(jobs
        .iter()
        .any(|j| j["resource_type"] == "AllergyIntolerance"));

    // Worker satu pass ke stub SATUSEHAT.
    let registry = build_registry(&config);
    worker::run_once(&db, &registry, &config, worker_now()).await;

    let jobs = outbox_jobs(&app, &token, "?destination=satusehat&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert!(jobs.iter().all(|j| j["status"] == "acked"), "{jobs:?}");
    assert!(jobs
        .iter()
        .all(|j| j["external_id"] == "IHS-CONTOH-001"));

    // Stub menerima token + POST Encounter (subject Patient/812) +
    // AllergyIntolerance.
    let reqs = stub.requests();
    assert!(reqs.iter().any(|(_, p, _)| p == "/oauth2/v1/accesstoken"));
    let enc_req = reqs
        .iter()
        .find(|(_, p, _)| p == "/fhir-r4/v1/Encounter")
        .expect("POST Encounter ke stub");
    assert_eq!(enc_req.2["resourceType"], "Encounter");
    assert_eq!(enc_req.2["subject"]["reference"], "Patient/812");
    assert!(reqs
        .iter()
        .any(|(_, p, _)| p == "/fhir-r4/v1/AllergyIntolerance"));

    // Ringkasan resource ikut terhitung.
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/satusehat/resources", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let by_resource = |name: &str| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|r| r["resource"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(by_resource("Encounter")["sent"], 1);
    assert_eq!(by_resource("AllergyIntolerance")["sent"], 1);
    assert_eq!(by_resource("Patient")["sent"], 0);

    // Audit worker tercatat.
    let audits = db.list_audit(100).await.unwrap();
    assert!(audits.iter().any(|e| e.action == "bridge.acked"));
    assert!(audits.iter().any(|e| e.action == "bridge.send_attempt"));
}

#[tokio::test]
async fn close_encounter_twice_is_idempotent() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    for _ in 0..2 {
        let (s, v, _) = call(
            app.clone(),
            req(
                "POST",
                "/api/encounters/ENC-2026-11-19-014/close",
                Some(&token),
                None,
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let jobs = outbox_jobs(&app, &token, "?destination=satusehat&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 2, "1 Encounter + 1 AllergyIntolerance: {jobs:?}");
}

#[tokio::test]
async fn sep_create_via_stub_vclaim_returns_number_and_links_invoice() {
    let (stub, addr) = spawn_bpjs_stub().await;
    let config = bpjs_stub_config(addr);
    let (app, db) = seeded_app_with_config(config.clone()).await;
    let token = login_as(&app, "ahmad").await;

    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000821",
                "encounter_id": "ENC-2026-11-19-020",
                "bpjs_number": "0001234567898",
                "jenis": "RawatJalan",
                "poli": "Poli Umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["sep_number"], "SEP-20261119-009");
    assert_eq!(v["id"], "SEP-20261119-009");
    assert_eq!(v["status"], "aktif");
    // Nomor SEP BPJS hasil insert live langsung mendarat di baris SEP.
    assert_eq!(v["bpjs_sep_number"], "SEP-CONTOH-001");

    // SEP tertaut ke tagihan encounter-nya (INV-2026-1181).
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/seps/SEP-20261119-009", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["invoice"]["id"], "INV-2026-1186");

    // Satu pengiriman saja (FASE2.md §9): job outbox langsung Acked
    // (bukan Pending), external_id = nomor SEP BPJS.
    let jobs = outbox_jobs(&app, &token, "?destination=vclaim&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["resource_type"], "SEP");
    assert_eq!(jobs[0]["status"], "acked");
    assert_eq!(jobs[0]["external_id"], "SEP-CONTOH-001");
    assert_eq!(
        jobs[0]["idempotency_key"],
        "vclaim:SEP:SEP-20261119-009:created"
    );

    // Stub menerima cek eligibilitas (GET Peserta) + insert SEP SEKALI.
    let reqs = stub.requests();
    assert!(reqs
        .iter()
        .any(|(_, p, _)| p.contains("Peserta/nik/3374033009800010/tglSEP/2026-11-19")));
    assert_eq!(
        reqs.iter()
            .filter(|(_, p, _)| p.contains("SEP/2.0/insert"))
            .count(),
        1,
        "insert live tepat sekali: {reqs:?}"
    );

    // Worker satu pass: job sudah Acked, TIDAK ada insert kedua.
    let registry = build_registry(&config);
    worker::run_once(&db, &registry, &config, worker_now()).await;
    let jobs = outbox_jobs(&app, &token, "?destination=vclaim&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs[0]["status"], "acked");
    assert_eq!(jobs[0]["external_id"], "SEP-CONTOH-001");
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|(_, p, _)| p.contains("SEP/2.0/insert"))
            .count(),
        1,
        "worker tidak mengirim ulang insert: {:?}",
        stub.requests()
    );
}

#[tokio::test]
async fn sep_create_twice_returns_existing_without_second_insert() {
    // Idempotensi lokal (FASE2.md §9): POST kedua untuk encounter yang
    // sama mengembalikan SEP yang sudah Aktif (200), tanpa panggilan
    // BPJS apa pun.
    let (stub, addr) = spawn_bpjs_stub().await;
    let config = bpjs_stub_config(addr);
    let (app, _db) = seeded_app_with_config(config).await;
    let token = login_as(&app, "ahmad").await;
    let body = json!({
        "patient_id": "RM-000821",
        "encounter_id": "ENC-2026-11-19-020",
        "bpjs_number": "0001234567898",
        "jenis": "RawatJalan",
        "poli": "Poli Umum",
    });
    let (s, v, _) = call(app.clone(), req("POST", "/api/seps", Some(&token), Some(body.clone()))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["bpjs_sep_number"], "SEP-CONTOH-001");
    let (s, v, _) = call(app.clone(), req("POST", "/api/seps", Some(&token), Some(body))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["id"], "SEP-20261119-009");
    assert_eq!(v["bpjs_sep_number"], "SEP-CONTOH-001");
    // Tetap 1 insert + 1 cek eligibilitas (POST kedua tidak memanggil).
    let reqs = stub.requests();
    assert_eq!(
        reqs.iter().filter(|(_, p, _)| p.contains("SEP/2.0/insert")).count(),
        1,
        "{reqs:?}"
    );
    assert_eq!(
        reqs.iter().filter(|(_, p, _)| p.contains("Peserta/nik/")).count(),
        1,
        "{reqs:?}"
    );
    // Tetap 1 baris SEP, 1 job outbox.
    let (s, v, _) = call(app, req("GET", "/api/seps", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(9));
}

#[tokio::test]
async fn sep_create_live_insert_failure_then_worker_acks() {
    // Jalur gagal (FASE2.md §9): insert live gagal -> baris Gagal +
    // job Pending; worker retry mengirim insert SEKALI dan ack,
    // sync-back menulis nomor BPJS + status Aktif ke baris SEP.
    let (stub, addr) = spawn_bpjs_stub().await;
    stub.set_fail_once_paths(&["SEP/2.0/insert"]);
    let config = bpjs_stub_config(addr);
    let (app, db) = seeded_app_with_config(config.clone()).await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000821",
                "encounter_id": "ENC-2026-11-19-020",
                "bpjs_number": "0001234567898",
                "jenis": "RawatJalan",
                "poli": "Poli Umum",
            })),
        ),
    )
    .await;
    // Klien tetap menerima error (insert live gagal), tapi baris Gagal
    // + job Pending sudah tersimpan untuk pemulihan worker.
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/seps/SEP-20261119-009", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "gagal");
    assert_eq!(v["bpjs_sep_number"], Value::Null);
    // Tagihan encounter sudah tertaut sejak baris Gagal dibuat, supaya
    // setelah worker memulihkan SEP-nya tagihan tidak tetap null.
    assert_eq!(v["invoice"]["id"], "INV-2026-1186");
    let jobs = outbox_jobs(&app, &token, "?destination=vclaim&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["status"], "pending");

    // Worker satu pass: insert kedua (satu-satunya yang sukses) -> ack.
    let registry = build_registry(&config);
    worker::run_once(&db, &registry, &config, worker_now()).await;
    let jobs = outbox_jobs(&app, &token, "?destination=vclaim&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs[0]["status"], "acked");
    assert_eq!(jobs[0]["external_id"], "SEP-CONTOH-001");
    // Total insert ke stub: 1 gagal + 1 sukses = 2 (bukan 3: POST ulang
    // tidak ada, worker tidak mengirim dua kali).
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|(_, p, _)| p.contains("SEP/2.0/insert"))
            .count(),
        2,
        "{:?}",
        stub.requests()
    );
    // Sync-back: baris SEP kini Aktif + bernomor BPJS.
    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/seps/SEP-20261119-009", Some(&token), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "aktif");
    assert_eq!(v["bpjs_sep_number"], "SEP-CONTOH-001");
}

#[tokio::test]
async fn sep_retry_after_failure_reuses_gagal_row_and_marks_pending_acked() {
    // POST ulang setelah gagal: memakai ulang baris Gagal yang sama
    // (bukan baris baru), insert live sukses, dan job Pending dari
    // percobaan pertama ditandai Acked — bukan dibiarkan untuk dikirim
    // worker (itu akan jadi insert kedua).
    let (stub, addr) = spawn_bpjs_stub().await;
    stub.set_fail_once_paths(&["SEP/2.0/insert"]);
    let config = bpjs_stub_config(addr);
    let (app, db) = seeded_app_with_config(config.clone()).await;
    let token = login_as(&app, "ahmad").await;
    let body = json!({
        "patient_id": "RM-000821",
        "encounter_id": "ENC-2026-11-19-020",
        "bpjs_number": "0001234567898",
        "jenis": "RawatJalan",
        "poli": "Poli Umum",
    });
    let (s, _, _) = call(app.clone(), req("POST", "/api/seps", Some(&token), Some(body.clone()))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    // POST ulang: sukses, baris yang sama.
    let (s, v, _) = call(app.clone(), req("POST", "/api/seps", Some(&token), Some(body))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "SEP-20261119-009");
    assert_eq!(v["status"], "aktif");
    assert_eq!(v["bpjs_sep_number"], "SEP-CONTOH-001");
    // Satu job, sudah Acked (bukan Pending baru).
    let jobs = outbox_jobs(&app, &token, "?destination=vclaim&limit=100").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["status"], "acked");
    assert_eq!(jobs[0]["external_id"], "SEP-CONTOH-001");
    // Worker satu pass: tidak ada insert lagi.
    let registry = build_registry(&config);
    worker::run_once(&db, &registry, &config, worker_now()).await;
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|(_, p, _)| p.contains("SEP/2.0/insert"))
            .count(),
        2,
        "1 gagal + 1 sukses, tanpa kiriman worker: {:?}",
        stub.requests()
    );
}

#[tokio::test]
async fn sep_create_unknown_nik_is_422() {
    // Tanpa encounter + poli Gigi: tidak ada seed SEP untuk kunci ini
    // (seed SEP-001 milik RM-000812 adalah encounter 014 / Poli Umum),
    // jadi alur sampai ke cek eligibilitas dan gagal 422 di sana.
    let (stub, addr) = spawn_bpjs_stub().await;
    stub.set_envelope_override("404", "NIK tidak ditemukan");
    let config = bpjs_stub_config(addr);
    let (app, _db) = seeded_app_with_config(config).await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000812",
                "bpjs_number": "0001234567890",
                "jenis": "RawatJalan",
                "poli": "Gigi",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "VALIDATION");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("NIK tidak ditemukan"));
    // SEP tidak tersimpan.
    let (s, v, _) = call(app, req("GET", "/api/seps", Some(&token), None)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(8));
}

#[tokio::test]
async fn sep_create_unconfigured_is_503() {
    let app = seeded_app().await;
    let token = login_as(&app, "ahmad").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&token),
            Some(json!({
                "patient_id": "RM-000812",
                "bpjs_number": "0001234567890",
                "jenis": "RawatJalan",
                "poli": "Poli Umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(v["error"]["code"], "BRIDGE_NOT_CONFIGURED");

    let (s, v, _) = call(
        app.clone(),
        req(
            "GET",
            "/api/bpjs/peserta?nik=3374021205850001",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(v["error"]["code"], "BRIDGE_NOT_CONFIGURED");

    let admin = login_as(&app, "admin").await;
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/beds/refresh", Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/antrean-online/sync", Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
}

#[tokio::test]
async fn kasir_cannot_create_sep_but_perawat_can_pass_rbac() {
    let app = seeded_app().await;
    let kasir = login_as(&app, "sinta").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&kasir),
            Some(json!({
                "patient_id": "RM-000812",
                "bpjs_number": "0001234567890",
                "jenis": "RawatJalan",
                "poli": "Poli Umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");

    // Perawat lolos RBAC (gagalnya di 503 karena BPJS belum dikonfigurasi).
    let perawat = login_as(&app, "sari").await;
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/seps",
            Some(&perawat),
            Some(json!({
                "patient_id": "RM-000812",
                "bpjs_number": "0001234567890",
                "jenis": "RawatJalan",
                "poli": "Poli Umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
}

#[tokio::test]
async fn retry_backoff_then_acked_after_stub_recovers() {
    let (stub, addr) = spawn_satusehat_stub().await;
    stub.set_fail_times(2);
    let config = satusehat_stub_config(addr);
    let (app, db) = seeded_app_with_config(config.clone()).await;
    let dokter = login_as(&app, "ahmad").await;
    let admin = login_as(&app, "admin").await;
    let registry = build_registry(&config);

    // Satu job Patient (stub gagal 500 dua kali, lalu 200).
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/patients",
            Some(&dokter),
            Some(json!({
                "nik": "3374010101900001", "name": "Test Pasien",
                "birth_date": "1990-01-01", "gender": "laki_laki",
                "address": "Jl. Uji No. 1", "insurance": "umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    // Pass 1: gagal 500 -> Failed, attempts 1, next_retry_at = +2 menit.
    worker::run_once(&db, &registry, &config, worker_now()).await;
    let jobs = outbox_jobs(&app, &admin, "?status=failed").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["attempts"], 1);
    assert_eq!(jobs[0]["next_retry_at"], "2026-11-19 12:02:00");
    assert!(jobs[0]["last_error"].as_str().unwrap().contains("500"));
    let job_id = jobs[0]["id"].as_str().unwrap().to_string();

    // Retry oleh dokter ditolak (Admin saja); oleh admin diterima.
    let (s, v, _) = call(
        app.clone(),
        req("POST", &format!("/api/outbox/{job_id}/retry"), Some(&dokter), None),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v, _) = call(
        app.clone(),
        req("POST", &format!("/api/outbox/{job_id}/retry"), Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "pending");
    assert_eq!(v["attempts"], 0);
    assert!(v["next_retry_at"].is_null());

    // Pass 2: gagal 500 kedua -> Failed lagi.
    worker::run_once(&db, &registry, &config, worker_now()).await;
    let jobs = outbox_jobs(&app, &admin, "?status=failed").await;
    assert_eq!(jobs.as_array().map(|a| a.len()), Some(1));
    let (s, _, _) = call(
        app.clone(),
        req("POST", &format!("/api/outbox/{job_id}/retry"), Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Pass 3: stub 200 -> Acked.
    worker::run_once(&db, &registry, &config, worker_now()).await;
    let jobs = outbox_jobs(&app, &admin, "?status=acked").await;
    let jobs = jobs.as_array().expect("array outbox").clone();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0]["external_id"], "IHS-CONTOH-001");
}

#[tokio::test]
async fn outbox_cancel_by_admin_only() {
    let app = seeded_app().await;
    let dokter = login_as(&app, "ahmad").await;
    let admin = login_as(&app, "admin").await;
    let (s, _, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/patients",
            Some(&dokter),
            Some(json!({
                "nik": "3374010101900001", "name": "Test Pasien",
                "birth_date": "1990-01-01", "gender": "laki_laki",
                "address": "Jl. Uji No. 1", "insurance": "umum",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let jobs = outbox_jobs(&app, &admin, "?status=pending").await;
    let job_id = jobs[0]["id"].as_str().unwrap().to_string();

    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            &format!("/api/outbox/{job_id}/cancel"),
            Some(&dokter),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            &format!("/api/outbox/{job_id}/cancel"),
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "dead");
    let jobs = outbox_jobs(&app, &admin, "?status=dead").await;
    assert_eq!(jobs.as_array().map(|a| a.len()), Some(1));
}

#[tokio::test]
async fn referral_pcare_claim_flows_enqueue() {
    let app = seeded_app().await;
    let dokter = login_as(&app, "ahmad").await;
    let kasir = login_as(&app, "sinta").await;

    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/referrals",
            Some(&dokter),
            Some(json!({
                "patient_id": "RM-000816", "from_poli": "Poli Umum",
                "to_facility": "RSUD Dr. Kariadi", "reason": "Bedah Saraf",
            })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "RUJ-20261119-003");

    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/pcare/visits",
            Some(&dokter),
            Some(json!({"patient_id": "RM-000815", "poli": "Gigi"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "PCV-20261119-004");

    // Klaim oleh dokter ditolak; oleh kasir diterima, SEP diambil dari
    // tagihan (INV-2026-1181 -> SEP-20261119-001 via encounter).
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/claims",
            Some(&dokter),
            Some(json!({"invoice_id": "INV-2026-1181", "ina_cbg_code": "J-4-17-I"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    let (s, v, _) = call(
        app.clone(),
        req(
            "POST",
            "/api/claims",
            Some(&kasir),
            Some(json!({"invoice_id": "INV-2026-1181", "ina_cbg_code": "J-4-17-I"})),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], "CLM-20261119-002");
    assert_eq!(v["sep_id"], "SEP-20261119-001");
    assert_eq!(v["amount"], 185000);

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/claims/CLM-20261119-002", Some(&kasir), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "draft");

    for (dest, rtype) in [
        ("vclaim", "Rujukan"),
        ("pcare", "Kunjungan"),
        ("eklaim", "Klaim"),
    ] {
        // Outbox hanya boleh dibaca Admin/Dokter (kasir tidak boleh).
        let jobs = outbox_jobs(&app, &dokter, &format!("?destination={dest}")).await;
        let jobs = jobs.as_array().expect("array outbox").clone();
        assert_eq!(jobs.len(), 1, "{dest}: {jobs:?}");
        assert_eq!(jobs[0]["resource_type"], rtype);
        assert_eq!(jobs[0]["status"], "pending");
    }
}

#[tokio::test]
async fn antrean_sync_and_beds_refresh_via_stub_bpjs() {
    let (stub, addr) = spawn_bpjs_stub().await;
    let config = bpjs_stub_config(addr);
    let (app, _db) = seeded_app_with_config(config).await;
    let admin = login_as(&app, "admin").await;
    let dokter = login_as(&app, "ahmad").await;

    let (s, v, _) = call(
        app.clone(),
        req("GET", "/api/antrean-online", Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().map(|a| a.len()), Some(7));

    // Sinkronkan 5 booking Menunggu hari ini (JKN-881203..207).
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/antrean-online/sync", Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["synced"], 5);
    let jobs = outbox_jobs(&app, &admin, "?destination=antrean_online").await;
    assert_eq!(jobs.as_array().map(|a| a.len()), Some(5));

    // Sinkron oleh dokter ditolak (Admin saja).
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/antrean-online/sync", Some(&dokter), None),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");

    // Refresh bed dari Aplicares (stub: Kelas 1 + Kelas 2).
    let (s, v, _) = call(
        app.clone(),
        req("POST", "/api/beds/refresh", Some(&admin), None),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let beds = v.as_array().expect("array bed");
    assert_eq!(beds.len(), 5);
    let kelas1 = beds.iter().find(|b| b["class"] == "Kelas 1").unwrap();
    assert_eq!(kelas1["available"], 12);
    assert_eq!(kelas1["total"], 16);
    assert!(stub
        .requests()
        .iter()
        .any(|(_, p, _)| p.contains("rest/bed/read/1234567/1/100")));
}
