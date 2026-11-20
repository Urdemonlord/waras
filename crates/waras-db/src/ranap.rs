//! Rawat inap (FASE3.md §4): bangsal, bed, admisi, mutasi, pulang,
//! catatan perawatan. Perubahan bed + admisi dalam satu transaksi.

use crate::rows::{
    bed_status_to_str, admission_status_to_str, clinic_now_string, AdmissionRow, CareNoteRow, WardBedRow, WardRow,
};
use crate::{clinic_today, db_err, Db};
use serde_json::json;
use waras_core::{
    types::{Admission, AdmissionStatus, BedStatus, CareNote, Ward, WardBed},
    WarasError,
};

impl Db {
    pub async fn list_wards(&self) -> Result<Vec<Ward>, WarasError> {
        let rows: Vec<WardRow> =
            sqlx::query_as("SELECT * FROM wards ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn list_ward_beds(
        &self,
        ward_id: Option<&str>,
    ) -> Result<Vec<WardBed>, WarasError> {
        let rows: Vec<WardBedRow> = match ward_id {
            Some(w) => {
                sqlx::query_as(
                    "SELECT * FROM ward_beds WHERE ward_id = ? ORDER BY room ASC, bed_no ASC",
                )
                .bind(w)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM ward_beds ORDER BY ward_id ASC, room ASC, bed_no ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_ward_bed(&self, id: &str) -> Result<Option<WardBed>, WarasError> {
        let row: Option<WardBedRow> =
            sqlx::query_as("SELECT * FROM ward_beds WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    pub async fn set_bed_status(&self, id: &str, status: BedStatus) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE ward_beds SET status = ?, updated_at = ? WHERE id = ?")
            .bind(bed_status_to_str(status))
            .bind(clinic_now_string())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("bed {id} tidak ditemukan")));
        }
        Ok(())
    }

    /// (ward_id, terisi, total) per bangsal.
    pub async fn bed_occupancy(&self) -> Result<Vec<(String, i64, i64)>, WarasError> {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT ward_id,
               COALESCE(SUM(CASE WHEN status = 'terisi' THEN 1 ELSE 0 END), 0),
               COUNT(*)
             FROM ward_beds GROUP BY ward_id ORDER BY ward_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows)
    }

    pub async fn list_admissions(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<Admission>, WarasError> {
        let rows: Vec<AdmissionRow> = match status {
            Some(s) => {
                sqlx::query_as(
                    "SELECT * FROM admissions WHERE status = ?
                     ORDER BY admitted_date DESC, id ASC",
                )
                .bind(s)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM admissions ORDER BY admitted_date DESC, id ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_admission(&self, id: &str) -> Result<Option<Admission>, WarasError> {
        let row: Option<AdmissionRow> =
            sqlx::query_as("SELECT * FROM admissions WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Admisi baru: gagal (422) bila bed bukan Kosong; bed jadi Terisi
    /// dalam transaksi yang sama dengan baris admisi.
    pub async fn create_admission(
        &self,
        patient_id: &str,
        bed_id: &str,
        encounter_id: Option<&str>,
        diagnosis_code: &str,
        diagnosis_display: &str,
        dpjp: &str,
        fall_risk: bool,
    ) -> Result<Admission, WarasError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let bed: Option<WardBedRow> =
            sqlx::query_as("SELECT * FROM ward_beds WHERE id = ? LIMIT 1")
                .bind(bed_id)
                .fetch_optional(&mut tx)
                .await
                .map_err(db_err)?;
        let bed = match bed {
            Some(b) => b.into_domain()?,
            None => return Err(WarasError::NotFound(format!("bed {bed_id} tidak ditemukan"))),
        };
        if bed.status != BedStatus::Kosong {
            return Err(WarasError::Unprocessable(format!(
                "bed {bed_id} tidak kosong (status: {})",
                bed.status.label()
            )));
        }
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("ADM-{date_part}-");
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM admissions WHERE id LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(&mut tx)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        let now = clinic_now_string();
        sqlx::query(
            "INSERT INTO admissions (id, patient_id, bed_id, encounter_id,
                 diagnosis_code, diagnosis_display, admitted_date, dpjp,
                 fall_risk, status, discharged_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(bed_id)
        .bind(encounter_id)
        .bind(diagnosis_code)
        .bind(diagnosis_display)
        .bind(today.to_string())
        .bind(dpjp)
        .bind(if fall_risk { 1 } else { 0 })
        .bind(admission_status_to_str(AdmissionStatus::Dirawat))
        .bind(&now)
        .execute(&mut tx)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE ward_beds SET status = 'terisi', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(bed_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        self.get_admission(&id)
            .await?
            .ok_or_else(|| WarasError::Database("admisi baru tidak ditemukan".into()))
    }

    /// Pulang: status Pulang + `discharged_at` diisi + bed Dibersihkan.
    pub async fn discharge_admission(
        &self,
        id: &str,
        by: &str,
    ) -> Result<Admission, WarasError> {
        let adm = self
            .get_admission(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("admisi {id} tidak ditemukan")))?;
        if adm.status != AdmissionStatus::Dirawat {
            return Err(WarasError::Unprocessable(format!(
                "admisi {id} sudah {}",
                adm.status.label()
            )));
        }
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let now = clinic_now_string();
        sqlx::query(
            "UPDATE admissions SET status = ?, discharged_at = ? WHERE id = ?",
        )
        .bind(admission_status_to_str(AdmissionStatus::Pulang))
        .bind(&now)
        .bind(id)
        .execute(&mut tx)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE ward_beds SET status = 'dibersihkan', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&adm.bed_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        self.log_audit(
            by,
            "ranap.discharge",
            "admission",
            id,
            json!({"bed_id": adm.bed_id}),
        )
        .await?;
        self.get_admission(id)
            .await?
            .ok_or_else(|| WarasError::Database("admisi tidak ditemukan setelah pulang".into()))
    }

    /// Mutasi: bed lama Dibersihkan, bed baru Terisi. Gagal (422) bila
    /// bed baru bukan Kosong.
    pub async fn transfer_admission(
        &self,
        id: &str,
        bed_id: &str,
        by: &str,
    ) -> Result<Admission, WarasError> {
        let adm = self
            .get_admission(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("admisi {id} tidak ditemukan")))?;
        if adm.status != AdmissionStatus::Dirawat {
            return Err(WarasError::Unprocessable(format!(
                "admisi {id} sudah {}",
                adm.status.label()
            )));
        }
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let bed: Option<WardBedRow> =
            sqlx::query_as("SELECT * FROM ward_beds WHERE id = ? LIMIT 1")
                .bind(bed_id)
                .fetch_optional(&mut tx)
                .await
                .map_err(db_err)?;
        let bed = match bed {
            Some(b) => b.into_domain()?,
            None => return Err(WarasError::NotFound(format!("bed {bed_id} tidak ditemukan"))),
        };
        if bed.status != BedStatus::Kosong {
            return Err(WarasError::Unprocessable(format!(
                "bed {bed_id} tidak kosong (status: {})",
                bed.status.label()
            )));
        }
        let now = clinic_now_string();
        sqlx::query("UPDATE admissions SET bed_id = ? WHERE id = ?")
            .bind(bed_id)
            .bind(id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        sqlx::query("UPDATE ward_beds SET status = 'dibersihkan', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&adm.bed_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        sqlx::query("UPDATE ward_beds SET status = 'terisi', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(bed_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        self.log_audit(
            by,
            "ranap.transfer",
            "admission",
            id,
            json!({"from_bed": adm.bed_id, "to_bed": bed_id}),
        )
        .await?;
        self.get_admission(id)
            .await?
            .ok_or_else(|| WarasError::Database("admisi tidak ditemukan setelah mutasi".into()))
    }

    pub async fn list_care_notes(
        &self,
        admission_id: &str,
    ) -> Result<Vec<CareNote>, WarasError> {
        let rows: Vec<CareNoteRow> = sqlx::query_as(
            "SELECT * FROM care_notes WHERE admission_id = ? ORDER BY recorded_at ASC, id ASC",
        )
        .bind(admission_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn add_care_note(
        &self,
        admission_id: &str,
        note: &str,
        by: &str,
    ) -> Result<CareNote, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM care_notes")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("CRN-{seq:06}");
        sqlx::query(
            "INSERT INTO care_notes (id, admission_id, note, recorded_by, recorded_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(admission_id)
        .bind(note)
        .bind(by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<CareNoteRow> =
            sqlx::query_as("SELECT * FROM care_notes WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("catatan perawatan baru tidak ditemukan".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_beds_match_design_occupancy() {
        let db = seeded_db().await;
        let wards = db.list_wards().await.unwrap();
        assert_eq!(wards.len(), 5);
        let beds = db.list_ward_beds(None).await.unwrap();
        assert_eq!(beds.len(), 120);
        let occ = db.bed_occupancy().await.unwrap();
        let total: (i64, i64) = occ
            .iter()
            .fold((0, 0), |a, (_, o, t)| (a.0 + o, a.1 + t));
        assert_eq!(total, (86, 120), "hunian seed harus 86/120");
        let by_ward: std::collections::HashMap<&str, (i64, i64)> = occ
            .iter()
            .map(|(w, o, t)| (w.as_str(), (*o, *t)))
            .collect();
        assert_eq!(by_ward["Melati"], (28, 32));
        assert_eq!(by_ward["Mawar"], (22, 28));
        assert_eq!(by_ward["Anggrek"], (18, 24));
        assert_eq!(by_ward["Cempaka"], (18, 24));
        assert_eq!(by_ward["ICU"], (0, 12));
        // Peta 3A persis FASE3.md §4.2 (3A-03 Terisi — tabel admisi menang).
        let bed3a = db.list_ward_beds(Some("Melati")).await.unwrap();
        let get = |id: &str| {
            bed3a.iter().find(|b| b.id == id).unwrap().status
        };
        assert_eq!(get("3A-01"), BedStatus::Terisi);
        assert_eq!(get("3A-03"), BedStatus::Terisi);
        assert_eq!(get("3A-05"), BedStatus::Dibersihkan);
        assert_eq!(get("3A-07"), BedStatus::Kosong);
        assert_eq!(get("3A-11"), BedStatus::Dibersihkan);
        // Admisi seed 7 baris.
        let adm = db.list_admissions(None).await.unwrap();
        assert_eq!(adm.len(), 7);
        let bambang = db
            .get_admission("ADM-20261119-001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bambang.patient_id, "RM-000820");
        assert_eq!(bambang.bed_id, "3A-01");
        assert_eq!(bambang.diagnosis_code, "I20.9");
        assert!(bambang.fall_risk);
        assert_eq!(bambang.admitted_date, NaiveDate::from_ymd_opt(2026, 11, 17).unwrap());
        let notes = db.list_care_notes("ADM-20261119-001").await.unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].note.contains("Nyeri dada berkurang"));
    }

    #[tokio::test]
    async fn admission_bed_state_transitions() {
        let db = seeded_db().await;
        // Bed terisi -> 422, bed tetap Terisi, tidak ada admisi baru.
        let err = db
            .create_admission(
                "RM-000813",
                "3A-01",
                None,
                "J06.9",
                "ISPA atas",
                "dr. Rina, Sp.PD",
                false,
            )
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        assert_eq!(db.list_admissions(None).await.unwrap().len(), 7);
        // Admisi di bed kosong -> bed jadi Terisi.
        let adm = db
            .create_admission(
                "RM-000813",
                "3A-07",
                None,
                "J06.9",
                "ISPA atas",
                "dr. Rina, Sp.PD",
                false,
            )
            .await
            .unwrap();
        assert_eq!(adm.id, "ADM-20261119-008");
        assert_eq!(adm.status, AdmissionStatus::Dirawat);
        assert_eq!(
            db.get_ward_bed("3A-07").await.unwrap().unwrap().status,
            BedStatus::Terisi
        );
        // Mutasi ke bed terisi -> 422; ke bed kosong -> lama Dibersihkan.
        let err = db
            .transfer_admission(&adm.id, "3A-01", "Ns. Sari Wijaya")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        let moved = db
            .transfer_admission(&adm.id, "3A-10", "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(moved.bed_id, "3A-10");
        assert_eq!(
            db.get_ward_bed("3A-07").await.unwrap().unwrap().status,
            BedStatus::Dibersihkan
        );
        assert_eq!(
            db.get_ward_bed("3A-10").await.unwrap().unwrap().status,
            BedStatus::Terisi
        );
        // Pulang -> bed Dibersihkan, admisi kedua kali -> 422.
        let out = db
            .discharge_admission(&adm.id, "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(out.status, AdmissionStatus::Pulang);
        assert!(out.discharged_at.is_some());
        assert_eq!(
            db.get_ward_bed("3A-10").await.unwrap().unwrap().status,
            BedStatus::Dibersihkan
        );
        assert!(matches!(
            db.discharge_admission(&adm.id, "Ns. Sari Wijaya").await,
            Err(WarasError::Unprocessable(_))
        ));
        // Catatan perawatan.
        let n = db
            .add_care_note(&adm.id, "Kondisi membaik.", "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(n.id, "CRN-000003");
        assert_eq!(db.list_care_notes(&adm.id).await.unwrap().len(), 1);
    }
}
