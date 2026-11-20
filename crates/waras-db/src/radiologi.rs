//! Radiologi (FASE3.md §4): order + laporan. Simpan laporan (upsert)
//! menandai order Selesai.

use crate::rows::{
    clinic_now_string, rad_order_status_to_str, RadOrderRow, RadReportRow,
};
use crate::{db_err, Db};
use waras_core::{
    types::{RadOrder, RadOrderStatus, RadReport},
    WarasError,
};

impl Db {
    pub async fn list_rad_orders(
        &self,
        date: &str,
        status: Option<&str>,
    ) -> Result<Vec<RadOrder>, WarasError> {
        let rows: Vec<RadOrderRow> = match status {
            Some(s) => {
                sqlx::query_as(
                    "SELECT * FROM rad_orders WHERE substr(created_at, 1, 10) = ? AND status = ?
                     ORDER BY created_at ASC, id ASC",
                )
                .bind(date)
                .bind(s)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM rad_orders WHERE substr(created_at, 1, 10) = ?
                     ORDER BY created_at ASC, id ASC",
                )
                .bind(date)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_rad_order(&self, id: &str) -> Result<Option<RadOrder>, WarasError> {
        let row: Option<RadOrderRow> =
            sqlx::query_as("SELECT * FROM rad_orders WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Order radiologi baru: id `RAD-<seq>` (tanpa tanggal, sesuai desain;
    /// seed 0911..0917, berikutnya 0918).
    pub async fn create_rad_order(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        modality: &str,
        area: &str,
        ordered_from: &str,
        ordered_by: &str,
    ) -> Result<RadOrder, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM rad_orders")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 911);
        let id = format!("RAD-{seq:04}");
        sqlx::query(
            "INSERT INTO rad_orders (id, patient_id, encounter_id, modality, area,
                 ordered_from, ordered_by, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'menunggu', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(modality)
        .bind(area)
        .bind(ordered_from)
        .bind(ordered_by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_rad_order(&id)
            .await?
            .ok_or_else(|| WarasError::Database("order radiologi baru tidak ditemukan".into()))
    }

    pub async fn set_rad_order_status(
        &self,
        id: &str,
        status: RadOrderStatus,
    ) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE rad_orders SET status = ? WHERE id = ?")
            .bind(rad_order_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "order radiologi {id} tidak ditemukan"
            )));
        }
        Ok(())
    }

    pub async fn get_rad_report(&self, order_id: &str) -> Result<Option<RadReport>, WarasError> {
        let row: Option<RadReportRow> =
            sqlx::query_as("SELECT * FROM rad_reports WHERE order_id = ? LIMIT 1")
                .bind(order_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Simpan laporan (upsert per order) + order jadi Selesai.
    pub async fn save_rad_report(
        &self,
        order_id: &str,
        findings: &str,
        radiologist: &str,
    ) -> Result<RadReport, WarasError> {
        let order = self
            .get_rad_order(order_id)
            .await?
            .ok_or_else(|| {
                WarasError::NotFound(format!("order radiologi {order_id} tidak ditemukan"))
            })?;
        let _ = order;
        let now = clinic_now_string();
        match self.get_rad_report(order_id).await? {
            Some(existing) => {
                sqlx::query(
                    "UPDATE rad_reports SET findings = ?, radiologist = ?, reported_at = ?
                     WHERE id = ?",
                )
                .bind(findings)
                .bind(radiologist)
                .bind(&now)
                .bind(&existing.id)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                sqlx::query("UPDATE rad_orders SET status = 'selesai' WHERE id = ?")
                    .bind(order_id)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                self.get_rad_report(order_id)
                    .await?
                    .ok_or_else(|| WarasError::Database("laporan radiologi tidak ditemukan".into()))
            }
            None => {
                let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM rad_reports")
                    .fetch_all(&self.pool)
                    .await
                    .map_err(db_err)?;
                let seq = crate::next_seq_from_ids(&ids, 1);
                let id = format!("RDR-{seq:06}");
                sqlx::query(
                    "INSERT INTO rad_reports (id, order_id, findings, radiologist, reported_at)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(order_id)
                .bind(findings)
                .bind(radiologist)
                .bind(&now)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                sqlx::query("UPDATE rad_orders SET status = 'selesai' WHERE id = ?")
                    .bind(order_id)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                self.get_rad_report(order_id)
                    .await?
                    .ok_or_else(|| WarasError::Database("laporan radiologi baru tidak ditemukan".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clinic_today;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_rad_orders_and_reports() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let orders = db.list_rad_orders(&date, None).await.unwrap();
        assert_eq!(orders.len(), 7);
        assert_eq!(orders[0].id, "RAD-0911");
        assert_eq!(orders[0].modality, "X-Ray");
        assert_eq!(orders[0].area, "Thorax AP");
        assert_eq!(orders[0].status, RadOrderStatus::Selesai);
        assert_eq!(orders[3].status, RadOrderStatus::Menunggu);
        assert_eq!(orders[4].status, RadOrderStatus::Proses);
        let menunggu = db
            .list_rad_orders(&date, Some("menunggu"))
            .await
            .unwrap();
        assert_eq!(menunggu.len(), 3);
        let rep = db.get_rad_report("RAD-0912").await.unwrap().unwrap();
        assert!(rep.findings.contains("Fraktur radius distal"));
        assert_eq!(rep.radiologist, "dr. Andi Wijaya, Sp.Rad");
        assert!(db.get_rad_report("RAD-0914").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rad_report_upsert_marks_done() {
        let db = seeded_db().await;
        let order = db
            .create_rad_order(
                "RM-000813",
                None,
                "X-Ray",
                "Thorax AP",
                "Poli Umum",
                "dr. Ahmad Hidayat",
            )
            .await
            .unwrap();
        assert_eq!(order.id, "RAD-0918");
        let rep = db
            .save_rad_report(&order.id, "Tidak tampak infiltrat.", "dr. Andi Wijaya, Sp.Rad")
            .await
            .unwrap();
        assert_eq!(rep.id, "RDR-000004");
        assert_eq!(
            db.get_rad_order(&order.id).await.unwrap().unwrap().status,
            RadOrderStatus::Selesai
        );
        // Simpan ulang: upsert, bukan baris baru.
        let rep2 = db
            .save_rad_report(&order.id, "Revisi: tampak infiltrat ringan.", "dr. Andi Wijaya, Sp.Rad")
            .await
            .unwrap();
        assert_eq!(rep2.id, rep.id);
        assert!(rep2.findings.contains("Revisi"));
        assert!(matches!(
            db.save_rad_report("RAD-9999", "x", "y").await,
            Err(WarasError::NotFound(_))
        ));
        db.set_rad_order_status(&order.id, RadOrderStatus::Proses)
            .await
            .unwrap();
        assert_eq!(
            db.get_rad_order(&order.id).await.unwrap().unwrap().status,
            RadOrderStatus::Proses
        );
    }
}
