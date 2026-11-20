//! Baris mentah SQLite (semua TEXT/INTEGER) + konversi ke tipe domain
//! `waras-core`. Dipakai dengan `sqlx::query_as` + `FromRow` saja —
//! tidak ada macro `query!`/`query_as!`.

use crate::convert::*;
use crate::{parse_date, parse_dt};
use sqlx::FromRow;
use waras_core::{
    types::{
        Admission, AdmissionStatus, AllergyIntolerance, ApiKey, AppSetting, Appointment,
        AuditEntry, BedStatus, BloodCrossmatch, BloodRequest, BloodRequestStatus, BloodStock,
        CareNote, Condition, DailyStat, Delivery, DeliveryRoom, DentalRecord, Drug, Encounter,
        HdMachine, HdMachineStatus, HdSession, HdSessionStatus, IgdObservation, IgdStatus,
        IgdVisit, InventoryItem, InventoryMove, Invoice, LabFlag, LabOrder, LabOrderStatus,
        LabResult, MedicationRequest, Notification, NutritionAssessment, NutritionDiet,
        Observation, Patient, Payment, QueueEntry, RadOrder, RadOrderStatus, RadReport,
        ShiftSchedule, SoapNote, SoapVersion, Staff, Surgery, SurgeryStatus, TriageLevel, User,
        Ward, WardBed,
    },
    WarasError,
};

/// Jam sekarang sebagai `YYYY-MM-DD HH:MM:SS` dengan TANGGAL KLINIK
/// (CONTRACT.md §0: transaksi baru memakai tanggal klinik, bukan tanggal
/// sistem) dan jam dari jam sistem. Dipakai `created_at`/`recorded_at`
/// runtime Fase 3 supaya `list_*` berfilter tanggal klinik menemukannya.
/// (Ditaruh di sini karena `lib.rs` milik koordinator; boleh dipindah ke
/// sana saat merge.)
pub(crate) fn clinic_now_string() -> String {
    use chrono::Timelike;
    let now = crate::now_naive();
    let today = crate::clinic_today();
    let dt = today
        .and_hms_opt(now.hour(), now.minute(), now.second())
        .unwrap_or(now);
    crate::fmt_dt(dt)
}

fn bad_fase3(what: &str, v: &str) -> WarasError {
    WarasError::Database(format!("nilai {what} tidak dikenal di DB: {v}"))
}

// --- Fase 3: enum <-> TEXT (snake_case, gaya convert.rs) -------------------

pub(crate) fn triage_to_str(t: TriageLevel) -> &'static str {
    match t {
        TriageLevel::Merah => "merah",
        TriageLevel::Kuning => "kuning",
        TriageLevel::Hijau => "hijau",
    }
}
pub(crate) fn triage_from_str(s: &str) -> Result<TriageLevel, WarasError> {
    Ok(match s {
        "merah" => TriageLevel::Merah,
        "kuning" => TriageLevel::Kuning,
        "hijau" => TriageLevel::Hijau,
        _ => return Err(bad_fase3("triage", s)),
    })
}

pub(crate) fn igd_status_to_str(s: IgdStatus) -> &'static str {
    match s {
        IgdStatus::Menunggu => "menunggu",
        IgdStatus::Observasi => "observasi",
        IgdStatus::Tindakan => "tindakan",
        IgdStatus::Resusitasi => "resusitasi",
        IgdStatus::Selesai => "selesai",
        IgdStatus::Dirujuk => "dirujuk",
    }
}
pub(crate) fn igd_status_from_str(s: &str) -> Result<IgdStatus, WarasError> {
    Ok(match s {
        "menunggu" => IgdStatus::Menunggu,
        "observasi" => IgdStatus::Observasi,
        "tindakan" => IgdStatus::Tindakan,
        "resusitasi" => IgdStatus::Resusitasi,
        "selesai" => IgdStatus::Selesai,
        "dirujuk" => IgdStatus::Dirujuk,
        _ => return Err(bad_fase3("igd_status", s)),
    })
}

pub(crate) fn bed_status_to_str(s: BedStatus) -> &'static str {
    match s {
        BedStatus::Terisi => "terisi",
        BedStatus::Kosong => "kosong",
        BedStatus::Dibersihkan => "dibersihkan",
    }
}
pub(crate) fn bed_status_from_str(s: &str) -> Result<BedStatus, WarasError> {
    Ok(match s {
        "terisi" => BedStatus::Terisi,
        "kosong" => BedStatus::Kosong,
        "dibersihkan" => BedStatus::Dibersihkan,
        _ => return Err(bad_fase3("bed_status", s)),
    })
}

pub(crate) fn admission_status_to_str(s: AdmissionStatus) -> &'static str {
    match s {
        AdmissionStatus::Dirawat => "dirawat",
        AdmissionStatus::Pulang => "pulang",
        AdmissionStatus::Dirujuk => "dirujuk",
    }
}
pub(crate) fn admission_status_from_str(s: &str) -> Result<AdmissionStatus, WarasError> {
    Ok(match s {
        "dirawat" => AdmissionStatus::Dirawat,
        "pulang" => AdmissionStatus::Pulang,
        "dirujuk" => AdmissionStatus::Dirujuk,
        _ => return Err(bad_fase3("admission_status", s)),
    })
}

pub(crate) fn surgery_status_to_str(s: SurgeryStatus) -> &'static str {
    match s {
        SurgeryStatus::Terjadwal => "terjadwal",
        SurgeryStatus::Menunggu => "menunggu",
        SurgeryStatus::Siap => "siap",
        SurgeryStatus::Berjalan => "berjalan",
        SurgeryStatus::Selesai => "selesai",
        SurgeryStatus::Batal => "batal",
    }
}
pub(crate) fn surgery_status_from_str(s: &str) -> Result<SurgeryStatus, WarasError> {
    Ok(match s {
        "terjadwal" => SurgeryStatus::Terjadwal,
        "menunggu" => SurgeryStatus::Menunggu,
        "siap" => SurgeryStatus::Siap,
        "berjalan" => SurgeryStatus::Berjalan,
        "selesai" => SurgeryStatus::Selesai,
        "batal" => SurgeryStatus::Batal,
        _ => return Err(bad_fase3("surgery_status", s)),
    })
}

pub(crate) fn lab_order_status_to_str(s: LabOrderStatus) -> &'static str {
    match s {
        LabOrderStatus::Menunggu => "menunggu",
        LabOrderStatus::Proses => "proses",
        LabOrderStatus::Selesai => "selesai",
        LabOrderStatus::Kritis => "kritis",
    }
}
pub(crate) fn lab_order_status_from_str(s: &str) -> Result<LabOrderStatus, WarasError> {
    Ok(match s {
        "menunggu" => LabOrderStatus::Menunggu,
        "proses" => LabOrderStatus::Proses,
        "selesai" => LabOrderStatus::Selesai,
        "kritis" => LabOrderStatus::Kritis,
        _ => return Err(bad_fase3("lab_order_status", s)),
    })
}

pub(crate) fn lab_flag_to_str(f: LabFlag) -> &'static str {
    match f {
        LabFlag::Normal => "normal",
        LabFlag::Tinggi => "tinggi",
        LabFlag::Rendah => "rendah",
        LabFlag::Kritis => "kritis",
    }
}
pub(crate) fn lab_flag_from_str(s: &str) -> Result<LabFlag, WarasError> {
    Ok(match s {
        "normal" => LabFlag::Normal,
        "tinggi" => LabFlag::Tinggi,
        "rendah" => LabFlag::Rendah,
        "kritis" => LabFlag::Kritis,
        _ => return Err(bad_fase3("lab_flag", s)),
    })
}

pub(crate) fn rad_order_status_to_str(s: RadOrderStatus) -> &'static str {
    match s {
        RadOrderStatus::Menunggu => "menunggu",
        RadOrderStatus::Proses => "proses",
        RadOrderStatus::Selesai => "selesai",
    }
}
pub(crate) fn rad_order_status_from_str(s: &str) -> Result<RadOrderStatus, WarasError> {
    Ok(match s {
        "menunggu" => RadOrderStatus::Menunggu,
        "proses" => RadOrderStatus::Proses,
        "selesai" => RadOrderStatus::Selesai,
        _ => return Err(bad_fase3("rad_order_status", s)),
    })
}

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: String,
    pub nip: String,
    pub username: String,
    pub name: String,
    pub role: String,
    pub active: i64,
    pub password_hash: String,
}
impl UserRow {
    pub fn into_domain(self) -> Result<User, WarasError> {
        Ok(User {
            id: self.id,
            nip: self.nip,
            username: self.username,
            name: self.name,
            role: role_from_str(&self.role)?,
            active: self.active != 0,
            password_hash: self.password_hash,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct PatientRow {
    pub id: String,
    pub nik: String,
    pub name: String,
    pub birth_date: String,
    pub gender: String,
    pub address: String,
    pub phone: Option<String>,
    pub blood_type: Option<String>,
    pub bpjs_number: Option<String>,
    pub insurance: String,
    pub created_at: String,
}
impl PatientRow {
    pub fn into_domain(self) -> Result<Patient, WarasError> {
        Ok(Patient {
            id: self.id,
            nik: self.nik,
            name: self.name,
            birth_date: parse_date(&self.birth_date)?,
            gender: gender_from_str(&self.gender)?,
            address: self.address,
            phone: self.phone,
            blood_type: self.blood_type,
            bpjs_number: self.bpjs_number,
            insurance: insurance_from_str(&self.insurance)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct AllergyRow {
    pub id: String,
    pub patient_id: String,
    pub substance: String,
    pub substance_class: Option<String>,
    pub criticality: String,
    pub note: Option<String>,
    pub recorded_at: String,
}
impl AllergyRow {
    pub fn into_domain(self) -> Result<AllergyIntolerance, WarasError> {
        Ok(AllergyIntolerance {
            id: self.id,
            patient_id: self.patient_id,
            substance: self.substance,
            substance_class: self.substance_class,
            criticality: criticality_from_str(&self.criticality)?,
            note: self.note,
            recorded_at: parse_dt(&self.recorded_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct EncounterRow {
    pub id: String,
    pub patient_id: String,
    pub poli: String,
    pub doctor_id: Option<String>,
    pub doctor_name: String,
    pub date: String,
    pub time: String,
    pub status: String,
    pub created_at: String,
}
impl EncounterRow {
    pub fn into_domain(self) -> Result<Encounter, WarasError> {
        Ok(Encounter {
            id: self.id,
            patient_id: self.patient_id,
            poli: self.poli,
            doctor_id: self.doctor_id,
            doctor_name: self.doctor_name,
            date: parse_date(&self.date)?,
            time: self.time,
            status: encounter_status_from_str(&self.status)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct ObservationRow {
    pub id: String,
    pub encounter_id: String,
    pub patient_id: String,
    pub code: String,
    pub value: String,
    pub unit: String,
    pub measured_at: String,
    pub measured_by: String,
    pub previous_value: Option<String>,
    pub previous_date: Option<String>,
}
impl ObservationRow {
    pub fn into_domain(self) -> Result<Observation, WarasError> {
        Ok(Observation {
            id: self.id,
            encounter_id: self.encounter_id,
            patient_id: self.patient_id,
            code: vital_code_from_str(&self.code)?,
            value: self.value,
            unit: self.unit,
            measured_at: parse_dt(&self.measured_at)?,
            measured_by: self.measured_by,
            previous_value: self.previous_value,
            previous_date: match self.previous_date {
                Some(d) => Some(parse_date(&d)?),
                None => None,
            },
        })
    }
}

#[derive(Debug, FromRow)]
pub struct SoapRow {
    pub id: String,
    pub encounter_id: String,
    pub subjective: String,
    pub objective: String,
    pub assessment: String,
    pub plan: String,
    pub author: String,
    pub status: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub draft_saved_at: Option<String>,
}
impl SoapRow {
    pub fn into_domain(self) -> Result<SoapNote, WarasError> {
        Ok(SoapNote {
            id: self.id,
            encounter_id: self.encounter_id,
            subjective: self.subjective,
            objective: self.objective,
            assessment: self.assessment,
            plan: self.plan,
            author: self.author,
            status: soap_status_from_str(&self.status)?,
            version: self.version as u32,
            created_at: parse_dt(&self.created_at)?,
            updated_at: parse_dt(&self.updated_at)?,
            draft_saved_at: match self.draft_saved_at {
                Some(d) => Some(parse_dt(&d)?),
                None => None,
            },
        })
    }
}

#[derive(Debug, FromRow)]
pub struct SoapVersionRow {
    pub id: String,
    pub soap_id: String,
    pub version: i64,
    pub subjective: String,
    pub objective: String,
    pub assessment: String,
    pub plan: String,
    pub author: String,
    pub saved_at: String,
}
impl SoapVersionRow {
    pub fn into_domain(self) -> Result<SoapVersion, WarasError> {
        Ok(SoapVersion {
            id: self.id,
            soap_id: self.soap_id,
            version: self.version as u32,
            subjective: self.subjective,
            objective: self.objective,
            assessment: self.assessment,
            plan: self.plan,
            author: self.author,
            saved_at: parse_dt(&self.saved_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct ConditionRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub condition_group: String,
    pub code: String,
    pub display: String,
    pub status: String,
    pub clinician: String,
    pub recorded_at: String,
}
impl ConditionRow {
    pub fn into_domain(self) -> Result<Condition, WarasError> {
        Ok(Condition {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            group: condition_group_from_str(&self.condition_group)?,
            code: self.code,
            display: self.display,
            status: condition_status_from_str(&self.status)?,
            clinician: self.clinician,
            recorded_at: parse_dt(&self.recorded_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct PrescriptionRow {
    pub id: String,
    pub encounter_id: String,
    pub patient_id: String,
    pub drug_name: String,
    pub drug_class: String,
    pub dosage: String,
    pub duration: String,
    pub status: String,
    pub doctor: String,
    pub blocked_reason: Option<String>,
    pub replaced_by: Option<String>,
    pub created_at: String,
}
impl PrescriptionRow {
    pub fn into_domain(self) -> Result<MedicationRequest, WarasError> {
        Ok(MedicationRequest {
            id: self.id,
            encounter_id: self.encounter_id,
            patient_id: self.patient_id,
            drug_name: self.drug_name,
            drug_class: self.drug_class,
            dosage: self.dosage,
            duration: self.duration,
            status: prescription_status_from_str(&self.status)?,
            doctor: self.doctor,
            blocked_reason: self.blocked_reason,
            replaced_by: self.replaced_by,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct DrugRow {
    pub kfa_code: String,
    pub name: String,
    pub drug_class: String,
    pub form: String,
    pub stock: i64,
    pub location: String,
    pub expiry: String,
}
impl DrugRow {
    pub fn into_domain(self) -> Result<Drug, WarasError> {
        Ok(Drug {
            kfa_code: self.kfa_code,
            name: self.name,
            drug_class: self.drug_class,
            form: self.form,
            stock: self.stock,
            location: self.location,
            expiry: parse_date(&self.expiry)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct QueueRow {
    pub id: String,
    pub number: String,
    pub patient_id: String,
    pub poli: String,
    pub doctor: String,
    pub date: String,
    pub time: String,
    pub status: String,
    pub called_at: Option<String>,
}
impl QueueRow {
    pub fn into_domain(self) -> Result<QueueEntry, WarasError> {
        Ok(QueueEntry {
            id: self.id,
            number: self.number,
            patient_id: self.patient_id,
            poli: self.poli,
            doctor: self.doctor,
            date: parse_date(&self.date)?,
            time: self.time,
            status: queue_status_from_str(&self.status)?,
            called_at: match self.called_at {
                Some(d) => Some(parse_dt(&d)?),
                None => None,
            },
        })
    }
}

#[derive(Debug, FromRow)]
pub struct AppointmentRow {
    pub id: String,
    pub patient_id: String,
    pub poli: String,
    pub date: String,
    pub appointment_time: String,
    pub consultation_time: String,
    pub status: String,
}
impl AppointmentRow {
    pub fn into_domain(self) -> Result<Appointment, WarasError> {
        Ok(Appointment {
            id: self.id,
            patient_id: self.patient_id,
            poli: self.poli,
            date: parse_date(&self.date)?,
            appointment_time: self.appointment_time,
            consultation_time: self.consultation_time,
            status: appointment_status_from_str(&self.status)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct InvoiceRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub payer: String,
    pub service: String,
    pub amount: i64,
    pub status: String,
    pub ina_cbg_code: Option<String>,
    pub sep_id: Option<String>,
    pub created_at: String,
}
impl InvoiceRow {
    pub fn into_domain(self) -> Result<Invoice, WarasError> {
        Ok(Invoice {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            payer: insurance_from_str(&self.payer)?,
            service: self.service,
            amount: self.amount,
            status: invoice_status_from_str(&self.status)?,
            ina_cbg_code: self.ina_cbg_code,
            sep_id: self.sep_id,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct PaymentRow {
    pub id: String,
    pub invoice_id: String,
    pub method: String,
    pub amount: i64,
    pub cashier: String,
    pub paid_at: String,
}
impl PaymentRow {
    pub fn into_domain(self) -> Result<Payment, WarasError> {
        Ok(Payment {
            id: self.id,
            invoice_id: self.invoice_id,
            method: payment_method_from_str(&self.method)?,
            amount: self.amount,
            cashier: self.cashier,
            paid_at: parse_dt(&self.paid_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct AuditRow {
    pub id: i64,
    pub timestamp: String,
    pub actor: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: String,
    pub prev_hash: String,
    pub entry_hash: String,
}
impl AuditRow {
    pub fn into_domain(self) -> Result<AuditEntry, WarasError> {
        Ok(AuditEntry {
            id: self.id,
            timestamp: parse_dt(&self.timestamp)?,
            actor: self.actor,
            action: self.action,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            details: serde_json::from_str(&self.details)
                .map_err(|e| WarasError::Database(format!("audit details bukan JSON: {e}")))?,
            prev_hash: self.prev_hash,
            entry_hash: self.entry_hash,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct DailyStatRow {
    pub date: String,
    pub registrations: i64,
    pub visits: i64,
    pub revenue_idr: i64,
    pub transactions: i64,
    pub lab_pending: i64,
    pub lab_critical: i64,
    pub emergency_red: i64,
    pub beds_occupied: i64,
    pub beds_total: i64,
}
impl DailyStatRow {
    pub fn into_domain(self) -> Result<DailyStat, WarasError> {
        Ok(DailyStat {
            date: parse_date(&self.date)?,
            registrations: self.registrations,
            visits: self.visits,
            revenue_idr: self.revenue_idr,
            transactions: self.transactions,
            lab_pending: self.lab_pending,
            lab_critical: self.lab_critical,
            emergency_red: self.emergency_red,
            beds_occupied: self.beds_occupied,
            beds_total: self.beds_total,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct NotificationRow {
    pub id: String,
    pub kind: String,
    pub message: String,
    pub time: String,
    pub date: String,
    pub read: i64,
}
impl NotificationRow {
    pub fn into_domain(self) -> Result<Notification, WarasError> {
        Ok(Notification {
            id: self.id,
            kind: notification_kind_from_str(&self.kind)?,
            message: self.message,
            time: self.time,
            date: parse_date(&self.date)?,
            read: self.read != 0,
        })
    }
}

// ---------------------------------------------------------------------------
// Fase 3 (FASE3.md §1–§2): baris tabel IGD / rawat inap / operasi &
// bersalin / lab / radiologi.
// ---------------------------------------------------------------------------

#[derive(Debug, FromRow)]
pub struct IgdVisitRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub complaint: String,
    pub triage: String,
    pub doctor: String,
    pub status: String,
    pub arrived_at: String,
    pub created_at: String,
}
impl IgdVisitRow {
    pub fn into_domain(self) -> Result<IgdVisit, WarasError> {
        Ok(IgdVisit {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            complaint: self.complaint,
            triage: triage_from_str(&self.triage)?,
            doctor: self.doctor,
            status: igd_status_from_str(&self.status)?,
            arrived_at: self.arrived_at,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct IgdObservationRow {
    pub id: String,
    pub igd_id: String,
    pub note: String,
    pub recorded_by: String,
    pub recorded_at: String,
}
impl IgdObservationRow {
    pub fn into_domain(self) -> Result<IgdObservation, WarasError> {
        Ok(IgdObservation {
            id: self.id,
            igd_id: self.igd_id,
            note: self.note,
            recorded_by: self.recorded_by,
            recorded_at: parse_dt(&self.recorded_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct WardRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub floor: String,
}
impl WardRow {
    pub fn into_domain(self) -> Result<Ward, WarasError> {
        Ok(Ward {
            id: self.id,
            name: self.name,
            kind: self.kind,
            floor: self.floor,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct WardBedRow {
    pub id: String,
    pub ward_id: String,
    pub room: String,
    pub bed_no: String,
    pub status: String,
    pub updated_at: String,
}
impl WardBedRow {
    pub fn into_domain(self) -> Result<WardBed, WarasError> {
        Ok(WardBed {
            id: self.id,
            ward_id: self.ward_id,
            room: self.room,
            bed_no: self.bed_no,
            status: bed_status_from_str(&self.status)?,
            updated_at: parse_dt(&self.updated_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct AdmissionRow {
    pub id: String,
    pub patient_id: String,
    pub bed_id: String,
    pub encounter_id: Option<String>,
    pub diagnosis_code: String,
    pub diagnosis_display: String,
    pub admitted_date: String,
    pub dpjp: String,
    pub fall_risk: i64,
    pub status: String,
    pub discharged_at: Option<String>,
    pub created_at: String,
}
impl AdmissionRow {
    pub fn into_domain(self) -> Result<Admission, WarasError> {
        Ok(Admission {
            id: self.id,
            patient_id: self.patient_id,
            bed_id: self.bed_id,
            encounter_id: self.encounter_id,
            diagnosis_code: self.diagnosis_code,
            diagnosis_display: self.diagnosis_display,
            admitted_date: parse_date(&self.admitted_date)?,
            dpjp: self.dpjp,
            fall_risk: self.fall_risk != 0,
            status: admission_status_from_str(&self.status)?,
            discharged_at: match self.discharged_at {
                Some(d) => Some(parse_dt(&d)?),
                None => None,
            },
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct CareNoteRow {
    pub id: String,
    pub admission_id: String,
    pub note: String,
    pub recorded_by: String,
    pub recorded_at: String,
}
impl CareNoteRow {
    pub fn into_domain(self) -> Result<CareNote, WarasError> {
        Ok(CareNote {
            id: self.id,
            admission_id: self.admission_id,
            note: self.note,
            recorded_by: self.recorded_by,
            recorded_at: parse_dt(&self.recorded_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct SurgeryRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub room: String,
    pub procedure: String,
    pub procedure_code: String,
    pub operator: String,
    pub team: String,
    pub scheduled_at: String,
    pub status: String,
    pub created_at: String,
}
impl SurgeryRow {
    pub fn into_domain(self) -> Result<Surgery, WarasError> {
        Ok(Surgery {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            room: self.room,
            procedure: self.procedure,
            procedure_code: self.procedure_code,
            operator: self.operator,
            team: self.team,
            scheduled_at: parse_dt(&self.scheduled_at)?,
            status: surgery_status_from_str(&self.status)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct DeliveryRoomRow {
    pub id: String,
    pub status: String,
}
impl DeliveryRoomRow {
    pub fn into_domain(self) -> Result<DeliveryRoom, WarasError> {
        Ok(DeliveryRoom {
            id: self.id,
            status: bed_status_from_str(&self.status)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct DeliveryRow {
    pub id: String,
    pub patient_id: String,
    pub age_years: i64,
    pub gravida_para: String,
    pub dilation_cm: i64,
    pub fetal_heart_rate: i64,
    pub midwife: String,
    pub doctor: String,
    pub plan: String,
    pub room: String,
    pub status: String,
    pub created_at: String,
}
impl DeliveryRow {
    pub fn into_domain(self) -> Result<Delivery, WarasError> {
        Ok(Delivery {
            id: self.id,
            patient_id: self.patient_id,
            age_years: self.age_years,
            gravida_para: self.gravida_para,
            dilation_cm: self.dilation_cm,
            fetal_heart_rate: self.fetal_heart_rate,
            midwife: self.midwife,
            doctor: self.doctor,
            plan: self.plan,
            room: self.room,
            status: self.status,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct LabOrderRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub test_name: String,
    pub loinc: String,
    pub ordered_from: String,
    pub ordered_by: String,
    pub status: String,
    pub created_at: String,
}
impl LabOrderRow {
    pub fn into_domain(self) -> Result<LabOrder, WarasError> {
        Ok(LabOrder {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            test_name: self.test_name,
            loinc: self.loinc,
            ordered_from: self.ordered_from,
            ordered_by: self.ordered_by,
            status: lab_order_status_from_str(&self.status)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct LabResultRow {
    pub id: String,
    pub order_id: String,
    pub analyte: String,
    pub loinc: Option<String>,
    pub value_text: String,
    pub unit: String,
    pub ref_text: String,
    pub ref_low: Option<f64>,
    pub ref_high: Option<f64>,
    pub critical_high: Option<f64>,
    pub critical_low: Option<f64>,
    pub flag: String,
    pub created_at: String,
}
impl LabResultRow {
    pub fn into_domain(self) -> Result<LabResult, WarasError> {
        Ok(LabResult {
            id: self.id,
            order_id: self.order_id,
            analyte: self.analyte,
            loinc: self.loinc,
            value_text: self.value_text,
            unit: self.unit,
            ref_text: self.ref_text,
            ref_low: self.ref_low,
            ref_high: self.ref_high,
            critical_high: self.critical_high,
            critical_low: self.critical_low,
            flag: lab_flag_from_str(&self.flag)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct RadOrderRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub modality: String,
    pub area: String,
    pub ordered_from: String,
    pub ordered_by: String,
    pub status: String,
    pub created_at: String,
}
impl RadOrderRow {
    pub fn into_domain(self) -> Result<RadOrder, WarasError> {
        Ok(RadOrder {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            modality: self.modality,
            area: self.area,
            ordered_from: self.ordered_from,
            ordered_by: self.ordered_by,
            status: rad_order_status_from_str(&self.status)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct RadReportRow {
    pub id: String,
    pub order_id: String,
    pub findings: String,
    pub radiologist: String,
    pub reported_at: String,
}
impl RadReportRow {
    pub fn into_domain(self) -> Result<RadReport, WarasError> {
        Ok(RadReport {
            id: self.id,
            order_id: self.order_id,
            findings: self.findings,
            radiologist: self.radiologist,
            reported_at: parse_dt(&self.reported_at)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Fase 4 (FASE4.md §1–§2): baris tabel hemodialisa / transfusi / gizi /
// gigi / inventori / kepegawaian / pengaturan / kunci API.
// ---------------------------------------------------------------------------

/// Dipakai agen API (pengaturan kunci) — disimpan di sini supaya
/// konversi status mesin HD konsisten dengan `*_from_str`.
#[allow(dead_code)]
pub(crate) fn hd_machine_status_to_str(s: HdMachineStatus) -> &'static str {
    match s {
        HdMachineStatus::Siap => "siap",
        HdMachineStatus::Terpakai => "terpakai",
        HdMachineStatus::Perawatan => "perawatan",
    }
}
pub(crate) fn hd_machine_status_from_str(s: &str) -> Result<HdMachineStatus, WarasError> {
    Ok(match s {
        "siap" => HdMachineStatus::Siap,
        "terpakai" => HdMachineStatus::Terpakai,
        "perawatan" => HdMachineStatus::Perawatan,
        _ => return Err(bad_fase3("hd_machine_status", s)),
    })
}

pub(crate) fn hd_session_status_to_str(s: HdSessionStatus) -> &'static str {
    match s {
        HdSessionStatus::Terjadwal => "terjadwal",
        HdSessionStatus::Berjalan => "berjalan",
        HdSessionStatus::Selesai => "selesai",
        HdSessionStatus::Batal => "batal",
    }
}
pub(crate) fn hd_session_status_from_str(s: &str) -> Result<HdSessionStatus, WarasError> {
    Ok(match s {
        "terjadwal" => HdSessionStatus::Terjadwal,
        "berjalan" => HdSessionStatus::Berjalan,
        "selesai" => HdSessionStatus::Selesai,
        "batal" => HdSessionStatus::Batal,
        _ => return Err(bad_fase3("hd_session_status", s)),
    })
}

pub(crate) fn blood_request_status_to_str(s: BloodRequestStatus) -> &'static str {
    match s {
        BloodRequestStatus::Menunggu => "menunggu",
        BloodRequestStatus::Crossmatch => "crossmatch",
        BloodRequestStatus::Siap => "siap",
        BloodRequestStatus::Diberikan => "diberikan",
        BloodRequestStatus::Batal => "batal",
    }
}
pub(crate) fn blood_request_status_from_str(s: &str) -> Result<BloodRequestStatus, WarasError> {
    Ok(match s {
        "menunggu" => BloodRequestStatus::Menunggu,
        "crossmatch" => BloodRequestStatus::Crossmatch,
        "siap" => BloodRequestStatus::Siap,
        "diberikan" => BloodRequestStatus::Diberikan,
        "batal" => BloodRequestStatus::Batal,
        _ => return Err(bad_fase3("blood_request_status", s)),
    })
}

#[derive(Debug, FromRow)]
pub struct HdMachineRow {
    pub id: String,
    pub name: String,
    pub status: String,
}
impl HdMachineRow {
    pub fn into_domain(self) -> Result<HdMachine, WarasError> {
        Ok(HdMachine {
            id: self.id,
            name: self.name,
            status: hd_machine_status_from_str(&self.status)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct HdSessionRow {
    pub id: String,
    pub patient_id: String,
    pub machine_id: String,
    pub encounter_id: Option<String>,
    pub dialyzer: String,
    pub duration_minutes: i64,
    pub blood_flow: i64,
    pub dialysate_flow: i64,
    pub uf_goal_ml: i64,
    pub status: String,
    pub scheduled_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub notes: String,
    pub created_at: String,
}
impl HdSessionRow {
    pub fn into_domain(self) -> Result<HdSession, WarasError> {
        Ok(HdSession {
            id: self.id,
            patient_id: self.patient_id,
            machine_id: self.machine_id,
            encounter_id: self.encounter_id,
            dialyzer: self.dialyzer,
            duration_minutes: self.duration_minutes,
            blood_flow: self.blood_flow,
            dialysate_flow: self.dialysate_flow,
            uf_goal_ml: self.uf_goal_ml,
            status: hd_session_status_from_str(&self.status)?,
            scheduled_at: self.scheduled_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            notes: self.notes,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct BloodStockRow {
    pub id: String,
    pub blood_type: String,
    pub component: String,
    pub units: i64,
    pub expires_at: String,
    pub status: String,
}
impl BloodStockRow {
    pub fn into_domain(self) -> Result<BloodStock, WarasError> {
        Ok(BloodStock {
            id: self.id,
            blood_type: self.blood_type,
            component: self.component,
            units: self.units,
            expires_at: parse_date(&self.expires_at)?,
            status: self.status,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct BloodRequestRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub blood_type: String,
    pub component: String,
    pub units: i64,
    pub reason: String,
    pub status: String,
    pub requested_by: String,
    pub created_at: String,
}
impl BloodRequestRow {
    pub fn into_domain(self) -> Result<BloodRequest, WarasError> {
        Ok(BloodRequest {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            blood_type: self.blood_type,
            component: self.component,
            units: self.units,
            reason: self.reason,
            status: blood_request_status_from_str(&self.status)?,
            requested_by: self.requested_by,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct BloodCrossmatchRow {
    pub id: String,
    pub request_id: String,
    pub bag_id: String,
    pub result: String,
    pub tested_by: String,
    pub tested_at: String,
}
impl BloodCrossmatchRow {
    pub fn into_domain(self) -> Result<BloodCrossmatch, WarasError> {
        Ok(BloodCrossmatch {
            id: self.id,
            request_id: self.request_id,
            bag_id: self.bag_id,
            result: self.result,
            tested_by: self.tested_by,
            tested_at: parse_dt(&self.tested_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct NutritionAssessmentRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub weight_kg: f64,
    pub height_cm: f64,
    pub bmi: f64,
    pub imt_category: String,
    pub risk: String,
    pub diet: String,
    pub notes: String,
    pub assessed_by: String,
    pub created_at: String,
}
impl NutritionAssessmentRow {
    pub fn into_domain(self) -> Result<NutritionAssessment, WarasError> {
        Ok(NutritionAssessment {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            weight_kg: self.weight_kg,
            height_cm: self.height_cm,
            bmi: self.bmi,
            imt_category: self.imt_category,
            risk: self.risk,
            diet: self.diet,
            notes: self.notes,
            assessed_by: self.assessed_by,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct NutritionDietRow {
    pub id: String,
    pub patient_id: String,
    pub diet_type: String,
    pub calories: i64,
    pub notes: String,
    pub start_date: String,
    pub status: String,
}
impl NutritionDietRow {
    pub fn into_domain(self) -> Result<NutritionDiet, WarasError> {
        Ok(NutritionDiet {
            id: self.id,
            patient_id: self.patient_id,
            diet_type: self.diet_type,
            calories: self.calories,
            notes: self.notes,
            start_date: parse_date(&self.start_date)?,
            status: self.status,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct DentalRecordRow {
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub tooth_number: String,
    pub diagnosis: String,
    pub diagnosis_code: String,
    pub treatment: String,
    pub treatment_code: String,
    pub dentist: String,
    pub status: String,
    pub created_at: String,
}
impl DentalRecordRow {
    pub fn into_domain(self) -> Result<DentalRecord, WarasError> {
        Ok(DentalRecord {
            id: self.id,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            tooth_number: self.tooth_number,
            diagnosis: self.diagnosis,
            diagnosis_code: self.diagnosis_code,
            treatment: self.treatment,
            treatment_code: self.treatment_code,
            dentist: self.dentist,
            status: self.status,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct InventoryItemRow {
    pub id: String,
    pub name: String,
    pub category: String,
    pub unit: String,
    pub stock: i64,
    pub min_stock: i64,
    pub location: String,
    pub expiry: String,
    pub created_at: String,
}
impl InventoryItemRow {
    pub fn into_domain(self) -> Result<InventoryItem, WarasError> {
        Ok(InventoryItem {
            id: self.id,
            name: self.name,
            category: self.category,
            unit: self.unit,
            stock: self.stock,
            min_stock: self.min_stock,
            location: self.location,
            expiry: self.expiry,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct InventoryMoveRow {
    pub id: String,
    pub item_id: String,
    pub kind: String,
    pub qty: i64,
    pub note: String,
    pub by_name: String,
    pub created_at: String,
}
impl InventoryMoveRow {
    pub fn into_domain(self) -> Result<InventoryMove, WarasError> {
        Ok(InventoryMove {
            id: self.id,
            item_id: self.item_id,
            kind: self.kind,
            qty: self.qty,
            note: self.note,
            by_name: self.by_name,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct StaffRow {
    pub id: String,
    pub name: String,
    pub nip: String,
    pub role: String,
    pub poli: String,
    pub phone: String,
    pub active: i64,
}
impl StaffRow {
    pub fn into_domain(self) -> Result<Staff, WarasError> {
        Ok(Staff {
            id: self.id,
            name: self.name,
            nip: self.nip,
            role: self.role,
            poli: self.poli,
            phone: self.phone,
            active: self.active != 0,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct ShiftScheduleRow {
    pub id: String,
    pub staff_id: String,
    pub date: String,
    pub shift: String,
    pub poli: String,
    pub created_at: String,
}
impl ShiftScheduleRow {
    pub fn into_domain(self) -> Result<ShiftSchedule, WarasError> {
        Ok(ShiftSchedule {
            id: self.id,
            staff_id: self.staff_id,
            date: parse_date(&self.date)?,
            shift: self.shift,
            poli: self.poli,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct AppSettingRow {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}
impl AppSettingRow {
    pub fn into_domain(self) -> Result<AppSetting, WarasError> {
        Ok(AppSetting {
            key: self.key,
            value: self.value,
            updated_at: parse_dt(&self.updated_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
/// Baris `api_keys`; repo pembuatan kunci ada di agen API Publik.
#[allow(dead_code)]
pub struct ApiKeyRow {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_hash: String,
    pub scopes: String,
    pub created_by: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked: i64,
}
#[allow(dead_code)]
impl ApiKeyRow {
    pub fn into_domain(self) -> Result<ApiKey, WarasError> {
        Ok(ApiKey {
            id: self.id,
            name: self.name,
            key_prefix: self.key_prefix,
            key_hash: self.key_hash,
            scopes: self.scopes,
            created_by: self.created_by,
            created_at: parse_dt(&self.created_at)?,
            last_used_at: match self.last_used_at {
                Some(d) => Some(parse_dt(&d)?),
                None => None,
            },
            revoked: self.revoked != 0,
        })
    }
}
