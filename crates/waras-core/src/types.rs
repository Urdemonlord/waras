//! Domain types, shaped after HL7 FHIR R4 resources from the first schema
//! (paper Bagian 4/6): Patient, Encounter, Observation (tanda vital),
//! Condition, MedicationRequest, AllergyIntolerance, Appointment, plus the
//! operational types (Queue, Drug/KFA, Invoice, Audit, Notification).
//!
//! Field names are Indonesian where the UI shows them; serde renames keep
//! the JSON API in snake_case.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pengguna & RBAC
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    DokterUmum,
    Perawat,
    Apoteker,
    Kasir,
    Admin,
}

impl Role {
    pub fn label(&self) -> &'static str {
        match self {
            Role::DokterUmum => "Dokter Umum",
            Role::Perawat => "Perawat",
            Role::Apoteker => "Apoteker",
            Role::Kasir => "Kasir",
            Role::Admin => "Admin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub nip: String,
    pub username: String,
    pub name: String,
    pub role: Role,
    pub active: bool,
    /// Argon2 password hash. Never serialized to API responses.
    #[serde(skip_serializing)]
    pub password_hash: String,
}

/// Credentials for login (NIP or username + password).
#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

// ---------------------------------------------------------------------------
// Pasien (FHIR Patient)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    LakiLaki,
    Perempuan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Insurance {
    Bpjs,
    Umum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patient {
    /// No. RM, mis. `RM-000812`. Primary key.
    pub id: String,
    /// NIK 16 digit.
    pub nik: String,
    pub name: String,
    pub birth_date: NaiveDate,
    pub gender: Gender,
    pub address: String,
    pub phone: Option<String>,
    pub blood_type: Option<String>,
    pub bpjs_number: Option<String>,
    pub insurance: Insurance,
    pub created_at: NaiveDateTime,
}

impl Patient {
    /// Umur dalam tahun pada tanggal tertentu.
    pub fn age_on(&self, on: NaiveDate) -> i64 {
        let mut age = on.year() - self.birth_date.year();
        if (on.month(), on.day()) < (self.birth_date.month(), self.birth_date.day()) {
            age -= 1;
        }
        age as i64
    }

    /// Referensi FHIR, mis. `Patient/812`.
    pub fn fhir_ref(&self) -> String {
        crate::ids::fhir_ref("Patient", &crate::ids::fhir_patient_numeric_id(&self.id))
    }
}

// ---------------------------------------------------------------------------
// Alergi (FHIR AllergyIntolerance)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Criticality {
    Tinggi,
    Sedang,
    Rendah,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllergyIntolerance {
    pub id: String,
    pub patient_id: String,
    /// Zat pemicu, mis. `Penisilin`.
    pub substance: String,
    /// Kelas obat yang cocok dengan `drug_class` pada obat, mis. `penicillin`.
    /// Dipakai oleh aturan keamanan obat.
    pub substance_class: Option<String>,
    pub criticality: Criticality,
    pub note: Option<String>,
    pub recorded_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Kunjungan (FHIR Encounter)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterStatus {
    Aktif,
    DalamPemeriksaan,
    Selesai,
    Dibatalkan,
}

impl EncounterStatus {
    pub fn label(&self) -> &'static str {
        match self {
            EncounterStatus::Aktif => "Aktif",
            EncounterStatus::DalamPemeriksaan => "Dalam pemeriksaan",
            EncounterStatus::Selesai => "Selesai",
            EncounterStatus::Dibatalkan => "Dibatalkan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encounter {
    /// Mis. `ENC-2026-11-19-014`.
    pub id: String,
    pub patient_id: String,
    pub poli: String,
    /// ID pengguna dokter bila dokter itu pengguna WARAS; None untuk dokter
    /// luar yang hanya dicatat namanya (mis. dokter rujukan di data seed).
    pub doctor_id: Option<String>,
    pub doctor_name: String,
    pub date: NaiveDate,
    /// Jam mulai kunjungan, mis. `09.15`.
    pub time: String,
    pub status: EncounterStatus,
    pub created_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Tanda vital (FHIR Observation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VitalCode {
    TekananDarah,
    Nadi,
    Suhu,
    SpO2,
    GulaDarah,
}

impl VitalCode {
    pub fn label(&self) -> &'static str {
        match self {
            VitalCode::TekananDarah => "Tekanan darah",
            VitalCode::Nadi => "Nadi",
            VitalCode::Suhu => "Suhu",
            VitalCode::SpO2 => "SpO2",
            VitalCode::GulaDarah => "Gula darah",
        }
    }

    pub fn unit(&self) -> &'static str {
        match self {
            VitalCode::TekananDarah => "mmHg",
            VitalCode::Nadi => "x/mnt",
            VitalCode::Suhu => "°C",
            VitalCode::SpO2 => "%",
            VitalCode::GulaDarah => "mg/dL",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub encounter_id: String,
    pub patient_id: String,
    pub code: VitalCode,
    /// Nilai terukur, mis. `130/85`.
    pub value: String,
    pub unit: String,
    pub measured_at: NaiveDateTime,
    pub measured_by: String,
    /// Nilai pengukuran sebelumnya (untuk kolom "Sebelumnya" di UI).
    pub previous_value: Option<String>,
    pub previous_date: Option<NaiveDate>,
}

// ---------------------------------------------------------------------------
// SOAP
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoapStatus {
    Draft,
    Lengkap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoapNote {
    pub id: String,
    pub encounter_id: String,
    pub subjective: String,
    pub objective: String,
    pub assessment: String,
    pub plan: String,
    pub author: String,
    pub status: SoapStatus,
    pub version: u32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// Kapan draft terakhir tersimpan otomatis.
    pub draft_saved_at: Option<NaiveDateTime>,
}

/// Satu versi tersimpan dari SOAP (riwayat versi, tidak pernah ditimpa).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoapVersion {
    pub id: String,
    pub soap_id: String,
    pub version: u32,
    pub subjective: String,
    pub objective: String,
    pub assessment: String,
    pub plan: String,
    pub author: String,
    pub saved_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Diagnosis (FHIR Condition)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionGroup {
    Utama,
    Sekunder,
    Kronis,
    Riwayat,
    TindakanMedis,
}

impl ConditionGroup {
    pub fn label(&self) -> &'static str {
        match self {
            ConditionGroup::Utama => "Diagnosis utama kunjungan",
            ConditionGroup::Sekunder => "Diagnosis sekunder",
            ConditionGroup::Kronis => "Diagnosis kronis",
            ConditionGroup::Riwayat => "Riwayat diagnosis",
            ConditionGroup::TindakanMedis => "Tindakan medis",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionStatus {
    Aktif,
    Kronis,
    Selesai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub id: String,
    pub patient_id: String,
    /// Encounter yang mengikat diagnosis ini; None untuk diagnosis kronis /
    /// riwayat yang tidak terikat satu kunjungan.
    pub encounter_id: Option<String>,
    pub group: ConditionGroup,
    /// Kode ICD-10 (atau ICD-9-CM untuk tindakan), mis. `J20.9`, `89.52`.
    pub code: String,
    pub display: String,
    pub status: ConditionStatus,
    pub clinician: String,
    pub recorded_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Resep (FHIR MedicationRequest)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrescriptionStatus {
    /// Menunggu konfirmasi dokter (mis. pengganti setelah pemblokiran).
    MenungguKonfirmasi,
    Aktif,
    /// Diblokir otomatis oleh aturan keamanan obat.
    Diblokir,
    Diberikan,
    Dibatalkan,
}

impl PrescriptionStatus {
    pub fn label(&self) -> &'static str {
        match self {
            PrescriptionStatus::MenungguKonfirmasi => "Menunggu konfirmasi",
            PrescriptionStatus::Aktif => "Aktif",
            PrescriptionStatus::Diblokir => "Diblokir",
            PrescriptionStatus::Diberikan => "Diberikan",
            PrescriptionStatus::Dibatalkan => "Dibatalkan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicationRequest {
    /// Mis. `RX-1041`.
    pub id: String,
    pub encounter_id: String,
    pub patient_id: String,
    pub drug_name: String,
    /// Kelas obat, mis. `penicillin`, `macrolide`. Dipakai aturan keamanan.
    pub drug_class: String,
    /// Frekuensi, mis. `1x1`. Kekuatan obat (mis. `500 mg`) ada di
    /// `drug_name`; dosage TIDAK boleh mengulang kekuatan
    /// (lihat `dosage::normalize_dosage`).
    pub dosage: String,
    /// Durasi, mis. `3 hari`.
    pub duration: String,
    pub status: PrescriptionStatus,
    pub doctor: String,
    /// Alasan pemblokiran (hanya bila status Diblokir).
    pub blocked_reason: Option<String>,
    /// ID resep pengganti (bila resep ini diblokir lalu diganti).
    pub replaced_by: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Input resep baru dari API/UI.
#[derive(Debug, Clone, Deserialize)]
pub struct NewPrescription {
    pub encounter_id: String,
    pub drug_name: String,
    pub drug_class: String,
    pub dosage: String,
    pub duration: String,
}

// ---------------------------------------------------------------------------
// Obat & stok (KFA)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drug {
    /// Kode KFA, mis. `KFA-9201`.
    pub kfa_code: String,
    pub name: String,
    /// Kelas obat untuk aturan keamanan, mis. `penicillin`.
    pub drug_class: String,
    /// Bentuk sediaan: Tablet / Kapsul / Inhaler.
    pub form: String,
    pub stock: i64,
    pub location: String,
    pub expiry: NaiveDate,
}

impl Drug {
    /// Status stok dari jumlah sisa. Ambang ini cocok dengan data desain:
    /// 12 Kritis, 15 Segera, 22 Segera, 3 Kritis, 540+ Aman.
    pub fn stock_status(&self) -> StockStatus {
        if self.stock <= 12 {
            StockStatus::Kritis
        } else if self.stock <= 30 {
            StockStatus::Segera
        } else {
            StockStatus::Aman
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockStatus {
    Aman,
    Segera,
    Kritis,
}

impl StockStatus {
    pub fn label(&self) -> &'static str {
        match self {
            StockStatus::Aman => "Aman",
            StockStatus::Segera => "Segera",
            StockStatus::Kritis => "Kritis",
        }
    }
}

// ---------------------------------------------------------------------------
// Antrean & janji
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    Menunggu,
    Dipanggil,
    DalamKonsultasi,
    Selesai,
    TidakHadir,
}

impl QueueStatus {
    pub fn label(&self) -> &'static str {
        match self {
            QueueStatus::Menunggu => "Menunggu",
            QueueStatus::Dipanggil => "Dipanggil",
            QueueStatus::DalamKonsultasi => "Dalam konsultasi",
            QueueStatus::Selesai => "Selesai",
            QueueStatus::TidakHadir => "Tidak hadir",
        }
    }

    /// Dipanggil dihitung sebagai "dalam konsultasi" di donut antrean
    /// (pasien sudah dipanggil menuju pemeriksaan).
    pub fn counts_as_in_consultation(&self) -> bool {
        matches!(self, QueueStatus::Dipanggil | QueueStatus::DalamKonsultasi)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    /// Mis. `A-045`.
    pub number: String,
    pub patient_id: String,
    pub poli: String,
    pub doctor: String,
    pub date: NaiveDate,
    /// Jam check-in, mis. `07.12`.
    pub time: String,
    pub status: QueueStatus,
    pub called_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppointmentStatus {
    Terkonfirmasi,
    Menunggu,
    Selesai,
    Dibatalkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appointment {
    pub id: String,
    pub patient_id: String,
    pub poli: String,
    pub date: NaiveDate,
    /// Jam janji (waktu booking), mis. `07.30`.
    pub appointment_time: String,
    /// Jam konsultasi (slot pemeriksaan), mis. `09.30`.
    pub consultation_time: String,
    pub status: AppointmentStatus,
}

// ---------------------------------------------------------------------------
// Tagihan & pembayaran (Billing & Kasir)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Lunas,
    Menunggu,
    Cicilan,
}

impl InvoiceStatus {
    pub fn label(&self) -> &'static str {
        match self {
            InvoiceStatus::Lunas => "Lunas",
            InvoiceStatus::Menunggu => "Menunggu",
            InvoiceStatus::Cicilan => "Cicilan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    /// Mis. `INV-2026-1181`.
    pub id: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub payer: Insurance,
    /// Layanan, mis. `Poli Umum + Lab`.
    pub service: String,
    /// Jumlah dalam rupiah.
    pub amount: i64,
    pub status: InvoiceStatus,
    /// Kode grouping INA-CBG bila ada (Fase 1: boleh kosong).
    pub ina_cbg_code: Option<String>,
    /// SEP terkait (Fase 2, diisi `link_invoice_sep`).
    pub sep_id: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Tunai,
    Qris,
    Transfer,
    Deposit,
}

impl PaymentMethod {
    pub fn label(&self) -> &'static str {
        match self {
            PaymentMethod::Tunai => "Tunai",
            PaymentMethod::Qris => "QRIS",
            PaymentMethod::Transfer => "Transfer",
            PaymentMethod::Deposit => "Deposit pasien",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub id: String,
    pub invoice_id: String,
    pub method: PaymentMethod,
    pub amount: i64,
    pub cashier: String,
    pub paid_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Jejak audit (append-only) & notifikasi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: NaiveDateTime,
    pub actor: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: serde_json::Value,
    /// Hash entri sebelumnya (rantai hash; entri pertama memakai "GENESIS").
    pub prev_hash: String,
    /// SHA-256 atas (prev_hash + isi entri).
    pub entry_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Info,
    Kritis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub kind: NotificationKind,
    pub message: String,
    /// Jam tampil, mis. `09.15`.
    pub time: String,
    pub date: NaiveDate,
    pub read: bool,
}

// ---------------------------------------------------------------------------
// Statistik harian & dasbor
// ---------------------------------------------------------------------------

/// Agregat harian yang di-seed dari data desain (riwayat 7 hari + angka
/// hari ini). Dipakai dasbor untuk angka yang bukan hasil hitung langsung
/// dari tabel domain (kunjungan 7 hari, pendaftaran hari ini, pendapatan).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStat {
    pub date: NaiveDate,
    /// Pasien terdaftar hari itu (248 pada 19 Nov 2026).
    pub registrations: i64,
    /// Kunjungan (grafik 7 hari; Rabu tertinggi 210, rata-rata 178/hari).
    pub visits: i64,
    /// Pendapatan hari itu dalam rupiah (Rp 18,4 jt).
    pub revenue_idr: i64,
    /// Jumlah transaksi hari itu (142).
    pub transactions: i64,
    /// Hasil lab menunggu (26) dan nilai kritis (2).
    pub lab_pending: i64,
    pub lab_critical: i64,
    /// Pasien IGD triase merah saat ini (3).
    pub emergency_red: i64,
    pub beds_occupied: i64,
    pub beds_total: i64,
}

/// KPI dasbor, dihitung dari data nyata (antrean + janji + tagihan hidup,
/// agregat harian dari DailyStat).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardKpis {
    pub patients_today: i64,
    pub queue_active: i64,
    pub queue_waiting: i64,
    pub queue_in_consultation: i64,
    pub queue_done: i64,
    pub queue_no_show: i64,
    pub queue_total: i64,
    pub lab_pending: i64,
    pub lab_critical: i64,
    pub followups_7d: i64,
    pub emergency_red: i64,
    pub visits_7d: Vec<VisitDay>,
    pub visits_avg_7d: i64,
    pub visits_peak_7d: i64,
    pub avg_wait_minutes: i64,
    pub longest_wait_minutes: i64,
    pub beds_occupied: i64,
    pub beds_total: i64,
    pub revenue_today_idr: i64,
    pub revenue_transactions: i64,
    pub unpaid_invoices_idr: i64,
    pub unpaid_invoices_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitDay {
    pub day: String,
    pub date: NaiveDate,
    pub count: i64,
}

// ---------------------------------------------------------------------------
// Jembatan nasional (Fase 2): SEP, rujukan, kunjungan PCare, antrean online,
// ketersediaan bed, klaim. Lihat docs/FASE2.md.
// ---------------------------------------------------------------------------

/// Status Surat Eligibilitas Peserta (SEP) VClaim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SepStatus {
    Aktif,
    Gagal,
    Dibatalkan,
}

/// SEP VClaim. `sep_number` format desain: `SEP-20261119-001`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sep {
    /// Id lokal, sama dengan nomor SEP (`SEP-20261119-001`).
    pub id: String,
    pub sep_number: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    /// Nomor kartu BPJS peserta.
    pub bpjs_number: String,
    /// `RawatJalan` / `RawatInap`.
    pub jenis: String,
    pub poli: String,
    pub status: SepStatus,
    pub created_by: String,
    pub created_at: NaiveDateTime,
}

/// Status rujukan keluar (VClaim).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferralStatus {
    Terkirim,
    Menunggu,
    Ditolak,
}

/// Rujukan keluar ke faskes rujukan (mis. RSUD Dr. Kariadi).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Referral {
    pub id: String,
    pub patient_id: String,
    pub from_poli: String,
    pub to_facility: String,
    pub reason: String,
    pub status: ReferralStatus,
    pub created_at: NaiveDateTime,
}

/// Kunjungan FKTP yang dicatat ke PCare.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcareVisit {
    pub id: String,
    pub patient_id: String,
    pub visit_date: NaiveDate,
    pub poli: String,
    /// `Tercatat` bila sudah dikirim ke PCare (via outbox).
    pub status: String,
    pub created_at: NaiveDateTime,
}

/// Booking antrean online dari Mobile JKN.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineBooking {
    /// Kode booking JKN, mis. `JKN-881201`.
    pub id: String,
    pub booking_code: String,
    pub patient_id: String,
    pub poli: String,
    /// Estimasi jam, mis. `07.30`.
    pub estimasi: String,
    /// `Menunggu` / `Hadir`.
    pub status: String,
    /// Id antrean lokal bila sudah check-in (`A-041`).
    pub local_queue_id: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Ketersediaan tempat tidur per kelas (cache lokal dari Aplicares).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BedAvailability {
    /// `Kelas 1`, `Kelas 2`, `Kelas 3`, `VIP`, `ICU`.
    pub class: String,
    pub total: i64,
    pub available: i64,
    pub updated_at: NaiveDateTime,
}

/// Status klaim E-Klaim INA-CBG. Fase 2 hanya membangun adapter + alur
/// submit/status; workbench Casemix penuh tetap Fase 3 (paper Bagian 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Draft,
    Terkirim,
    Diterima,
    Ditolak,
}

/// Berkas klaim satu tagihan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub invoice_id: String,
    pub sep_id: Option<String>,
    pub ina_cbg_code: String,
    pub amount: i64,
    pub status: ClaimStatus,
    pub submitted_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}
