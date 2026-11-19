//! Tipe domain jembatan nasional (Fase 2): outbox, status bridge, dan
//! status sinkronisasi per baris. Pure Rust, tanpa I/O — dipakai `waras-db`
//! (repo outbox), `waras-bridge` (adapter), dan `waras-api` (handler/worker).
//!
//! Aturan paper Bagian 4.2: pengiriman ke SATUSEHAT/BPJS tidak pernah
//! sinkron di jalur transaksi klinis. Setiap pengiriman adalah baris
//! `OutboxJob` yang persisten; worker latar yang mengurasnya.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tujuan pengiriman = satu adapter. Satu adapter per tujuan; adapter baru
/// ditambahkan tanpa menyentuh worker (paper Bagian 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxDestination {
    Satusehat,
    VClaim,
    PCare,
    AntreanOnline,
    Aplicares,
    EKlaim,
}

impl OutboxDestination {
    /// Nama tampilan (Indonesia, sesuai layar desain v16).
    pub fn label(&self) -> &'static str {
        match self {
            OutboxDestination::Satusehat => "SATUSEHAT",
            OutboxDestination::VClaim => "VClaim",
            OutboxDestination::PCare => "PCare",
            OutboxDestination::AntreanOnline => "Antrean Online",
            OutboxDestination::Aplicares => "Aplicares",
            OutboxDestination::EKlaim => "E-Klaim",
        }
    }

    /// Semua tujuan, dalam urutan tampil di layar status jembatan.
    pub fn all() -> [OutboxDestination; 6] {
        [
            OutboxDestination::Satusehat,
            OutboxDestination::VClaim,
            OutboxDestination::PCare,
            OutboxDestination::AntreanOnline,
            OutboxDestination::Aplicares,
            OutboxDestination::EKlaim,
        ]
    }
}

/// Status satu pekerjaan outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    /// Menunggu giliran worker.
    Pending,
    /// Sedang dikirim (diklaim oleh satu worker; jangan diambil worker lain).
    Sending,
    /// Terkirim (HTTP 2xx) tapi belum ada ack/id eksternal yang dicatat.
    Sent,
    /// Diakui tujuan + id eksternal tersimpan (keadaan akhir yang baik).
    Acked,
    /// Gagal sementara; akan dicoba lagi sampai `max_attempts`.
    Failed,
    /// Gagal permanen (validasi ditolak atau percobaan habis); perlu operator.
    Dead,
}

impl OutboxStatus {
    pub fn label(&self) -> &'static str {
        match self {
            OutboxStatus::Pending => "Menunggu kirim",
            OutboxStatus::Sending => "Mengirim",
            OutboxStatus::Sent => "Terkirim",
            OutboxStatus::Acked => "Diakui",
            OutboxStatus::Failed => "Gagal",
            OutboxStatus::Dead => "Gagal permanen",
        }
    }

    /// Keadaan akhir: tidak lagi diproses worker.
    pub fn is_terminal(&self) -> bool {
        matches!(self, OutboxStatus::Acked | OutboxStatus::Dead)
    }
}

/// Kesehatan satu jembatan, seperti pil status di layar desain v16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeHealth {
    Online,
    Aktif,
    Peringatan,
    Kritis,
    Offline,
    /// Belum ada kredensial/config: bukan error, belum diaktifkan.
    BelumDikonfigurasi,
}

impl BridgeHealth {
    pub fn label(&self) -> &'static str {
        match self {
            BridgeHealth::Online => "Online",
            BridgeHealth::Aktif => "Aktif",
            BridgeHealth::Peringatan => "Peringatan",
            BridgeHealth::Kritis => "Kritis",
            BridgeHealth::Offline => "Offline",
            BridgeHealth::BelumDikonfigurasi => "Belum dikonfigurasi",
        }
    }
}

/// Status sinkronisasi satu baris domain ke SATUSEHAT (kolom
/// `satusehat_status` di patients/encounters/observations/
/// medication_requests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    BelumDikirim,
    Terkirim,
    Gagal,
}

impl SyncStatus {
    pub fn label(&self) -> &'static str {
        match self {
            SyncStatus::BelumDikirim => "Belum dikirim",
            SyncStatus::Terkirim => "Terkirim ke SATUSEHAT",
            SyncStatus::Gagal => "Gagal",
        }
    }
}

/// Satu pekerjaan pengiriman di tabel `outbox`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    /// Mis. `OBX-000001`.
    pub id: String,
    pub destination: OutboxDestination,
    /// Jenis resource/event, mis. `Patient`, `Encounter`, `SEP`, `Klaim`.
    pub resource_type: String,
    /// Id lokal baris sumber, mis. `RM-000812`, `ENC-2026-11-19-014`.
    pub resource_id: String,
    /// Kunci idempotensi, mis. `satusehat:Encounter:ENC-...:closed`.
    /// UNIQUE: enqueue dengan kunci yang sama tidak membuat baris kedua.
    pub idempotency_key: String,
    /// Payload yang dikirim adapter (resource FHIR JSON atau body BPJS).
    pub payload: Value,
    pub status: OutboxStatus,
    pub attempts: i64,
    pub max_attempts: i64,
    /// ISO-8601 kapan boleh dicoba lagi (NULL = sekarang).
    pub next_retry_at: Option<String>,
    pub last_error: Option<String>,
    /// Id yang dikembalikan tujuan (IHS id SATUSEHAT, nomor SEP, dsb.).
    pub external_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl OutboxJob {
    /// Backoff percobaan ulang: 2^attempts menit, dibatasi 60 menit.
    /// attempts=0 -> 1 menit, 1 -> 2, 2 -> 4, 3 -> 8, 4 -> 16, 5+ -> 60.
    pub fn backoff_minutes(attempts: i64) -> i64 {
        let m = 1i64 << attempts.min(6);
        m.min(60)
    }
}
