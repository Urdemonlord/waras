//! Transfusi darah (FASE4.md §4): stok kantong, permintaan, crossmatch.
//! Status stok dihitung dari `units`: 0 Habis, <=2 Menipis, else Tersedia.
//! Crossmatch `Cocok` mengubah permintaan jadi Crossmatch.

use crate::rows::{
    blood_request_status_to_str, clinic_now_string, BloodCrossmatchRow, BloodRequestRow,
    BloodStockRow,
};
use crate::{clinic_today, db_err, Db};
use waras_core::{
    types::{BloodCrossmatch, BloodRequest, BloodRequestStatus, BloodStock},
    WarasError,
};

/// Status stok darah dari jumlah kantong (FASE4.md §4).
pub(crate) fn blood_stock_status(units: i64) -> &'static str {
    if units <= 0 {
        "Habis"
    } else if units <= 2 {
        "Menipis"
    } else {
        "Tersedia"
    }
}

impl Db {
    pub async fn list_blood_stock(&self) -> Result<Vec<BloodStock>, WarasError> {
        let rows: Vec<BloodStockRow> = sqlx::query_as(
            "SELECT * FROM blood_stock ORDER BY blood_type ASC, component ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_blood_stock(&self, id: &str) -> Result<Option<BloodStock>, WarasError> {
        let row: Option<BloodStockRow> =
            sqlx::query_as("SELECT * FROM blood_stock WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Ubah jumlah kantong; status ikut dihitung ulang dari `units`.
    pub async fn set_blood_stock_units(
        &self,
        id: &str,
        units: i64,
    ) -> Result<BloodStock, WarasError> {
        if units < 0 {
            return Err(WarasError::Validation(
                "jumlah kantong tidak boleh negatif".into(),
            ));
        }
        let n = sqlx::query("UPDATE blood_stock SET units = ?, status = ? WHERE id = ?")
            .bind(units)
            .bind(blood_stock_status(units))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "stok darah {id} tidak ditemukan"
            )));
        }
        self.get_blood_stock(id)
            .await?
            .ok_or_else(|| WarasError::Database("stok darah tidak ditemukan setelah update".into()))
    }

    pub async fn list_blood_requests(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<BloodRequest>, WarasError> {
        let rows: Vec<BloodRequestRow> = match status {
            Some(s) => {
                sqlx::query_as(
                    "SELECT * FROM blood_requests WHERE status = ?
                     ORDER BY created_at ASC, id ASC",
                )
                .bind(s)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM blood_requests ORDER BY created_at ASC, id ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_blood_request(&self, id: &str) -> Result<Option<BloodRequest>, WarasError> {
        let row: Option<BloodRequestRow> =
            sqlx::query_as("SELECT * FROM blood_requests WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Permintaan baru: id `TR-<tanggal klinik>-<seq>`, status Menunggu.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_blood_request(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        blood_type: &str,
        component: &str,
        units: i64,
        reason: &str,
        requested_by: &str,
    ) -> Result<BloodRequest, WarasError> {
        if units <= 0 {
            return Err(WarasError::Validation(
                "jumlah kantong harus lebih dari 0".into(),
            ));
        }
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("TR-{date_part}-");
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM blood_requests WHERE id LIKE ?")
                .bind(format!("{prefix}%"))
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        sqlx::query(
            "INSERT INTO blood_requests (id, patient_id, encounter_id, blood_type,
                 component, units, reason, status, requested_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'menunggu', ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(blood_type)
        .bind(component)
        .bind(units)
        .bind(reason)
        .bind(requested_by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_blood_request(&id)
            .await?
            .ok_or_else(|| WarasError::Database("permintaan darah baru tidak ditemukan".into()))
    }

    pub async fn set_blood_request_status(
        &self,
        id: &str,
        status: BloodRequestStatus,
    ) -> Result<BloodRequest, WarasError> {
        let n = sqlx::query("UPDATE blood_requests SET status = ? WHERE id = ?")
            .bind(blood_request_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "permintaan darah {id} tidak ditemukan"
            )));
        }
        self.get_blood_request(id)
            .await?
            .ok_or_else(|| {
                WarasError::Database("permintaan darah tidak ditemukan setelah update".into())
            })
    }

    pub async fn list_crossmatches(
        &self,
        request_id: &str,
    ) -> Result<Vec<BloodCrossmatch>, WarasError> {
        let rows: Vec<BloodCrossmatchRow> = sqlx::query_as(
            "SELECT * FROM blood_crossmatches WHERE request_id = ?
             ORDER BY tested_at ASC, id ASC",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Crossmatch baru: id `XM-<seq>`. Hasil `Cocok` mengubah permintaan
    /// jadi Crossmatch; `Tidak Cocok` membiarkan status permintaan.
    pub async fn add_crossmatch(
        &self,
        request_id: &str,
        bag_id: &str,
        result: &str,
        tested_by: &str,
    ) -> Result<BloodCrossmatch, WarasError> {
        if result != "Cocok" && result != "Tidak Cocok" {
            return Err(WarasError::Validation(format!(
                "hasil crossmatch harus 'Cocok' atau 'Tidak Cocok', bukan {result:?}"
            )));
        }
        let req = self
            .get_blood_request(request_id)
            .await?
            .ok_or_else(|| {
                WarasError::NotFound(format!("permintaan darah {request_id} tidak ditemukan"))
            })?;
        let _ = req;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM blood_crossmatches")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("XM-{seq:06}");
        sqlx::query(
            "INSERT INTO blood_crossmatches (id, request_id, bag_id, result,
                 tested_by, tested_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(request_id)
        .bind(bag_id)
        .bind(result)
        .bind(tested_by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        if result == "Cocok" {
            self.set_blood_request_status(request_id, BloodRequestStatus::Crossmatch)
                .await?;
        }
        let row: Option<BloodCrossmatchRow> =
            sqlx::query_as("SELECT * FROM blood_crossmatches WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("crossmatch baru tidak ditemukan".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_blood_stock_and_requests() {
        let db = seeded_db().await;
        let stock = db.list_blood_stock().await.unwrap();
        assert_eq!(stock.len(), 8);
        let by_id = |id: &str| stock.iter().find(|s| s.id == id).unwrap();
        assert_eq!(by_id("BL-A-POS-001").units, 2);
        assert_eq!(by_id("BL-A-POS-001").status, "Tersedia");
        assert_eq!(by_id("BL-O-POS-001").units, 1);
        assert_eq!(by_id("BL-O-POS-001").status, "Menipis");
        assert_eq!(by_id("BL-AB-NEG-001").units, 0);
        assert_eq!(by_id("BL-AB-NEG-001").status, "Habis");
        assert!(stock.iter().all(|s| s.component == "PRC"));
        let reqs = db.list_blood_requests(None).await.unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].patient_id, "RM-000820");
        assert_eq!(reqs[0].blood_type, "O+");
        assert_eq!(reqs[0].units, 2);
        assert_eq!(reqs[0].status, BloodRequestStatus::Menunggu);
        assert_eq!(reqs[1].status, BloodRequestStatus::Crossmatch);
        assert_eq!(
            db.list_blood_requests(Some("menunggu")).await.unwrap().len(),
            1
        );
        let xm = db.list_crossmatches("TR-20261119-002").await.unwrap();
        assert_eq!(xm.len(), 1);
        assert_eq!(xm[0].bag_id, "BL-B-POS-001");
        assert_eq!(xm[0].result, "Cocok");
        assert_eq!(xm[0].tested_by, "Ns. Sari Wijaya");
        assert!(db.get_blood_stock("BL-X-001").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn blood_stock_status_derivation() {
        let db = seeded_db().await;
        let s = db.set_blood_stock_units("BL-A-POS-001", 0).await.unwrap();
        assert_eq!(s.status, "Habis");
        let s = db.set_blood_stock_units("BL-A-POS-001", 2).await.unwrap();
        assert_eq!(s.status, "Menipis");
        let s = db.set_blood_stock_units("BL-A-POS-001", 3).await.unwrap();
        assert_eq!(s.status, "Tersedia");
        assert!(matches!(
            db.set_blood_stock_units("BL-A-POS-001", -1).await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_blood_stock_units("BL-X-001", 5).await,
            Err(WarasError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn blood_request_and_crossmatch_flow() {
        let db = seeded_db().await;
        let r = db
            .create_blood_request(
                "RM-000813", None, "A+", "PRC", 1, "Anemia", "dr. Ahmad Hidayat",
            )
            .await
            .unwrap();
        assert_eq!(r.id, "TR-20261119-003");
        assert_eq!(r.status, BloodRequestStatus::Menunggu);
        assert!(matches!(
            db.create_blood_request("RM-000813", None, "A+", "PRC", 0, "x", "dr. X")
                .await,
            Err(WarasError::Validation(_))
        ));
        // Tidak Cocok tidak mengubah status permintaan.
        db.add_crossmatch(&r.id, "BL-A-POS-001", "Tidak Cocok", "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(
            db.get_blood_request(&r.id).await.unwrap().unwrap().status,
            BloodRequestStatus::Menunggu
        );
        // Cocok mengubah permintaan jadi Crossmatch.
        let xm = db
            .add_crossmatch(&r.id, "BL-A-POS-001", "Cocok", "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(xm.id, "XM-000003");
        assert_eq!(
            db.get_blood_request(&r.id).await.unwrap().unwrap().status,
            BloodRequestStatus::Crossmatch
        );
        assert_eq!(db.list_crossmatches(&r.id).await.unwrap().len(), 2);
        assert!(matches!(
            db.add_crossmatch(&r.id, "BL-A-POS-001", "Ragu", "Ns. Sari Wijaya")
                .await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.add_crossmatch("TR-20261119-999", "BL-A-POS-001", "Cocok", "Ns. Sari")
                .await,
            Err(WarasError::NotFound(_))
        ));
        let done = db
            .set_blood_request_status(&r.id, BloodRequestStatus::Diberikan)
            .await
            .unwrap();
        assert_eq!(done.status, BloodRequestStatus::Diberikan);
    }
}
