//! Notifikasi hari klinik.

use crate::db_err;
use crate::rows::NotificationRow;
use crate::Db;
use chrono::NaiveDate;
use waras_core::{types::Notification, WarasError};

impl Db {
    /// Notifikasi satu tanggal, terbaru (jam terbesar) dulu.
    pub async fn list_notifications(
        &self,
        date: NaiveDate,
    ) -> Result<Vec<Notification>, WarasError> {
        let rows: Vec<NotificationRow> = sqlx::query_as(
            "SELECT * FROM notifications WHERE date = ? ORDER BY time DESC",
        )
        .bind(date.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }
}
