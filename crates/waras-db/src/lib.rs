//! WARAS persistence: SQLx SQLite, migrasi, repository, seed, audit trail
//! append-only + rantai hash SHA-256.
//!
//! Satu-satunya titik masuk: [`Db`]. Semua tipe domain berasal dari
//! `waras-core`; crate ini hanya memetakan baris SQLite <-> tipe domain
//! (lihat `rows.rs` / `convert.rs`) dan menegakkan aturan bisnis yang
//! butuh data (mis. `safety::check_prescription` di `prescribe`).
//!
//! SQLx dipakai lewat `sqlx::query` / `query_as` + `FromRow` saja.
//! Tidak ada macro `query!`/`query_as!` (butuh DB saat compile).

mod audit;
mod claims;
mod convert;
mod dashboard;
mod drugs;
mod encounters;
mod fhir_data;
mod invoices;
mod notifications;
mod outbox;
mod patients;
mod prescriptions;
mod queue;
mod rows;
mod seed;
mod seps;
mod users;

pub use seed::seed;
pub use waras_core::types::NewPrescription;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use waras_core::types::{Gender, Insurance, PrescriptionStatus};
use waras_core::WarasError;

/// Tanggal klinik ("hari ini"): env `WARAS_TODAY`, default `2026-11-19`
/// (tanggal desain, KONTRAK §0). Semua seed dan KPI memakai tanggal ini.
pub fn clinic_today() -> NaiveDate {
    if let Ok(s) = std::env::var("WARAS_TODAY") {
        if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
            return d;
        }
    }
    NaiveDate::from_ymd_opt(2026, 11, 19).expect("tanggal default valid")
}

/// Input pasien baru (tanpa id/created_at; id `RM-######` diisi DB).
#[derive(Debug, Clone)]
pub struct NewPatient {
    pub nik: String,
    pub name: String,
    pub birth_date: NaiveDate,
    pub gender: Gender,
    pub address: String,
    pub phone: Option<String>,
    pub blood_type: Option<String>,
    pub bpjs_number: Option<String>,
    pub insurance: Insurance,
}

/// Filter daftar resep.
#[derive(Debug, Clone, Default)]
pub struct PrescriptionFilter {
    pub status: Option<PrescriptionStatus>,
    pub patient_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Buka pool SQLite. `path` boleh path file (`waras.db`), URL
    /// `sqlite:...`, atau `sqlite::memory:` (untuk test).
    pub async fn connect(path: &str) -> Result<Self, WarasError> {
        let url = if path.contains("://") {
            path.to_string()
        } else {
            format!("sqlite:{path}?mode=rwc")
        };
        let in_memory = url.contains(":memory:");
        let pool = SqlitePoolOptions::new()
            .max_connections(if in_memory { 1 } else { 5 })
            .connect(&url)
            .await
            .map_err(|e| WarasError::Database(format!("gagal membuka DB {path}: {e}")))?;
        Ok(Self { pool })
    }

    /// Jalankan migrasi embedded (`migrations/0001_init.sql`).
    pub async fn migrate(&self) -> Result<(), WarasError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| WarasError::Database(format!("migrasi gagal: {e}")))
    }

    /// Seed bila tabel `users` kosong. Idempotent; mengembalikan `true`
    /// bila seed dijalankan, `false` bila DB sudah berisi data.
    pub async fn seed_if_empty(&self) -> Result<bool, WarasError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if n > 0 {
            return Ok(false);
        }
        seed::seed(self).await?;
        Ok(true)
    }

    // -- helpers password (Argon2) ----------------------------------------

    /// Hash Argon2 untuk password baru / seed.
    pub fn hash_password(password: &str) -> Result<String, WarasError> {
        use argon2::password_hash::SaltString;
        // Salt 16 byte acak dari rand (bukan OsRng password-hash: rand_core
        // 0.6 di argon2 tidak mengaktifkan fitur getrandom).
        let salt_bytes: [u8; 16] = rand::random();
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|e| WarasError::Internal(format!("salt gagal: {e}")))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| WarasError::Internal(format!("hash password gagal: {e}")))
    }

    /// Verifikasi password terhadap hash Argon2 yang tersimpan.
    pub fn verify_password(password_hash: &str, password: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    /// Hash rantai audit: sha256 hex atas gabungan
    /// `prev_hash + timestamp + actor + action + resource_type +
    /// resource_id + details_json` (tanpa pemisah).
    pub fn audit_entry_hash(
        prev_hash: &str,
        timestamp: NaiveDateTime,
        actor: &str,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        details: &serde_json::Value,
    ) -> String {
        let payload = format!(
            "{prev_hash}{timestamp}{actor}{action}{resource_type}{resource_id}{details}"
        );
        hex_encode(Sha256::digest(payload.as_bytes()))
    }

    /// Hitung jumlah tablet dari dosis + durasi, aturan KONTRAK §3:
    /// angka hari x frekuensi per hari. Mis. dosage `1x1` +
    /// duration `3 hari` = 3 tablet; `3x1, 30 hari` = 90.
    /// Format lama `500 mg 1x1` tetap diparse (kekuatan diabaikan),
    /// karena `dosage` kanonik kini hanya frekuensi
    /// (lihat `waras_core::dosage::normalize_dosage`).
    /// Bila tidak bisa diparse, kembalikan 1 (jangan gagalkan dispensing).
    pub fn dose_count(dosage: &str, duration: &str) -> i64 {
        let freq = dosage
            .split_whitespace()
            .find(|t| t.contains('x'))
            .and_then(|t| t.split('x').next())
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        let days = duration
            .split_whitespace()
            .find_map(|t| t.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        freq * days
    }

    /// Nama hari Indonesia untuk grafik kunjungan 7 hari.
    pub fn day_name_id(date: NaiveDate) -> &'static str {
        match date.weekday() {
            chrono::Weekday::Mon => "Senin",
            chrono::Weekday::Tue => "Selasa",
            chrono::Weekday::Wed => "Rabu",
            chrono::Weekday::Thu => "Kamis",
            chrono::Weekday::Fri => "Jumat",
            chrono::Weekday::Sat => "Sabtu",
            chrono::Weekday::Sun => "Minggu",
        }
    }
}

// -- helper internal --------------------------------------------------------

pub(crate) fn db_err(e: sqlx::Error) -> WarasError {
    WarasError::Database(e.to_string())
}

pub(crate) fn fmt_dt(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Jam sekarang (waktu lokal, WIB di sandbox) sebagai `NaiveDateTime`.
pub(crate) fn now_naive() -> NaiveDateTime {
    Utc::now().naive_utc()
}

/// Jam sekarang sebagai string `HH.MM` (format jam di UI/desain).
pub(crate) fn now_time_str() -> String {
    chrono::Local::now().format("%H.%M").to_string()
}

pub(crate) fn parse_date(s: &str) -> Result<NaiveDate, WarasError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| WarasError::Database(format!("tanggal tidak valid {s:?}: {e}")))
}

pub(crate) fn parse_dt(s: &str) -> Result<NaiveDateTime, WarasError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| WarasError::Database(format!("timestamp tidak valid {s:?}: {e}")))
}

pub(crate) fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Token sesi: 32 byte acak -> 64 karakter hex.
pub(crate) fn random_token_hex() -> String {
    let bytes: [u8; 32] = rand::random();
    hex_encode(bytes)
}

pub(crate) fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Ambil angka terbesar dari suffix numerik id-id yang ada
/// (mis. `RX-1041` -> 1041), lalu +1. Bila tidak ada, `start`.
pub(crate) fn next_seq_from_ids(ids: &[String], start: u32) -> u32 {
    ids.iter()
        .filter_map(|id| {
            id.rsplit('-')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .max()
        .map(|m| m + 1)
        .unwrap_or(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::types::DashboardKpis;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:")
            .await
            .expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[test]
    fn dose_count_parses_contract_example() {
        assert_eq!(Db::dose_count("1x1", "3 hari"), 3);
        assert_eq!(Db::dose_count("3x1", "30 hari"), 90);
        assert_eq!(Db::dose_count("2x1", "5 hari"), 10);
        // Format lama (kekuatan di dosage) tetap diparse.
        assert_eq!(Db::dose_count("500 mg 1x1", "3 hari"), 3);
        assert_eq!(Db::dose_count("tanpa pola", "7 hari"), 7);
    }

    #[test]
    fn clinic_today_defaults_to_design_date() {
        std::env::remove_var("WARAS_TODAY");
        assert_eq!(
            clinic_today(),
            NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
        );
    }

    #[tokio::test]
    async fn seed_loads_exact_counts() {
        let db = seeded_db().await;
        let patients = db.list_patients(None).await.unwrap();
        assert_eq!(patients.len(), 10);
        let queue = db.list_queue(clinic_today()).await.unwrap();
        assert_eq!(queue.len(), 44);
        let rx = db
            .list_prescriptions(PrescriptionFilter::default())
            .await
            .unwrap();
        assert_eq!(rx.len(), 8);
        let drugs = db.list_drugs().await.unwrap();
        assert_eq!(drugs.len(), 9);
        let invoices = db.list_invoices(None).await.unwrap();
        assert_eq!(invoices.len(), 8);
        let appointments = db.list_appointments(None).await.unwrap();
        assert_eq!(appointments.len(), 31);
        let notifs = db.list_notifications(clinic_today()).await.unwrap();
        assert_eq!(notifs.len(), 3);
        let allergies = db.get_allergies("RM-000812").await.unwrap();
        assert_eq!(allergies.len(), 1);
        assert_eq!(allergies[0].substance, "Penisilin");
        let conditions = db.get_conditions("RM-000812").await.unwrap();
        assert_eq!(conditions.len(), 7);
        // 31 janji: hitung lewat KPI followup (6 hari ini + 25 kontrol ulang).
        let kpis = db.dashboard_kpis(clinic_today()).await.unwrap();
        assert_eq!(kpis.followups_7d, 31);
        // 7 baris daily_stats -> visits_7d berisi 7 hari.
        assert_eq!(kpis.visits_7d.len(), 7);
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let db = seeded_db().await;
        assert!(!db.seed_if_empty().await.unwrap(), "seed kedua harus skip");
        assert_eq!(db.list_patients(None).await.unwrap().len(), 10);
    }

    #[tokio::test]
    async fn seed_users_can_login() {
        let db = seeded_db().await;
        let user = db
            .find_user_by_username_or_nip("ahmad")
            .await
            .unwrap()
            .expect("ahmad ada");
        assert_eq!(user.id, "U-001");
        assert!(Db::verify_password(&user.password_hash, "waras123"));
        assert!(!Db::verify_password(&user.password_hash, "salah"));
        // Login lewat NIP juga bisa.
        let by_nip = db
            .find_user_by_username_or_nip("198505122010011001")
            .await
            .unwrap()
            .expect("NIP ahmad ada");
        assert_eq!(by_nip.username, "ahmad");
    }

    #[tokio::test]
    async fn sessions_roundtrip() {
        let db = seeded_db().await;
        let token = db.create_session("U-001").await.unwrap();
        assert_eq!(token.len(), 64, "32 byte acak -> 64 hex");
        let user = db.find_user_by_token(&token).await.unwrap().unwrap();
        assert_eq!(user.username, "ahmad");
        db.delete_session(&token).await.unwrap();
        assert!(db.find_user_by_token(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prescribe_blocks_amoxicillin_allows_azithromycin() {
        let db = seeded_db().await;
        let blocked = db
            .prescribe(
                "dr. Ahmad Hidayat",
                NewPrescription {
                    encounter_id: "ENC-2026-11-19-014".into(),
                    drug_name: "Amoxicillin 500 mg".into(),
                    drug_class: "penicillin".into(),
                    dosage: "1x1".into(),
                    duration: "3 hari".into(),
                },
            )
            .await;
        assert!(matches!(blocked, Err(WarasError::Blocked(_))));
        // Resep yang diblokir tetap tersimpan dengan status Diblokir.
        let rx = db
            .list_prescriptions(PrescriptionFilter {
                status: Some(PrescriptionStatus::Diblokir),
                patient_id: Some("RM-000812".into()),
            })
            .await
            .unwrap();
        assert_eq!(rx.len(), 2, "seed RX-1040 + yang baru diblokir");
        assert!(rx.iter().all(|r| r.blocked_reason.is_some()));

        let ok = db
            .prescribe(
                "dr. Ahmad Hidayat",
                NewPrescription {
                    encounter_id: "ENC-2026-11-19-014".into(),
                    drug_name: "Azithromycin 500 mg".into(),
                    drug_class: "macrolide".into(),
                    dosage: "1x1".into(),
                    duration: "3 hari".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(ok.status, PrescriptionStatus::Aktif);
    }

    #[tokio::test]
    async fn dashboard_kpis_match_contract() {
        let db = seeded_db().await;
        let k: DashboardKpis = db.dashboard_kpis(clinic_today()).await.unwrap();
        assert_eq!(k.queue_total, 44);
        assert_eq!(k.queue_active, 32);
        assert_eq!(k.queue_waiting, 15);
        assert_eq!(k.queue_in_consultation, 17);
        assert_eq!(k.queue_done, 10);
        assert_eq!(k.queue_no_show, 2);
        assert_eq!(k.followups_7d, 31);
        assert_eq!(k.visits_avg_7d, 178);
        assert_eq!(k.visits_peak_7d, 210);
        assert_eq!(k.patients_today, 248);
        assert_eq!(k.revenue_today_idr, 18_400_000);
        assert_eq!(k.revenue_transactions, 142);
        assert_eq!(k.avg_wait_minutes, 18);
        assert_eq!(k.longest_wait_minutes, 45);
        assert_eq!(k.unpaid_invoices_count, 4);
        assert_eq!(k.unpaid_invoices_idr, 5_875_000);
    }

    #[tokio::test]
    async fn audit_hash_chain_verifies() {
        let db = seeded_db().await;
        // Tambah satu entri, lalu verifikasi seluruh rantai dari awal.
        db.log_audit(
            "tester",
            "test.cek",
            "test",
            "T-1",
            serde_json::json!({"x": 1}),
        )
        .await
        .unwrap();
        let entries = db.list_audit(100).await.unwrap();
        assert_eq!(entries.len(), 7, "6 seed + 1 baru");
        // list_audit mengembalikan terbaru dulu; balik untuk verifikasi rantai.
        let mut prev = "GENESIS".to_string();
        for e in entries.iter().rev() {
            assert_eq!(e.prev_hash, prev, "prev_hash entri {}", e.id);
            let recomputed = Db::audit_entry_hash(
                &e.prev_hash,
                e.timestamp,
                &e.actor,
                &e.action,
                &e.resource_type,
                &e.resource_id,
                &e.details,
            );
            assert_eq!(recomputed, e.entry_hash, "entry_hash entri {}", e.id);
            prev = e.entry_hash.clone();
        }
    }

    #[tokio::test]
    async fn soap_versioning_keeps_old_versions() {
        let db = seeded_db().await;
        let soap = db.get_soap("ENC-2026-11-19-014").await.unwrap().unwrap();
        assert_eq!(soap.version, 3);
        let versions = db.get_soap_versions(&soap.id).await.unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].subjective.contains("Batuk"), true);
        assert!(versions[0].objective.is_empty(), "v1 hanya S");
        assert!(!versions[2].plan.is_empty(), "v3 lengkap");

        let saved = db
            .save_soap(
                "ENC-2026-11-19-014",
                "Batuk berdahak 3 hari. Demam malam hari.",
                "Paru: ronki basal kanan.",
                "Bronkitis akut (J20.9).",
                "Azithromycin 500 mg 1x1, 3 hari. Kontrol 3 hari lagi.",
                "dr. Ahmad Hidayat",
            )
            .await
            .unwrap();
        assert_eq!(saved.version, 4);
        let versions = db.get_soap_versions(&soap.id).await.unwrap();
        assert_eq!(versions.len(), 4, "versi lama tidak ditimpa");
        assert_eq!(versions[0].version, 1);
        assert_eq!(versions[3].version, 4);
    }

    #[tokio::test]
    async fn queue_take_assigns_next_number() {
        let db = seeded_db().await;
        let entry = db
            .take_queue("RM-000813", "Poli Umum", "dr. Ahmad Hidayat", clinic_today())
            .await
            .unwrap();
        assert_eq!(entry.number, "A-045");
        let called = db.call_queue(&entry.id).await.unwrap();
        assert_eq!(called.status, waras_core::types::QueueStatus::Dipanggil);
        assert!(called.called_at.is_some());
    }

    #[tokio::test]
    async fn dispense_decrements_stock() {
        let db = seeded_db().await;
        // RX-1042 Paracetamol 500 mg 3x1, 3 hari = 9 tablet; stok 1240 -> 1231.
        let before = db.get_drug("KFA-9202").await.unwrap().unwrap();
        assert_eq!(before.stock, 1240);
        db.dispense_prescription("RX-1042", "apt. Dina Puspita")
            .await
            .unwrap();
        let after = db.get_drug("KFA-9202").await.unwrap().unwrap();
        assert_eq!(after.stock, 1231);
        let rx = db.get_prescription("RX-1042").await.unwrap().unwrap();
        assert_eq!(rx.status, PrescriptionStatus::Diberikan);
    }

    #[tokio::test]
    async fn pay_invoice_marks_lunas() {
        let db = seeded_db().await;
        // INV-2026-1183 Dewi 450.000 Menunggu; bayar penuh -> Lunas.
        let inv = db
            .pay_invoice(
                "INV-2026-1183",
                waras_core::types::PaymentMethod::Tunai,
                450_000,
                "Sinta Maharani",
            )
            .await
            .unwrap();
        assert_eq!(inv.status, waras_core::types::InvoiceStatus::Lunas);
        let payments = db.get_payments("INV-2026-1183").await.unwrap();
        assert_eq!(payments.len(), 1);
        assert_eq!(payments[0].amount, 450_000);
    }

    #[tokio::test]
    async fn create_patient_assigns_next_rm() {
        let db = seeded_db().await;
        let p = db
            .create_patient(NewPatient {
                nik: "3374010101900001".into(),
                name: "Test Pasien".into(),
                birth_date: NaiveDate::from_ymd_opt(1990, 1, 1).unwrap(),
                gender: Gender::LakiLaki,
                address: "Jl. Test No. 1".into(),
                phone: None,
                blood_type: None,
                bpjs_number: None,
                insurance: Insurance::Umum,
            })
            .await
            .unwrap();
        assert_eq!(p.id, "RM-000822");
        let found = db.get_patient("RM-000822").await.unwrap().unwrap();
        assert_eq!(found.name, "Test Pasien");
    }

    #[tokio::test]
    async fn encounter_bundle_data_loads() {
        let db = seeded_db().await;
        let (patient, encounter, vitals, conditions, rx) = db
            .encounter_bundle_data("ENC-2026-11-19-014")
            .await
            .unwrap()
            .expect("bundle ada");
        assert_eq!(patient.id, "RM-000812");
        assert_eq!(encounter.id, "ENC-2026-11-19-014");
        assert_eq!(vitals.len(), 5);
        assert!(!conditions.is_empty());
        assert_eq!(rx.len(), 2, "RX-1040 + RX-1041");
    }
}
