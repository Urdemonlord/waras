//! Handler API Fase 3 (Rumah Sakit) — FASE3.md §5.
//!
//! Pola mengikuti `handlers.rs` / `bridge_handlers.rs`: `AuthUser` di
//! semua endpoint, guard RBAC lewat `auth::require` sebelum menyentuh DB,
//! aksi tulis dicatat ke audit trail (best-effort), daftar sebagai array
//! JSON langsung, detail sebagai objek gabungan, error sebagai
//! `{ "error": { "code", "message" } }` via `ApiError`.
//!
//! Dua hal yang SENGAJA tidak ada di sini (FASE3.md §7): tidak ada
//! endpoint submit E-Klaim; `GET /api/casemix/{id}/export` hanya
//! mengembalikan JSON berkas klaim untuk disalin ke aplikasi E-Klaim.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::Deserialize;
use serde_json::{json, Value};
use waras_core::{
    casemix::group_casemix as core_group_casemix,
    rbac::Action,
    types::{
        Admission, AdmissionStatus, BedStatus, CareNote, ConditionGroup, Delivery, IgdObservation,
        IgdStatus, IgdVisit, LabOrder, LabOrderStatus, RadOrder, RadOrderStatus, Surgery,
        SurgeryStatus, TriageLevel, WardBed,
    },
};
use waras_db::{clinic_today, Db, NewLabResult};

use crate::{
    auth::{require, AuthUser, JsonBody},
    error::ApiError,
    AppState,
};

type ApiResult<T> = Result<T, ApiError>;

async fn audit(
    db: &Db,
    actor: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    details: Value,
) {
    let _ = db
        .log_audit(actor, action, resource_type, resource_id, details)
        .await;
}

fn non_empty(v: &str, field: &str) -> ApiResult<()> {
    if v.trim().is_empty() {
        return Err(ApiError::validation(format!("{field} wajib diisi")));
    }
    Ok(())
}

fn parse_date_param(s: &str) -> ApiResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::validation(format!("tanggal tidak valid: {s}")))
}

/// Tanggal dari query `?date=`, default tanggal klinik (WARAS_TODAY).
fn date_query(q: &HashMap<String, String>) -> ApiResult<NaiveDate> {
    match q.get("date") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s),
        _ => Ok(clinic_today()),
    }
}

/// Normalisasi input enum: huruf kecil, hanya alfanumerik (gaya
/// `parse.rs`: terima `snake_case`, label Indonesia, maupun nama varian).
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn parse_triage_level(s: &str) -> Option<TriageLevel> {
    match key(s).as_str() {
        "merah" => Some(TriageLevel::Merah),
        "kuning" => Some(TriageLevel::Kuning),
        "hijau" => Some(TriageLevel::Hijau),
        _ => None,
    }
}

fn parse_igd_status(s: &str) -> Option<IgdStatus> {
    match key(s).as_str() {
        "menunggu" => Some(IgdStatus::Menunggu),
        "observasi" => Some(IgdStatus::Observasi),
        "tindakan" => Some(IgdStatus::Tindakan),
        "resusitasi" => Some(IgdStatus::Resusitasi),
        "selesai" => Some(IgdStatus::Selesai),
        "dirujuk" => Some(IgdStatus::Dirujuk),
        _ => None,
    }
}

fn parse_admission_status(s: &str) -> Option<AdmissionStatus> {
    match key(s).as_str() {
        "dirawat" => Some(AdmissionStatus::Dirawat),
        "pulang" => Some(AdmissionStatus::Pulang),
        "dirujuk" => Some(AdmissionStatus::Dirujuk),
        _ => None,
    }
}

fn parse_surgery_status(s: &str) -> Option<SurgeryStatus> {
    match key(s).as_str() {
        "terjadwal" => Some(SurgeryStatus::Terjadwal),
        "menunggu" => Some(SurgeryStatus::Menunggu),
        "siap" => Some(SurgeryStatus::Siap),
        "berjalan" => Some(SurgeryStatus::Berjalan),
        "selesai" => Some(SurgeryStatus::Selesai),
        "batal" => Some(SurgeryStatus::Batal),
        _ => None,
    }
}

fn parse_lab_order_status(s: &str) -> Option<LabOrderStatus> {
    match key(s).as_str() {
        "menunggu" => Some(LabOrderStatus::Menunggu),
        "proses" => Some(LabOrderStatus::Proses),
        "selesai" => Some(LabOrderStatus::Selesai),
        "kritis" => Some(LabOrderStatus::Kritis),
        _ => None,
    }
}

fn parse_rad_order_status(s: &str) -> Option<RadOrderStatus> {
    match key(s).as_str() {
        "menunggu" => Some(RadOrderStatus::Menunggu),
        "proses" => Some(RadOrderStatus::Proses),
        "selesai" => Some(RadOrderStatus::Selesai),
        _ => None,
    }
}

/// Bentuk DB (snake_case) untuk filter `?status=` pada daftar. Input
/// yang tidak dikenal -> 400, seperti filter status di `handlers.rs`.
fn canonical_status_filter(s: &str, allowed: &[&str]) -> ApiResult<String> {
    let k = key(s);
    allowed
        .iter()
        .find(|a| key(a) == k)
        .map(|a| a.to_string())
        .ok_or_else(|| ApiError::validation(format!("status tidak dikenal: {s}")))
}

/// 422 `{error:{code:"VALIDATION"}}` (gaya `bridge_handlers.rs`):
/// dipakai untuk penolakan kontrak yang HARUS 422 (bed/ruang bukan
/// Kosong, LOINC tidak dikenal, diagnosis belum terpetakan) — bukan
/// `ApiError::validation` yang memetakan ke 400.
fn unprocessable(msg: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"error": {"code": "VALIDATION", "message": msg}})),
    )
}

/// Cross-check pasien salah (FASE3.md §6): bila `encounter_id` diisi,
/// encounter itu harus milik `patient_id`, kalau tidak
/// `Db::verify_encounter_patient` mengembalikan 422 WRONG_PATIENT.
/// Bila `encounter_id` kosong -> tidak ada yang dicocokkan.
async fn verify_encounter(db: &Db, encounter_id: Option<&str>, patient_id: &str) -> ApiResult<()> {
    if let Some(enc) = encounter_id.map(str::trim).filter(|s| !s.is_empty()) {
        db.verify_encounter_patient(enc, patient_id).await?;
    }
    Ok(())
}

async fn patient_must_exist(db: &Db, patient_id: &str) -> ApiResult<()> {
    db.get_patient(patient_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("pasien {patient_id} tidak ditemukan")))?;
    Ok(())
}

async fn patient_name(db: &Db, patient_id: &str) -> ApiResult<Option<String>> {
    Ok(db.get_patient(patient_id).await?.map(|p| p.name))
}

/// Jam sekarang `HH.MM` (format jam desain) untuk `arrived_at` triase.
fn now_time_str() -> String {
    Local::now().format("%H.%M").to_string()
}

// ---------------------------------------------------------------------------
// LOINC (FASE3.md §2) — tabel panel lab, dipakai mengisi `loinc` order
// bila kosong / tidak dikenal. Disalin dari kontrak supaya handler tidak
// bergantung pada nama konstanta core yang belum ditetapkan di §2.
// ---------------------------------------------------------------------------

fn loinc_for_test(test_name: &str) -> Option<&'static str> {
    match test_name.trim().to_lowercase().as_str() {
        "darah lengkap" => Some("58410-2"),
        "troponin i" => Some("10839-9"),
        "hba1c" => Some("4548-4"),
        "urinalisis lengkap" => Some("24357-6"),
        "golongan darah" => Some("883-9"),
        "elektrolit (na/k)" | "elektrolit" => Some("2951-2"),
        "gula darah puasa" => Some("1558-6"),
        _ => None,
    }
}

fn is_known_loinc(loinc: &str) -> bool {
    matches!(
        loinc.trim(),
        "58410-2" | "10839-9" | "4548-4" | "24357-6" | "883-9" | "2951-2" | "1558-6"
    )
}

/// `loinc` yang diberikan dipakai hanya bila dikenal di panel; bila
/// kosong / tidak dikenal, isi dari nama tes; bila tes tidak dikenal
/// -> `None` (handler menjawab 422 "LOINC tidak dikenal").
fn resolve_loinc(test_name: &str, loinc: Option<&str>) -> Option<String> {
    if let Some(l) = loinc.map(str::trim).filter(|s| !s.is_empty()) {
        if is_known_loinc(l) {
            return Some(l.to_string());
        }
    }
    loinc_for_test(test_name).map(str::to_string)
}

// ---------------------------------------------------------------------------
// Gerbang 422 casemix: diagnosis utama encounter harus terpetakan oleh
// grouper lokal core (`waras_core::casemix::group_casemix`, tabel CONTOH
// FASE3.md §2, bukan tarif resmi). Handler mengecek dulu lewat grouper
// yang sama dengan DB, supaya jawabannya pasti 422 "belum terpetakan,
// perbaiki pengkodean" (JANGAN mengarang kode). Kode + tarif di respons
// SELALU diambil dari klaim yang dibuat DB (`ina_cbg_code` / `amount`).
// ---------------------------------------------------------------------------

/// Kode diagnosis utama (grup Utama) encounter, bila ada.
async fn primary_diagnosis_code(db: &Db, encounter_id: &str) -> ApiResult<Option<String>> {
    let encounter = db
        .get_encounter(encounter_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {encounter_id} tidak ditemukan")))?;
    let conditions = db.get_conditions(&encounter.patient_id).await?;
    Ok(conditions
        .into_iter()
        .find(|c| {
            c.encounter_id.as_deref() == Some(encounter_id) && c.group == ConditionGroup::Utama
        })
        .map(|c| c.code))
}

// ---------------------------------------------------------------------------
// IGD
// ---------------------------------------------------------------------------

pub async fn list_igd(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let date_str = date.to_string();
    let (merah, kuning, hijau) = state.db.igd_counts(&date_str).await?;
    let visits = state.db.list_igd(&date_str).await?;
    Ok(Json(json!({
        "counts": {"merah": merah, "kuning": kuning, "hijau": hijau},
        "visits": visits,
    })))
}

#[derive(Deserialize)]
pub struct TriageDto {
    pub patient_id: String,
    pub complaint: String,
    pub triage: String,
    pub doctor: String,
    pub encounter_id: Option<String>,
}

pub async fn create_triage(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<TriageDto>,
) -> ApiResult<(StatusCode, Json<IgdVisit>)> {
    require(&user, Action::TriaseIgd)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.complaint, "complaint")?;
    non_empty(&body.doctor, "doctor")?;
    let triage = parse_triage_level(&body.triage)
        .ok_or_else(|| ApiError::validation(format!("triase tidak dikenal: {}", body.triage)))?;
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    let visit = state
        .db
        .create_igd_triage(
            body.patient_id.trim(),
            body.complaint.trim(),
            triage,
            body.doctor.trim(),
            &now_time_str(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "igd.triage_created",
        "igd_visit",
        &visit.id,
        json!({"patient_id": visit.patient_id, "triage": body.triage}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(visit)))
}

pub async fn get_igd(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let visit = state
        .db
        .get_igd(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("kunjungan IGD {id} tidak ditemukan")))?;
    let observations = state.db.list_igd_observations(&id).await?;
    let mut v = serde_json::to_value(&visit)?;
    v["observations"] = serde_json::to_value(&observations)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct IgdStatusDto {
    pub status: String,
}

pub async fn set_igd_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<IgdStatusDto>,
) -> ApiResult<Json<IgdVisit>> {
    require(&user, Action::TriaseIgd)?;
    let status = parse_igd_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!("status IGD tidak dikenal: {}", body.status))
    })?;
    state
        .db
        .get_igd(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("kunjungan IGD {id} tidak ditemukan")))?;
    state.db.set_igd_status(&id, status).await?;
    let visit = state
        .db
        .get_igd(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("kunjungan IGD {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "igd.status_changed",
        "igd_visit",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(visit))
}

#[derive(Deserialize)]
pub struct IgdObservationDto {
    pub note: String,
}

pub async fn add_igd_observation(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<IgdObservationDto>,
) -> ApiResult<(StatusCode, Json<IgdObservation>)> {
    require(&user, Action::TriaseIgd)?;
    non_empty(&body.note, "note")?;
    state
        .db
        .get_igd(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("kunjungan IGD {id} tidak ditemukan")))?;
    let obs = state
        .db
        .add_igd_observation(&id, body.note.trim(), &user.name)
        .await?;
    audit(
        &state.db,
        &user.name,
        "igd.observation_added",
        "igd_observation",
        &obs.id,
        json!({"igd_id": id}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(obs)))
}

// ---------------------------------------------------------------------------
// Rawat inap
// ---------------------------------------------------------------------------

pub async fn list_beds(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let ward_filter = q
        .get("ward")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let wards = state.db.list_wards().await?;
    let all_beds = state.db.list_ward_beds(None).await?;
    let mut out = Vec::new();
    for w in &wards {
        if let Some(f) = &ward_filter {
            if &w.id != f {
                continue;
            }
        }
        let beds: Vec<&WardBed> = all_beds.iter().filter(|b| b.ward_id == w.id).collect();
        out.push(json!({
            "ward_id": w.id,
            "ward_name": w.name,
            "kind": w.kind,
            "beds": beds,
        }));
    }
    Ok(Json(json!(out)))
}

async fn enrich_admission(state: &AppState, adm: &Admission) -> ApiResult<Value> {
    let patient_name = patient_name(&state.db, &adm.patient_id).await?;
    let bed = state.db.get_ward_bed(&adm.bed_id).await?;
    let mut v = serde_json::to_value(adm)?;
    v["patient_name"] = serde_json::to_value(&patient_name)?;
    v["bed"] = serde_json::to_value(&bed)?;
    Ok(v)
}

pub async fn list_admissions(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => {
            // Validasi dulu sebagai enum supaya "Dirawat"/"dirawat"/
            // "di_rawat" sama-sama diterima, lalu kirim bentuk DB.
            parse_admission_status(s)
                .ok_or_else(|| ApiError::validation(format!("status admisi tidak dikenal: {s}")))?;
            Some(canonical_status_filter(
                s,
                &["dirawat", "pulang", "dirujuk"],
            )?)
        }
        _ => None,
    };
    let admissions = state.db.list_admissions(status.as_deref()).await?;
    let mut out = Vec::with_capacity(admissions.len());
    for adm in &admissions {
        out.push(enrich_admission(&state, adm).await?);
    }
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
pub struct CreateAdmissionDto {
    pub patient_id: String,
    pub bed_id: String,
    pub encounter_id: Option<String>,
    pub diagnosis_code: String,
    pub diagnosis_display: String,
    pub dpjp: String,
    #[serde(default)]
    pub fall_risk: bool,
}

pub async fn create_admission(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateAdmissionDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaRawatInap)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.bed_id, "bed_id")?;
    non_empty(&body.diagnosis_code, "diagnosis_code")?;
    non_empty(&body.diagnosis_display, "diagnosis_display")?;
    non_empty(&body.dpjp, "dpjp")?;
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    // 422 (kontrak §5) bila bed bukan Kosong — dicek di sini supaya
    // jawabannya pasti 422 apa pun varian error Db::create_admission.
    let bed = state
        .db
        .get_ward_bed(body.bed_id.trim())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("bed {} tidak ditemukan", body.bed_id)))?;
    if bed.status != BedStatus::Kosong {
        return Ok(unprocessable(&format!(
            "bed {} bukan Kosong, tidak bisa dipakai admisi",
            bed.id
        )));
    }
    let admission = state
        .db
        .create_admission(
            body.patient_id.trim(),
            body.bed_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.diagnosis_code.trim(),
            body.diagnosis_display.trim(),
            body.dpjp.trim(),
            body.fall_risk,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "ranap.admission_created",
        "admission",
        &admission.id,
        json!({"patient_id": admission.patient_id, "bed_id": admission.bed_id}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(enrich_admission(&state, &admission).await?),
    ))
}

pub async fn get_admission(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let admission = state
        .db
        .get_admission(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("admisi {id} tidak ditemukan")))?;
    let notes = state.db.list_care_notes(&id).await?;
    let mut v = enrich_admission(&state, &admission).await?;
    v["care_notes"] = serde_json::to_value(&notes)?;
    Ok(Json(v))
}

pub async fn discharge_admission(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::KelolaRawatInap)?;
    state
        .db
        .get_admission(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("admisi {id} tidak ditemukan")))?;
    // Db::discharge_admission: status Pulang + bed Dibersihkan.
    let admission = state.db.discharge_admission(&id, &user.name).await?;
    audit(
        &state.db,
        &user.name,
        "ranap.discharged",
        "admission",
        &id,
        json!({"status": "pulang"}),
    )
    .await;
    Ok(Json(enrich_admission(&state, &admission).await?))
}

#[derive(Deserialize)]
pub struct TransferDto {
    pub bed_id: String,
}

pub async fn transfer_admission(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<TransferDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaRawatInap)?;
    non_empty(&body.bed_id, "bed_id")?;
    state
        .db
        .get_admission(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("admisi {id} tidak ditemukan")))?;
    // 422 (kontrak §5) bila bed tujuan bukan Kosong.
    let bed = state
        .db
        .get_ward_bed(body.bed_id.trim())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("bed {} tidak ditemukan", body.bed_id)))?;
    if bed.status != BedStatus::Kosong {
        return Ok(unprocessable(&format!(
            "bed {} bukan Kosong, tidak bisa dipakai mutasi",
            bed.id
        )));
    }
    // Db::transfer_admission: bed lama Dibersihkan, bed baru Terisi.
    let admission = state
        .db
        .transfer_admission(&id, body.bed_id.trim(), &user.name)
        .await?;
    audit(
        &state.db,
        &user.name,
        "ranap.transferred",
        "admission",
        &id,
        json!({"bed_id": admission.bed_id}),
    )
    .await;
    Ok((
        StatusCode::OK,
        Json(enrich_admission(&state, &admission).await?),
    ))
}

#[derive(Deserialize)]
pub struct CareNoteDto {
    pub note: String,
}

pub async fn add_care_note(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<CareNoteDto>,
) -> ApiResult<(StatusCode, Json<CareNote>)> {
    require(&user, Action::KelolaRawatInap)?;
    non_empty(&body.note, "note")?;
    state
        .db
        .get_admission(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("admisi {id} tidak ditemukan")))?;
    let note = state
        .db
        .add_care_note(&id, body.note.trim(), &user.name)
        .await?;
    audit(
        &state.db,
        &user.name,
        "ranap.care_note_added",
        "care_note",
        &note.id,
        json!({"admission_id": id}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(note)))
}

// ---------------------------------------------------------------------------
// Kamar operasi & bersalin
// ---------------------------------------------------------------------------

pub async fn list_operasi(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let surgeries = state.db.list_surgeries(&date.to_string()).await?;
    let deliveries = state.db.list_deliveries().await?;
    let delivery_rooms = state.db.list_delivery_rooms().await?;
    Ok(Json(json!({
        "surgeries": surgeries,
        "deliveries": deliveries,
        "delivery_rooms": delivery_rooms,
    })))
}

#[derive(Deserialize)]
pub struct CreateSurgeryDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub room: String,
    pub procedure: String,
    pub procedure_code: String,
    pub operator: String,
    pub team: Option<String>,
    pub scheduled_at: String,
}

fn valid_scheduled_at(s: &str) -> bool {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").is_ok()
        || NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").is_ok()
}

pub async fn create_surgery(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateSurgeryDto>,
) -> ApiResult<(StatusCode, Json<Surgery>)> {
    require(&user, Action::KelolaOperasi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.room, "room")?;
    non_empty(&body.procedure, "procedure")?;
    non_empty(&body.procedure_code, "procedure_code")?;
    non_empty(&body.operator, "operator")?;
    non_empty(&body.scheduled_at, "scheduled_at")?;
    if !valid_scheduled_at(body.scheduled_at.trim()) {
        return Err(ApiError::validation(
            "scheduled_at harus format YYYY-MM-DD HH:MM",
        ));
    }
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    let surgery = state
        .db
        .create_surgery(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.room.trim(),
            body.procedure.trim(),
            body.procedure_code.trim(),
            body.operator.trim(),
            body.team.as_deref().unwrap_or(""),
            body.scheduled_at.trim(),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "operasi.created",
        "surgery",
        &surgery.id,
        json!({"patient_id": surgery.patient_id, "procedure": surgery.procedure}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(surgery)))
}

#[derive(Deserialize)]
pub struct SurgeryStatusDto {
    pub status: String,
}

pub async fn set_surgery_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<SurgeryStatusDto>,
) -> ApiResult<Json<Surgery>> {
    require(&user, Action::KelolaOperasi)?;
    let status = parse_surgery_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!("status operasi tidak dikenal: {}", body.status))
    })?;
    state
        .db
        .get_surgery(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("operasi {id} tidak ditemukan")))?;
    state.db.set_surgery_status(&id, status).await?;
    let surgery = state
        .db
        .get_surgery(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("operasi {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "operasi.status_changed",
        "surgery",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(surgery))
}

#[derive(Deserialize)]
pub struct CreateDeliveryDto {
    pub patient_id: String,
    pub age_years: i64,
    pub gravida_para: String,
    pub dilation_cm: i64,
    pub fetal_heart_rate: i64,
    pub midwife: String,
    pub doctor: String,
    pub plan: String,
    pub room: String,
}

pub async fn create_delivery(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateDeliveryDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaOperasi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.gravida_para, "gravida_para")?;
    non_empty(&body.midwife, "midwife")?;
    non_empty(&body.doctor, "doctor")?;
    non_empty(&body.plan, "plan")?;
    non_empty(&body.room, "room")?;
    if body.age_years <= 0 {
        return Err(ApiError::validation("age_years harus lebih dari 0"));
    }
    if body.dilation_cm < 0 || body.dilation_cm > 10 {
        return Err(ApiError::validation("dilation_cm harus 0-10"));
    }
    if body.fetal_heart_rate <= 0 {
        return Err(ApiError::validation("fetal_heart_rate harus lebih dari 0"));
    }
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    // 422 (kontrak §5) bila ruang bersalin bukan Kosong.
    let rooms = state.db.list_delivery_rooms().await?;
    let room = rooms
        .iter()
        .find(|r| r.id == body.room.trim())
        .ok_or_else(|| {
            ApiError::not_found(format!("ruang bersalin {} tidak ditemukan", body.room))
        })?;
    if room.status != BedStatus::Kosong {
        return Ok(unprocessable(&format!(
            "ruang bersalin {} bukan Kosong, tidak bisa dipakai persalinan",
            room.id
        )));
    }
    let delivery = state
        .db
        .create_delivery(
            body.patient_id.trim(),
            body.age_years,
            body.gravida_para.trim(),
            body.dilation_cm,
            body.fetal_heart_rate,
            body.midwife.trim(),
            body.doctor.trim(),
            body.plan.trim(),
            body.room.trim(),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "bersalin.created",
        "delivery",
        &delivery.id,
        json!({"patient_id": delivery.patient_id, "room": delivery.room}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(json!(delivery))))
}

pub async fn list_bersalin(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Value>> {
    let deliveries = state.db.list_deliveries().await?;
    let rooms = state.db.list_delivery_rooms().await?;
    Ok(Json(json!({
        "deliveries": deliveries,
        "rooms": rooms,
    })))
}

#[derive(Deserialize)]
pub struct DeliveryStatusDto {
    pub status: String,
}

pub async fn set_delivery_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DeliveryStatusDto>,
) -> ApiResult<Json<Delivery>> {
    require(&user, Action::KelolaOperasi)?;
    // Db::set_delivery_status menerima &str dan membandingkan
    // case-insensitive, tapi nilai yang TERSIMPAN dipakai apa adanya —
    // seed + create_delivery memakai bentuk tampilan ("Aktif" /
    // "Selesai" / "Dirujuk", FASE3.md §1), jadi kirim bentuk itu supaya
    // nilainya konsisten, bukan snake_case.
    let status = match canonical_status_filter(&body.status, &["aktif", "selesai", "dirujuk"])? {
        s if s == "aktif" => "Aktif",
        s if s == "selesai" => "Selesai",
        _ => "Dirujuk",
    };
    state
        .db
        .get_delivery(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("persalinan {id} tidak ditemukan")))?;
    // Selesai -> ruang kembali Kosong (diurus DB).
    state.db.set_delivery_status(&id, status).await?;
    let delivery = state
        .db
        .get_delivery(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("persalinan {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "bersalin.status_changed",
        "delivery",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(delivery))
}

// ---------------------------------------------------------------------------
// Laboratorium
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LabResultDto {
    pub analyte: String,
    pub loinc: Option<String>,
    pub value_text: String,
    pub unit: Option<String>,
    pub ref_text: Option<String>,
    pub ref_low: Option<f64>,
    pub ref_high: Option<f64>,
    pub critical_high: Option<f64>,
    pub critical_low: Option<f64>,
}

impl LabResultDto {
    fn into_new(self) -> NewLabResult {
        NewLabResult {
            analyte: self.analyte,
            loinc: self.loinc.filter(|s| !s.trim().is_empty()),
            value_text: self.value_text,
            unit: self.unit.unwrap_or_default(),
            ref_text: self.ref_text.unwrap_or_default(),
            ref_low: self.ref_low,
            ref_high: self.ref_high,
            critical_high: self.critical_high,
            critical_low: self.critical_low,
        }
    }
}

#[derive(Deserialize)]
pub struct AddLabResultsDto {
    pub results: Vec<LabResultDto>,
}

#[derive(Deserialize)]
pub struct CreateLabOrderDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub test_name: String,
    pub loinc: Option<String>,
    pub ordered_from: String,
}

async fn enrich_lab_order(state: &AppState, order: &LabOrder) -> ApiResult<Value> {
    let patient_name = patient_name(&state.db, &order.patient_id).await?;
    let mut v = serde_json::to_value(order)?;
    v["patient_name"] = serde_json::to_value(&patient_name)?;
    Ok(v)
}

pub async fn list_lab_orders(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => {
            parse_lab_order_status(s).ok_or_else(|| {
                ApiError::validation(format!("status order lab tidak dikenal: {s}"))
            })?;
            Some(canonical_status_filter(
                s,
                &["menunggu", "proses", "selesai", "kritis"],
            )?)
        }
        _ => None,
    };
    let orders = state
        .db
        .list_lab_orders(&date.to_string(), status.as_deref())
        .await?;
    let mut out = Vec::with_capacity(orders.len());
    for o in &orders {
        out.push(enrich_lab_order(&state, o).await?);
    }
    Ok(Json(json!(out)))
}

pub async fn create_lab_order(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateLabOrderDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::PesanLab)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.test_name, "test_name")?;
    non_empty(&body.ordered_from, "ordered_from")?;
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    // `loinc` kosong / tidak dikenal -> isi dari tabel LOINC §2 bila
    // tes dikenal, bila tidak 422 "LOINC tidak dikenal".
    let Some(loinc) = resolve_loinc(&body.test_name, body.loinc.as_deref()) else {
        return Ok(unprocessable(&format!(
            "LOINC tidak dikenal untuk tes {}",
            body.test_name
        )));
    };
    let order = state
        .db
        .create_lab_order(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.test_name.trim(),
            &loinc,
            body.ordered_from.trim(),
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "lab.order_created",
        "lab_order",
        &order.id,
        json!({"patient_id": order.patient_id, "test_name": order.test_name}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(enrich_lab_order(&state, &order).await?),
    ))
}

pub async fn get_lab_order(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let order = state
        .db
        .get_lab_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order lab {id} tidak ditemukan")))?;
    let results = state.db.list_lab_results(&id).await?;
    let mut v = enrich_lab_order(&state, &order).await?;
    v["results"] = serde_json::to_value(&results)?;
    Ok(Json(v))
}

pub async fn add_lab_results(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<AddLabResultsDto>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::HasilLab)?;
    if body.results.is_empty() {
        return Err(ApiError::validation(
            "results wajib berisi minimal satu analit",
        ));
    }
    for r in &body.results {
        non_empty(&r.analyte, "analyte")?;
        non_empty(&r.value_text, "value_text")?;
    }
    state
        .db
        .get_lab_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order lab {id} tidak ditemukan")))?;
    let inputs: Vec<NewLabResult> = body
        .results
        .into_iter()
        .map(LabResultDto::into_new)
        .collect();
    let count = inputs.len();
    // Db::add_lab_results menghitung flag per analit (core::lab_flag);
    // order -> Kritis bila ada analit Kritis (plus notifikasi KRITIS +
    // audit `lab.critical`), else Selesai.
    let order = state.db.add_lab_results(&id, &inputs, &user.name).await?;
    let results = state.db.list_lab_results(&id).await?;
    let mut v = enrich_lab_order(&state, &order).await?;
    v["results"] = serde_json::to_value(&results)?;
    audit(
        &state.db,
        &user.name,
        "lab.results_added",
        "lab_order",
        &id,
        json!({"count": count}),
    )
    .await;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct LabOrderStatusDto {
    pub status: String,
}

pub async fn set_lab_order_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<LabOrderStatusDto>,
) -> ApiResult<Json<LabOrder>> {
    require(&user, Action::HasilLab)?;
    let status = parse_lab_order_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!("status order lab tidak dikenal: {}", body.status))
    })?;
    state
        .db
        .get_lab_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order lab {id} tidak ditemukan")))?;
    // CATATAN: FASE3.md §5 memakai endpoint ini, tapi §4 tidak
    // mencantumkan method-nya — dipanggil dengan nama/signature yang
    // mengikuti set_igd_status / set_rad_order_status. Bila agen A
    // menamainya lain, ini yang harus diselaraskan (lihat laporan agen B).
    state.db.set_lab_order_status(&id, status).await?;
    let order = state
        .db
        .get_lab_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order lab {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "lab.status_changed",
        "lab_order",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(order))
}

// ---------------------------------------------------------------------------
// Radiologi
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateRadOrderDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub modality: String,
    pub area: String,
    pub ordered_from: String,
}

async fn enrich_rad_order(state: &AppState, order: &RadOrder) -> ApiResult<Value> {
    let patient_name = patient_name(&state.db, &order.patient_id).await?;
    let mut v = serde_json::to_value(order)?;
    v["patient_name"] = serde_json::to_value(&patient_name)?;
    Ok(v)
}

pub async fn list_rad_orders(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => {
            parse_rad_order_status(s).ok_or_else(|| {
                ApiError::validation(format!("status order radiologi tidak dikenal: {s}"))
            })?;
            Some(canonical_status_filter(
                s,
                &["menunggu", "proses", "selesai"],
            )?)
        }
        _ => None,
    };
    let orders = state
        .db
        .list_rad_orders(&date.to_string(), status.as_deref())
        .await?;
    let mut out = Vec::with_capacity(orders.len());
    for o in &orders {
        out.push(enrich_rad_order(&state, o).await?);
    }
    Ok(Json(json!(out)))
}

pub async fn create_rad_order(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateRadOrderDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::PesanRadiologi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.modality, "modality")?;
    non_empty(&body.area, "area")?;
    non_empty(&body.ordered_from, "ordered_from")?;
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    let order = state
        .db
        .create_rad_order(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.modality.trim(),
            body.area.trim(),
            body.ordered_from.trim(),
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "radiologi.order_created",
        "rad_order",
        &order.id,
        json!({"patient_id": order.patient_id, "modality": order.modality}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(enrich_rad_order(&state, &order).await?),
    ))
}

pub async fn get_rad_order(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let order = state
        .db
        .get_rad_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order radiologi {id} tidak ditemukan")))?;
    let report = state.db.get_rad_report(&id).await?;
    let mut v = enrich_rad_order(&state, &order).await?;
    v["report"] = serde_json::to_value(&report)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct RadReportDto {
    pub findings: String,
    pub radiologist: String,
}

pub async fn save_rad_report(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RadReportDto>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::LaporanRadiologi)?;
    non_empty(&body.findings, "findings")?;
    non_empty(&body.radiologist, "radiologist")?;
    state
        .db
        .get_rad_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order radiologi {id} tidak ditemukan")))?;
    // Db::save_rad_report: upsert + order -> Selesai.
    let report = state
        .db
        .save_rad_report(&id, body.findings.trim(), body.radiologist.trim())
        .await?;
    let order = state
        .db
        .get_rad_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order radiologi {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "radiologi.report_saved",
        "rad_report",
        &report.id,
        json!({"order_id": id}),
    )
    .await;
    let mut v = serde_json::to_value(&report)?;
    v["order"] = serde_json::to_value(&order)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct RadOrderStatusDto {
    pub status: String,
}

pub async fn set_rad_order_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RadOrderStatusDto>,
) -> ApiResult<Json<RadOrder>> {
    require(&user, Action::PesanRadiologi)?;
    let status = parse_rad_order_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!(
            "status order radiologi tidak dikenal: {}",
            body.status
        ))
    })?;
    state
        .db
        .get_rad_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order radiologi {id} tidak ditemukan")))?;
    state.db.set_rad_order_status(&id, status).await?;
    let order = state
        .db
        .get_rad_order(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("order radiologi {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "radiologi.status_changed",
        "rad_order",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(order))
}

// ---------------------------------------------------------------------------
// Casemix (INA-CBG)
// ---------------------------------------------------------------------------

pub async fn list_casemix(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Value>> {
    let (total, terverifikasi, dibayar_idr, pending) = state.db.casemix_kpis().await?;
    let claims = state.db.list_casemix().await?;
    Ok(Json(json!({
        "kpis": {
            "total": total,
            "terverifikasi": terverifikasi,
            "dibayar_idr": dibayar_idr,
            "pending": pending,
        },
        "claims": claims,
    })))
}

#[derive(Deserialize)]
pub struct GroupCasemixDto {
    pub encounter_id: String,
}

pub async fn group_casemix(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<GroupCasemixDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::GroupingCasemix)?;
    non_empty(&body.encounter_id, "encounter_id")?;
    let encounter_id = body.encounter_id.trim();
    // Gerbang 422 (lihat komentar di atas): diagnosis utama encounter
    // harus terpetakan oleh grouper lokal; bila tidak, jawab 422 "belum
    // terpetakan, perbaiki pengkodean" — JANGAN mengarang kode.
    let diag = primary_diagnosis_code(&state.db, encounter_id).await?;
    if diag
        .as_deref()
        .and_then(|c| core_group_casemix(c, None))
        .is_none()
    {
        let msg = match &diag {
            Some(code) => {
                format!("diagnosis {code} belum terpetakan ke INA-CBG, perbaiki pengkodean")
            }
            None => format!("encounter {encounter_id} belum punya diagnosis utama"),
        };
        return Ok(unprocessable(&msg));
    }
    // Db::group_claim_for_encounter: buat klaim Draft baru (atau
    // perbarui Draft yang sudah ada untuk encounter itu).
    let claim = state
        .db
        .group_claim_for_encounter(encounter_id, &user.name)
        .await?;
    audit(
        &state.db,
        &user.name,
        "casemix.grouped",
        "claim",
        &claim.id,
        json!({"encounter_id": encounter_id, "ina_cbg_code": claim.ina_cbg_code}),
    )
    .await;
    let mut v = serde_json::to_value(&claim)?;
    v["code"] = serde_json::to_value(&claim.ina_cbg_code)?;
    v["tariff_idr"] = serde_json::to_value(&claim.amount)?;
    Ok((StatusCode::OK, Json(v)))
}

pub async fn get_casemix_claim(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let claim = state
        .db
        .get_claim(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("klaim {id} tidak ditemukan")))?;
    Ok(Json(json!(claim)))
}

pub async fn export_casemix_claim(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::GroupingCasemix)?;
    let claim = state
        .db
        .get_claim(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("klaim {id} tidak ditemukan")))?;
    // Berkas JSON untuk disalin ke aplikasi E-Klaim (FASE3.md §7):
    // grouping lokal (perkiraan) + persiapan berkas saja, WARAS tidak
    // mengirim langsung. TIDAK ada endpoint submit.
    let mut v = serde_json::to_value(&claim)?;
    v["code"] = serde_json::to_value(&claim.ina_cbg_code)?;
    v["tariff_idr"] = serde_json::to_value(&claim.amount)?;
    v["catatan_eklaim"] = json!(
        "Kirim lewat aplikasi E-Klaim; WARAS tidak mengirim langsung. \
         Grouping lokal (perkiraan), bukan grouper resmi."
    );
    audit(
        &state.db,
        &user.name,
        "casemix.exported",
        "claim",
        &id,
        json!({"ina_cbg_code": claim.ina_cbg_code}),
    )
    .await;
    Ok(Json(v))
}
