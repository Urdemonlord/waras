//! Worker latar outbox (FASE2.md §4.1): menguras tabel `outbox` ke
//! SATUSEHAT/BPJS lewat [`BridgeRegistry`].
//!
//! Loop: tidur interval -> klaim batch atomik -> kirim tiap job ->
//! tandai hasil -> audit. TIDAK PERNAH panic: semua error ditangkap +
//! log, loop lanjut.
//!
//! Pemetaan hasil kirim (FASE2.md §4.1):
//! - `Ok(Acked)` -> status Acked, external_id tersimpan, audit `bridge.acked`.
//! - `Ok(Sent)` -> status Sent, dianggap selesai (tidak dikirim ulang).
//! - `Err(NotConfigured)` -> tetap Pending, TANPA menghabiskan percobaan,
//!   tanpa mengubah next_retry_at, log debug.
//! - `Err(Validation)` -> Dead, audit `bridge.failed_validation`.
//! - `Err(lain)` -> attempts+1; attempts >= max_attempts -> Dead, bila
//!   belum -> Failed + next_retry_at = now + `OutboxJob::backoff_minutes`.
//! - Setiap percobaan kirim diaudit (`bridge.send_attempt`).
//!
//! Penyimpangan dari FASE2.md §2 (agen A, didokumentasikan di
//! `waras-db/src/outbox.rs`): `Db::mark_outbox_sent` TIDAK menerima
//! `&SendOutcome` (waras-db tidak boleh bergantung pada waras-bridge),
//! melainkan `(status, external_id)` terurai. Worker memetakan
//! `SendOutcome { status, external_id }` ke dua parameter itu.
//!
//! Format stempel waktu: `"%Y-%m-%d %H:%M:%S"` (format Fase 1,
//! `fmt_dt`), sama dengan yang dipakai `claim_outbox_batch_atomic`
//! untuk membandingkan `next_retry_at` sebagai string.

use std::sync::Arc;

use chrono::{Duration, NaiveDateTime, Utc};
use serde_json::{json, Value};
use waras_bridge::{BridgeConfig, BridgeError, BridgeRegistry};
use waras_core::bridge::{OutboxDestination, OutboxJob, OutboxStatus};
use waras_db::Db;

/// Jam sekarang (UTC) sebagai `NaiveDateTime`.
pub fn now_utc() -> NaiveDateTime {
    Utc::now().naive_utc()
}

/// Format stempel Fase 1 (`"%Y-%m-%d %H:%M:%S"`).
pub fn now_iso(now: NaiveDateTime) -> String {
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Satu pekerjaan outbox baru (id + stempel diisi `enqueue_outbox`).
pub fn new_outbox_job(
    destination: OutboxDestination,
    resource_type: &str,
    resource_id: &str,
    idempotency_key: &str,
    payload: Value,
    max_attempts: i64,
) -> OutboxJob {
    OutboxJob {
        id: String::new(),
        destination,
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        idempotency_key: idempotency_key.to_string(),
        payload,
        status: OutboxStatus::Pending,
        attempts: 0,
        max_attempts,
        next_retry_at: None,
        last_error: None,
        external_id: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// Enqueue best-effort (FASE2.md §4.2): INSERT OR IGNORE; kunci yang
/// sudah ada bukan error (kembalikan `false`); kegagalan DB hanya
/// di-log warning — jangan pernah gagalkan request klinis karena
/// enqueue gagal.
pub async fn try_enqueue(db: &Db, job: &OutboxJob) -> bool {
    match db.enqueue_outbox(job).await {
        Ok(true) => true,
        Ok(false) => {
            tracing::debug!(
                "outbox: kunci {} sudah ada, lewati",
                job.idempotency_key
            );
            false
        }
        Err(e) => {
            tracing::warn!(
                "outbox: gagal enqueue {} ({}): {e}",
                job.idempotency_key,
                job.destination.label()
            );
            false
        }
    }
}

/// Audit best-effort dari worker (aktor `worker`).
async fn audit(db: &Db, action: &str, job: &OutboxJob, details: Value) {
    let _ = db
        .log_audit("worker", action, "outbox", &job.id, details)
        .await;
}

/// Satu pass worker (bisa diuji langsung): klaim batch jatuh tempo,
/// kirim tiap job, tandai hasilnya. `now` disuntikkan supaya test bisa
/// memajukan waktu untuk retry/backoff.
pub async fn run_once(
    db: &Db,
    registry: &BridgeRegistry,
    config: &BridgeConfig,
    now: NaiveDateTime,
) {
    let now_s = now_iso(now);
    let jobs = match db
        .claim_outbox_batch_atomic(config.worker_batch as i64, &now_s)
        .await
    {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::error!("worker: gagal klaim batch outbox: {e}");
            return;
        }
    };
    if jobs.is_empty() {
        return;
    }
    let mut not_configured = 0usize;
    for job in &jobs {
        if process_job(db, registry, job, now).await {
            not_configured += 1;
        }
    }
    if not_configured > 0 {
        // Satu baris debug per loop, bukan per job (FASE2.md §4.1).
        tracing::debug!(
            "worker: {not_configured} pekerjaan dibiarkan Pending (jembatan belum dikonfigurasi)"
        );
    }
}

/// Proses satu job yang sudah diklaim (status Sending).
/// Mengembalikan `true` bila hasilnya NotConfigured (tetap Pending).
async fn process_job(
    db: &Db,
    registry: &BridgeRegistry,
    job: &OutboxJob,
    now: NaiveDateTime,
) -> bool {
    let Some(bridge) = registry.get(job.destination) else {
        // Registry tanpa adapter untuk tujuan ini = belum dikonfigurasi.
        reset_to_pending(db, job).await;
        tracing::debug!(
            "worker: tidak ada adapter untuk {}, job {} tetap Pending",
            job.destination.label(),
            job.id
        );
        return true;
    };
    match bridge.send(job).await {
        Ok(outcome) => {
            let attempts = job.attempts + 1;
            let status = outcome.status;
            if let Err(e) = db
                .mark_outbox_sent(
                    &job.id,
                    status,
                    outcome.external_id.as_deref(),
                    attempts,
                    None,
                    None,
                )
                .await
            {
                tracing::error!("worker: gagal menandai job {}: {e}", job.id);
            }
            let details = json!({
                "destination": job.destination.label(),
                "resource_type": job.resource_type,
                "status": format!("{status:?}"),
                "attempts": attempts,
                "external_id": outcome.external_id,
            });
            audit(db, "bridge.send_attempt", job, details.clone()).await;
            if status == OutboxStatus::Acked {
                audit(db, "bridge.acked", job, details).await;
            }
            false
        }
        Err(BridgeError::NotConfigured(msg)) => {
            // JANGAN tambah attempts, JANGAN ubah next_retry_at.
            reset_to_pending(db, job).await;
            tracing::debug!("worker: {msg} — job {} tetap Pending", job.id);
            true
        }
        Err(BridgeError::Validation(msg)) => {
            let attempts = job.attempts + 1;
            if let Err(e) = db
                .mark_outbox_sent(
                    &job.id,
                    OutboxStatus::Dead,
                    None,
                    attempts,
                    None,
                    Some(msg.clone()),
                )
                .await
            {
                tracing::error!("worker: gagal menandai job {} Dead: {e}", job.id);
            }
            let details = json!({
                "destination": job.destination.label(),
                "resource_type": job.resource_type,
                "status": "dead",
                "attempts": attempts,
                "error": msg,
            });
            audit(db, "bridge.send_attempt", job, details.clone()).await;
            audit(db, "bridge.failed_validation", job, details).await;
            false
        }
        Err(e) => {
            let attempts = job.attempts + 1;
            let msg = e.to_string();
            if attempts >= job.max_attempts {
                if let Err(err) = db
                    .mark_outbox_sent(
                        &job.id,
                        OutboxStatus::Dead,
                        None,
                        attempts,
                        None,
                        Some(msg.clone()),
                    )
                    .await
                {
                    tracing::error!("worker: gagal menandai job {} Dead: {err}", job.id);
                }
                tracing::warn!(
                    "worker: job {} Dead setelah {attempts} percobaan: {msg}",
                    job.id
                );
            } else {
                let next = now + Duration::minutes(OutboxJob::backoff_minutes(attempts));
                if let Err(err) = db
                    .mark_outbox_sent(
                        &job.id,
                        OutboxStatus::Failed,
                        None,
                        attempts,
                        Some(now_iso(next)),
                        Some(msg.clone()),
                    )
                    .await
                {
                    tracing::error!("worker: gagal menandai job {} Failed: {err}", job.id);
                }
                tracing::warn!(
                    "worker: job {} gagal (percobaan {attempts}), coba lagi {}: {msg}",
                    job.id,
                    now_iso(next)
                );
            }
            audit(
                db,
                "bridge.send_attempt",
                job,
                json!({
                    "destination": job.destination.label(),
                    "resource_type": job.resource_type,
                    "status": if attempts >= job.max_attempts { "dead" } else { "failed" },
                    "attempts": attempts,
                    "error": msg,
                }),
            )
            .await;
            false
        }
    }
}

/// Kembalikan job yang diklaim ke Pending tanpa mengubah attempts /
/// next_retry_at / last_error (kasus NotConfigured).
async fn reset_to_pending(db: &Db, job: &OutboxJob) {
    if let Err(e) = db
        .mark_outbox_sent(
            &job.id,
            OutboxStatus::Pending,
            None,
            job.attempts,
            job.next_retry_at.clone(),
            job.last_error.clone(),
        )
        .await
    {
        tracing::error!("worker: gagal mengembalikan job {} ke Pending: {e}", job.id);
    }
}

/// Loop worker: tidur interval -> satu pass -> ulang. Dijalankan binary
/// sebagai task Tokio bila `WARAS_WORKER_ENABLED=true` (jangan blokir
/// serve). Tidak pernah panic: `run_once` menangkap semua error.
pub async fn run_worker(db: Arc<Db>, registry: Arc<BridgeRegistry>, config: BridgeConfig) {
    tracing::info!(
        "worker outbox aktif: interval {} dtk, batch {}",
        config.worker_interval_secs,
        config.worker_batch
    );
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(config.worker_interval_secs)).await;
        run_once(&db, &registry, &config, now_utc()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_bridge::{Bridge, BridgeError, SendOutcome};

    /// Bridge palsu: mengembalikan hasil kaleng, tanpa HTTP.
    struct FakeBridge {
        destination: OutboxDestination,
        behavior: std::sync::Mutex<FakeBehavior>,
    }

    #[derive(Clone)]
    enum FakeBehavior {
        Ack(Option<String>),
        Sent,
        Validation(String),
        Http(String),
        NotConfigured,
    }

    impl FakeBridge {
        fn new(destination: OutboxDestination, behavior: FakeBehavior) -> Self {
            Self {
                destination,
                behavior: std::sync::Mutex::new(behavior),
            }
        }
    }

    // Implementasi manual (tanpa macro async-trait, supaya waras-api
    // tidak perlu menambah dependensi): tanda tangan ini yang dihasilkan
    // `#[async_trait]` untuk `Bridge::send`.
    impl Bridge for FakeBridge {
        fn destination(&self) -> OutboxDestination {
            self.destination
        }

        fn name(&self) -> &'static str {
            "Fake"
        }

        fn configured(&self) -> bool {
            true
        }

        fn send<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _job: &'life1 OutboxJob,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<SendOutcome, BridgeError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let behavior = self.behavior.lock().unwrap().clone();
            Box::pin(async move {
                match behavior {
                    FakeBehavior::Ack(external_id) => Ok(SendOutcome {
                        external_id,
                        status: OutboxStatus::Acked,
                        note: None,
                    }),
                    FakeBehavior::Sent => Ok(SendOutcome {
                        external_id: None,
                        status: OutboxStatus::Sent,
                        note: None,
                    }),
                    FakeBehavior::Validation(msg) => Err(BridgeError::Validation(msg)),
                    FakeBehavior::Http(msg) => Err(BridgeError::Http(msg)),
                    FakeBehavior::NotConfigured => Err(BridgeError::NotConfigured(
                        "fake belum dikonfigurasi".into(),
                    )),
                }
            })
        }
    }

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests()
            .await
            .expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    fn test_config() -> BridgeConfig {
        BridgeConfig {
            satusehat: waras_bridge::SatusehatConfig {
                enabled: false,
                base_url: String::new(),
                auth_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                org_id: String::new(),
            },
            bpjs: waras_bridge::BpjsConfig {
                enabled: false,
                cons_id: String::new(),
                secret_key: String::new(),
                user_key: String::new(),
                vclaim_url: String::new(),
                pcare_url: String::new(),
                antrean_url: String::new(),
                aplicares_url: String::new(),
                kode_ppk: String::new(),
                eklaim_url: String::new(),
            },
            worker_enabled: true,
            worker_interval_secs: 10,
            worker_batch: 10,
            outbox_max_attempts: 5,
        }
    }

    fn now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn patient_job() -> OutboxJob {
        new_outbox_job(
            OutboxDestination::Satusehat,
            "Patient",
            "RM-000812",
            "satusehat:Patient:RM-000812:created",
            serde_json::json!({"resourceType": "Patient", "id": "812"}),
            5,
        )
    }

    async fn run_with(db: &Db, behavior: FakeBehavior, job: OutboxJob) -> OutboxJob {
        let mut registry = BridgeRegistry::new();
        registry.register(Arc::new(FakeBridge::new(job.destination, behavior)));
        assert!(db.enqueue_outbox(&job).await.unwrap());
        run_once(db, &registry, &test_config(), now()).await;
        let jobs = db.list_outbox(None, None, 50).await.unwrap();
        assert_eq!(jobs.len(), 1);
        jobs.into_iter().next().unwrap()
    }

    async fn audit_has(db: &Db, action: &str) -> bool {
        db.list_audit(100)
            .await
            .unwrap()
            .iter()
            .any(|e| e.action == action)
    }

    #[tokio::test]
    async fn ack_marks_acked_and_audits() {
        let db = seeded_db().await;
        let job = run_with(
            &db,
            FakeBehavior::Ack(Some("IHS-812".into())),
            patient_job(),
        )
        .await;
        assert_eq!(job.status, OutboxStatus::Acked);
        assert_eq!(job.external_id.as_deref(), Some("IHS-812"));
        assert_eq!(job.attempts, 1);
        assert!(audit_has(&db, "bridge.acked").await);
        assert!(audit_has(&db, "bridge.send_attempt").await);
    }

    #[tokio::test]
    async fn sent_is_terminal_without_ack() {
        let db = seeded_db().await;
        let job = run_with(&db, FakeBehavior::Sent, patient_job()).await;
        assert_eq!(job.status, OutboxStatus::Sent);
        assert_eq!(job.attempts, 1);
        assert!(audit_has(&db, "bridge.send_attempt").await);
        assert!(!audit_has(&db, "bridge.acked").await);
    }

    #[tokio::test]
    async fn validation_goes_dead_immediately() {
        let db = seeded_db().await;
        let job = run_with(
            &db,
            FakeBehavior::Validation("Patient: field 'nik' wajib diisi".into()),
            patient_job(),
        )
        .await;
        assert_eq!(job.status, OutboxStatus::Dead);
        assert_eq!(job.attempts, 1);
        assert!(job.last_error.as_deref().unwrap().contains("nik"));
        assert!(audit_has(&db, "bridge.failed_validation").await);
    }

    #[tokio::test]
    async fn http_failure_goes_failed_with_backoff() {
        let db = seeded_db().await;
        let job = run_with(
            &db,
            FakeBehavior::Http("tujuan gagal (HTTP 500)".into()),
            patient_job(),
        )
        .await;
        assert_eq!(job.status, OutboxStatus::Failed);
        assert_eq!(job.attempts, 1);
        // backoff_minutes(1) = 2 menit dari now 12:00.
        assert_eq!(job.next_retry_at.as_deref(), Some("2026-11-19 12:02:00"));
        assert!(job.last_error.is_some());
    }

    #[tokio::test]
    async fn http_failure_at_max_attempts_goes_dead() {
        let db = seeded_db().await;
        let mut job = patient_job();
        job.attempts = 4; // percobaan ke-5 = terakhir (max 5)
        job.max_attempts = 5;
        let stored = run_with(&db, FakeBehavior::Http("500".into()), job).await;
        assert_eq!(stored.status, OutboxStatus::Dead);
        assert_eq!(stored.attempts, 5);
        assert_eq!(stored.next_retry_at, None);
    }

    #[tokio::test]
    async fn not_configured_stays_pending_without_attempt() {
        let db = seeded_db().await;
        let job = run_with(&db, FakeBehavior::NotConfigured, patient_job()).await;
        assert_eq!(job.status, OutboxStatus::Pending);
        assert_eq!(job.attempts, 0);
        assert_eq!(job.next_retry_at, None);
        assert!(!audit_has(&db, "bridge.send_attempt").await);
    }

    #[tokio::test]
    async fn enqueue_same_key_twice_creates_one_row() {
        let db = seeded_db().await;
        assert!(try_enqueue(&db, &patient_job()).await);
        assert!(!try_enqueue(&db, &patient_job()).await);
        assert_eq!(db.list_outbox(None, None, 50).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn failed_job_is_reclaimed_once_retry_time_is_due() {
        // Perilaku percobaan ulang otomatis (diperbaiki saat integrasi):
        // job Failed dengan next_retry_at yang sudah lewat HARUS diklaim
        // lagi oleh worker; sebelum waktunya, tidak. Tanpa ini, job
        // macet sampai operator menekan retry manual.
        let db = seeded_db().await;
        let mut registry = BridgeRegistry::new();
        registry.register(Arc::new(FakeBridge::new(
            OutboxDestination::Satusehat,
            FakeBehavior::Http("500".into()),
        )));
        assert!(db.enqueue_outbox(&patient_job()).await.unwrap());
        run_once(&db, &registry, &test_config(), now()).await;
        let job = db.list_outbox(None, None, 50).await.unwrap().into_iter().next().unwrap();
        assert_eq!(job.status, OutboxStatus::Failed);
        assert_eq!(job.attempts, 1);
        // Backoff percobaan pertama: 2 menit (12:02).
        assert_eq!(job.next_retry_at.as_deref(), Some("2026-11-19 12:02:00"));
        // Sebelum waktunya (12:01): tidak diklaim.
        run_once(&db, &registry, &test_config(), now() + Duration::minutes(1)).await;
        let job = db.list_outbox(None, None, 50).await.unwrap().into_iter().next().unwrap();
        assert_eq!(job.status, OutboxStatus::Failed);
        assert_eq!(job.attempts, 1);
        // Sesudah waktunya (12:30): diklaim lagi, gagal lagi, attempts 2.
        run_once(&db, &registry, &test_config(), now() + Duration::minutes(30)).await;
        let job = db.list_outbox(None, None, 50).await.unwrap().into_iter().next().unwrap();
        assert_eq!(job.status, OutboxStatus::Failed);
        assert_eq!(job.attempts, 2);
        // Retry operator tetap bekerja: kembali Pending, attempts 0.
        db.retry_outbox(&job.id).await.unwrap();
        let job = db.get_outbox(&job.id).await.unwrap().unwrap();
        assert_eq!(job.status, OutboxStatus::Pending);
        assert_eq!(job.attempts, 0);
    }
}
