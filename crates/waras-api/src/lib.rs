//! WARAS HTTP API (Axum): router, auth token sesi, guard RBAC, handler,
//! DTO, dan pemetaan error ke JSON. Lihat `docs/CONTRACT.md` §3.
//!
//! Titik masuk publik: [`AppState`] + [`router`]. Binary `waras` memanggil
//! `router(AppState { db })` lalu me-serve-nya.
//!
//! Bentuk respons mengikuti kontrak: daftar sebagai array JSON langsung,
//! detail sebagai objek yang digabung (mis. pasien + alergi + kondisi +
//! encounter terakhir), error sebagai
//! `{ "error": { "code": "...", "message": "..." } }`.

pub mod api_key_handlers;
pub mod auth;
pub mod bridge_handlers;
pub mod error;
pub mod fase3_handlers;
pub mod fase4_handlers;
pub mod handlers;
pub mod parse;
pub mod portal_handlers;
pub mod public_api;
pub mod web;
pub mod worker;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::{
    http::{header, HeaderValue},
    routing::{get, post, put},
    Router,
};
use tower_http::set_header::SetResponseHeaderLayer;
use waras_bridge::{BridgeConfig, BridgeRegistry};
use waras_db::Db;

/// State aplikasi yang dibagikan ke semua handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    /// Registry adapter jembatan (Fase 2, §5): dipakai worker + handler
    /// yang butuh panggilan live. Tujuan yang belum dikonfigurasi tetap
    /// terdaftar sebagai `DisabledBridge`.
    pub bridges: Arc<BridgeRegistry>,
    /// Konfigurasi jembatan + worker dari env (Fase 2, §7).
    pub bridge_config: BridgeConfig,
}

/// Router lengkap: API (`/api/...`) + UI statis dari `waras-web`.
pub fn router(state: AppState) -> Router {
    use handlers::*;
    Router::new()
        // -- Publik -------------------------------------------------------
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        // -- Auth ---------------------------------------------------------
        .route("/api/auth/me", get(me))
        // -- Pasien -------------------------------------------------------
        .route("/api/patients", get(list_patients).post(create_patient))
        .route("/api/patients/{id}", get(get_patient))
        .route("/api/patients/{id}/fhir", get(get_patient_fhir))
        .route(
            "/api/patients/{id}/conditions",
            get(list_patient_conditions),
        )
        // -- Antrean ------------------------------------------------------
        .route("/api/queue", get(list_queue))
        .route("/api/queue/take", post(take_queue))
        .route("/api/queue/{id}/call", post(call_queue))
        .route("/api/queue/{id}/status", post(set_queue_status))
        // -- Encounter / SOAP / vital / diagnosis -------------------------
        .route(
            "/api/encounters",
            get(list_encounters).post(create_encounter),
        )
        .route("/api/encounters/{id}", get(get_encounter))
        .route("/api/encounters/{id}/fhir", get(get_encounter_fhir))
        .route("/api/encounters/{id}/close", post(close_encounter))
        .route(
            "/api/encounters/{id}/soap",
            get(get_soap).put(save_soap),
        )
        .route(
            "/api/encounters/{id}/vitals",
            get(list_vitals).post(add_vital),
        )
        .route(
            "/api/encounters/{id}/conditions",
            post(add_condition),
        )
        // -- Resep & obat -------------------------------------------------
        .route(
            "/api/prescriptions",
            get(list_prescriptions).post(create_prescription),
        )
        .route(
            "/api/prescriptions/{id}/confirm",
            post(confirm_prescription),
        )
        .route(
            "/api/prescriptions/{id}/dispense",
            post(dispense_prescription),
        )
        .route("/api/drugs", get(list_drugs))
        // -- Dasbor, tagihan, audit, notifikasi ---------------------------
        .route("/api/dashboard", get(get_dashboard))
        .route("/api/invoices", get(list_invoices).post(create_invoice))
        .route("/api/invoices/{id}", get(get_invoice))
        .route("/api/invoices/{id}/pay", post(pay_invoice))
        .route("/api/audit", get(list_audit))
        .route("/api/notifications", get(list_notifications))
        // -- Jembatan nasional Fase 2 (FASE2.md §5) -----------------------
        .route("/api/bridges", get(bridge_handlers::list_bridges))
        .route(
            "/api/bridges/{destination}",
            get(bridge_handlers::get_bridge),
        )
        .route("/api/outbox", get(bridge_handlers::list_outbox))
        .route(
            "/api/outbox/{id}/retry",
            post(bridge_handlers::retry_outbox),
        )
        .route(
            "/api/outbox/{id}/cancel",
            post(bridge_handlers::cancel_outbox),
        )
        .route(
            "/api/seps",
            get(bridge_handlers::list_seps).post(bridge_handlers::create_sep),
        )
        .route("/api/seps/{id}", get(bridge_handlers::get_sep))
        .route(
            "/api/seps/{id}/cancel",
            post(bridge_handlers::cancel_sep),
        )
        .route("/api/bpjs/peserta", get(bridge_handlers::check_peserta))
        .route(
            "/api/referrals",
            get(bridge_handlers::list_referrals).post(bridge_handlers::create_referral),
        )
        .route(
            "/api/pcare/visits",
            get(bridge_handlers::list_pcare_visits).post(bridge_handlers::create_pcare_visit),
        )
        .route(
            "/api/antrean-online",
            get(bridge_handlers::list_online_queue),
        )
        .route(
            "/api/antrean-online/sync",
            post(bridge_handlers::sync_online_queue),
        )
        .route("/api/beds", get(bridge_handlers::list_beds))
        .route("/api/beds/refresh", post(bridge_handlers::refresh_beds))
        .route(
            "/api/claims",
            get(bridge_handlers::list_claims).post(bridge_handlers::create_claim),
        )
        .route("/api/claims/{id}", get(bridge_handlers::get_claim))
        .route(
            "/api/satusehat/resources",
            get(bridge_handlers::satusehat_resources),
        )
        // -- Rumah sakit Fase 3 (FASE3.md §5) -----------------------------
        .route("/api/igd", get(fase3_handlers::list_igd))
        .route("/api/igd/triage", post(fase3_handlers::create_triage))
        .route("/api/igd/{id}", get(fase3_handlers::get_igd))
        .route(
            "/api/igd/{id}/status",
            post(fase3_handlers::set_igd_status),
        )
        .route(
            "/api/igd/{id}/observations",
            post(fase3_handlers::add_igd_observation),
        )
        .route("/api/ranap/beds", get(fase3_handlers::list_beds))
        .route(
            "/api/ranap/admissions",
            get(fase3_handlers::list_admissions).post(fase3_handlers::create_admission),
        )
        .route(
            "/api/ranap/admissions/{id}",
            get(fase3_handlers::get_admission),
        )
        .route(
            "/api/ranap/admissions/{id}/discharge",
            post(fase3_handlers::discharge_admission),
        )
        .route(
            "/api/ranap/admissions/{id}/transfer",
            post(fase3_handlers::transfer_admission),
        )
        .route(
            "/api/ranap/admissions/{id}/notes",
            post(fase3_handlers::add_care_note),
        )
        .route(
            "/api/operasi",
            get(fase3_handlers::list_operasi).post(fase3_handlers::create_surgery),
        )
        .route(
            "/api/operasi/{id}/status",
            post(fase3_handlers::set_surgery_status),
        )
        .route(
            "/api/bersalin",
            get(fase3_handlers::list_bersalin).post(fase3_handlers::create_delivery),
        )
        .route(
            "/api/bersalin/{id}/status",
            post(fase3_handlers::set_delivery_status),
        )
        .route(
            "/api/lab/orders",
            get(fase3_handlers::list_lab_orders).post(fase3_handlers::create_lab_order),
        )
        .route("/api/lab/orders/{id}", get(fase3_handlers::get_lab_order))
        .route(
            "/api/lab/orders/{id}/results",
            post(fase3_handlers::add_lab_results),
        )
        .route(
            "/api/lab/orders/{id}/status",
            post(fase3_handlers::set_lab_order_status),
        )
        .route(
            "/api/radiologi/orders",
            get(fase3_handlers::list_rad_orders).post(fase3_handlers::create_rad_order),
        )
        .route(
            "/api/radiologi/orders/{id}",
            get(fase3_handlers::get_rad_order),
        )
        .route(
            "/api/radiologi/orders/{id}/report",
            post(fase3_handlers::save_rad_report),
        )
        .route(
            "/api/radiologi/orders/{id}/status",
            post(fase3_handlers::set_rad_order_status),
        )
        .route(
            "/api/casemix",
            get(fase3_handlers::list_casemix),
        )
        .route(
            "/api/casemix/group",
            post(fase3_handlers::group_casemix),
        )
        .route(
            "/api/casemix/{id}",
            get(fase3_handlers::get_casemix_claim),
        )
        .route(
            "/api/casemix/{id}/export",
            get(fase3_handlers::export_casemix_claim),
        )
        // -- Ekosistem Fase 4 (FASE4.md §5.1) ------------------------------
        .route("/api/hemodialisa", get(fase4_handlers::list_hemodialisa))
        .route(
            "/api/hemodialisa/sessions",
            post(fase4_handlers::create_hd_session),
        )
        .route(
            "/api/hemodialisa/sessions/{id}/status",
            post(fase4_handlers::set_hd_session_status),
        )
        .route(
            "/api/transfusi/stock",
            get(fase4_handlers::list_blood_stock),
        )
        .route(
            "/api/transfusi/requests",
            get(fase4_handlers::list_blood_requests).post(fase4_handlers::create_blood_request),
        )
        .route(
            "/api/transfusi/requests/{id}/crossmatch",
            post(fase4_handlers::add_crossmatch),
        )
        .route(
            "/api/transfusi/requests/{id}/status",
            post(fase4_handlers::set_blood_request_status),
        )
        .route(
            "/api/gizi/assessments",
            get(fase4_handlers::list_nutrition_assessments)
                .post(fase4_handlers::create_nutrition_assessment),
        )
        .route(
            "/api/gizi/diets",
            get(fase4_handlers::list_nutrition_diets).post(fase4_handlers::create_nutrition_diet),
        )
        .route(
            "/api/gizi/diets/{id}/status",
            post(fase4_handlers::set_nutrition_diet_status),
        )
        .route(
            "/api/gigi/records",
            get(fase4_handlers::list_dental_records).post(fase4_handlers::create_dental_record),
        )
        .route(
            "/api/gigi/records/{id}/status",
            post(fase4_handlers::set_dental_record_status),
        )
        .route(
            "/api/inventori/items",
            get(fase4_handlers::list_inventory_items).post(fase4_handlers::create_inventory_item),
        )
        .route(
            "/api/inventori/moves",
            get(fase4_handlers::list_inventory_moves),
        )
        .route(
            "/api/inventori/items/{id}/moves",
            post(fase4_handlers::add_inventory_move),
        )
        .route("/api/kepegawaian/staff", get(fase4_handlers::list_staff).post(fase4_handlers::create_staff))
        .route(
            "/api/kepegawaian/staff/{id}/active",
            post(fase4_handlers::set_staff_active),
        )
        .route(
            "/api/kepegawaian/shifts",
            get(fase4_handlers::list_shifts).post(fase4_handlers::create_shift),
        )
        .route(
            "/api/laporan/kunjungan",
            get(fase4_handlers::report_visits),
        )
        .route(
            "/api/laporan/penyakit",
            get(fase4_handlers::report_top_diseases),
        )
        .route("/api/laporan/sirs", get(fase4_handlers::report_sirs))
        .route("/api/pengaturan", get(fase4_handlers::list_settings))
        .route(
            "/api/pengaturan/{key}",
            put(fase4_handlers::set_setting),
        )
        .route(
            "/api/rawat-jalan",
            get(fase4_handlers::list_outpatient_visits),
        )
        // -- API publik Fase 4 (FASE4.md §5.2, agen D) ---------------------
        // `public_api.rs` milik agen D; baris merge ini ditambah agen B
        // sesuai §5.4 walau file-nya belum ada saat build ini — koordinator
        // menyelaraskan di integrasi.
        .merge(public_api::public_router())
        // -- Kunci API + Portal Pasien Fase 4 (FASE4.md §5.2, agen D) -------
        .route(
            "/api/api-keys",
            get(api_key_handlers::list_api_keys).post(api_key_handlers::create_api_key),
        )
        .route(
            "/api/api-keys/{id}/revoke",
            post(api_key_handlers::revoke_api_key),
        )
        .route("/api/auth/patient-login", post(portal_handlers::patient_login))
        .route("/api/portal/me", get(portal_handlers::portal_me))
        .route(
            "/api/portal/appointments",
            get(portal_handlers::portal_appointments),
        )
        .route("/api/portal/queue", get(portal_handlers::portal_queue))
        .route("/api/portal/lab", get(portal_handlers::portal_lab))
        .route(
            "/api/portal/prescriptions",
            get(portal_handlers::portal_prescriptions),
        )
        // -- UI statis (waras-web, rust-embed) ----------------------------
        .route("/", get(web::serve_index))
        .route("/index.html", get(web::serve_index))
        .route("/dashboard", get(web::serve_dashboard))
        .route("/registrasi", get(web::serve_registrasi))
        .route("/rme", get(web::serve_rme))
        .route("/farmasi", get(web::serve_farmasi))
        .route("/billing", get(web::serve_billing))
        // -- UI Fase 2 (FASE2.md §6) --------------------------------------
        .route("/satusehat", get(web::serve_satusehat))
        .route("/bpjs", get(web::serve_bpjs))
        .route("/antrean-online", get(web::serve_antrean))
        // -- UI Fase 3 (FASE3.md §5) --------------------------------------
        .route("/igd", get(web::serve_igd))
        .route("/ranap", get(web::serve_ranap))
        .route("/operasi", get(web::serve_operasi))
        .route("/lab", get(web::serve_lab))
        .route("/radiologi", get(web::serve_radiologi))
        .route("/casemix", get(web::serve_casemix))
        // -- UI Fase 4 (FASE4.md §5.3) --------------------------------------
        .route("/hemodialisa", get(web::serve_hemodialisa))
        .route("/transfusi", get(web::serve_transfusi))
        .route("/gizi", get(web::serve_gizi))
        .route("/gigi", get(web::serve_gigi))
        .route("/portal", get(web::serve_portal))
        .route("/api-publik", get(web::serve_api_publik))
        .route("/inventori", get(web::serve_inventori))
        .route("/kepegawaian", get(web::serve_kepegawaian))
        .route("/laporan", get(web::serve_laporan))
        .route("/pengaturan", get(web::serve_pengaturan))
        .route("/rawat-jalan", get(web::serve_rawat_jalan))
        .route("/assets/{*path}", get(web::serve_assets))
        .route("/css/{*path}", get(web::serve_css))
        .route("/js/{*path}", get(web::serve_js))
        .route("/icons/{*path}", get(web::serve_icons))
        .route("/waras-tile-sharp.png", get(web::serve_tile))
        // -- Fallback: API tidak dikenal -> JSON 404; selain itu -> `/`.
        .fallback(web::fallback)
        // -- Header keamanan (Fase A/A4, `tower-http` SetHeader):
        // `X-Content-Type-Options: nosniff` (cegah sniffing MIME),
        // `X-Frame-Options: DENY` (cegah clickjacking — UI tidak pernah
        // di-iframe), `Referrer-Policy: strict-origin-when-cross-origin`
        // (jangan bocorkan path lengkap ke situs lain). Diterapkan ke
        // SEMUA respons (API + aset statis + fallback). Sebelum A4,
        // server tidak mengirim satu pun header ini (dicek lewat curl
        // saat verifikasi — lihat docs/FASE-A-A4.md).
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .with_state(state)
}
