//! IGD (FASE3.md §4): kunjungan triase + observasi + hitungan triase.

use crate::rows::{clinic_now_string, igd_status_to_str, triage_to_str, IgdObservationRow, IgdVisitRow};
use crate::{clinic_today, db_err, Db};
use waras_core::{
    types::{IgdObservation, IgdStatus, IgdVisit, TriageLevel},
    WarasError,
};

impl Db {
    /// Kunjungan IGD satu tanggal (`created_at` tanggal itu), urut jam tiba.
    pub async fn list_igd(&self, date: &str) -> Result<Vec<IgdVisit>, WarasError> {
        let rows: Vec<IgdVisitRow> = sqlx::query_as(
            "SELECT * FROM igd_visits WHERE created_at LIKE ?
             ORDER BY arrived_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_igd(&self, id: &str) -> Result<Option<IgdVisit>, WarasError> {
        let row: Option<IgdVisitRow> =
            sqlx::query_as("SELECT * FROM igd_visits WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Triase baru: id `IGD-<tanggal klinik>-<seq>`, status Menunggu.
    pub async fn create_igd_triage(
        &self,
        patient_id: &str,
        complaint: &str,
        triage: TriageLevel,
        doctor: &str,
        arrived_at: &str,
        encounter_id: Option<&str>,
    ) -> Result<IgdVisit, WarasError> {
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("IGD-{date_part}-");
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM igd_visits WHERE id LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        sqlx::query(
            "INSERT INTO igd_visits (id, patient_id, encounter_id, complaint,
                 triage, doctor, status, arrived_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'menunggu', ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(complaint)
        .bind(triage_to_str(triage))
        .bind(doctor)
        .bind(arrived_at)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_igd(&id)
            .await?
            .ok_or_else(|| WarasError::Database("kunjungan IGD baru tidak ditemukan".into()))
    }

    pub async fn set_igd_status(&self, id: &str, status: IgdStatus) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE igd_visits SET status = ? WHERE id = ?")
            .bind(igd_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("kunjungan IGD {id} tidak ditemukan")));
        }
        Ok(())
    }

    pub async fn add_igd_observation(
        &self,
        igd_id: &str,
        note: &str,
        by: &str,
    ) -> Result<IgdObservation, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM igd_observations")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("IGO-{seq:06}");
        sqlx::query(
            "INSERT INTO igd_observations (id, igd_id, note, recorded_by, recorded_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(igd_id)
        .bind(note)
        .bind(by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<IgdObservationRow> =
            sqlx::query_as("SELECT * FROM igd_observations WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("observasi IGD baru tidak ditemukan".into()))
    }

    pub async fn list_igd_observations(
        &self,
        igd_id: &str,
    ) -> Result<Vec<IgdObservation>, WarasError> {
        let rows: Vec<IgdObservationRow> = sqlx::query_as(
            "SELECT * FROM igd_observations WHERE igd_id = ? ORDER BY recorded_at ASC, id ASC",
        )
        .bind(igd_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// (merah, kuning, hijau) aktif hari itu (status bukan Selesai/Dirujuk).
    pub async fn igd_counts(&self, date: &str) -> Result<(i64, i64, i64), WarasError> {
        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT
               COALESCE(SUM(CASE WHEN triage = 'merah' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN triage = 'kuning' THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN triage = 'hijau' THEN 1 ELSE 0 END), 0)
             FROM igd_visits
             WHERE substr(created_at, 1, 10) = ? AND status NOT IN ('selesai', 'dirujuk')",
        )
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::types::TriageLevel;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_igd_counts_and_visits() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let visits = db.list_igd(&date).await.unwrap();
        assert_eq!(visits.len(), 8);
        assert_eq!(visits[0].complaint, "Nyeri dada, sesak");
        assert_eq!(visits[0].triage, TriageLevel::Merah);
        assert_eq!(visits[0].status, IgdStatus::Resusitasi);
        assert_eq!(visits[0].encounter_id.as_deref(), Some("ENC-2026-11-19-021"));
        // Hitungan nyata dari 8 baris seed: 1 merah / 3 kuning / 4 hijau.
        assert_eq!(db.igd_counts(&date).await.unwrap(), (1, 3, 4));
        let obs = db.list_igd_observations("IGD-20261119-001").await.unwrap();
        assert_eq!(obs.len(), 1);
        assert!(obs[0].note.contains("150/95"));
        assert!(db.get_igd("IGD-20261119-999").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn triage_roundtrip_and_status() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let v = db
            .create_igd_triage(
                "RM-000813",
                "Nyeri perut",
                TriageLevel::Kuning,
                "dr. Sari Dewi",
                "10.05",
                None,
            )
            .await
            .unwrap();
        assert_eq!(v.id, "IGD-20261119-009");
        assert_eq!(v.status, IgdStatus::Menunggu);
        db.set_igd_status(&v.id, IgdStatus::Observasi).await.unwrap();
        assert_eq!(
            db.get_igd(&v.id).await.unwrap().unwrap().status,
            IgdStatus::Observasi
        );
        assert_eq!(db.igd_counts(&date).await.unwrap(), (1, 4, 4));
        let o = db
            .add_igd_observation(&v.id, "TD 120/80, observasi.", "Ns. Sari Wijaya")
            .await
            .unwrap();
        assert_eq!(o.id, "IGO-000003");
        assert_eq!(db.list_igd_observations(&v.id).await.unwrap().len(), 1);
        assert!(matches!(
            db.set_igd_status("IGD-20261119-999", IgdStatus::Selesai).await,
            Err(WarasError::NotFound(_))
        ));
    }
}
