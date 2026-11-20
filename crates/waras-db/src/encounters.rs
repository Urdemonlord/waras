//! Encounter, SOAP (versioning), tanda vital, diagnosis.

use crate::convert::*;
use crate::rows::{ConditionRow, EncounterRow, ObservationRow, SoapRow, SoapVersionRow};
use crate::{
    clinic_today, db_err, fmt_dt, new_uuid, now_naive, now_time_str, Db,
};
use waras_core::{
    ids,
    types::{
        Condition, ConditionGroup, ConditionStatus, Encounter, EncounterStatus, Observation,
        SoapNote, SoapStatus, VitalCode,
    },
    WarasError,
};

impl Db {
    /// Encounter baru `ENC-<tanggal>-<seq>` (seq = maks hari itu + 1),
    /// status Aktif, tanggal = hari klinik.
    pub async fn create_encounter(
        &self,
        patient_id: &str,
        poli: &str,
        doctor_name: &str,
        doctor_id: Option<&str>,
    ) -> Result<Encounter, WarasError> {
        let today = clinic_today();
        let ids_today: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM encounters WHERE date = ?",
        )
        .bind(today.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids_today, 1);
        let id = ids::encounter_id(today, seq);
        let time = now_time_str();
        let created = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO encounters (id, patient_id, poli, doctor_id, doctor_name,
                 date, time, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'aktif', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(poli)
        .bind(doctor_id)
        .bind(doctor_name)
        .bind(today.to_string())
        .bind(&time)
        .bind(&created)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_encounter(&id)
            .await?
            .ok_or_else(|| WarasError::Database("encounter baru tidak ditemukan".into()))
    }

    pub async fn get_encounter(&self, id: &str) -> Result<Option<Encounter>, WarasError> {
        let row: Option<EncounterRow> =
            sqlx::query_as("SELECT * FROM encounters WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Validasi pasien salah (FASE3.md §6): encounter harus milik pasien
    /// yang dimaksud. Encounter tidak ada → `NotFound`; pasien tidak
    /// cocok → `WrongPatient` (422). `patient_id` kosong → dilewati
    /// (tidak ada yang dicocokkan).
    pub async fn verify_encounter_patient(
        &self,
        encounter_id: &str,
        patient_id: &str,
    ) -> Result<(), WarasError> {
        if patient_id.trim().is_empty() {
            return Ok(());
        }
        let encounter = self
            .get_encounter(encounter_id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("encounter {encounter_id} tidak ditemukan")))?;
        if encounter.patient_id == patient_id {
            return Ok(());
        }
        let expected = self
            .get_patient(&encounter.patient_id)
            .await?
            .map(|p| format!("{} ({})", p.name, p.id))
            .unwrap_or_else(|| encounter.patient_id.clone());
        let got = self
            .get_patient(patient_id)
            .await?
            .map(|p| format!("{} ({})", p.name, p.id))
            .unwrap_or_else(|| patient_id.to_string());
        Err(WarasError::WrongPatient(format!(
            "Encounter {encounter_id} milik pasien {expected}, bukan {got}"
        )))
    }

    /// Ubah status encounter (dipakai `close_encounter` Fase 2: -> Selesai).
    pub async fn set_encounter_status(
        &self,
        id: &str,
        status: EncounterStatus,
    ) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE encounters SET status = ? WHERE id = ?")
            .bind(encounter_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("encounter {id} tidak ditemukan")));
        }
        Ok(())
    }

    pub async fn list_encounters(
        &self,
        patient_id: &str,
    ) -> Result<Vec<Encounter>, WarasError> {
        let rows: Vec<EncounterRow> = sqlx::query_as(
            "SELECT * FROM encounters WHERE patient_id = ?
             ORDER BY date DESC, time DESC",
        )
        .bind(patient_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Simpan draft SOAP: version+1, baris baru di `soap_versions`,
    /// update `soap_notes` + `draft_saved_at`. TIDAK PERNAH menimpa
    /// versi lama. Status Lengkap bila keempat bidang terisi, Draft bila
    /// ada yang kosong.
    pub async fn save_soap(
        &self,
        encounter_id: &str,
        subjective: &str,
        objective: &str,
        assessment: &str,
        plan: &str,
        author: &str,
    ) -> Result<SoapNote, WarasError> {
        let encounter = self
            .get_encounter(encounter_id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("encounter {encounter_id} tidak ditemukan")))?;
        let _ = encounter;
        let now = now_naive();
        let now_s = fmt_dt(now);
        let status = if [subjective, objective, assessment, plan]
            .iter()
            .all(|f| !f.trim().is_empty())
        {
            SoapStatus::Lengkap
        } else {
            SoapStatus::Draft
        };
        let existing = self.get_soap(encounter_id).await?;
        let (soap_id, version) = match &existing {
            Some(s) => (s.id.clone(), s.version + 1),
            None => (format!("SOAP-{encounter_id}"), 1),
        };
        if existing.is_none() {
            sqlx::query(
                "INSERT INTO soap_notes (id, encounter_id, subjective, objective, assessment,
                     plan, author, status, version, created_at, updated_at, draft_saved_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&soap_id)
            .bind(encounter_id)
            .bind(subjective)
            .bind(objective)
            .bind(assessment)
            .bind(plan)
            .bind(author)
            .bind(soap_status_to_str(status))
            .bind(version as i64)
            .bind(&now_s)
            .bind(&now_s)
            .bind(&now_s)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        } else {
            sqlx::query(
                "UPDATE soap_notes SET subjective = ?, objective = ?, assessment = ?,
                     plan = ?, author = ?, status = ?, version = ?,
                     updated_at = ?, draft_saved_at = ? WHERE id = ?",
            )
            .bind(subjective)
            .bind(objective)
            .bind(assessment)
            .bind(plan)
            .bind(author)
            .bind(soap_status_to_str(status))
            .bind(version as i64)
            .bind(&now_s)
            .bind(&now_s)
            .bind(&soap_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        sqlx::query(
            "INSERT INTO soap_versions (id, soap_id, version, subjective, objective,
                 assessment, plan, author, saved_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("SV-{}", new_uuid()))
        .bind(&soap_id)
        .bind(version as i64)
        .bind(subjective)
        .bind(objective)
        .bind(assessment)
        .bind(plan)
        .bind(author)
        .bind(&now_s)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_soap(encounter_id)
            .await?
            .ok_or_else(|| WarasError::Database("SOAP hilang setelah simpan".into()))
    }

    pub async fn get_soap(&self, encounter_id: &str) -> Result<Option<SoapNote>, WarasError> {
        let row: Option<SoapRow> =
            sqlx::query_as("SELECT * FROM soap_notes WHERE encounter_id = ? LIMIT 1")
                .bind(encounter_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Riwayat versi SOAP, urut versi naik.
    pub async fn get_soap_versions(
        &self,
        soap_id: &str,
    ) -> Result<Vec<waras_core::types::SoapVersion>, WarasError> {
        let rows: Vec<SoapVersionRow> = sqlx::query_as(
            "SELECT * FROM soap_versions WHERE soap_id = ? ORDER BY version ASC",
        )
        .bind(soap_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Catat tanda vital; unit diisi dari `VitalCode::unit()`.
    pub async fn add_vital(
        &self,
        encounter_id: &str,
        code: VitalCode,
        value: &str,
        measured_by: &str,
        previous_value: Option<&str>,
        previous_date: Option<chrono::NaiveDate>,
    ) -> Result<Observation, WarasError> {
        let encounter = self
            .get_encounter(encounter_id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("encounter {encounter_id} tidak ditemukan")))?;
        let id = format!("OBS-{}", new_uuid());
        let measured_at = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO observations (id, encounter_id, patient_id, code, value, unit,
                 measured_at, measured_by, previous_value, previous_date)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(encounter_id)
        .bind(&encounter.patient_id)
        .bind(vital_code_to_str(code))
        .bind(value)
        .bind(code.unit())
        .bind(&measured_at)
        .bind(measured_by)
        .bind(previous_value)
        .bind(previous_date.map(|d| d.to_string()))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(Observation {
            id,
            encounter_id: encounter_id.to_string(),
            patient_id: encounter.patient_id,
            code,
            value: value.to_string(),
            unit: code.unit().to_string(),
            measured_at: now_naive(),
            measured_by: measured_by.to_string(),
            previous_value: previous_value.map(|s| s.to_string()),
            previous_date,
        })
    }

    pub async fn list_vitals(
        &self,
        encounter_id: &str,
    ) -> Result<Vec<Observation>, WarasError> {
        let rows: Vec<ObservationRow> = sqlx::query_as(
            "SELECT * FROM observations WHERE encounter_id = ? ORDER BY measured_at ASC",
        )
        .bind(encounter_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Tambah diagnosis. `encounter_id` None untuk diagnosis kronis /
    /// riwayat yang tidak terikat satu kunjungan.
    pub async fn add_condition(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        group: ConditionGroup,
        code: &str,
        display: &str,
        status: ConditionStatus,
        clinician: &str,
    ) -> Result<Condition, WarasError> {
        let id = format!("COND-{}", new_uuid());
        let recorded = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
                 code, display, status, clinician, recorded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(condition_group_to_str(group))
        .bind(code)
        .bind(display)
        .bind(condition_status_to_str(status))
        .bind(clinician)
        .bind(&recorded)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(Condition {
            id,
            patient_id: patient_id.to_string(),
            encounter_id: encounter_id.map(|s| s.to_string()),
            group,
            code: code.to_string(),
            display: display.to_string(),
            status,
            clinician: clinician.to_string(),
            recorded_at: now_naive(),
        })
    }

    /// Diagnosis yang terikat satu encounter (untuk panel FHIR / detail).
    pub async fn list_conditions_for_encounter(
        &self,
        encounter_id: &str,
    ) -> Result<Vec<Condition>, WarasError> {
        let rows: Vec<ConditionRow> = sqlx::query_as(
            "SELECT * FROM conditions WHERE encounter_id = ? ORDER BY recorded_at ASC",
        )
        .bind(encounter_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
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
    async fn verify_encounter_patient_wrong_patient_is_422() {
        let db = seeded_db().await;
        // ENC-...-014 milik Siti (RM-000812).
        db.verify_encounter_patient("ENC-2026-11-19-014", "RM-000812")
            .await
            .unwrap();
        // patient_id kosong -> dilewati.
        db.verify_encounter_patient("ENC-2026-11-19-014", "")
            .await
            .unwrap();
        let err = db
            .verify_encounter_patient("ENC-2026-11-19-014", "RM-000813")
            .await;
        assert!(matches!(err, Err(WarasError::WrongPatient(_))));
        let err = err.unwrap_err();
        assert_eq!(err.http_status(), 422);
        assert_eq!(
            err.to_string(),
            "pasien salah: Encounter ENC-2026-11-19-014 milik pasien Siti Rahmawati (RM-000812), bukan Budi Santoso (RM-000813)"
        );
        assert!(matches!(
            db.verify_encounter_patient("ENC-2026-11-19-999", "RM-000812")
                .await,
            Err(WarasError::NotFound(_))
        ));
    }
}
