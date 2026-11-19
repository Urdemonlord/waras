//! Deterministic ID formats used across the system (and shown in the UI).
//! The formats mirror the FigCraft design: RM-000812, ENC-2026-11-19-014,
//! RX-1041, INV-2026-1181, A-045.

use chrono::NaiveDate;

/// No. rekam medis, mis. `RM-000812`.
pub fn rm_number(seq: u32) -> String {
    format!("RM-{seq:06}")
}

/// ID encounter, mis. `ENC-2026-11-19-014`.
pub fn encounter_id(date: NaiveDate, seq: u32) -> String {
    format!("ENC-{}-{seq:03}", date.format("%Y-%m-%d"))
}

/// No. resep, mis. `RX-1041`.
pub fn prescription_id(seq: u32) -> String {
    format!("RX-{seq}")
}

/// No. tagihan, mis. `INV-2026-1181`.
pub fn invoice_id(year: i32, seq: u32) -> String {
    format!("INV-{year}-{seq}")
}

/// Nomor antrean per poli, mis. `A-045` (prefix poli + urutan hari itu).
pub fn queue_number(prefix: &str, seq: u32) -> String {
    format!("{prefix}-{seq:03}")
}

/// Referensi FHIR untuk sebuah resource, mis. `Patient/812`.
pub fn fhir_ref(resource: &str, id: &str) -> String {
    format!("{resource}/{id}")
}

/// Bagian numerik dari ID pasien FHIR: `RM-000812` -> `812`.
pub fn fhir_patient_numeric_id(rm_number: &str) -> String {
    rm_number.trim_start_matches("RM-").trim_start_matches('0').to_string()
}
