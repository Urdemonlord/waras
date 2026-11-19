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

pub mod auth;
pub mod error;
pub mod handlers;
pub mod parse;
pub mod web;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use waras_db::Db;

/// State aplikasi yang dibagikan ke semua handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
}

/// Router lengkap: API (`/api/...`) + UI statis dari `waras-web`.
pub fn router(state: AppState) -> Router {
    use handlers::*;
    Router::new()
        // -- Publik -------------------------------------------------------
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login))
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
        // -- UI statis (waras-web, rust-embed) ----------------------------
        .route("/", get(web::serve_index))
        .route("/index.html", get(web::serve_index))
        .route("/dashboard", get(web::serve_dashboard))
        .route("/registrasi", get(web::serve_registrasi))
        .route("/rme", get(web::serve_rme))
        .route("/farmasi", get(web::serve_farmasi))
        .route("/billing", get(web::serve_billing))
        .route("/assets/{*path}", get(web::serve_assets))
        .route("/css/{*path}", get(web::serve_css))
        .route("/js/{*path}", get(web::serve_js))
        .route("/icons/{*path}", get(web::serve_icons))
        .route("/waras-tile-sharp.png", get(web::serve_tile))
        // -- Fallback: API tidak dikenal -> JSON 404; selain itu -> `/`.
        .fallback(web::fallback)
        .with_state(state)
}
