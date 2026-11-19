//! Baris mentah SQLite (semua TEXT/INTEGER) + konversi ke tipe domain
//! `waras-core`. Dipakai dengan `sqlx::query_as` + `FromRow` saja —
//! tidak ada macro `query!`/`query_as!`.

use crate::convert::*;
use crate::{parse_date, parse_dt};
use sqlx::FromRow;
use waras_core::{
    types::{
        AllergyIntolerance, Appointment, AuditEntry, Condition, DailyStat, Drug, Encounter,
        Invoice, MedicationRequest, Notification, Observation, Patient, Payment, QueueEntry,
        SoapNote, SoapVersion, User,
    },
    WarasError,
};

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
