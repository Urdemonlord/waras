//! Handler jembatan nasional Fase 2 (FASE2.md §5): status bridge,
//! outbox, SEP, rujukan, PCare, antrean online, bed, klaim, dan
//! ringkasan resource SATUSEHAT.
//!
//! Aturan: endpoint yang BUTUH panggilan live mengembalikan 503
//! `{error:{code:"BRIDGE_NOT_CONFIGURED"}}` bila bridge itu belum
//! dikonfigurasi; endpoint yang hanya membaca DB lokal tetap jalan.
//! `ApiError` tidak punya varian 503, jadi respons 503/422/502 di sini
//! dikembalikan langsung sebagai `Ok((StatusCode, Json))` — bentuk
//! `{error:{code, message}}`-nya sama seperti `ApiError`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::Deserialize;
use serde_json::{json, Value};
use waras_bridge::{BridgeError, SendOutcome};
use waras_core::{
    bridge::{BridgeHealth, OutboxDestination, OutboxJob},
    rbac::Action,
    types::{
        BedAvailability, Claim, ClaimStatus, OnlineBooking, PcareVisit, Referral,
        ReferralStatus, Role, Sep, SepStatus, User,
    },
};
use waras_db::{clinic_today, Db};

use crate::{
    auth::{require, AuthUser, JsonBody},
    error::ApiError,
    worker::{new_outbox_job, try_enqueue},
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

/// Jam sekarang, tapi tanggalnya tanggal klinik (WARAS_TODAY), supaya
/// baris baru masuk daftar "hari ini" (filter `created_at LIKE date%`).
fn now_clinic() -> NaiveDateTime {
    NaiveDateTime::new(clinic_today(), Local::now().time())
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

fn date_query(q: &HashMap<String, String>) -> ApiResult<NaiveDate> {
    match q.get("date") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s),
        _ => Ok(clinic_today()),
    }
}

/// 503 `{error:{code:"BRIDGE_NOT_CONFIGURED"}}` (FASE2.md §5).
fn not_configured(label: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": {
            "code": "BRIDGE_NOT_CONFIGURED",
            "message": format!("{label} belum dikonfigurasi"),
        }})),
    )
}

/// 502 untuk kegagalan upstream (bridge terkonfigurasi tapi panggilannya gagal).
fn bridge_error(e: &BridgeError) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error": {"code": "BRIDGE_ERROR", "message": e.to_string()}})),
    )
}

/// 422 untuk penolakan validasi dari tujuan (mis. NIK tidak ditemukan).
fn unprocessable(msg: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"error": {"code": "VALIDATION", "message": msg}})),
    )
}

fn dest_to_str(d: OutboxDestination) -> &'static str {
    match d {
        OutboxDestination::Satusehat => "satusehat",
        OutboxDestination::VClaim => "vclaim",
        OutboxDestination::PCare => "pcare",
        OutboxDestination::AntreanOnline => "antrean_online",
        OutboxDestination::Aplicares => "aplicares",
        OutboxDestination::EKlaim => "eklaim",
    }
}

fn dest_from_str(s: &str) -> Option<OutboxDestination> {
    // Terima dua ejaan: bentuk DB (`vclaim`) dan bentuk serde snake_case
    // (`v_claim`) yang dikembalikan GET /api/bridges — keduanya harus
    // bisa dipakai di path/query, kalau tidak respons daftar dan detail
    // tidak konsisten.
    Some(match s {
        "satusehat" => OutboxDestination::Satusehat,
        "vclaim" | "v_claim" => OutboxDestination::VClaim,
        "pcare" | "p_care" => OutboxDestination::PCare,
        "antrean_online" => OutboxDestination::AntreanOnline,
        "aplicares" => OutboxDestination::Aplicares,
        "eklaim" | "e_klaim" => OutboxDestination::EKlaim,
        _ => return None,
    })
}

fn bridge_configured(state: &AppState, dest: OutboxDestination) -> bool {
    state
        .bridges
        .get(dest)
        .map(|b| b.configured())
        .unwrap_or(false)
}

/// Guard peran eksplisit untuk kombinasi yang tidak punya satu aksi RBAC
/// (mis. "Admin/Dokter" atau "Dokter/Perawat/Admin").
fn require_roles(user: &User, roles: &[Role], label: &str) -> ApiResult<()> {
    if roles.contains(&user.role) {
        Ok(())
    } else {
        Err(ApiError::forbidden(label))
    }
}

/// Satu panggilan live lewat adapter (tanpa outbox): dipakai cek
/// eligibilitas, insert/batal SEP, dan refresh bed. Job-nya sementara,
/// tidak pernah disimpan ke outbox.
async fn live_send(
    state: &AppState,
    dest: OutboxDestination,
    resource_type: &str,
    payload: Value,
) -> Result<SendOutcome, BridgeError> {
    let bridge = state.bridges.get(dest).ok_or_else(|| {
        BridgeError::NotConfigured(format!("{} belum dikonfigurasi", dest.label()))
    })?;
    if !bridge.configured() {
        return Err(BridgeError::NotConfigured(format!(
            "{} belum dikonfigurasi (cek env di docs/FASE2.md)",
            dest.label()
        )));
    }
    let job = new_outbox_job(dest, resource_type, "", "live", payload, 1);
    bridge.send(&job).await
}

// ---------------------------------------------------------------------------
// Status jembatan + outbox
// ---------------------------------------------------------------------------

pub async fn list_bridges(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ViewBridges)?;
    let counts = state.db.outbox_counts().await?;
    let healths = state.bridges.health_all().await;
    let mut out = Vec::with_capacity(healths.len());
    for (dest, health) in healths {
        let dstr = dest_to_str(dest);
        let mut pending = 0i64;
        let mut sent = 0i64;
        let mut acked = 0i64;
        let mut failed = 0i64;
        let mut dead = 0i64;
        for (d, s, n) in &counts {
            if d == dstr {
                match s.as_str() {
                    "pending" => pending = *n,
                    "sent" => sent = *n,
                    "acked" => acked = *n,
                    "failed" => failed = *n,
                    "dead" => dead = *n,
                    _ => {}
                }
            }
        }
        let last_success_at = state
            .db
            .list_outbox(Some("acked"), Some(dstr), 1)
            .await?
            .into_iter()
            .next()
            .map(|j| j.updated_at);
        out.push(json!({
            "destination": dest,
            "name": dest.label(),
            "health": health,
            "configured": bridge_configured(&state, dest),
            "counts": {
                "pending": pending,
                "sent": sent,
                "acked": acked,
                "failed": failed,
                "dead": dead,
            },
            "last_success_at": last_success_at,
        }));
    }
    Ok(Json(json!(out)))
}

pub async fn get_bridge(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(destination): Path<String>,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ViewBridges)?;
    let dest = dest_from_str(&destination)
        .ok_or_else(|| ApiError::not_found(format!("jembatan {destination} tidak dikenal")))?;
    let dstr = dest_to_str(dest);
    let jobs = state.db.list_outbox(None, Some(dstr), 10).await?;
    let health = match state.bridges.get(dest) {
        Some(b) => b.health().await,
        None => BridgeHealth::BelumDikonfigurasi,
    };
    Ok(Json(json!({
        "destination": dest,
        "name": dest.label(),
        "health": health,
        "configured": bridge_configured(&state, dest),
        "jobs": jobs,
    })))
}

pub async fn list_outbox(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<OutboxJob>>> {
    require_roles(&user, &[Role::Admin, Role::DokterUmum], "lihat outbox")?;
    let status = match q.get("status") {
        Some(s) if !s.trim().is_empty() => {
            match s.as_str() {
                "pending" | "sending" | "sent" | "acked" | "failed" | "dead" => {
                    Some(s.clone())
                }
                _ => {
                    return Err(ApiError::validation(format!(
                        "status outbox tidak dikenal: {s}"
                    )))
                }
            }
        }
        _ => None,
    };
    let destination = match q.get("destination") {
        Some(s) if !s.trim().is_empty() => {
            match dest_from_str(s) {
                // Normalisasi ke bentuk DB (mis. `v_claim` -> `vclaim`).
                Some(d) => Some(dest_to_str(d).to_string()),
                None => {
                    return Err(ApiError::validation(format!(
                        "destination tidak dikenal: {s}"
                    )))
                }
            }
        }
        _ => None,
    };
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
        .clamp(1, 500);
    Ok(Json(
        state
            .db
            .list_outbox(status.as_deref(), destination.as_deref(), limit)
            .await?,
    ))
}

pub async fn retry_outbox(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<OutboxJob>> {
    require(&user, Action::ManageOutbox)?;
    let job = state
        .db
        .get_outbox(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("outbox {id} tidak ditemukan")))?;
    state.db.retry_outbox(&id).await?;
    audit(
        &state.db,
        &user.name,
        "outbox.retried",
        "outbox",
        &id,
        json!({"status_before": job.status}),
    )
    .await;
    let job = state
        .db
        .get_outbox(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("outbox {id} tidak ditemukan")))?;
    Ok(Json(job))
}

pub async fn cancel_outbox(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<OutboxJob>> {
    require(&user, Action::ManageOutbox)?;
    let job = state
        .db
        .get_outbox(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("outbox {id} tidak ditemukan")))?;
    state.db.cancel_outbox(&id).await?;
    audit(
        &state.db,
        &user.name,
        "outbox.cancelled",
        "outbox",
        &id,
        json!({"status_before": job.status}),
    )
    .await;
    let job = state
        .db
        .get_outbox(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("outbox {id} tidak ditemukan")))?;
    Ok(Json(job))
}

// ---------------------------------------------------------------------------
// SEP (VClaim)
// ---------------------------------------------------------------------------

pub async fn list_seps(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<Sep>>> {
    let date = date_query(&q)?;
    Ok(Json(state.db.list_seps(&date.to_string()).await?))
}

#[derive(Deserialize)]
pub struct CreateSepDto {
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub bpjs_number: String,
    pub jenis: String,
    pub poli: String,
}

pub async fn create_sep(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateSepDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::CreateSep)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.bpjs_number, "bpjs_number")?;
    non_empty(&body.jenis, "jenis")?;
    non_empty(&body.poli, "poli")?;
    if body.jenis != "RawatJalan" && body.jenis != "RawatInap" {
        return Err(ApiError::validation(
            "jenis hanya boleh RawatJalan atau RawatInap",
        ));
    }
    let patient = state
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
    if !bridge_configured(&state, OutboxDestination::VClaim) {
        return Ok(not_configured("VClaim"));
    }
    let today = clinic_today().to_string();
    // 1) Cek eligibilitas via VClaim dulu (NIK tidak ditemukan -> 422).
    let eligibility = live_send(
        &state,
        OutboxDestination::VClaim,
        "Peserta",
        json!({"nik": patient.nik, "tanggal": today}),
    )
    .await;
    if let Err(e) = eligibility {
        return Ok(match e {
            BridgeError::Validation(msg) => unprocessable(&msg),
            BridgeError::NotConfigured(_) => not_configured("VClaim"),
            other => bridge_error(&other),
        });
    }
    // 2) Insert SEP via adapter. Nomor SEP lokal tetap mengikuti pola
    // repo (SEP-YYYYMMDD-### via `create_sep`); nomor yang dikembalikan
    // BPJS tersimpan sebagai external_id di outbox saat worker ack.
    let insert_body = json!({
        "noKartu": body.bpjs_number,
        "tglSep": today,
        "jnsPelayanan": if body.jenis == "RawatInap" { "2" } else { "1" },
        "poli": body.poli,
        "noMR": patient.id,
        "nik": patient.nik,
        "nama": patient.name,
    });
    if let Err(e) = live_send(
        &state,
        OutboxDestination::VClaim,
        "SEP",
        insert_body.clone(),
    )
    .await
    {
        return Ok(match e {
            BridgeError::Validation(msg) => unprocessable(&msg),
            BridgeError::NotConfigured(_) => not_configured("VClaim"),
            other => bridge_error(&other),
        });
    }
    // 3) Simpan + tautkan ke tagihan encounter (bila ada) + enqueue.
    let sep = state
        .db
        .create_sep(&Sep {
            id: String::new(),
            sep_number: String::new(),
            patient_id: patient.id.clone(),
            encounter_id: encounter_id.clone(),
            bpjs_number: body.bpjs_number,
            jenis: body.jenis,
            poli: body.poli,
            status: SepStatus::Aktif,
            created_by: user.name.clone(),
            created_at: now_clinic(),
        })
        .await?;
    if let Some(enc_id) = &encounter_id {
        if let Ok(invoices) = state.db.list_invoices(None).await {
            if let Some(inv) = invoices.iter().find(|i| i.encounter_id.as_deref() == Some(enc_id.as_str())) {
                if let Err(e) = state.db.link_invoice_sep(&inv.id, &sep.id).await {
                    tracing::warn!("gagal menautkan SEP {} ke {}: {e}", sep.id, inv.id);
                }
            }
        }
    }
    audit(
        &state.db,
        &user.name,
        "sep.created",
        "sep",
        &sep.id,
        json!({"patient_id": sep.patient_id, "jenis": sep.jenis, "poli": sep.poli}),
    )
    .await;
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::VClaim,
            "SEP",
            &sep.id,
            &format!("vclaim:SEP:{}:created", sep.sep_number),
            insert_body,
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(json!(sep))))
}

pub async fn get_sep(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let sep = state
        .db
        .get_sep(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("SEP {id} tidak ditemukan")))?;
    // Tagihan terkait: yang encounter-nya sama dengan encounter SEP.
    // (`Invoice` tidak membawa kolom `sep_id`, jadi pencocokan lewat
    // encounter — lihat juga `create_claim`.)
    let invoice = match &sep.encounter_id {
        Some(enc_id) => state
            .db
            .list_invoices(None)
            .await?
            .into_iter()
            .find(|i| i.encounter_id.as_deref() == Some(enc_id.as_str())),
        None => None,
    };
    let mut v = serde_json::to_value(&sep)?;
    v["invoice"] = serde_json::to_value(&invoice)?;
    Ok(Json(v))
}

pub async fn cancel_sep(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_roles(
        &user,
        &[Role::DokterUmum, Role::Admin],
        "batalkan SEP",
    )?;
    let sep = state
        .db
        .get_sep(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("SEP {id} tidak ditemukan")))?;
    if sep.status == SepStatus::Dibatalkan {
        return Ok((StatusCode::OK, Json(json!(sep))));
    }
    if !bridge_configured(&state, OutboxDestination::VClaim) {
        return Ok(not_configured("VClaim"));
    }
    // Batal via VClaim dulu; hanya bila berhasil, ubah status lokal.
    // (Tidak di-enqueue: worker yang mengirim ulang "SEP Batal" akan
    // menghapus SEP yang sudah dibatalkan.)
    if let Err(e) = live_send(
        &state,
        OutboxDestination::VClaim,
        "SEP Batal",
        json!({"noSep": sep.sep_number}),
    )
    .await
    {
        return Ok(match e {
            BridgeError::Validation(msg) => unprocessable(&msg),
            BridgeError::NotConfigured(_) => not_configured("VClaim"),
            other => bridge_error(&other),
        });
    }
    state
        .db
        .set_sep_status(&id, SepStatus::Dibatalkan)
        .await?;
    audit(
        &state.db,
        &user.name,
        "sep.cancelled",
        "sep",
        &id,
        json!({"sep_number": sep.sep_number}),
    )
    .await;
    let sep = state
        .db
        .get_sep(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("SEP {id} tidak ditemukan")))?;
    Ok((StatusCode::OK, Json(json!(sep))))
}

// ---------------------------------------------------------------------------
// Peserta BPJS (cek eligibilitas, live)
// ---------------------------------------------------------------------------

pub async fn check_peserta(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let nik = q
        .get("nik")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::validation("nik wajib diisi"))?;
    if nik.len() != 16 || !nik.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::validation("NIK harus 16 digit angka"));
    }
    let tanggal = match q.get("tanggal") {
        Some(s) if !s.trim().is_empty() => parse_date_param(s)?.to_string(),
        _ => clinic_today().to_string(),
    };
    if !bridge_configured(&state, OutboxDestination::VClaim) {
        return Ok(not_configured("VClaim"));
    }
    match live_send(
        &state,
        OutboxDestination::VClaim,
        "Peserta",
        json!({"nik": nik, "tanggal": tanggal}),
    )
    .await
    {
        Ok(outcome) => Ok((
            StatusCode::OK,
            Json(json!({
                "nik": nik,
                "tanggal": tanggal,
                "eligible": true,
                "no_kartu": outcome.external_id,
            })),
        )),
        Err(BridgeError::Validation(msg)) => Ok(unprocessable(&msg)),
        Err(BridgeError::NotConfigured(_)) => Ok(not_configured("VClaim")),
        Err(other) => Ok(bridge_error(&other)),
    }
}

// ---------------------------------------------------------------------------
// Rujukan (VClaim) + kunjungan PCare
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateReferralDto {
    pub patient_id: String,
    pub from_poli: String,
    pub to_facility: String,
    pub reason: String,
}

pub async fn create_referral(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateReferralDto>,
) -> ApiResult<(StatusCode, Json<Referral>)> {
    require(&user, Action::CreateReferral)?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.from_poli, "from_poli")?;
    non_empty(&body.to_facility, "to_facility")?;
    non_empty(&body.reason, "reason")?;
    let patient = state
        .db
        .get_patient(&body.patient_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("pasien {} tidak ditemukan", body.patient_id))
        })?;
    let referral = state
        .db
        .create_referral(&Referral {
            id: String::new(),
            patient_id: patient.id.clone(),
            from_poli: body.from_poli.clone(),
            to_facility: body.to_facility.clone(),
            reason: body.reason.clone(),
            status: ReferralStatus::Menunggu,
            created_at: now_clinic(),
        })
        .await?;
    audit(
        &state.db,
        &user.name,
        "referral.created",
        "referral",
        &referral.id,
        json!({"patient_id": referral.patient_id, "to_facility": referral.to_facility}),
    )
    .await;
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::VClaim,
            "Rujukan",
            &referral.id,
            &format!("vclaim:Rujukan:{}:created", referral.id),
            json!({
                "noKartu": patient.bpjs_number,
                "tglRujukan": clinic_today().to_string(),
                "from_poli": referral.from_poli,
                "ppkDirujuk": referral.to_facility,
                "reason": referral.reason,
            }),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(referral)))
}

pub async fn list_referrals(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<Referral>>> {
    let date = date_query(&q)?;
    Ok(Json(state.db.list_referrals(&date.to_string()).await?))
}

#[derive(Deserialize)]
pub struct CreatePcareVisitDto {
    pub patient_id: String,
    pub poli: String,
}

pub async fn create_pcare_visit(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreatePcareVisitDto>,
) -> ApiResult<(StatusCode, Json<PcareVisit>)> {
    require_roles(
        &user,
        &[Role::DokterUmum, Role::Perawat, Role::Admin],
        "catat kunjungan PCare",
    )?;
    non_empty(&body.patient_id, "patient_id")?;
    non_empty(&body.poli, "poli")?;
    let patient = state
        .db
        .get_patient(&body.patient_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!("pasien {} tidak ditemukan", body.patient_id))
        })?;
    let visit = state
        .db
        .create_pcare_visit(&PcareVisit {
            id: String::new(),
            patient_id: patient.id.clone(),
            visit_date: clinic_today(),
            poli: body.poli.clone(),
            status: "Tercatat".into(),
            created_at: now_clinic(),
        })
        .await?;
    audit(
        &state.db,
        &user.name,
        "pcare_visit.created",
        "pcare_visit",
        &visit.id,
        json!({"patient_id": visit.patient_id, "poli": visit.poli}),
    )
    .await;
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::PCare,
            "Kunjungan",
            &visit.id,
            &format!("pcare:Kunjungan:{}:created", visit.id),
            json!({
                "noKartu": patient.bpjs_number,
                "tglKunjungan": clinic_today().to_string(),
                "poli": visit.poli,
                "noMR": patient.id,
            }),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(visit)))
}

pub async fn list_pcare_visits(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<PcareVisit>>> {
    let date = date_query(&q)?;
    Ok(Json(state.db.list_pcare_visits(&date.to_string()).await?))
}

// ---------------------------------------------------------------------------
// Antrean online + bed (Aplicares)
// ---------------------------------------------------------------------------

pub async fn list_online_queue(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<OnlineBooking>>> {
    let date = date_query(&q)?;
    Ok(Json(state.db.list_online_queue(&date.to_string()).await?))
}

pub async fn sync_online_queue(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::SyncBridges)?;
    if !bridge_configured(&state, OutboxDestination::AntreanOnline) {
        return Ok(not_configured("Antrean Online"));
    }
    let today = clinic_today().to_string();
    let bookings = state.db.list_online_queue(&today).await?;
    let mut synced = 0i64;
    for b in bookings.iter().filter(|b| b.status == "Menunggu") {
        let no_kartu = state
            .db
            .get_patient(&b.patient_id)
            .await
            .ok()
            .flatten()
            .and_then(|p| p.bpjs_number);
        if try_enqueue(
            &state.db,
            &new_outbox_job(
                OutboxDestination::AntreanOnline,
                "Antrean",
                &b.booking_code,
                &format!("antrean_online:Antrean:{}:sync", b.booking_code),
                json!({
                    "kodebooking": b.booking_code,
                    "noKartu": no_kartu,
                    "poli": b.poli,
                    "estimasi": b.estimasi,
                    "tanggal": today,
                }),
                state.bridge_config.outbox_max_attempts,
            ),
        )
        .await
        {
            synced += 1;
        }
    }
    audit(
        &state.db,
        &user.name,
        "antrean.synced",
        "online_queue",
        &today,
        json!({"synced": synced}),
    )
    .await;
    Ok((StatusCode::OK, Json(json!({"date": today, "synced": synced}))))
}

pub async fn list_beds(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<BedAvailability>>> {
    Ok(Json(state.db.list_beds().await?))
}

pub async fn refresh_beds(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::SyncBridges)?;
    if !bridge_configured(&state, OutboxDestination::Aplicares) {
        return Ok(not_configured("Aplicares"));
    }
    let outcome = match live_send(&state, OutboxDestination::Aplicares, "Bed", json!({})).await
    {
        Ok(o) => o,
        Err(BridgeError::NotConfigured(_)) => return Ok(not_configured("Aplicares")),
        Err(e) => return Ok(bridge_error(&e)),
    };
    // Adapter mengembalikan daftar bed sebagai JSON di `note`
    // (stub: {"list": [{"kodeKelas", "total", "tersedia"}]}).
    let note = outcome.note.ok_or_else(|| {
        ApiError::internal("respons Aplicares tidak berisi daftar bed")
    })?;
    let parsed: Value = serde_json::from_str(&note).map_err(|_| {
        ApiError::internal("respons Aplicares bukan JSON yang valid")
    })?;
    let list = parsed
        .get("list")
        .and_then(|v| v.as_array())
        .or_else(|| parsed.as_array())
        .cloned()
        .ok_or_else(|| ApiError::internal("respons Aplicares tidak berisi daftar bed"))?;
    let mut beds = Vec::with_capacity(list.len());
    for item in &list {
        let class = item
            .get("kodeKelas")
            .or_else(|| item.get("class"))
            .or_else(|| item.get("kode_kelas"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let total = item.get("total").and_then(|v| v.as_i64());
        let available = item
            .get("tersedia")
            .or_else(|| item.get("available"))
            .and_then(|v| v.as_i64());
        if let (Some(class), Some(total), Some(available)) = (class, total, available) {
            beds.push(BedAvailability {
                class,
                total,
                available,
                updated_at: now_clinic(),
            });
        }
    }
    if beds.is_empty() {
        return Err(ApiError::internal(
            "respons Aplicares tidak berisi baris bed yang valid",
        ));
    }
    state.db.upsert_beds(&beds).await?;
    audit(
        &state.db,
        &user.name,
        "beds.refreshed",
        "bed_availability",
        "aplicares",
        json!({"classes": beds.len()}),
    )
    .await;
    Ok((StatusCode::OK, Json(json!(state.db.list_beds().await?))))
}

// ---------------------------------------------------------------------------
// Klaim (E-Klaim)
// ---------------------------------------------------------------------------

pub async fn list_claims(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> ApiResult<Json<Vec<Claim>>> {
    Ok(Json(state.db.list_claims().await?))
}

#[derive(Deserialize)]
pub struct CreateClaimDto {
    pub invoice_id: String,
    pub ina_cbg_code: String,
}

pub async fn create_claim(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateClaimDto>,
) -> ApiResult<(StatusCode, Json<Claim>)> {
    require(&user, Action::CreateClaim)?;
    non_empty(&body.invoice_id, "invoice_id")?;
    non_empty(&body.ina_cbg_code, "ina_cbg_code")?;
    let invoice = state
        .db
        .get_invoice(&body.invoice_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("tagihan {} tidak ditemukan", body.invoice_id)))?;
    // SEP diambil dari invoice bila ada. PENYIMPANGAN (keterbatasan
    // tipe): `Invoice` tidak membawa kolom `sep_id` (disimpan DB tapi
    // tidak diserialisasi), jadi SEP dicari lewat encounter tagihan di
    // SEP hari ini. Bila tidak ada, klaim tetap dibuat tanpa SEP.
    let sep_id = match &invoice.encounter_id {
        Some(enc_id) => state
            .db
            .list_seps(&clinic_today().to_string())
            .await?
            .into_iter()
            .find(|s| s.encounter_id.as_deref() == Some(enc_id.as_str()))
            .map(|s| s.id),
        None => None,
    };
    let claim = state
        .db
        .create_claim(&Claim {
            id: String::new(),
            invoice_id: invoice.id.clone(),
            sep_id: sep_id.clone(),
            ina_cbg_code: body.ina_cbg_code.clone(),
            amount: invoice.amount,
            status: ClaimStatus::Draft,
            submitted_at: None,
            created_at: now_clinic(),
        })
        .await?;
    audit(
        &state.db,
        &user.name,
        "claim.created",
        "claim",
        &claim.id,
        json!({"invoice_id": claim.invoice_id, "amount": claim.amount}),
    )
    .await;
    try_enqueue(
        &state.db,
        &new_outbox_job(
            OutboxDestination::EKlaim,
            "Klaim",
            &claim.id,
            &format!("eklaim:Klaim:{}:created", claim.id),
            json!({
                "invoice_id": claim.invoice_id,
                "sep_id": sep_id,
                "noSEP": sep_id,
                "ina_cbg_code": claim.ina_cbg_code,
                "amount": claim.amount,
            }),
            state.bridge_config.outbox_max_attempts,
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(claim)))
}

pub async fn get_claim(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Claim>> {
    let claim = state
        .db
        .get_claim(&id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("klaim {id} tidak ditemukan")))?;
    Ok(Json(claim))
}

// ---------------------------------------------------------------------------
// Ringkasan resource SATUSEHAT
// ---------------------------------------------------------------------------

/// 8 resource FHIR prioritas (FASE2.md §5). `Medication` dan `Procedure`
/// belum pernah di-enqueue (tidak ada job-nya) jadi selalu 0 — tetap
/// ditampilkan supaya tabel UI lengkap. Jangan tertukar
/// `MedicationRequest` dengan `Medication`.
const SATUSEHAT_RESOURCES: [&str; 8] = [
    "Patient",
    "Encounter",
    "Condition",
    "Observation",
    "MedicationRequest",
    "Medication",
    "Procedure",
    "AllergyIntolerance",
];

pub async fn satusehat_resources(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<Value>> {
    require(&user, Action::ViewBridges)?;
    let jobs = state.db.list_outbox(None, Some("satusehat"), 1000).await?;
    let mut out = Vec::with_capacity(SATUSEHAT_RESOURCES.len());
    for resource in SATUSEHAT_RESOURCES {
        let mut sent = 0i64;
        let mut failed = 0i64;
        let mut pending = 0i64;
        for j in jobs.iter().filter(|j| j.resource_type == resource) {
            match j.status {
                waras_core::bridge::OutboxStatus::Acked
                | waras_core::bridge::OutboxStatus::Sent => sent += 1,
                waras_core::bridge::OutboxStatus::Failed
                | waras_core::bridge::OutboxStatus::Dead => failed += 1,
                waras_core::bridge::OutboxStatus::Pending
                | waras_core::bridge::OutboxStatus::Sending => pending += 1,
            }
        }
        out.push(json!({
            "resource": resource,
            "sent": sent,
            "failed": failed,
            "pending": pending,
        }));
    }
    Ok(Json(json!(out)))
}
