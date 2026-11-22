//! Rekam medis gigi (FASE4.md §4).
//! Nomor gigi WAJIB notasi FDI 11–18, 21–28, 31–38, 41–48 (bila tidak 400).

use crate::rows::{clinic_now_string, DentalRecordRow};
use crate::{db_err, Db};
use waras_core::{types::DentalRecord, WarasError};

/// Benar bila `tooth_number` notasi FDI: kuadran 1–4, gigi 1–8.
pub(crate) fn valid_fdi(tooth_number: &str) -> bool {
    let Ok(n) = tooth_number.parse::<u32>() else {
        return false;
    };
    matches!(n, 11..=18 | 21..=28 | 31..=38 | 41..=48)
}

impl Db {
    pub async fn list_dental_records(
        &self,
        patient_id: Option<&str>,
    ) -> Result<Vec<DentalRecord>, WarasError> {
        let rows: Vec<DentalRecordRow> = match patient_id {
            Some(p) => {
                sqlx::query_as(
                    "SELECT * FROM dental_records WHERE patient_id = ?
                     ORDER BY created_at DESC, id ASC",
                )
                .bind(p)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM dental_records ORDER BY created_at DESC, id ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Rekam gigi baru: id `GG-<seq>`, status awal Selesai (default DDL).
    /// Nomor gigi bukan FDI → 400. `create_dental_record_once` memegang
    /// `Db::alloc_lock` selama hitung-max + INSERT; retry maks 5x di sini
    /// hanya jaring pengaman untuk tabrakan antar proses.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_dental_record(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        tooth_number: &str,
        diagnosis: &str,
        diagnosis_code: &str,
        treatment: &str,
        treatment_code: &str,
        dentist: &str,
    ) -> Result<DentalRecord, WarasError> {
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self
                .create_dental_record_once(
                    patient_id,
                    encounter_id,
                    tooth_number,
                    diagnosis,
                    diagnosis_code,
                    treatment,
                    treatment_code,
                    dentist,
                )
                .await
            {
                Ok(r) => return Ok(r),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id rekam gigi".into())))
    }

    async fn create_dental_record_once(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        tooth_number: &str,
        diagnosis: &str,
        diagnosis_code: &str,
        treatment: &str,
        treatment_code: &str,
        dentist: &str,
    ) -> Result<DentalRecord, WarasError> {
        if !valid_fdi(tooth_number) {
            return Err(WarasError::Validation(format!(
                "nomor gigi {tooth_number} bukan notasi FDI (11–48)"
            )));
        }
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM dental_records")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("GG-{seq:06}");
        sqlx::query(
            "INSERT INTO dental_records (id, patient_id, encounter_id, tooth_number,
                 diagnosis, diagnosis_code, treatment, treatment_code, dentist,
                 status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'Selesai', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(tooth_number)
        .bind(diagnosis)
        .bind(diagnosis_code)
        .bind(treatment)
        .bind(treatment_code)
        .bind(dentist)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<DentalRecordRow> =
            sqlx::query_as("SELECT * FROM dental_records WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("rekam gigi baru tidak ditemukan".into()))
    }

    pub async fn set_dental_record_status(
        &self,
        id: &str,
        status: &str,
    ) -> Result<DentalRecord, WarasError> {
        if status != "Rencana" && status != "Berjalan" && status != "Selesai" {
            return Err(WarasError::Validation(format!(
                "status rekam gigi harus 'Rencana', 'Berjalan', atau 'Selesai', bukan {status:?}"
            )));
        }
        let n = sqlx::query("UPDATE dental_records SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "rekam gigi {id} tidak ditemukan"
            )));
        }
        let row: Option<DentalRecordRow> =
            sqlx::query_as("SELECT * FROM dental_records WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| {
                WarasError::Database("rekam gigi tidak ditemukan setelah update".into())
            })
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
    async fn seed_dental_records() {
        let db = seeded_db().await;
        let list = db.list_dental_records(None).await.unwrap();
        assert_eq!(list.len(), 5);
        let agus = list.iter().find(|r| r.patient_id == "RM-000815").unwrap();
        assert_eq!(agus.tooth_number, "36");
        assert_eq!(agus.diagnosis_code, "K02.9");
        assert_eq!(agus.treatment, "Tambal komposit");
        assert_eq!(agus.treatment_code, "23.2");
        assert_eq!(agus.dentist, "drg. Maya K.");
        assert_eq!(agus.status, "Selesai");
        let budi = list.iter().find(|r| r.patient_id == "RM-000813").unwrap();
        assert_eq!(budi.status, "Rencana");
        let putri = list.iter().find(|r| r.patient_id == "RM-000819").unwrap();
        assert_eq!(putri.tooth_number, "21");
        assert_eq!(putri.diagnosis_code, "K00.6");
        assert_eq!(putri.treatment_code, "");
        assert_eq!(
            db.list_dental_records(Some("RM-000815")).await.unwrap().len(),
            1
        );
    }

    #[test]
    fn fdi_validation() {
        for ok in ["11", "18", "21", "28", "31", "38", "41", "48", "36"] {
            assert!(valid_fdi(ok), "{ok} harus valid");
        }
        // Nomor layar Gigi (19, 20, 47–62) BUKAN FDI — ditolak.
        for bad in ["10", "19", "20", "29", "39", "49", "50", "62", "0", "99", "1A", ""] {
            assert!(!valid_fdi(bad), "{bad} harus ditolak");
        }
    }

    #[tokio::test]
    async fn dental_record_create_rejects_non_fdi() {
        let db = seeded_db().await;
        assert!(matches!(
            db.create_dental_record(
                "RM-000813", None, "19", "Karies", "K02.9", "Tambal", "23.2", "drg. Maya K.",
            )
            .await,
            Err(WarasError::Validation(_))
        ));
        let err = db
            .create_dental_record(
                "RM-000813", None, "19", "Karies", "K02.9", "Tambal", "23.2", "drg. Maya K.",
            )
            .await
            .unwrap_err();
        assert_eq!(err.http_status(), 400);
        let r = db
            .create_dental_record(
                "RM-000813", None, "46", "Karies dentin", "K02.9", "Tambal komposit", "23.2",
                "drg. Maya K.",
            )
            .await
            .unwrap();
        assert_eq!(r.id, "GG-000006");
        assert_eq!(r.status, "Selesai");
        let planned = db
            .set_dental_record_status(&r.id, "Rencana")
            .await
            .unwrap();
        assert_eq!(planned.status, "Rencana");
        assert!(matches!(
            db.set_dental_record_status(&r.id, "Menunggu").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_dental_record_status("GG-999999", "Selesai").await,
            Err(WarasError::NotFound(_))
        ));
    }
}
