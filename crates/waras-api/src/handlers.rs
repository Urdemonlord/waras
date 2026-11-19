//! Semua handler API (KONTRAK §3).
//!
//! Pola umum: `AuthUser` di semua endpoint kecuali `/api/health` dan
//! `/api/auth/login`; guard RBAC lewat `auth::require` sebelum menyentuh
//! DB; aksi klinis dicatat ke audit trail lewat `Db::log_audit` (kecuali
//! yang sudah dicatat di dalam `waras-db`: prescribe/confirm/dispense/
//! pay, supaya tidak ganda).

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use waras_core::{
    bridge::OutboxDestination,
    fhir,
    rbac::Action,
    types::{EncounterStatus, LoginRequest, NewPrescription},
    WarasError,
};
use waras_db::{clinic_today, Db, NewPatient, PrescriptionFilter};

use crate::{
    auth::{require, AuthUser, JsonBody},
    error::ApiError,
    parse::*,
    worker::{new_outbox_job, try_enqueue},
    AppState,
};

type ApiResult<T> = Result<T, ApiError>;

/// Audit best-effort: kegagalan mencatat audit tidak boleh menggagalkan
/// aksi klinis yang sudah berhasil.
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

fn valid_nik(nik: &str) -> ApiResult<()> {
    if nik.len() != 16 || !nik.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::validation("NIK harus 16 digit angka"));
    }
    Ok(())
}

fn parse_date_param(s: &str) -> ApiResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::validation(format!("tanggal tidak valid: {s}")))
}

// ---------------------------------------------------------------------------
// Publik: health + login
// ---------------------------------------------------------------------------

pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": "0.1.0",
        "mode": "sqlite",
        "today": clinic_today().to_string(),
    }))
}

pub async fn login(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<LoginRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if body.username.trim().is_empty() || body.password.is_empty() {
        return Err(ApiError::validation(
            "nama pengguna/NIP dan kata sandi wajib diisi",
        ));
    }
    let user = state
        .db
        .find_user_by_username_or_nip(body.username.trim())
        .await?;
    let user = match user {
        Some(u) if Db::verify_password(&u.password_hash, &body.password) && u.active => u,
        _ => return Err(ApiError::unauthorized()),
    };
    let token = state.db.create_session(&user.id).await?;
    audit(
        &state.db,
        &user.name,
        "auth.login",
        "user",
        &user.id,
        json!({"username": user.username}),
    )
    .await;
    // `User` tidak menyerialisasi `password_hash` (skip_serializing).
    Ok((StatusCode::OK, Json(json!({ "token": token, "user": user }))))
}

pub async fn me(AuthUser(user): AuthUser) -> Json<Value> {
    Json(json!({ "user": user }))
}

// ---------------------------------------------------------------------------
// Pasien
// ---------------------------------------------------------------------------

pub async fn list_patients(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::Patient>>> {
    let patients = state
        .db
        .list_patients(q.get("search").map(String::as_str))
        .await?;
    Ok(Json(patients))
}

#[derive(Deserialize)]
pub struct CreatePatientDto {
    pub nik: String,
    pub name: String,
    pub birth_date: NaiveDate,
    pub gender: String,
    pub address: String,
    pub phone: Option<String>,
    pub blood_type: Option<String>,
    pub bpjs_number: Option<String>,
    pub insurance: String,
}

pub async fn create_patient(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreatePatientDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::Patient>)> {
    require(&user, Action::CreatePatient)?;
    valid_nik(&body.nik)?;
    non_empty(&body.name, "nama")?;
    non_empty(&body.address, "alamat")?;
    let gender = parse_gender(&body.gender)
        .ok_or_else(|| ApiError::validation(format!("jenis kelamin tidak dikenal: {}", body.gender)))?;
    let insurance = parse_insurance(&body.insurance).ok_or_else(|| {
        ApiError::validation(format!("penjamin tidak dikenal: {}", body.insurance))
    })?;
    let patient = state
        .db
        .create_patient(NewPatient {
            nik: body.nik,
            name: body.name,
            birth_date: body.birth_date,
            gender,
            address: body.address,
            phone: body.phone.filter(|s| !s.trim().is_empty()),
            blood_type: body.blood_type.filter(|s| !s.trim().is_empty()),
            bpjs_number: body.bpjs_number.filter(|s| !s.trim().is_empty()),
            insurance,
        })
        .await?;
    audit(
        &state.db,
        &user.name,
        "patient.created",
        "patient",
        &patient.id,
        json!({"name": patient.name, "nik": patient.nik}),
    )
    .await;
    // Fase 2 (§4.2): enqueue Patient ke SATUSEHAT (best-effort).
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::Satusehat,
            "Patient",
            &patient.id,
            &format!("satusehat:Patient:{}:created", patient.id),
            fhir::patient_resource(&patient),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(patient)))
}

pub async fn get_patient(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let patient = state
        .db
        .get_patient(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("pasien {id} tidak ditemukan")))?;
    let allergies = state.db.get_allergies(&id).await?;
    let conditions = state.db.get_conditions(&id).await?;
    let last_encounter = state.db.list_encounters(&id).await?.into_iter().next();
    let mut v = serde_json::to_value(&patient)?;
    v["allergies"] = serde_json::to_value(&allergies)?;
    v["conditions"] = serde_json::to_value(&conditions)?;
    v["last_encounter"] = serde_json::to_value(&last_encounter)?;
    Ok(Json(v))
}

pub async fn get_patient_fhir(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let patient = state
        .db
        .get_patient(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("pasien {id} tidak ditemukan")))?;
    Ok(Json(fhir::patient_resource(&patient)))
}

pub async fn list_patient_conditions(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<waras_core::types::Condition>>> {
    state
        .db
        .get_patient(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("pasien {id} tidak ditemukan")))?;
    Ok(Json(state.db.get_conditions(&id).await?))
}

// ---------------------------------------------------------------------------
// Antrean
// ---------------------------------------------------------------------------

pub async fn list_queue(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::QueueEntry>>> {
    let date = match q.get("date") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s)?,
        _ => clinic_today(),
    };
    Ok(Json(state.db.list_queue(date).await?))
}

#[derive(Deserialize)]
pub struct TakeQueueDto {
    pub patient_id: String,
    pub poli: String,
    pub doctor: String,
}

pub async fn take_queue(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<TakeQueueDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::QueueEntry>)> {
    require(&user, Action::TakeQueue)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.poli, "poli")?;
    non_empty(&body.doctor, "dokter")?;
    state
        .db
        .get_patient(&body.patient_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("pasien {} tidak ditemukan", body.patient_id))
        })?;
    let entry = state
        .db
        .take_queue(&body.patient_id, &body.poli, &body.doctor, clinic_today())
        .await?;
    audit(
        &state.db,
        &user.name,
        "queue.taken",
        "queue_entry",
        &entry.id,
        json!({"number": entry.number, "patient_id": entry.patient_id, "poli": entry.poli}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(entry)))
}

pub async fn call_queue(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<waras_core::types::QueueEntry>> {
    require(&user, Action::CallQueue)?;
    let entry = state.db.call_queue(&id).await?;
    audit(
        &state.db,
        &user.name,
        "queue.called",
        "queue_entry",
        &entry.id,
        json!({"number": entry.number, "patient_id": entry.patient_id}),
    )
    .await;
    Ok(Json(entry))
}

#[derive(Deserialize)]
pub struct QueueStatusDto {
    pub status: String,
}

pub async fn set_queue_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<QueueStatusDto>,
) -> ApiResult<Json<waras_core::types::QueueEntry>> {
    require(&user, Action::CallQueue)?;
    let status = parse_queue_status(&body.status)
        .ok_or_else(|| ApiError::validation(format!("status antrean tidak dikenal: {}", body.status)))?;
    // Kontrak §3: endpoint ini hanya untuk DalamKonsultasi / Selesai /
    // TidakHadir (Dipanggil lewat /call, Menunggu lewat /take).
    match status {
        waras_core::types::QueueStatus::DalamKonsultasi
        | waras_core::types::QueueStatus::Selesai
        | waras_core::types::QueueStatus::TidakHadir => {}
        _ => {
            return Err(ApiError::validation(
                "status antrean hanya boleh DalamKonsultasi, Selesai, atau TidakHadir",
            ))
        }
    }
    let entry = state.db.set_queue_status(&id, status).await?;
    audit(
        &state.db,
        &user.name,
        "queue.status_changed",
        "queue_entry",
        &entry.id,
        json!({"number": entry.number, "status": body.status}),
    )
    .await;
    Ok(Json(entry))
}

// ---------------------------------------------------------------------------
// Encounter, SOAP, vital, diagnosis
// ---------------------------------------------------------------------------

pub async fn list_encounters(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::Encounter>>> {
    let patient_id = q
        .get("patient_id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::validation("patient_id wajib diisi"))?;
    Ok(Json(state.db.list_encounters(&patient_id).await?))
}

#[derive(Deserialize)]
pub struct CreateEncounterDto {
    pub patient_id: String,
    pub poli: String,
    pub doctor_name: String,
    pub doctor_id: Option<String>,
}

pub async fn create_encounter(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateEncounterDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::Encounter>)> {
    require(&user, Action::CreateEncounter)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.poli, "poli")?;
    non_empty(&body.doctor_name, "doctor_name")?;
    state
        .db
        .get_patient(&body.patient_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("pasien {} tidak ditemukan", body.patient_id))
        })?;
    let doctor_id = body.doctor_id.filter(|s| !s.trim().is_empty());
    let encounter = state
        .db
        .create_encounter(
            &body.patient_id,
            &body.poli,
            &body.doctor_name,
            doctor_id.as_deref(),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "encounter.created",
        "encounter",
        &encounter.id,
        json!({"patient_id": encounter.patient_id, "poli": encounter.poli}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(encounter)))
}

pub async fn get_encounter(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let encounter = state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    let (patient, _, vitals, conditions, prescriptions) = state
        .db
        .encounter_bundle_data(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    let soap = state.db.get_soap(&id).await?;
    let bundle = fhir::encounter_bundle(&patient, &encounter, &vitals, &conditions, &prescriptions);
    let mut v = serde_json::to_value(&encounter)?;
    v["soap"] = serde_json::to_value(&soap)?;
    v["vitals"] = serde_json::to_value(&vitals)?;
    v["conditions"] = serde_json::to_value(&conditions)?;
    v["prescriptions"] = serde_json::to_value(&prescriptions)?;
    v["fhir_bundle"] = bundle;
    Ok(Json(v))
}

pub async fn get_encounter_fhir(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (patient, encounter, vitals, conditions, prescriptions) = state
        .db
        .encounter_bundle_data(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    Ok(Json(fhir::encounter_bundle(
        &patient,
        &encounter,
        &vitals,
        &conditions,
        &prescriptions,
    )))
}

pub async fn get_soap(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    let soap = state
        .db
        .get_soap(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("SOAP encounter {id} belum ada")))?;
    let versions = state.db.get_soap_versions(&soap.id).await?;
    let mut v = serde_json::to_value(&soap)?;
    v["versions"] = serde_json::to_value(&versions)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct SoapDto {
    pub subjective: String,
    pub objective: String,
    pub assessment: String,
    pub plan: String,
}

pub async fn save_soap(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<SoapDto>,
) -> ApiResult<Json<waras_core::types::SoapNote>> {
    require(&user, Action::WriteSoap)?;
    let saved = state
        .db
        .save_soap(
            &id,
            &body.subjective,
            &body.objective,
            &body.assessment,
            &body.plan,
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "soap.saved",
        "soap_note",
        &saved.id,
        json!({"encounter_id": id, "version": saved.version}),
    )
    .await;
    Ok(Json(saved))
}

pub async fn list_vitals(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<waras_core::types::Observation>>> {
    state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    Ok(Json(state.db.list_vitals(&id).await?))
}

#[derive(Deserialize)]
pub struct AddVitalDto {
    pub code: String,
    pub value: String,
    pub measured_by: String,
    pub previous_value: Option<String>,
    pub previous_date: Option<NaiveDate>,
}

pub async fn add_vital(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<AddVitalDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::Observation>)> {
    require(&user, Action::RecordVitals)?;
    non_empty(&body.value, "value")?;
    non_empty(&body.measured_by, "measured_by")?;
    let code = parse_vital_code(&body.code)
        .ok_or_else(|| ApiError::validation(format!("kode vital tidak dikenal: {}", body.code)))?;
    let obs = state
        .db
        .add_vital(
            &id,
            code,
            &body.value,
            &body.measured_by,
            body.previous_value.as_deref(),
            body.previous_date,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "vital.recorded",
        "observation",
        &obs.id,
        json!({"encounter_id": id, "code": body.code, "value": body.value}),
    )
    .await;
    // Fase 2 (§4.2): enqueue Observation ke SATUSEHAT (best-effort).
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::Satusehat,
            "Observation",
            &obs.id,
            &format!("satusehat:Observation:{}:created", obs.id),
            fhir::observation_resource(&obs),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(obs)))
}

#[derive(Deserialize)]
pub struct AddConditionDto {
    pub group: String,
    pub code: String,
    pub display: String,
    pub status: String,
    pub clinician: String,
}

pub async fn add_condition(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<AddConditionDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::Condition>)> {
    require(&user, Action::AddCondition)?;
    non_empty(&body.code, "code")?;
    non_empty(&body.display, "display")?;
    non_empty(&body.clinician, "clinician")?;
    let group = parse_condition_group(&body.group).ok_or_else(|| {
        ApiError::validation(format!("grup diagnosis tidak dikenal: {}", body.group))
    })?;
    let status = parse_condition_status(&body.status).ok_or_else(|| {
        ApiError::validation(format!("status diagnosis tidak dikenal: {}", body.status))
    })?;
    let encounter = state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    let condition = state
        .db
        .add_condition(
            &encounter.patient_id,
            Some(&id),
            group,
            &body.code,
            &body.display,
            status,
            &body.clinician,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "condition.added",
        "condition",
        &condition.id,
        json!({"encounter_id": id, "code": body.code, "display": body.display}),
    )
    .await;
    // Fase 2 (§4.2): enqueue Condition ke SATUSEHAT (best-effort).
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::Satusehat,
            "Condition",
            &condition.id,
            &format!("satusehat:Condition:{}:created", condition.id),
            fhir::condition_resource(&condition),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(condition)))
}

// ---------------------------------------------------------------------------
// Tutup encounter (Fase 2, §5): status -> Selesai + enqueue SATUSEHAT.
// ---------------------------------------------------------------------------

pub async fn close_encounter(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<waras_core::types::Encounter>> {
    // Dokter/Admin: sama dengan yang boleh membuat encounter.
    require(&user, Action::CreateEncounter)?;
    let encounter = state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    if encounter.status != EncounterStatus::Selesai {
        state
            .db
            .set_encounter_status(&id, EncounterStatus::Selesai)
            .await?;
    }
    let encounter = state
        .db
        .get_encounter(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("encounter {id} tidak ditemukan")))?;
    audit(
        &state.db,
        &user.name,
        "encounter.closed",
        "encounter",
        &encounter.id,
        json!({"patient_id": encounter.patient_id, "poli": encounter.poli}),
    )
    .await;
    // Fase 2 (§4.2): enqueue resource Encounter + satu AllergyIntolerance
    // per alergi pasien. PENYIMPANGAN dari tabel §4.2 (dicatat): tabel
    // menulis payload `encounter_bundle` untuk resource_type `Encounter`,
    // tapi adapter SATUSEHAT agen B menolak payload Encounter tanpa
    // `subject` di level atas (sebuah Bundle tidak punya `subject`) dan
    // `mark_outbox_sent` agen A hanya menyinkronkan `satusehat_id` untuk
    // resource_type `Encounter` (bukan `Bundle`). Kombinasi literal
    // tabel selalu mati Validation dan tidak pernah menyimpan
    // `satusehat_id`. Jadi yang di-enqueue adalah resource Encounter
    // tunggal (`fhir::encounter_resource_with_org`, identifier
    // `http://sys-ids.kemkes.go.id/encounter/{org_id}` bila org id
    // dikonfigurasi); bundle lengkap tetap tersedia
    // lewat GET /api/encounters/{id}/fhir.
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::Satusehat,
            "Encounter",
            &encounter.id,
            &format!("satusehat:Encounter:{}:closed", encounter.id),
            fhir::encounter_resource_with_org(
                &encounter,
                &state.bridge_config.satusehat.org_id,
            ),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    for allergy in state.db.get_allergies(&encounter.patient_id).await? {
        try_enqueue(
            &state.db,
            &new_outbox_job(
                OutboxDestination::Satusehat,
                "AllergyIntolerance",
                &allergy.id,
                &format!("satusehat:AllergyIntolerance:{}:closed", allergy.id),
                fhir::allergy_intolerance_resource(&allergy),
                state.bridge_config.outbox_max_attempts,
            ),
        )
        .await;
    }
    Ok(Json(encounter))
}

// ---------------------------------------------------------------------------
// Resep & obat
// ---------------------------------------------------------------------------

pub async fn list_prescriptions(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::MedicationRequest>>> {
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => Some(
            parse_prescription_status(s).ok_or_else(|| {
                ApiError::validation(format!("status resep tidak dikenal: {s}"))
            })?,
        ),
        _ => None,
    };
    let patient_id = q.get("patient_id").cloned().filter(|s| !s.trim().is_empty());
    Ok(Json(
        state
            .db
            .list_prescriptions(PrescriptionFilter { status, patient_id })
            .await?,
    ))
}

pub async fn create_prescription(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<NewPrescription>,
) -> ApiResult<(StatusCode, Json<waras_core::types::MedicationRequest>)> {
    require(&user, Action::Prescribe)?;
    non_empty(&body.encounter_id, "encounter_id")?;
    non_empty(&body.drug_name, "drug_name")?;
    non_empty(&body.drug_class, "drug_class")?;
    non_empty(&body.dosage, "dosage")?;
    non_empty(&body.duration, "duration")?;
    // `Db::prescribe` menegakkan aturan keamanan obat: bila diblokir,
    // resep tetap disimpan (status Diblokir + blocked_reason + audit
    // `prescription.blocked`) dan mengembalikan Err(Blocked) -> 422.
    // Audit `prescription.created` juga sudah dicatat di sana.
    let rx = state.db.prescribe(&user.name, body).await?;
    // Fase 2 (§4.2): hanya bila lolos safety (sampai sini = lolos),
    // enqueue MedicationRequest ke SATUSEHAT (best-effort).
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::Satusehat,
            "MedicationRequest",
            &rx.id,
            &format!("satusehat:MedicationRequest:{}:created", rx.id),
            fhir::medication_request_resource(&rx),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(rx)))
}

pub async fn confirm_prescription(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<waras_core::types::MedicationRequest>> {
    require(&user, Action::ConfirmPrescription)?;
    Ok(Json(state.db.confirm_prescription(&id).await?))
}

pub async fn dispense_prescription(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<waras_core::types::MedicationRequest>> {
    require(&user, Action::Dispense)?;
    Ok(Json(state.db.dispense_prescription(&id, &user.name).await?))
}

pub async fn list_drugs(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Value>>> {
    let drugs = state.db.list_drugs().await?;
    let mut out = Vec::with_capacity(drugs.len());
    for d in &drugs {
        let mut v = serde_json::to_value(d)?;
        // UI farmasi membaca `stock_status` (KONTRAK: dari Drug::stock_status()).
        v["stock_status"] = serde_json::to_value(d.stock_status())?;
        out.push(v);
    }
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Dasbor, tagihan, audit, notifikasi
// ---------------------------------------------------------------------------

pub async fn get_dashboard(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<waras_core::types::DashboardKpis>> {
    Ok(Json(state.db.dashboard_kpis(clinic_today()).await?))
}

pub async fn list_invoices(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::Invoice>>> {
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => Some(
            parse_invoice_status(s)
                .ok_or_else(|| ApiError::validation(format!("status tagihan tidak dikenal: {s}")))?,
        ),
        _ => None,
    };
    Ok(Json(state.db.list_invoices(status).await?))
}

#[derive(Deserialize)]
pub struct CreateInvoiceDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub payer: String,
    pub service: String,
    pub amount: i64,
    pub ina_cbg_code: Option<String>,
}

pub async fn create_invoice(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateInvoiceDto>,
) -> ApiResult<(StatusCode, Json<waras_core::types::Invoice>)> {
    require(&user, Action::CreateInvoice)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.service, "service")?;
    if body.amount <= 0 {
        return Err(ApiError::validation("amount harus lebih dari 0"));
    }
    let payer = parse_insurance(&body.payer)
        .ok_or_else(|| ApiError::validation(format!("penjamin tidak dikenal: {}", body.payer)))?;
    state
        .db
        .get_patient(&body.patient_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("pasien {} tidak ditemukan", body.patient_id))
        })?;
    let encounter_id = body.encounter_id.filter(|s| !s.trim().is_empty());
    if let Some(enc_id) = &encounter_id {
        state
            .db
            .get_encounter(enc_id)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("encounter {enc_id} tidak ditemukan")))?;
    }
    let invoice = state
        .db
        .create_invoice(
            &body.patient_id,
            encounter_id.as_deref(),
            payer,
            &body.service,
            body.amount,
            body.ina_cbg_code.as_deref(),
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "invoice.created",
        "invoice",
        &invoice.id,
        json!({"patient_id": invoice.patient_id, "amount": invoice.amount}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(invoice)))
}

pub async fn get_invoice(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let invoice = state
        .db
        .get_invoice(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("tagihan {id} tidak ditemukan")))?;
    let payments = state.db.get_payments(&id).await?;
    let mut v = serde_json::to_value(&invoice)?;
    v["payments"] = serde_json::to_value(&payments)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct PayInvoiceDto {
    pub method: String,
    pub amount: i64,
}

pub async fn pay_invoice(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<PayInvoiceDto>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ReceivePayment)?;
    let method = parse_payment_method(&body.method).ok_or_else(|| {
        ApiError::validation(format!("metode pembayaran tidak dikenal: {}", body.method))
    })?;
    // `Db::pay_invoice` mencatat Payment + audit `invoice.paid` dan
    // mengubah status ke Lunas bila total bayar >= jumlah tagihan.
    let invoice = state
        .db
        .pay_invoice(&id, method, body.amount, &user.name)
        .await?;
    let payments = state.db.get_payments(&id).await?;
    let mut v = serde_json::to_value(&invoice)?;
    v["payments"] = serde_json::to_value(&payments)?;
    Ok(Json(v))
}

pub async fn list_audit(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<waras_core::types::AuditEntry>>> {
    require(&user, Action::ViewAudit)?;
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
        .clamp(1, 500);
    Ok(Json(state.db.list_audit(limit).await?))
}

pub async fn list_notifications(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<waras_core::types::Notification>>> {
    Ok(Json(state.db.list_notifications(clinic_today()).await?))
}

// Supaya impor WarasError tidak dianggap tak terpakai bila suatu saat
// tidak ada handler yang menyebutkannya langsung.
#[allow(dead_code)]
fn _uses_waras_error(e: WarasError) -> WarasError {
    e
}
