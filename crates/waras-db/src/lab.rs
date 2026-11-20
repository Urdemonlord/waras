//! Laboratorium (FASE3.md §4): order + hasil. Flag dihitung di server
//! (`waras_core::casemix::lab_flag`), BUKAN dari klien. Order jadi
//! Kritis bila ada analit Kritis — saat itu dibuat notifikasi KRITIS +
//! audit `lab.critical`.

use crate::rows::{
    clinic_now_string, lab_flag_to_str, lab_order_status_to_str, LabOrderRow, LabResultRow,
};
use crate::{clinic_today, db_err, Db};
use serde_json::json;
use waras_core::{
    casemix::{lab_flag, parse_lab_value},
    types::{LabFlag, LabOrder, LabOrderStatus, LabResult},
    WarasError,
};

/// Input satu analit dari API/UI. Flag TIDAK dibawa klien — dihitung
/// `add_lab_results` dari nilai + rujukan + ambang kritis.
#[derive(Debug, Clone)]
pub struct NewLabResult {
    pub analyte: String,
    pub loinc: Option<String>,
    pub value_text: String,
    pub unit: String,
    pub ref_text: String,
    pub ref_low: Option<f64>,
    pub ref_high: Option<f64>,
    pub critical_high: Option<f64>,
    pub critical_low: Option<f64>,
}

impl Db {
    pub async fn list_lab_orders(
        &self,
        date: &str,
        status: Option<&str>,
    ) -> Result<Vec<LabOrder>, WarasError> {
        let rows: Vec<LabOrderRow> = match status {
            Some(s) => {
                sqlx::query_as(
                    "SELECT * FROM lab_orders WHERE substr(created_at, 1, 10) = ? AND status = ?
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
                    "SELECT * FROM lab_orders WHERE substr(created_at, 1, 10) = ?
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

    pub async fn get_lab_order(&self, id: &str) -> Result<Option<LabOrder>, WarasError> {
        let row: Option<LabOrderRow> =
            sqlx::query_as("SELECT * FROM lab_orders WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Order lab baru: id `LAB-<seq>` (tanpa tanggal, sesuai desain;
    /// seed 2210..2217, berikutnya 2218).
    pub async fn create_lab_order(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        test_name: &str,
        loinc: &str,
        ordered_from: &str,
        ordered_by: &str,
    ) -> Result<LabOrder, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM lab_orders")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 2210);
        let id = format!("LAB-{seq}");
        sqlx::query(
            "INSERT INTO lab_orders (id, patient_id, encounter_id, test_name,
                 loinc, ordered_from, ordered_by, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'menunggu', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(test_name)
        .bind(loinc)
        .bind(ordered_from)
        .bind(ordered_by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_lab_order(&id)
            .await?
            .ok_or_else(|| WarasError::Database("order lab baru tidak ditemukan".into()))
    }

    /// Ubah status order (dipakai endpoint `POST /api/lab/orders/{id}/status`;
    /// kontrak FASE3.md §5, method §4 — ditambahkan saat integrasi karena
    /// agen A belum menulisnya).
    pub async fn set_lab_order_status(
        &self,
        id: &str,
        status: LabOrderStatus,
    ) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE lab_orders SET status = ? WHERE id = ?")
            .bind(lab_order_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "order lab {id} tidak ditemukan"
            )));
        }
        Ok(())
    }

    pub async fn list_lab_results(&self, order_id: &str) -> Result<Vec<LabResult>, WarasError> {
        let rows: Vec<LabResultRow> = sqlx::query_as(
            "SELECT * FROM lab_results WHERE order_id = ? ORDER BY created_at ASC, id ASC",
        )
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Simpan hasil lab. Flag tiap analit dihitung di sini; order jadi
    /// Kritis bila ada analit Kritis, bila tidak Selesai. Bila Kritis:
    /// notifikasi KRITIS + audit `lab.critical`.
    pub async fn add_lab_results(
        &self,
        order_id: &str,
        results: &[NewLabResult],
        by: &str,
    ) -> Result<LabOrder, WarasError> {
        let order = self
            .get_lab_order(order_id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("order lab {order_id} tidak ditemukan")))?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM lab_results")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let mut seq = crate::next_seq_from_ids(&ids, 1);
        let now = clinic_now_string();
        let mut any_critical = false;
        let mut critical_lines: Vec<String> = Vec::new();
        for r in results {
            let flag = match parse_lab_value(&r.value_text) {
                Some(v) => lab_flag(v, r.ref_low, r.ref_high, r.critical_low, r.critical_high),
                None => LabFlag::Normal,
            };
            if flag == LabFlag::Kritis {
                any_critical = true;
                critical_lines.push(format!(
                    "{} {} {} (rujukan: {})",
                    r.analyte, r.value_text, r.unit, r.ref_text
                ));
            }
            let id = format!("LBR-{seq:06}");
            seq += 1;
            sqlx::query(
                "INSERT INTO lab_results (id, order_id, analyte, loinc, value_text,
                     unit, ref_text, ref_low, ref_high, critical_high, critical_low,
                     flag, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(order_id)
            .bind(&r.analyte)
            .bind(&r.loinc)
            .bind(&r.value_text)
            .bind(&r.unit)
            .bind(&r.ref_text)
            .bind(r.ref_low)
            .bind(r.ref_high)
            .bind(r.critical_high)
            .bind(r.critical_low)
            .bind(lab_flag_to_str(flag))
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        let new_status = if any_critical {
            LabOrderStatus::Kritis
        } else {
            LabOrderStatus::Selesai
        };
        sqlx::query("UPDATE lab_orders SET status = ? WHERE id = ?")
            .bind(lab_order_status_to_str(new_status))
            .bind(order_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        if any_critical {
            let patient_name = self
                .get_patient(&order.patient_id)
                .await?
                .map(|p| p.name)
                .unwrap_or_else(|| order.patient_id.clone());
            let message = format!(
                "Hasil lab kritis {}: {}.",
                patient_name,
                critical_lines.join("; ")
            );
            self.insert_notification_kritis(&message).await?;
            self.log_audit(
                by,
                "lab.critical",
                "lab_order",
                order_id,
                json!({"patient_id": order.patient_id, "critical": critical_lines}),
            )
            .await?;
        }
        self.get_lab_order(order_id)
            .await?
            .ok_or_else(|| WarasError::Database("order lab tidak ditemukan".into()))
    }

    /// (menunggu/proses, kritis) hari itu.
    pub async fn lab_counts(&self, date: &str) -> Result<(i64, i64), WarasError> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT
               COALESCE(SUM(CASE WHEN status IN ('menunggu', 'proses') THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN status = 'kritis' THEN 1 ELSE 0 END), 0)
             FROM lab_orders WHERE substr(created_at, 1, 10) = ?",
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row)
    }

    /// Sisip notifikasi KRITIS hari klinik (dipakai hasil lab kritis).
    /// Id `NOTIF-<seq>` mengikuti seed (`NOTIF-001..003`).
    async fn insert_notification_kritis(&self, message: &str) -> Result<(), WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM notifications")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("NOTIF-{seq:03}");
        let now = clinic_now_string();
        let time = now[11..16].replace(':', ".");
        sqlx::query(
            "INSERT INTO notifications (id, kind, message, time, date, read)
             VALUES (?, 'kritis', ?, ?, ?, 0)",
        )
        .bind(&id)
        .bind(message)
        .bind(&time)
        .bind(clinic_today().to_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_lab_orders_results_counts() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let orders = db.list_lab_orders(&date, None).await.unwrap();
        assert_eq!(orders.len(), 8);
        assert_eq!(orders[0].id, "LAB-2210");
        assert_eq!(orders[0].test_name, "Darah lengkap");
        assert_eq!(orders[0].loinc, "58410-2");
        assert_eq!(orders[1].status, LabOrderStatus::Kritis);
        // Hitungan nyata seed: 3 menunggu/proses, 1 kritis.
        assert_eq!(db.lab_counts(&date).await.unwrap(), (3, 1));
        let res2211 = db.list_lab_results("LAB-2211").await.unwrap();
        assert_eq!(res2211.len(), 5);
        let troponin = res2211.iter().find(|r| r.analyte == "Troponin I").unwrap();
        assert_eq!(troponin.value_text, "0,42");
        assert_eq!(troponin.flag, LabFlag::Kritis);
        assert_eq!(troponin.ref_high, Some(0.04));
        let leukosit = res2211.iter().find(|r| r.analyte == "Leukosit").unwrap();
        assert_eq!(leukosit.flag, LabFlag::Tinggi);
        let hb = res2211.iter().find(|r| r.analyte == "Hemoglobin").unwrap();
        assert_eq!(hb.flag, LabFlag::Normal);
        let res2210 = db.list_lab_results("LAB-2210").await.unwrap();
        assert_eq!(res2210.len(), 3);
        assert!(res2210.iter().all(|r| r.flag == LabFlag::Normal));
    }

    #[tokio::test]
    async fn critical_flag_flips_order_and_notifies() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        // Order baru -> LAB-2218.
        let order = db
            .create_lab_order(
                "RM-000820",
                Some("ENC-2026-11-19-021"),
                "Troponin I",
                "10839-9",
                "IGD",
                "dr. Andi Wijaya",
            )
            .await
            .unwrap();
        assert_eq!(order.id, "LAB-2218");
        assert_eq!(order.status, LabOrderStatus::Menunggu);
        // Nilai non-numerik -> Normal, order Selesai, tanpa notifikasi.
        let done = db
            .add_lab_results(
                &order.id,
                &[NewLabResult {
                    analyte: "Golongan darah".into(),
                    loinc: Some("883-9".into()),
                    value_text: "A+".into(),
                    unit: String::new(),
                    ref_text: String::new(),
                    ref_low: None,
                    ref_high: None,
                    critical_high: None,
                    critical_low: None,
                }],
                "dr. Andi Wijaya",
            )
            .await
            .unwrap();
        assert_eq!(done.status, LabOrderStatus::Selesai);
        // Troponin kritis -> order Kritis + notifikasi + audit.
        let notifs_before = db.list_notifications(clinic_today()).await.unwrap().len();
        let order2 = db
            .create_lab_order(
                "RM-000820",
                Some("ENC-2026-11-19-021"),
                "Troponin I",
                "10839-9",
                "IGD",
                "dr. Andi Wijaya",
            )
            .await
            .unwrap();
        let done2 = db
            .add_lab_results(
                &order2.id,
                &[NewLabResult {
                    analyte: "Troponin I".into(),
                    loinc: Some("10839-9".into()),
                    value_text: "0,42".into(),
                    unit: "ng/mL".into(),
                    ref_text: "< 0,04 ng/mL".into(),
                    ref_low: None,
                    ref_high: Some(0.04),
                    critical_high: Some(0.04),
                    critical_low: None,
                }],
                "dr. Andi Wijaya",
            )
            .await
            .unwrap();
        assert_eq!(done2.status, LabOrderStatus::Kritis);
        let results = db.list_lab_results(&order2.id).await.unwrap();
        assert_eq!(results[0].flag, LabFlag::Kritis);
        let notifs = db.list_notifications(clinic_today()).await.unwrap();
        assert_eq!(notifs.len(), notifs_before + 1);
        assert!(notifs.iter().any(|n| n.message.contains("Troponin I")));
        assert_eq!(db.lab_counts(&date).await.unwrap(), (3, 2));
        assert!(matches!(
            db.add_lab_results(&order2.id, &[], "x").await,
            Ok(_)
        ));
        assert!(matches!(
            db.add_lab_results("LAB-9999", &[], "x").await,
            Err(WarasError::NotFound(_))
        ));
    }
}
