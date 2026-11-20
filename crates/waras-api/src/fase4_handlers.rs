//! Handler API Fase 4 (Ekosistem) — FASE4.md §5.1.
//!
//! Pola mengikuti `fase3_handlers.rs`: `AuthUser` di semua endpoint,
//! guard RBAC lewat `auth::require` sebelum menyentuh DB, aksi tulis
//! dicatat ke audit trail (best-effort), daftar sebagai array JSON
//! langsung, error sebagai `{ "error": { "code", "message" } }` via
//! `ApiError`.
//!
//! CATATAN INTEGRASI (koordinator): file ini ditulis terhadap kontrak
//! FASE4.md §2/§4 sebelum agen A mendaratkan tipe core, aksi RBAC, dan
//! method `Db` Fase 4. Nama yang dipakai di sini mengikuti kontrak
//! persis; bila agen A menamainya lain, yang harus diselaraskan:
//! - `waras_core::types::{HdMachine, HdMachineStatus, HdSession,
//!   HdSessionStatus, BloodStock, BloodRequest, BloodRequestStatus,
//!   BloodCrossmatch, NutritionAssessment, NutritionDiet, DentalRecord,
//!   InventoryItem, InventoryMove, Staff, ShiftSchedule, AppSetting,
//!   SirsSummary, OutpatientVisit}` — struct per tabel FASE4.md §1,
//!   enum status dengan varian nama tampilan Indonesia (gaya Fase 3:
//!   `TriageLevel::Merah`, `IgdStatus::Menunggu`).
//! - `waras_core::rbac::Action::{ViewEkosistem, KelolaHemodialisa,
//!   KelolaTransfusi, KelolaGizi, KelolaGigi, KelolaInventori,
//!   KelolaKepegawaian, ViewLaporan, KelolaPengaturan}` (FASE4.md §3).
//! - Method `Db` Fase 4 (FASE4.md §4) dengan signature yang tertulis
//!   di sana; handler memanggil nama-nama itu persis.
//! - `stock_status` inventori memakai helper core
//!   `waras_core::types::inventory_stock_status` (ambang sama dengan
//!   Farmasi, FASE4.md §2).

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde::Deserialize;
use serde_json::{json, Value};
use waras_core::{
    rbac::Action,
    types::{
        inventory_stock_status, AppSetting, BloodCrossmatch, BloodRequest, BloodRequestStatus,
        BloodStock, DentalRecord, HdMachineStatus, HdSession, HdSessionStatus, InventoryMove,
        NutritionAssessment, NutritionDiet, OutpatientVisit, ShiftSchedule, SirsSummary, Staff,
    },
};
use waras_db::{clinic_today, Db};

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
/// `parse.rs` / `fase3_handlers.rs`: terima `snake_case`, label
/// Indonesia, maupun nama varian).
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 422 `{error:{code:"VALIDATION"}}` (gaya `fase3_handlers.rs`):
/// dipakai untuk penolakan kontrak yang HARUS 422 (mesin bukan Siap,
/// stok tidak cukup, shift ganda) — bukan `ApiError::validation` yang
/// memetakan ke 400.
fn unprocessable(msg: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"error": {"code": "VALIDATION", "message": msg}})),
    )
}

/// Cross-check pasien salah (FASE3.md §6, FASE4.md §5): bila
/// `encounter_id` diisi, encounter itu harus milik `patient_id`,
/// kalau tidak `Db::verify_encounter_patient` mengembalikan 422
/// WRONG_PATIENT. Bila `encounter_id` kosong -> tidak ada yang
/// dicocokkan.
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

/// Filter `?patient_id=` opsional (string kosong = tidak ada filter).
fn patient_filter(q: &HashMap<String, String>) -> Option<String> {
    q.get("patient_id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Status stok inventori memakai helper core yang sama dengan Farmasi
/// (`waras_core::types::inventory_stock_status`: <=12 Kritis, <=30
/// Segera, else Aman; FASE4.md §2). Label "Segera" di sini artinya
/// segera restock, bukan "segera hadir".
/// (Dipanggil langsung sebagai `inventory_stock_status(stock)` —
/// diimpor di atas.)

fn parse_hd_session_status(s: &str) -> Option<HdSessionStatus> {
    match key(s).as_str() {
        "terjadwal" => Some(HdSessionStatus::Terjadwal),
        "berjalan" => Some(HdSessionStatus::Berjalan),
        "selesai" => Some(HdSessionStatus::Selesai),
        "batal" => Some(HdSessionStatus::Batal),
        _ => None,
    }
}

fn parse_blood_request_status(s: &str) -> Option<BloodRequestStatus> {
    match key(s).as_str() {
        "menunggu" => Some(BloodRequestStatus::Menunggu),
        "crossmatch" => Some(BloodRequestStatus::Crossmatch),
        "siap" => Some(BloodRequestStatus::Siap),
        "diberikan" => Some(BloodRequestStatus::Diberikan),
        "batal" => Some(BloodRequestStatus::Batal),
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

/// Format `scheduled_at` yang diterima: `YYYY-MM-DD HH:MM` (atau
/// dengan detik).
fn valid_scheduled_at(s: &str) -> bool {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").is_ok()
        || NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").is_ok()
}

// ---------------------------------------------------------------------------
// Hemodialisa (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_hemodialisa(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let machines = state.db.list_hd_machines().await?;
    let sessions = state.db.list_hd_sessions(&date.to_string()).await?;
    Ok(Json(json!({
        "machines": machines,
        "sessions": sessions,
    })))
}

#[derive(Deserialize)]
pub struct CreateHdSessionDto {
    pub patient_id: String,
    pub machine_id: String,
    pub encounter_id: Option<String>,
    pub dialyzer: Option<String>,
    pub duration_minutes: Option<i64>,
    pub blood_flow: Option<i64>,
    pub dialysate_flow: Option<i64>,
    pub uf_goal_ml: Option<i64>,
    pub scheduled_at: String,
    pub notes: Option<String>,
}

pub async fn create_hd_session(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateHdSessionDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaHemodialisa)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.machine_id, "machine_id")?;
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
    // 422 (kontrak §5.1) bila mesin bukan Siap — dicek di sini supaya
    // jawabannya pasti 422 apa pun varian error Db::create_hd_session.
    let machines = state.db.list_hd_machines().await?;
    let machine = machines
        .iter()
        .find(|m| m.id == body.machine_id.trim())
        .ok_or_else(|| {
            ApiError::not_found(format!("mesin HD {} tidak ditemukan", body.machine_id))
        })?;
    if machine.status != HdMachineStatus::Siap {
        return Ok(unprocessable(&format!(
            "mesin HD {} bukan Siap, tidak bisa dipakai sesi baru",
            machine.id
        )));
    }
    let session = state
        .db
        .create_hd_session(
            body.patient_id.trim(),
            body.machine_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.dialyzer.as_deref().unwrap_or(""),
            body.duration_minutes.unwrap_or(240),
            body.blood_flow.unwrap_or(300),
            body.dialysate_flow.unwrap_or(500),
            body.uf_goal_ml.unwrap_or(2000),
            body.scheduled_at.trim(),
            body.notes.as_deref().unwrap_or(""),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "hemodialisa.session_created",
        "hd_session",
        &session.id,
        json!({"patient_id": session.patient_id, "machine_id": session.machine_id}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(json!(session))))
}

#[derive(Deserialize)]
pub struct HdSessionStatusDto {
    pub status: String,
}

pub async fn set_hd_session_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<HdSessionStatusDto>,
) -> ApiResult<Json<HdSession>> {
    require(&user, Action::KelolaHemodialisa)?;
    let status = parse_hd_session_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!("status sesi HD tidak dikenal: {}", body.status))
    })?;
    state
        .db
        .get_hd_session(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("sesi HD {id} tidak ditemukan")))?;
    // Db::set_hd_session_status: Selesai/Batal -> mesin kembali Siap.
    let session = state.db.set_hd_session_status(&id, status).await?;
    audit(
        &state.db,
        &user.name,
        "hemodialisa.status_changed",
        "hd_session",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(session))
}

// ---------------------------------------------------------------------------
// Transfusi darah (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_blood_stock(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<BloodStock>>> {
    Ok(Json(state.db.list_blood_stock().await?))
}

async fn enrich_blood_request(state: &AppState, req: &BloodRequest) -> ApiResult<Value> {
    let patient_name = patient_name(&state.db, &req.patient_id).await?;
    let mut v = serde_json::to_value(req)?;
    v["patient_name"] = serde_json::to_value(&patient_name)?;
    Ok(v)
}

pub async fn list_blood_requests(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => {
            parse_blood_request_status(s).ok_or_else(|| {
                ApiError::validation(format!("status permintaan darah tidak dikenal: {s}"))
            })?;
            Some(canonical_status_filter(
                s,
                &["menunggu", "crossmatch", "siap", "diberikan", "batal"],
            )?)
        }
        _ => None,
    };
    let requests = state.db.list_blood_requests(status.as_deref()).await?;
    let mut out = Vec::with_capacity(requests.len());
    for r in &requests {
        out.push(enrich_blood_request(&state, r).await?);
    }
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
pub struct CreateBloodRequestDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub blood_type: String,
    pub component: String,
    pub units: i64,
    pub reason: String,
}

fn valid_blood_type(s: &str) -> bool {
    matches!(
        s.trim(),
        "A+" | "A-" | "B+" | "B-" | "O+" | "O-" | "AB+" | "AB-"
    )
}

fn valid_blood_component(s: &str) -> bool {
    matches!(s.trim(), "WB" | "PRC" | "TC" | "FFP")
}

pub async fn create_blood_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateBloodRequestDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaTransfusi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.blood_type, "blood_type")?;
    non_empty(&body.component, "component")?;
    non_empty(&body.reason, "reason")?;
    if !valid_blood_type(&body.blood_type) {
        return Err(ApiError::validation(format!(
            "golongan darah tidak dikenal: {}",
            body.blood_type
        )));
    }
    if !valid_blood_component(&body.component) {
        return Err(ApiError::validation(format!(
            "komponen darah tidak dikenal: {}",
            body.component
        )));
    }
    if body.units <= 0 {
        return Err(ApiError::validation("units harus lebih dari 0"));
    }
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    let request = state
        .db
        .create_blood_request(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.blood_type.trim(),
            body.component.trim(),
            body.units,
            body.reason.trim(),
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "transfusi.request_created",
        "blood_request",
        &request.id,
        json!({"patient_id": request.patient_id, "blood_type": request.blood_type}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(enrich_blood_request(&state, &request).await?),
    ))
}

#[derive(Deserialize)]
pub struct AddCrossmatchDto {
    pub bag_id: String,
    pub result: String,
    pub tested_by: Option<String>,
}

pub async fn add_crossmatch(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<AddCrossmatchDto>,
) -> ApiResult<(StatusCode, Json<BloodCrossmatch>)> {
    require(&user, Action::KelolaTransfusi)?;
    non_empty(&body.bag_id, "bag_id")?;
    non_empty(&body.result, "result")?;
    let result = match key(&body.result).as_str() {
        "cocok" => "Cocok",
        "tidakcocok" => "Tidak Cocok",
        _ => {
            return Err(ApiError::validation(format!(
                "hasil crossmatch tidak dikenal: {}",
                body.result
            )))
        }
    };
    state
        .db
        .get_blood_request(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("permintaan darah {id} tidak ditemukan")))?;
    state
        .db
        .get_blood_stock(body.bag_id.trim())
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("kantong darah {} tidak ditemukan", body.bag_id))
        })?;
    // Db::add_crossmatch: hasil Cocok -> permintaan jadi Crossmatch.
    let crossmatch = state
        .db
        .add_crossmatch(
            &id,
            body.bag_id.trim(),
            result,
            body.tested_by.as_deref().unwrap_or(&user.name),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "transfusi.crossmatch_added",
        "blood_crossmatch",
        &crossmatch.id,
        json!({"request_id": id, "result": result}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(crossmatch)))
}

#[derive(Deserialize)]
pub struct BloodRequestStatusDto {
    pub status: String,
}

pub async fn set_blood_request_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<BloodRequestStatusDto>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::KelolaTransfusi)?;
    let status = parse_blood_request_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!(
            "status permintaan darah tidak dikenal: {}",
            body.status
        ))
    })?;
    state
        .db
        .get_blood_request(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("permintaan darah {id} tidak ditemukan")))?;
    let request = state.db.set_blood_request_status(&id, status).await?;
    audit(
        &state.db,
        &user.name,
        "transfusi.status_changed",
        "blood_request",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(enrich_blood_request(&state, &request).await?))
}

// ---------------------------------------------------------------------------
// Gizi (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_nutrition_assessments(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<NutritionAssessment>>> {
    Ok(Json(
        state
            .db
            .list_nutrition_assessments(patient_filter(&q).as_deref())
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct CreateNutritionAssessmentDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub weight_kg: f64,
    pub height_cm: f64,
    pub risk: Option<String>,
    pub diet: Option<String>,
    pub notes: Option<String>,
}

pub async fn create_nutrition_assessment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateNutritionAssessmentDto>,
) -> ApiResult<(StatusCode, Json<NutritionAssessment>)> {
    require(&user, Action::KelolaGizi)?;
    non_empty(&body.patient_id, "patient_id")?;
    if !(body.weight_kg > 0.0) {
        return Err(ApiError::validation("weight_kg harus lebih dari 0"));
    }
    if !(body.height_cm > 0.0) {
        return Err(ApiError::validation("height_cm harus lebih dari 0"));
    }
    let risk = body.risk.as_deref().unwrap_or("Rendah");
    if !matches!(key(risk).as_str(), "rendah" | "sedang" | "tinggi") {
        return Err(ApiError::validation(format!(
            "risiko tidak dikenal: {risk}"
        )));
    }
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    // Db::create_nutrition_assessment menghitung BMI + kategori di
    // server (core::nutrition) — klien tidak mengirim keduanya.
    let assessment = state
        .db
        .create_nutrition_assessment(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.weight_kg,
            body.height_cm,
            risk,
            body.diet.as_deref().unwrap_or(""),
            body.notes.as_deref().unwrap_or(""),
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "gizi.assessment_created",
        "nutrition_assessment",
        &assessment.id,
        json!({"patient_id": assessment.patient_id}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(assessment)))
}

pub async fn list_nutrition_diets(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<NutritionDiet>>> {
    Ok(Json(
        state
            .db
            .list_nutrition_diets(patient_filter(&q).as_deref())
            .await?,
    ))
}

#[derive(Deserialize)]
pub struct CreateNutritionDietDto {
    pub patient_id: String,
    pub diet_type: String,
    pub calories: i64,
    pub notes: Option<String>,
    pub start_date: Option<String>,
}

fn valid_diet_type(s: &str) -> bool {
    matches!(
        key(s).as_str(),
        "dm" | "jantung" | "rendahgaram" | "tinggiprotein" | "biasa"
    )
}

pub async fn create_nutrition_diet(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateNutritionDietDto>,
) -> ApiResult<(StatusCode, Json<NutritionDiet>)> {
    require(&user, Action::KelolaGizi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.diet_type, "diet_type")?;
    if !valid_diet_type(&body.diet_type) {
        return Err(ApiError::validation(format!(
            "jenis diet tidak dikenal: {}",
            body.diet_type
        )));
    }
    if body.calories < 0 {
        return Err(ApiError::validation("calories tidak boleh negatif"));
    }
    let start_date = match body
        .start_date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => parse_date_param(s)?.to_string(),
        None => clinic_today().to_string(),
    };
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    let diet = state
        .db
        .create_nutrition_diet(
            body.patient_id.trim(),
            body.diet_type.trim(),
            body.calories,
            body.notes.as_deref().unwrap_or(""),
            &start_date,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "gizi.diet_created",
        "nutrition_diet",
        &diet.id,
        json!({"patient_id": diet.patient_id, "diet_type": diet.diet_type}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(diet)))
}

#[derive(Deserialize)]
pub struct NutritionDietStatusDto {
    pub status: String,
}

pub async fn set_nutrition_diet_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<NutritionDietStatusDto>,
) -> ApiResult<Json<NutritionDiet>> {
    require(&user, Action::KelolaGizi)?;
    // Db::set_nutrition_diet_status menerima &str dan membandingkan
    // case-insensitive, tapi nilai yang TERSIMPAN dipakai apa adanya —
    // seed memakai bentuk tampilan ("Aktif" / "Selesai", FASE4.md §1),
    // jadi kirim bentuk itu supaya nilainya konsisten.
    let status = match canonical_status_filter(&body.status, &["aktif", "selesai"])? {
        s if s == "aktif" => "Aktif",
        _ => "Selesai",
    };
    // 404 dulu bila diet tidak ada (DB tidak punya get diet per id).
    let diets = state.db.list_nutrition_diets(None).await?;
    if !diets.iter().any(|d| d.id == id) {
        return Err(ApiError::not_found(format!("diet {id} tidak ditemukan")));
    }
    let diet = state.db.set_nutrition_diet_status(&id, status).await?;
    audit(
        &state.db,
        &user.name,
        "gizi.diet_status_changed",
        "nutrition_diet",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(diet))
}

// ---------------------------------------------------------------------------
// Rekam medis gigi (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_dental_records(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<DentalRecord>>> {
    Ok(Json(
        state
            .db
            .list_dental_records(patient_filter(&q).as_deref())
            .await?,
    ))
}

/// Validasi notasi FDI: kuadran 1–4, gigi 1–8 (11–18, 21–28, 31–38,
/// 41–48). Bukan FDI -> 400 (kontrak §5.1).
fn valid_fdi(tooth: &str) -> bool {
    let t = tooth.trim();
    if t.len() != 2 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let quadrant = t.as_bytes()[0] - b'0';
    let number = t.as_bytes()[1] - b'0';
    (1..=4).contains(&quadrant) && (1..=8).contains(&number)
}

#[derive(Deserialize)]
pub struct CreateDentalRecordDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub tooth_number: String,
    pub diagnosis: String,
    pub diagnosis_code: Option<String>,
    pub treatment: String,
    pub treatment_code: Option<String>,
    pub dentist: String,
}

pub async fn create_dental_record(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateDentalRecordDto>,
) -> ApiResult<(StatusCode, Json<DentalRecord>)> {
    require(&user, Action::KelolaGigi)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.tooth_number, "tooth_number")?;
    non_empty(&body.diagnosis, "diagnosis")?;
    non_empty(&body.treatment, "treatment")?;
    non_empty(&body.dentist, "dentist")?;
    if !valid_fdi(&body.tooth_number) {
        return Err(ApiError::validation(format!(
            "nomor gigi {} bukan notasi FDI (11-18, 21-28, 31-38, 41-48)",
            body.tooth_number
        )));
    }
    patient_must_exist(&state.db, body.patient_id.trim()).await?;
    verify_encounter(
        &state.db,
        body.encounter_id.as_deref(),
        body.patient_id.trim(),
    )
    .await?;
    let record = state
        .db
        .create_dental_record(
            body.patient_id.trim(),
            body.encounter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            body.tooth_number.trim(),
            body.diagnosis.trim(),
            body.diagnosis_code.as_deref().unwrap_or(""),
            body.treatment.trim(),
            body.treatment_code.as_deref().unwrap_or(""),
            body.dentist.trim(),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "gigi.record_created",
        "dental_record",
        &record.id,
        json!({"patient_id": record.patient_id, "tooth_number": record.tooth_number}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(record)))
}

#[derive(Deserialize)]
pub struct DentalRecordStatusDto {
    pub status: String,
}

pub async fn set_dental_record_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DentalRecordStatusDto>,
) -> ApiResult<Json<DentalRecord>> {
    require(&user, Action::KelolaGigi)?;
    // Bentuk tampilan ("Rencana" / "Berjalan" / "Selesai", FASE4.md §1).
    let status = match canonical_status_filter(&body.status, &["rencana", "berjalan", "selesai"])? {
        s if s == "rencana" => "Rencana",
        s if s == "berjalan" => "Berjalan",
        _ => "Selesai",
    };
    let records = state.db.list_dental_records(None).await?;
    if !records.iter().any(|r| r.id == id) {
        return Err(ApiError::not_found(format!(
            "rekam gigi {id} tidak ditemukan"
        )));
    }
    let record = state.db.set_dental_record_status(&id, status).await?;
    audit(
        &state.db,
        &user.name,
        "gigi.status_changed",
        "dental_record",
        &id,
        json!({"status": body.status}),
    )
    .await;
    Ok(Json(record))
}

// ---------------------------------------------------------------------------
// Inventori (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_inventory_items(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Value>> {
    let items = state.db.list_inventory_items().await?;
    let mut out = Vec::with_capacity(items.len());
    for it in &items {
        let mut v = serde_json::to_value(it)?;
        // UI inventori membaca `stock_status` (ambang sama dengan
        // Farmasi, FASE4.md §2).
        v["stock_status"] = serde_json::to_value(inventory_stock_status(it.stock))?;
        out.push(v);
    }
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
pub struct CreateInventoryItemDto {
    pub name: String,
    pub category: String,
    pub unit: String,
    pub stock: i64,
    pub min_stock: i64,
    pub location: Option<String>,
    pub expiry: Option<String>,
}

pub async fn create_inventory_item(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateInventoryItemDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaInventori)?;
    non_empty(&body.name, "name")?;
    non_empty(&body.category, "category")?;
    non_empty(&body.unit, "unit")?;
    if !matches!(key(&body.category).as_str(), "obat" | "alkes" | "umum") {
        return Err(ApiError::validation(format!(
            "kategori inventori tidak dikenal: {}",
            body.category
        )));
    }
    if body.stock < 0 {
        return Err(ApiError::validation("stock tidak boleh negatif"));
    }
    if body.min_stock < 0 {
        return Err(ApiError::validation("min_stock tidak boleh negatif"));
    }
    if let Some(exp) = body
        .expiry
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parse_date_param(exp)?;
    }
    let item = state
        .db
        .create_inventory_item(
            body.name.trim(),
            body.category.trim(),
            body.unit.trim(),
            body.stock,
            body.min_stock,
            body.location.as_deref().unwrap_or(""),
            body.expiry.as_deref().unwrap_or(""),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "inventori.item_created",
        "inventory_item",
        &item.id,
        json!({"name": item.name, "category": item.category}),
    )
    .await;
    let mut v = serde_json::to_value(&item)?;
    v["stock_status"] = serde_json::to_value(inventory_stock_status(item.stock))?;
    Ok((StatusCode::CREATED, Json(v)))
}

pub async fn list_inventory_moves(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<InventoryMove>>> {
    let item_id = q
        .get("item_id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(Json(
        state.db.list_inventory_moves(item_id.as_deref()).await?,
    ))
}

#[derive(Deserialize)]
pub struct AddInventoryMoveDto {
    pub kind: String,
    pub qty: i64,
    pub note: Option<String>,
}

pub async fn add_inventory_move(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<AddInventoryMoveDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaInventori)?;
    non_empty(&body.kind, "kind")?;
    let kind = match canonical_status_filter(&body.kind, &["masuk", "keluar", "opname"])? {
        s if s == "masuk" => "Masuk",
        s if s == "keluar" => "Keluar",
        _ => "Opname",
    };
    if kind != "Opname" && body.qty <= 0 {
        return Err(ApiError::validation("qty harus lebih dari 0"));
    }
    if kind == "Opname" && body.qty < 0 {
        return Err(ApiError::validation("qty opname tidak boleh negatif"));
    }
    let item = state
        .db
        .get_inventory_item(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("item inventori {id} tidak ditemukan")))?;
    // 422 (kontrak §5.1) bila stok tidak cukup untuk Keluar — dicek di
    // sini supaya jawabannya pasti 422 apa pun varian error DB.
    if kind == "Keluar" && body.qty > item.stock {
        return Ok(unprocessable(&format!(
            "stok {} ({}) tidak cukup untuk keluar {}",
            item.name, item.stock, body.qty
        )));
    }
    // Db::add_inventory_move: Masuk stok += qty; Keluar stok -= qty;
    // Opname stok = qty.
    let movement = state
        .db
        .add_inventory_move(
            &id,
            kind,
            body.qty,
            body.note.as_deref().unwrap_or(""),
            &user.name,
        )
        .await?;
    let updated = state
        .db
        .get_inventory_item(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("item inventori {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "inventori.move_added",
        "inventory_move",
        &movement.id,
        json!({"item_id": id, "kind": kind, "qty": body.qty}),
    )
    .await;
    let mut v = serde_json::to_value(&movement)?;
    let mut item_v = serde_json::to_value(&updated)?;
    item_v["stock_status"] = serde_json::to_value(inventory_stock_status(updated.stock))?;
    v["item"] = item_v;
    Ok((StatusCode::CREATED, Json(v)))
}

// ---------------------------------------------------------------------------
// Kepegawaian (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_staff(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Staff>>> {
    Ok(Json(state.db.list_staff().await?))
}

#[derive(Deserialize)]
pub struct CreateStaffDto {
    pub name: String,
    pub nip: Option<String>,
    pub role: String,
    pub poli: Option<String>,
    pub phone: Option<String>,
}

fn valid_staff_role(s: &str) -> bool {
    matches!(
        key(s).as_str(),
        "dokterumum" | "perawat" | "apoteker" | "kasir" | "admin" | "bidan" | "doktergigi"
    )
}

pub async fn create_staff(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateStaffDto>,
) -> ApiResult<(StatusCode, Json<Staff>)> {
    require(&user, Action::KelolaKepegawaian)?;
    non_empty(&body.name, "name")?;
    non_empty(&body.role, "role")?;
    if !valid_staff_role(&body.role) {
        return Err(ApiError::validation(format!(
            "peran staf tidak dikenal: {}",
            body.role
        )));
    }
    let staff = state
        .db
        .create_staff(
            body.name.trim(),
            body.nip.as_deref().unwrap_or(""),
            body.role.trim(),
            body.poli.as_deref().unwrap_or(""),
            body.phone.as_deref().unwrap_or(""),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "kepegawaian.staff_created",
        "staff",
        &staff.id,
        json!({"name": staff.name, "role": staff.role}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(staff)))
}

#[derive(Deserialize)]
pub struct SetStaffActiveDto {
    pub active: bool,
}

pub async fn set_staff_active(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<SetStaffActiveDto>,
) -> ApiResult<Json<Staff>> {
    require(&user, Action::KelolaKepegawaian)?;
    let staff = state.db.list_staff().await?;
    if !staff.iter().any(|s| s.id == id) {
        return Err(ApiError::not_found(format!("staf {id} tidak ditemukan")));
    }
    let staff = state.db.set_staff_active(&id, body.active).await?;
    audit(
        &state.db,
        &user.name,
        "kepegawaian.staff_active_changed",
        "staff",
        &id,
        json!({"active": body.active}),
    )
    .await;
    Ok(Json(staff))
}

async fn enrich_shift(state: &AppState, shift: &ShiftSchedule) -> ApiResult<Value> {
    let staff_name = state
        .db
        .list_staff()
        .await?
        .into_iter()
        .find(|s| s.id == shift.staff_id)
        .map(|s| s.name);
    let mut v = serde_json::to_value(shift)?;
    v["staff_name"] = serde_json::to_value(&staff_name)?;
    Ok(v)
}

pub async fn list_shifts(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let shifts = state.db.list_shifts(&date.to_string()).await?;
    let mut out = Vec::with_capacity(shifts.len());
    for s in &shifts {
        out.push(enrich_shift(&state, s).await?);
    }
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
pub struct CreateShiftDto {
    pub staff_id: String,
    pub date: Option<String>,
    pub shift: String,
    pub poli: Option<String>,
}

pub async fn create_shift(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateShiftDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::KelolaKepegawaian)?;
    non_empty(&body.staff_id, "staff_id")?;
    non_empty(&body.shift, "shift")?;
    let shift = match canonical_status_filter(&body.shift, &["pagi", "siang", "malam"])? {
        s if s == "pagi" => "Pagi",
        s if s == "siang" => "Siang",
        _ => "Malam",
    };
    let date = match body
        .date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => parse_date_param(s)?.to_string(),
        None => clinic_today().to_string(),
    };
    let staff = state.db.list_staff().await?;
    if !staff.iter().any(|s| s.id == body.staff_id.trim()) {
        return Err(ApiError::not_found(format!(
            "staf {} tidak ditemukan",
            body.staff_id
        )));
    }
    // 422 (kontrak §5.1) bila staf sudah punya shift di tanggal itu —
    // dicek di sini supaya jawabannya pasti 422 apa pun varian error DB.
    let existing = state.db.list_shifts(&date).await?;
    if existing.iter().any(|s| s.staff_id == body.staff_id.trim()) {
        return Ok(unprocessable(&format!(
            "staf {} sudah punya shift tanggal {date}",
            body.staff_id
        )));
    }
    let created = state
        .db
        .create_shift(
            body.staff_id.trim(),
            &date,
            shift,
            body.poli.as_deref().unwrap_or(""),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "kepegawaian.shift_created",
        "shift_schedule",
        &created.id,
        json!({"staff_id": created.staff_id, "date": date, "shift": shift}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(enrich_shift(&state, &created).await?),
    ))
}

// ---------------------------------------------------------------------------
// Laporan & SIRS (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn report_visits(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ViewLaporan)?;
    let today = clinic_today();
    let to = match q.get("to") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s)?,
        _ => today,
    };
    let from = match q.get("from") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s)?,
        // Default 7 hari terakhir s.d. WARAS_TODAY (inklusif).
        _ => to - Duration::days(6),
    };
    if from > to {
        return Err(ApiError::validation("from tidak boleh lebih besar dari to"));
    }
    let rows = state
        .db
        .report_visits(&from.to_string(), &to.to_string())
        .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(date, visits)| json!({"date": date, "visits": visits}))
        .collect();
    Ok(Json(json!(out)))
}

pub async fn report_top_diseases(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ViewLaporan)?;
    let limit = match q.get("limit") {
        Some(s) if !s.trim().is_empty() => s
            .trim()
            .parse::<i64>()
            .map_err(|_| ApiError::validation(format!("limit tidak valid: {s}")))?,
        _ => 10,
    };
    if limit <= 0 {
        return Err(ApiError::validation("limit harus lebih dari 0"));
    }
    let rows = state.db.report_top_diseases(limit).await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(code, display, count)| json!({"code": code, "display": display, "count": count}))
        .collect();
    Ok(Json(json!(out)))
}

pub async fn report_sirs(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<SirsSummary>> {
    require(&user, Action::ViewLaporan)?;
    // SirsSummary dikembalikan apa adanya sebagai JSON (FASE4.md §4).
    Ok(Json(state.db.report_sirs().await?))
}

// ---------------------------------------------------------------------------
// Pengaturan (FASE4.md §5.1 + §4.1)
// ---------------------------------------------------------------------------

pub async fn list_settings(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<AppSetting>>> {
    Ok(Json(state.db.list_settings().await?))
}

/// Kunci pengaturan yang diizinkan (FASE4.md §4.1). Kunci lain -> 400
/// "kunci pengaturan tidak dikenal".
fn validate_setting_value(key: &str, value: &str) -> ApiResult<()> {
    match key {
        "backup_otomatis" | "sinkron_satusehat" | "mode_offline" => {
            if !matches!(value.trim(), "on" | "off") {
                return Err(ApiError::validation(format!(
                    "nilai {key} harus on atau off"
                )));
            }
        }
        "retensi_audit_hari" => {
            let n: i64 = value
                .trim()
                .parse()
                .map_err(|_| ApiError::validation("retensi_audit_hari harus angka"))?;
            if !(1..=3650).contains(&n) {
                return Err(ApiError::validation("retensi_audit_hari harus 1-3650"));
            }
        }
        _ => {
            return Err(ApiError::validation(format!(
                "kunci pengaturan tidak dikenal: {key}"
            )))
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct SetSettingDto {
    pub value: String,
}

pub async fn set_setting(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(key): Path<String>,
    JsonBody(body): JsonBody<SetSettingDto>,
) -> ApiResult<Json<AppSetting>> {
    require(&user, Action::KelolaPengaturan)?;
    let key = key.trim();
    validate_setting_value(key, body.value.trim())?;
    let setting = state.db.set_setting(key, body.value.trim()).await?;
    audit(
        &state.db,
        &user.name,
        "pengaturan.changed",
        "app_setting",
        key,
        json!({"value": body.value}),
    )
    .await;
    Ok(Json(setting))
}

// ---------------------------------------------------------------------------
// Rawat jalan (FASE4.md §5.1)
// ---------------------------------------------------------------------------

pub async fn list_outpatient_visits(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let date = date_query(&q)?;
    let visits: Vec<OutpatientVisit> = state.db.list_outpatient_visits(&date.to_string()).await?;
    Ok(Json(json!({ "visits": visits })))
}
