//! Parser enum yang toleran untuk input API: menerima bentuk snake_case
//! (`dalam_konsultasi`), label Indonesia (`Dalam konsultasi`), maupun
//! nama varian (`DalamKonsultasi`). Dipakai untuk body dan query string,
//! supaya klien tidak gagal hanya karena perbedaan kapitalisasi.

use waras_core::types::{
    ConditionGroup, ConditionStatus, Gender, Insurance, InvoiceStatus, PaymentMethod,
    PrescriptionStatus, QueueStatus, VitalCode,
};

/// Normalisasi: huruf kecil, hanya alfanumerik yang dipertahankan.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub fn parse_gender(s: &str) -> Option<Gender> {
    match key(s).as_str() {
        "lakilaki" | "l" => Some(Gender::LakiLaki),
        "perempuan" | "p" => Some(Gender::Perempuan),
        _ => None,
    }
}

pub fn parse_insurance(s: &str) -> Option<Insurance> {
    match key(s).as_str() {
        "bpjs" | "bpjskesehatan" => Some(Insurance::Bpjs),
        "umum" => Some(Insurance::Umum),
        _ => None,
    }
}

pub fn parse_queue_status(s: &str) -> Option<QueueStatus> {
    match key(s).as_str() {
        "menunggu" => Some(QueueStatus::Menunggu),
        "dipanggil" => Some(QueueStatus::Dipanggil),
        "dalamkonsultasi" => Some(QueueStatus::DalamKonsultasi),
        "selesai" => Some(QueueStatus::Selesai),
        "tidakhadir" => Some(QueueStatus::TidakHadir),
        _ => None,
    }
}

pub fn parse_prescription_status(s: &str) -> Option<PrescriptionStatus> {
    match key(s).as_str() {
        "menunggukonfirmasi" => Some(PrescriptionStatus::MenungguKonfirmasi),
        "aktif" => Some(PrescriptionStatus::Aktif),
        "diblokir" => Some(PrescriptionStatus::Diblokir),
        "diberikan" => Some(PrescriptionStatus::Diberikan),
        "dibatalkan" => Some(PrescriptionStatus::Dibatalkan),
        _ => None,
    }
}

pub fn parse_invoice_status(s: &str) -> Option<InvoiceStatus> {
    match key(s).as_str() {
        "lunas" => Some(InvoiceStatus::Lunas),
        "menunggu" => Some(InvoiceStatus::Menunggu),
        "cicilan" => Some(InvoiceStatus::Cicilan),
        _ => None,
    }
}

pub fn parse_payment_method(s: &str) -> Option<PaymentMethod> {
    match key(s).as_str() {
        "tunai" => Some(PaymentMethod::Tunai),
        "qris" => Some(PaymentMethod::Qris),
        "transfer" => Some(PaymentMethod::Transfer),
        "deposit" | "depositpasien" => Some(PaymentMethod::Deposit),
        _ => None,
    }
}

pub fn parse_vital_code(s: &str) -> Option<VitalCode> {
    match key(s).as_str() {
        "tekanandarah" => Some(VitalCode::TekananDarah),
        "nadi" => Some(VitalCode::Nadi),
        "suhu" => Some(VitalCode::Suhu),
        "spo2" => Some(VitalCode::SpO2),
        "guladarah" => Some(VitalCode::GulaDarah),
        _ => None,
    }
}

pub fn parse_condition_group(s: &str) -> Option<ConditionGroup> {
    match key(s).as_str() {
        "utama" | "diagnosisutamakunjungan" => Some(ConditionGroup::Utama),
        "sekunder" | "diagnosissekunder" => Some(ConditionGroup::Sekunder),
        "kronis" | "diagnosiskronis" => Some(ConditionGroup::Kronis),
        "riwayat" | "riwayatdiagnosis" => Some(ConditionGroup::Riwayat),
        "tindakanmedis" => Some(ConditionGroup::TindakanMedis),
        _ => None,
    }
}

pub fn parse_condition_status(s: &str) -> Option<ConditionStatus> {
    match key(s).as_str() {
        "aktif" => Some(ConditionStatus::Aktif),
        "kronis" => Some(ConditionStatus::Kronis),
        "selesai" => Some(ConditionStatus::Selesai),
        _ => None,
    }
}
