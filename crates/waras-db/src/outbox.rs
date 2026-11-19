//! Outbox jembatan nasional (FASE2.md §2): satu baris = satu pengiriman
//! asinkron ke SATUSEHAT/BPJS. Idempotensi lewat UNIQUE `idempotency_key`.
//!
//! Penyimpangan dari FASE2.md §2 (terdokumentasi): `mark_outbox_sent`
//! menerima hasil kirim terurai (`status`, `external_id`) BUKAN
//! `&waras_bridge::SendOutcome`. `waras-db` hanya boleh bergantung pada
//! `waras-core` (FASE2.md §0) dan dilarang menambah dependensi, jadi
//! `SendOutcome` (yang tinggal di `waras-bridge`) tidak bisa dipakai di
//! sini. Agen C memetakan `SendOutcome { status, external_id }` ke dua
//! parameter ini — pemetaan satu baris, tanpa kehilangan informasi.

use crate::{db_err, fmt_dt, now_naive, Db};
use sqlx::FromRow;
use waras_core::{
    bridge::{OutboxDestination, OutboxJob, OutboxStatus},
    WarasError,
};

// --- enum <-> TEXT (snake_case, gaya convert.rs Fase 1) -------------------

pub(crate) fn destination_to_str(d: OutboxDestination) -> &'static str {
    match d {
        OutboxDestination::Satusehat => "satusehat",
        OutboxDestination::VClaim => "vclaim",
        OutboxDestination::PCare => "pcare",
        OutboxDestination::AntreanOnline => "antrean_online",
        OutboxDestination::Aplicares => "aplicares",
        OutboxDestination::EKlaim => "eklaim",
    }
}

pub(crate) fn destination_from_str(s: &str) -> Result<OutboxDestination, WarasError> {
    Ok(match s {
        "satusehat" => OutboxDestination::Satusehat,
        "vclaim" => OutboxDestination::VClaim,
        "pcare" => OutboxDestination::PCare,
        "antrean_online" => OutboxDestination::AntreanOnline,
        "aplicares" => OutboxDestination::Aplicares,
        "eklaim" => OutboxDestination::EKlaim,
        _ => {
            return Err(WarasError::Database(format!(
                "nilai destination tidak dikenal di DB: {s}"
            )))
        }
    })
}

pub(crate) fn outbox_status_to_str(s: OutboxStatus) -> &'static str {
    match s {
        OutboxStatus::Pending => "pending",
        OutboxStatus::Sending => "sending",
        OutboxStatus::Sent => "sent",
        OutboxStatus::Acked => "acked",
        OutboxStatus::Failed => "failed",
        OutboxStatus::Dead => "dead",
    }
}

pub(crate) fn outbox_status_from_str(s: &str) -> Result<OutboxStatus, WarasError> {
    Ok(match s {
        "pending" => OutboxStatus::Pending,
        "sending" => OutboxStatus::Sending,
        "sent" => OutboxStatus::Sent,
        "acked" => OutboxStatus::Acked,
        "failed" => OutboxStatus::Failed,
        "dead" => OutboxStatus::Dead,
        _ => {
            return Err(WarasError::Database(format!(
                "nilai outbox_status tidak dikenal di DB: {s}"
            )))
        }
    })
}

#[derive(Debug, FromRow)]
pub struct OutboxRow {
    pub id: String,
    pub destination: String,
    pub resource_type: String,
    pub resource_id: String,
    pub idempotency_key: String,
    pub payload: String,
    pub status: String,
    pub attempts: i64,
    pub max_attempts: i64,
    pub next_retry_at: Option<String>,
    pub last_error: Option<String>,
    pub external_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl OutboxRow {
    pub fn into_domain(self) -> Result<OutboxJob, WarasError> {
        Ok(OutboxJob {
            id: self.id,
            destination: destination_from_str(&self.destination)?,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            idempotency_key: self.idempotency_key,
            payload: serde_json::from_str(&self.payload).map_err(|e| {
                WarasError::Database(format!("outbox payload bukan JSON: {e}"))
            })?,
            status: outbox_status_from_str(&self.status)?,
            attempts: self.attempts,
            max_attempts: self.max_attempts,
            next_retry_at: self.next_retry_at,
            last_error: self.last_error,
            external_id: self.external_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

impl Db {
    /// Enqueue idempotent: `INSERT OR IGNORE` atas `idempotency_key`.
    /// Kembalikan `true` bila baris baru dibuat, `false` bila kunci sudah
    /// ada. Bila `job.id` kosong, isi `OBX-######` berikutnya (pola yang
    /// sama seperti `RX-####` di `prescriptions.rs`).
    pub async fn enqueue_outbox(&self, job: &OutboxJob) -> Result<bool, WarasError> {
        let id = if job.id.trim().is_empty() {
            let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM outbox")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
            format!("OBX-{:06}", crate::next_seq_from_ids(&ids, 1))
        } else {
            job.id.clone()
        };
        // Stempel kosong diisi jam sekarang (format Fase 1: "%Y-%m-%d %H:%M:%S").
        let now = fmt_dt(now_naive());
        let created_at = if job.created_at.trim().is_empty() {
            now.clone()
        } else {
            job.created_at.clone()
        };
        let updated_at = if job.updated_at.trim().is_empty() {
            now
        } else {
            job.updated_at.clone()
        };
        let result = sqlx::query(
            "INSERT OR IGNORE INTO outbox (id, destination, resource_type, resource_id,
                 idempotency_key, payload, status, attempts, max_attempts, next_retry_at,
                 last_error, external_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(destination_to_str(job.destination))
        .bind(&job.resource_type)
        .bind(&job.resource_id)
        .bind(&job.idempotency_key)
        .bind(job.payload.to_string())
        .bind(outbox_status_to_str(job.status))
        .bind(job.attempts)
        .bind(job.max_attempts)
        .bind(&job.next_retry_at)
        .bind(&job.last_error)
        .bind(&job.external_id)
        .bind(&created_at)
        .bind(&updated_at)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(result.rows_affected() == 1)
    }

    /// Ambil batch jatuh tempo (baca saja, tanpa mengubah status):
    /// `status='pending'` DAN (`next_retry_at` NULL ATAU `<= now_iso`),
    /// urut `created_at`, LIMIT n. `next_retry_at` dibandingkan sebagai
    /// string ISO-8601 (leksikografis = kronologis untuk format UTC).
    pub async fn claim_outbox_batch(
        &self,
        limit: i64,
        now_iso: &str,
    ) -> Result<Vec<OutboxJob>, WarasError> {
        let rows: Vec<OutboxRow> = sqlx::query_as(
            "SELECT * FROM outbox
             WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= ?)
             ORDER BY created_at ASC, id ASC LIMIT ?",
        )
        .bind(now_iso)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Klaim batch SECARA ATOMIK: dalam satu transaksi, ubah status batch
    /// pending -> sending lalu kembalikan barisnya. Worker memanggil ini,
    /// bukan claim + update terpisah, supaya dua worker tidak mengambil
    /// pekerjaan yang sama.
    pub async fn claim_outbox_batch_atomic(
        &self,
        limit: i64,
        now_iso: &str,
    ) -> Result<Vec<OutboxJob>, WarasError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let rows: Vec<OutboxRow> = sqlx::query_as(
            "SELECT * FROM outbox
             WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= ?)
             ORDER BY created_at ASC, id ASC LIMIT ?",
        )
        .bind(now_iso)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let updated_at = fmt_dt(now_naive());
        let mut jobs = Vec::with_capacity(rows.len());
        for row in rows {
            // Guard `status = 'pending'` di WHERE: bila baris berubah di
            // tengah transaksi (tidak mungkin di SQLite satu writer, tapi
            // murah), jangan klaim dua kali.
            let n = sqlx::query(
                "UPDATE outbox SET status = 'sending', updated_at = ?
                 WHERE id = ? AND status = 'pending'",
            )
            .bind(&updated_at)
            .bind(&row.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
            if n == 1 {
                let mut job = row.into_domain()?;
                job.status = OutboxStatus::Sending;
                job.updated_at = updated_at.clone();
                jobs.push(job);
            }
        }
        tx.commit().await.map_err(db_err)?;
        Ok(jobs)
    }

    /// Tandai hasil kirim: status + attempts + next_retry_at + last_error +
    /// external_id + updated_at. Bila `status == Acked` dan pekerjaan
    /// adalah SATUSEHAT, perbarui juga `satusehat_id`/`satusehat_status`
    /// baris sumber (§2.1); bila `status == Dead` dan SATUSEHAT, set
    /// `satusehat_status = 'gagal'` (tanpa id). Semua dalam transaksi
    /// yang sama dengan update outbox.
    ///
    /// CATATAN §2.1: `Condition` -> `conditions` dan `AllergyIntolerance`
    /// -> `allergies` TIDAK punya kolom `satusehat_*` (migrasi 0002 hanya
    /// menambahkannya ke patients/encounters/observations/
    /// medication_requests), jadi untuk kedua resource itu update baris
    /// dilewati — cukup `external_id` di outbox. `Bundle` juga tidak
    /// punya satu baris sumber, jadi dilewati.
    pub async fn mark_outbox_sent(
        &self,
        id: &str,
        status: OutboxStatus,
        external_id: Option<&str>,
        attempts: i64,
        next_retry_at: Option<String>,
        last_error: Option<String>,
    ) -> Result<(), WarasError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let row: Option<OutboxRow> =
            sqlx::query_as("SELECT * FROM outbox WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
        let job = match row {
            Some(r) => r.into_domain()?,
            None => return Err(WarasError::NotFound(format!("outbox {id} tidak ditemukan"))),
        };
        let updated_at = fmt_dt(now_naive());
        sqlx::query(
            "UPDATE outbox SET status = ?, attempts = ?, next_retry_at = ?,
                 last_error = ?, external_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(outbox_status_to_str(status))
        .bind(attempts)
        .bind(&next_retry_at)
        .bind(&last_error)
        .bind(external_id)
        .bind(&updated_at)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        // Sinkronisasi balik §2.1, hanya untuk tujuan SATUSEHAT.
        if job.destination == OutboxDestination::Satusehat {
            let table: Option<&str> = match job.resource_type.as_str() {
                "Patient" => Some("patients"),
                "Encounter" => Some("encounters"),
                "Observation" => Some("observations"),
                "MedicationRequest" => Some("medication_requests"),
                // "Condition" dan "AllergyIntolerance": tidak ada kolom
                // satusehat_* di conditions/allergies — lewati (lihat
                // catatan modul). "Bundle" dan lainnya: tidak ada satu
                // baris sumber — lewati juga.
                _ => None,
            };
            if let Some(table) = table {
                match status {
                    OutboxStatus::Acked => {
                        let q = format!(
                            "UPDATE {table} SET satusehat_id = ?, satusehat_status = 'terkirim' WHERE id = ?"
                        );
                        sqlx::query(&q)
                            .bind(external_id)
                            .bind(&job.resource_id)
                            .execute(&mut *tx)
                            .await
                            .map_err(db_err)?;
                    }
                    OutboxStatus::Dead => {
                        let q = format!(
                            "UPDATE {table} SET satusehat_status = 'gagal' WHERE id = ?"
                        );
                        sqlx::query(&q)
                            .bind(&job.resource_id)
                            .execute(&mut *tx)
                            .await
                            .map_err(db_err)?;
                    }
                    _ => {}
                }
            }
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    /// Hitung per (destination, status): dipakai layar status jembatan.
    pub async fn outbox_counts(&self) -> Result<Vec<(String, String, i64)>, WarasError> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT destination, status, COUNT(*) FROM outbox
             GROUP BY destination, status ORDER BY destination, status",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows)
    }

    /// Daftar + filter, terbaru dulu. `status`/`destination` memakai nilai
    /// DB snake_case (`pending`, `satusehat`, ...); `None` = tanpa filter.
    pub async fn list_outbox(
        &self,
        status: Option<&str>,
        destination: Option<&str>,
        limit: i64,
    ) -> Result<Vec<OutboxJob>, WarasError> {
        let rows: Vec<OutboxRow> = match (status, destination) {
            (Some(s), Some(d)) => {
                sqlx::query_as(
                    "SELECT * FROM outbox WHERE status = ? AND destination = ?
                     ORDER BY created_at DESC, id DESC LIMIT ?",
                )
                .bind(s)
                .bind(d)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (Some(s), None) => {
                sqlx::query_as(
                    "SELECT * FROM outbox WHERE status = ?
                     ORDER BY created_at DESC, id DESC LIMIT ?",
                )
                .bind(s)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (None, Some(d)) => {
                sqlx::query_as(
                    "SELECT * FROM outbox WHERE destination = ?
                     ORDER BY created_at DESC, id DESC LIMIT ?",
                )
                .bind(d)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (None, None) => {
                sqlx::query_as(
                    "SELECT * FROM outbox ORDER BY created_at DESC, id DESC LIMIT ?",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_outbox(&self, id: &str) -> Result<Option<OutboxJob>, WarasError> {
        let row: Option<OutboxRow> =
            sqlx::query_as("SELECT * FROM outbox WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::bridge::{OutboxDestination, OutboxStatus};

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:")
            .await
            .expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    fn job(key: &str, resource_id: &str) -> OutboxJob {
        OutboxJob {
            id: String::new(),
            destination: OutboxDestination::Satusehat,
            resource_type: "Patient".into(),
            resource_id: resource_id.into(),
            idempotency_key: key.into(),
            payload: serde_json::json!({"resourceType": "Patient", "id": "812"}),
            status: OutboxStatus::Pending,
            attempts: 0,
            max_attempts: 5,
            next_retry_at: None,
            last_error: None,
            external_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[tokio::test]
    async fn enqueue_is_idempotent_on_key() {
        let db = seeded_db().await;
        assert!(db.enqueue_outbox(&job("satusehat:Patient:RM-000812:created", "RM-000812")).await.unwrap());
        assert!(!db.enqueue_outbox(&job("satusehat:Patient:RM-000812:created", "RM-000812")).await.unwrap());
        assert_eq!(db.list_outbox(None, None, 50).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn enqueue_generates_obx_id() {
        let db = seeded_db().await;
        db.enqueue_outbox(&job("k1", "RM-000812")).await.unwrap();
        db.enqueue_outbox(&job("k2", "RM-000813")).await.unwrap();
        let jobs = db.list_outbox(None, None, 50).await.unwrap();
        let mut ids: Vec<_> = jobs.iter().map(|j| j.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["OBX-000001", "OBX-000002"]);
    }

    #[tokio::test]
    async fn claim_batch_atomic_moves_to_sending_once() {
        let db = seeded_db().await;
        db.enqueue_outbox(&job("k1", "RM-000812")).await.unwrap();
        db.enqueue_outbox(&job("k2", "RM-000813")).await.unwrap();
        // Pekerjaan dengan next_retry_at di masa depan belum jatuh tempo.
        let mut future = job("k3", "RM-000814");
        future.next_retry_at = Some("2999-01-01 00:00:00".into());
        db.enqueue_outbox(&future).await.unwrap();

        let claimed = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        assert_eq!(claimed.len(), 2);
        assert!(claimed.iter().all(|j| j.status == OutboxStatus::Sending));
        // Klaim kedua: tidak ada lagi yang pending-jatuh-tempo.
        let again = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        assert!(again.is_empty());
        // Yang future tetap pending.
        let pending = db.list_outbox(Some("pending"), None, 50).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].idempotency_key, "k3");
    }

    #[tokio::test]
    async fn claim_batch_read_only_does_not_change_status() {
        let db = seeded_db().await;
        db.enqueue_outbox(&job("k1", "RM-000812")).await.unwrap();
        let batch = db.claim_outbox_batch(10, "2026-11-19 12:00:00").await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].status, OutboxStatus::Pending);
        let stored = db.get_outbox(&batch[0].id).await.unwrap().unwrap();
        assert_eq!(stored.status, OutboxStatus::Pending);
    }

    #[tokio::test]
    async fn mark_acked_syncs_satusehat_id_back() {
        let db = seeded_db().await;
        db.enqueue_outbox(&job("satusehat:Patient:RM-000812:created", "RM-000812"))
            .await
            .unwrap();
        let claimed = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);
        db.mark_outbox_sent(&claimed[0].id, OutboxStatus::Acked, Some("IHS-812"), 1, None, None)
            .await
            .unwrap();
        let stored = db.get_outbox(&claimed[0].id).await.unwrap().unwrap();
        assert_eq!(stored.status, OutboxStatus::Acked);
        assert_eq!(stored.external_id.as_deref(), Some("IHS-812"));
        assert_eq!(stored.attempts, 1);
        let (sid, sstatus): (Option<String>, String) =
            sqlx::query_as("SELECT satusehat_id, satusehat_status FROM patients WHERE id = ?")
                .bind("RM-000812")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(sid.as_deref(), Some("IHS-812"));
        assert_eq!(sstatus, "terkirim");
    }

    #[tokio::test]
    async fn mark_dead_syncs_gagal_without_id() {
        let db = seeded_db().await;
        let mut j = job("k1", "RM-000812");
        j.resource_type = "Encounter".into();
        j.resource_id = "ENC-2026-11-19-014".into();
        db.enqueue_outbox(&j).await.unwrap();
        let claimed = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        db.mark_outbox_sent(
            &claimed[0].id,
            OutboxStatus::Dead,
            None,
            1,
            None,
            Some("validasi ditolak".into()),
        )
        .await
        .unwrap();
        let (sid, sstatus): (Option<String>, String) =
            sqlx::query_as("SELECT satusehat_id, satusehat_status FROM encounters WHERE id = ?")
                .bind("ENC-2026-11-19-014")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(sid, None);
        assert_eq!(sstatus, "gagal");
    }

    #[tokio::test]
    async fn mark_acked_condition_skips_row_update() {
        // Condition tidak punya kolom satusehat_* (FASE2 §2.1): ack tetap
        // tersimpan di outbox, tanpa update baris — dan tanpa error.
        let db = seeded_db().await;
        let mut j = job("satusehat:Condition:COND-001:created", "COND-001");
        j.resource_type = "Condition".into();
        db.enqueue_outbox(&j).await.unwrap();
        let claimed = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        db.mark_outbox_sent(&claimed[0].id, OutboxStatus::Acked, Some("COND-IHS-1"), 1, None, None)
            .await
            .unwrap();
        let stored = db.get_outbox(&claimed[0].id).await.unwrap().unwrap();
        assert_eq!(stored.status, OutboxStatus::Acked);
        assert_eq!(stored.external_id.as_deref(), Some("COND-IHS-1"));
    }

    #[tokio::test]
    async fn mark_sent_non_satusehat_does_not_touch_source() {
        let db = seeded_db().await;
        let mut j = job("vclaim:SEP:SEP-20261119-001:created", "SEP-20261119-001");
        j.destination = OutboxDestination::VClaim;
        j.resource_type = "SEP".into();
        db.enqueue_outbox(&j).await.unwrap();
        let claimed = db
            .claim_outbox_batch_atomic(10, "2026-11-19 12:00:00")
            .await
            .unwrap();
        db.mark_outbox_sent(&claimed[0].id, OutboxStatus::Acked, Some("SEP-20261119-001"), 1, None, None)
            .await
            .unwrap();
        let stored = db.get_outbox(&claimed[0].id).await.unwrap().unwrap();
        assert_eq!(stored.status, OutboxStatus::Acked);
    }

    #[tokio::test]
    async fn outbox_counts_and_filters() {
        let db = seeded_db().await;
        db.enqueue_outbox(&job("k1", "RM-000812")).await.unwrap();
        let mut v = job("k2", "RM-000813");
        v.destination = OutboxDestination::VClaim;
        v.resource_type = "SEP".into();
        db.enqueue_outbox(&v).await.unwrap();
        let counts = db.outbox_counts().await.unwrap();
        assert_eq!(counts.len(), 2);
        assert!(counts.iter().all(|(_, s, n)| s == "pending" && *n == 1));
        assert_eq!(db.list_outbox(Some("pending"), Some("vclaim"), 50).await.unwrap().len(), 1);
        assert_eq!(db.list_outbox(Some("acked"), None, 50).await.unwrap().len(), 0);
        assert!(db.get_outbox("OBX-999999").await.unwrap().is_none());
    }
}
