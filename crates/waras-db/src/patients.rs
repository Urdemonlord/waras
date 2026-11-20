//! Pasien, alergi, kondisi.

use crate::convert::*;
use crate::rows::{AllergyRow, ConditionRow, PatientRow};
use crate::{db_err, fmt_dt, new_uuid, now_naive, Db, NewPatient};
use waras_core::{
    ids,
    types::{AllergyIntolerance, Condition, Patient},
    WarasError,
};

impl Db {
    /// Daftar pasien, urut nama. `search` mencocokkan nama / NIK / RM.
    /// Case-insensitive di KEDUA backend: SQLite `LIKE` sudah
    /// case-insensitive untuk ASCII, tapi PostgreSQL `LIKE` case-sensitive
    /// — jadi bandingkan `LOWER(kolom) LIKE LOWER(?)` (A2, terbukti di
    /// PG 16: `LIKE '%siti%'` 0 baris, `ILIKE` 1 baris; `LOWER` jalan
    /// di SQLite maupun PG).
    pub async fn list_patients(
        &self,
        search: Option<&str>,
    ) -> Result<Vec<Patient>, WarasError> {
        let rows: Vec<PatientRow> = match search {
            Some(q) if !q.trim().is_empty() => {
                let like = format!("%{}%", q.trim());
                sqlx::query_as(
                    "SELECT * FROM patients
                     WHERE LOWER(name) LIKE LOWER(?) OR LOWER(nik) LIKE LOWER(?) OR LOWER(id) LIKE LOWER(?)
                     ORDER BY name ASC",
                )
                .bind(&like)
                .bind(&like)
                .bind(&like)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            _ => {
                sqlx::query_as("SELECT * FROM patients ORDER BY name ASC")
                    .fetch_all(&self.pool)
                    .await
                    .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_patient(&self, id: &str) -> Result<Option<Patient>, WarasError> {
        let row: Option<PatientRow> =
            sqlx::query_as("SELECT * FROM patients WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Pasien baru dengan nomor RM berikutnya (`RM-######`).
    pub async fn create_patient(&self, input: NewPatient) -> Result<Patient, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM patients")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = ids
            .iter()
            .filter_map(|id| id.trim_start_matches("RM-").parse::<u32>().ok())
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        let id = ids::rm_number(seq);
        let created = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO patients (id, nik, name, birth_date, gender, address, phone,
                 blood_type, bpjs_number, insurance, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.nik)
        .bind(&input.name)
        .bind(input.birth_date.to_string())
        .bind(gender_to_str(input.gender))
        .bind(&input.address)
        .bind(input.phone)
        .bind(input.blood_type)
        .bind(input.bpjs_number)
        .bind(insurance_to_str(input.insurance))
        .bind(&created)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_patient(&id)
            .await?
            .ok_or_else(|| WarasError::Database("pasien baru tidak ditemukan".into()))
    }

    pub async fn get_allergies(
        &self,
        patient_id: &str,
    ) -> Result<Vec<AllergyIntolerance>, WarasError> {
        let rows: Vec<AllergyRow> = sqlx::query_as(
            "SELECT * FROM allergies WHERE patient_id = ? ORDER BY recorded_at ASC",
        )
        .bind(patient_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Kondisi pasien, urut grup (Utama, Sekunder, Kronis, Riwayat,
    /// Tindakan medis) lalu waktu catat.
    pub async fn get_conditions(
        &self,
        patient_id: &str,
    ) -> Result<Vec<Condition>, WarasError> {
        let rows: Vec<ConditionRow> = sqlx::query_as(
            "SELECT * FROM conditions WHERE patient_id = ?
             ORDER BY CASE condition_group
                 WHEN 'utama' THEN 0 WHEN 'sekunder' THEN 1 WHEN 'kronis' THEN 2
                 WHEN 'riwayat' THEN 3 WHEN 'tindakan_medis' THEN 4 ELSE 5 END,
             recorded_at ASC",
        )
        .bind(patient_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Tambah alergi (dipakai API bila ada; seed memakai insert langsung).
    pub async fn add_allergy(
        &self,
        patient_id: &str,
        substance: &str,
        substance_class: Option<&str>,
        criticality: waras_core::types::Criticality,
        note: Option<&str>,
    ) -> Result<AllergyIntolerance, WarasError> {
        let id = format!("ALG-{}", new_uuid());
        let recorded = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO allergies (id, patient_id, substance, substance_class,
                 criticality, note, recorded_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(substance)
        .bind(substance_class)
        .bind(criticality_to_str(criticality))
        .bind(note)
        .bind(&recorded)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(AllergyIntolerance {
            id,
            patient_id: patient_id.to_string(),
            substance: substance.to_string(),
            substance_class: substance_class.map(|s| s.to_string()),
            criticality,
            note: note.map(|s| s.to_string()),
            recorded_at: now_naive(),
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

    /// Pencarian pasien case-insensitive di kedua backend (A2: PG `LIKE`
    /// case-sensitive, jadi query memakai `LOWER()` — test ini gagal di
    /// PG sebelum perbaikan, lolos sesudahnya).
    #[tokio::test]
    async fn search_is_case_insensitive() {
        let db = seeded_db().await;
        for q in ["siti", "SITI", "Siti", "sItI rAhMaWaTi"] {
            let found = db
                .list_patients(Some(q))
                .await
                .unwrap_or_else(|e| panic!("cari {q:?}: {e}"));
            assert_eq!(found.len(), 1, "cari {q:?} harus 1 hasil");
            assert_eq!(found[0].id, "RM-000812");
        }
        // Cari NIK + RM (case-insensitive juga untuk RM).
        assert_eq!(
            db.list_patients(Some("3374021205850001"))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.list_patients(Some("rm-000812")).await.unwrap().len(),
            1,
            "RM huruf kecil harus ketemu"
        );
        assert!(db
            .list_patients(Some("tidak-ada-nama-ini"))
            .await
            .unwrap()
            .is_empty());
    }
}
