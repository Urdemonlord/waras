//! Audit trail append-only + rantai hash SHA-256.
//! Tidak ada UPDATE/DELETE di kode mana pun untuk tabel ini.

use crate::rows::AuditRow;
use crate::{db_err, fmt_dt, now_naive, Db};
use chrono::{NaiveDateTime, Timelike};
use waras_core::{types::AuditEntry, WarasError};

impl Db {
    /// Tambah satu entri audit. `prev_hash` diambil dari entri terakhir,
    /// `"GENESIS"` untuk entri pertama. `entry_hash` dihitung lewat
    /// `Db::audit_entry_hash` (lihat lib.rs untuk format pastinya).
    pub async fn log_audit(
        &self,
        actor: &str,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        details: serde_json::Value,
    ) -> Result<AuditEntry, WarasError> {
        self.insert_audit_at(now_naive(), actor, action, resource_type, resource_id, details)
            .await
    }

    /// Sama seperti `log_audit`, tapi timestamp ditentukan pemanggil
    /// (dipakai seed untuk 6 entri audit bersejarah KONTRAK §5.13).
    pub(crate) async fn insert_audit_at(
        &self,
        timestamp: NaiveDateTime,
        actor: &str,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        details: serde_json::Value,
    ) -> Result<AuditEntry, WarasError> {
        // Simpan + hash dalam presisi detik: kolom timestamp hanya
        // menyimpan "%Y-%m-%d %H:%M:%S", jadi nanodetik harus dibuang
        // SEBELUM hashing, kalau tidak hash tidak bisa diverifikasi ulang.
        let timestamp = timestamp
            .with_nanosecond(0)
            .unwrap_or(timestamp);
        let last_hash: Option<String> = sqlx::query_scalar(
            "SELECT entry_hash FROM audit_log ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let prev_hash = last_hash.unwrap_or_else(|| "GENESIS".to_string());
        let entry_hash = Self::audit_entry_hash(
            &prev_hash,
            timestamp,
            actor,
            action,
            resource_type,
            resource_id,
            &details,
        );
        let details_s = details.to_string();
        sqlx::query(
            "INSERT INTO audit_log (timestamp, actor, action, resource_type,
                 resource_id, details, prev_hash, entry_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(fmt_dt(timestamp))
        .bind(actor)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(&details_s)
        .bind(&prev_hash)
        .bind(&entry_hash)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        // ID diambil lewat lookup `entry_hash`, BUKAN
        // `last_insert_rowid()`: hasil `AnyPool` (`AnyQueryResult`)
        // tidak punya rowid — dan backend SQLite-nya `Any` pun tidak
        // meneruskan rowid (`last_insert_id` selalu `None`). Lookup ini
        // berlaku sama di SQLite maupun Postgres.
        let id: i64 = sqlx::query_scalar("SELECT id FROM audit_log WHERE entry_hash = ?")
            .bind(&entry_hash)
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(AuditEntry {
            id,
            timestamp,
            actor: actor.to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            details,
            prev_hash,
            entry_hash,
        })
    }

    /// Entri audit terbaru dulu.
    pub async fn list_audit(&self, limit: i64) -> Result<Vec<AuditEntry>, WarasError> {
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT * FROM audit_log ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }
}
