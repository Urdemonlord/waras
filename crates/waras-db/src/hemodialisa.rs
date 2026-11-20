//! Hemodialisa (FASE4.md §4): mesin + sesi.
//! Sesi baru gagal 422 bila mesin bukan Siap; mesin jadi Terpakai.
//! Selesai/Batal membebaskan mesin (Siap).

use crate::rows::{
    clinic_now_string, hd_session_status_to_str, HdMachineRow, HdSessionRow,
};
use crate::{clinic_today, db_err, Db};
use chrono::Timelike;
use waras_core::{
    types::{HdMachine, HdMachineStatus, HdSession, HdSessionStatus},
    WarasError,
};

/// Jam sekarang sebagai `YYYY-MM-DD HH:MM` (tanggal klinik + jam sistem).
fn now_hhmm() -> String {
    let now = crate::now_naive();
    let today = crate::clinic_today();
    format!(
        "{} {:02}:{:02}",
        today,
        now.hour(),
        now.minute()
    )
}

impl Db {
    pub async fn list_hd_machines(&self) -> Result<Vec<HdMachine>, WarasError> {
        let rows: Vec<HdMachineRow> =
            sqlx::query_as("SELECT * FROM hd_machines ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Sesi satu tanggal (`scheduled_at` tanggal itu), urut jam jadwal.
    pub async fn list_hd_sessions(&self, date: &str) -> Result<Vec<HdSession>, WarasError> {
        let rows: Vec<HdSessionRow> = sqlx::query_as(
            "SELECT * FROM hd_sessions WHERE scheduled_at LIKE ?
             ORDER BY scheduled_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_hd_session(&self, id: &str) -> Result<Option<HdSession>, WarasError> {
        let row: Option<HdSessionRow> =
            sqlx::query_as("SELECT * FROM hd_sessions WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Sesi baru: id `HD-<tanggal klinik>-<seq>`, status Terjadwal.
    /// Gagal 422 bila mesin bukan Siap; mesin jadi Terpakai dalam
    /// transaksi yang sama dengan baris sesi.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_hd_session(
        &self,
        patient_id: &str,
        machine_id: &str,
        encounter_id: Option<&str>,
        dialyzer: &str,
        duration_minutes: i64,
        blood_flow: i64,
        dialysate_flow: i64,
        uf_goal_ml: i64,
        scheduled_at: &str,
        notes: &str,
    ) -> Result<HdSession, WarasError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let machine: Option<HdMachineRow> =
            sqlx::query_as("SELECT * FROM hd_machines WHERE id = ? LIMIT 1")
                .bind(machine_id)
                .fetch_optional(&mut tx)
                .await
                .map_err(db_err)?;
        let machine = match machine {
            Some(m) => m.into_domain()?,
            None => {
                return Err(WarasError::NotFound(format!(
                    "mesin HD {machine_id} tidak ditemukan"
                )))
            }
        };
        if machine.status != HdMachineStatus::Siap {
            return Err(WarasError::Unprocessable(format!(
                "mesin HD {machine_id} tidak siap (status: {})",
                machine.status.label()
            )));
        }
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("HD-{date_part}-");
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM hd_sessions WHERE id LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(&mut tx)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        let now = clinic_now_string();
        sqlx::query(
            "INSERT INTO hd_sessions (id, patient_id, machine_id, encounter_id,
                 dialyzer, duration_minutes, blood_flow, dialysate_flow,
                 uf_goal_ml, status, scheduled_at, started_at, finished_at,
                 notes, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'terjadwal', ?, NULL, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(machine_id)
        .bind(encounter_id)
        .bind(dialyzer)
        .bind(duration_minutes)
        .bind(blood_flow)
        .bind(dialysate_flow)
        .bind(uf_goal_ml)
        .bind(scheduled_at)
        .bind(notes)
        .bind(&now)
        .execute(&mut tx)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE hd_machines SET status = 'terpakai' WHERE id = ?")
            .bind(machine_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        self.get_hd_session(&id)
            .await?
            .ok_or_else(|| WarasError::Database("sesi HD baru tidak ditemukan".into()))
    }

    /// Ubah status sesi. Selesai/Batal membebaskan mesin (Siap);
    /// Berjalan mengisi `started_at`, Selesai mengisi `finished_at`
    /// (bila masih kosong).
    pub async fn set_hd_session_status(
        &self,
        id: &str,
        status: HdSessionStatus,
    ) -> Result<HdSession, WarasError> {
        let session = self
            .get_hd_session(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("sesi HD {id} tidak ditemukan")))?;
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        match status {
            HdSessionStatus::Berjalan => {
                let started = session
                    .started_at
                    .clone()
                    .unwrap_or_else(now_hhmm);
                sqlx::query(
                    "UPDATE hd_sessions SET status = ?, started_at = ? WHERE id = ?",
                )
                .bind(hd_session_status_to_str(status))
                .bind(started)
                .bind(id)
                .execute(&mut tx)
                .await
                .map_err(db_err)?;
            }
            HdSessionStatus::Selesai => {
                let finished = session
                    .finished_at
                    .clone()
                    .unwrap_or_else(now_hhmm);
                sqlx::query(
                    "UPDATE hd_sessions SET status = ?, finished_at = ? WHERE id = ?",
                )
                .bind(hd_session_status_to_str(status))
                .bind(finished)
                .bind(id)
                .execute(&mut tx)
                .await
                .map_err(db_err)?;
                sqlx::query("UPDATE hd_machines SET status = 'siap' WHERE id = ?")
                    .bind(&session.machine_id)
                    .execute(&mut tx)
                    .await
                    .map_err(db_err)?;
            }
            HdSessionStatus::Batal => {
                sqlx::query("UPDATE hd_sessions SET status = ? WHERE id = ?")
                    .bind(hd_session_status_to_str(status))
                    .bind(id)
                    .execute(&mut tx)
                    .await
                    .map_err(db_err)?;
                sqlx::query("UPDATE hd_machines SET status = 'siap' WHERE id = ?")
                    .bind(&session.machine_id)
                    .execute(&mut tx)
                    .await
                    .map_err(db_err)?;
            }
            HdSessionStatus::Terjadwal => {
                sqlx::query("UPDATE hd_sessions SET status = ? WHERE id = ?")
                    .bind(hd_session_status_to_str(status))
                    .bind(id)
                    .execute(&mut tx)
                    .await
                    .map_err(db_err)?;
            }
        }
        tx.commit().await.map_err(db_err)?;
        self.get_hd_session(id)
            .await?
            .ok_or_else(|| WarasError::Database("sesi HD tidak ditemukan setelah update".into()))
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
    async fn seed_hd_machines_and_sessions() {
        let db = seeded_db().await;
        let machines = db.list_hd_machines().await.unwrap();
        assert_eq!(machines.len(), 4);
        assert_eq!(machines[0].id, "HD-1");
        assert_eq!(machines[0].status, HdMachineStatus::Terpakai);
        assert_eq!(machines[2].status, HdMachineStatus::Siap);
        assert_eq!(machines[3].status, HdMachineStatus::Perawatan);
        let date = clinic_today().to_string();
        let sessions = db.list_hd_sessions(&date).await.unwrap();
        assert_eq!(sessions.len(), 3);
        // Urut jam jadwal: Dewi 06.00, Slamet 07.00, Agus 13.00.
        assert_eq!(sessions[0].patient_id, "RM-000814");
        assert_eq!(sessions[0].status, HdSessionStatus::Selesai);
        assert_eq!(sessions[0].dialyzer, "F8");
        assert_eq!(sessions[1].patient_id, "RM-000824");
        assert_eq!(sessions[1].status, HdSessionStatus::Berjalan);
        assert_eq!(sessions[1].dialyzer, "F6");
        assert_eq!(sessions[1].blood_flow, 300);
        assert_eq!(sessions[1].uf_goal_ml, 2000);
        assert_eq!(sessions[2].patient_id, "RM-000815");
        assert_eq!(sessions[2].status, HdSessionStatus::Terjadwal);
        assert!(db.get_hd_session("HD-20261119-999").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn hd_session_machine_state_transitions() {
        let db = seeded_db().await;
        // Mesin Terpakai -> 422, tidak ada sesi baru.
        let err = db
            .create_hd_session(
                "RM-000813", "HD-1", None, "F6", 240, 300, 500, 2000,
                "2026-11-19 15:00", "",
            )
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        // Mesin tidak ada -> 404.
        assert!(matches!(
            db.create_hd_session(
                "RM-000813", "HD-9", None, "F6", 240, 300, 500, 2000,
                "2026-11-19 15:00", "",
            )
            .await,
            Err(WarasError::NotFound(_))
        ));
        // Sesi di mesin Siap (HD-3) -> mesin jadi Terpakai.
        let s = db
            .create_hd_session(
                "RM-000813", "HD-3", None, "F6", 240, 300, 500, 2000,
                "2026-11-19 15:00", "Sesi sore",
            )
            .await
            .unwrap();
        assert_eq!(s.id, "HD-20261119-004");
        assert_eq!(s.status, HdSessionStatus::Terjadwal);
        assert_eq!(
            db.list_hd_machines()
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == "HD-3")
                .unwrap()
                .status,
            HdMachineStatus::Terpakai
        );
        // Berjalan mengisi started_at; Selesai membebaskan mesin.
        let running = db
            .set_hd_session_status(&s.id, HdSessionStatus::Berjalan)
            .await
            .unwrap();
        assert!(running.started_at.is_some());
        let done = db
            .set_hd_session_status(&s.id, HdSessionStatus::Selesai)
            .await
            .unwrap();
        assert_eq!(done.status, HdSessionStatus::Selesai);
        assert!(done.finished_at.is_some());
        assert_eq!(
            db.list_hd_machines()
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == "HD-3")
                .unwrap()
                .status,
            HdMachineStatus::Siap
        );
        // Batal juga membebaskan mesin.
        let s2 = db
            .create_hd_session(
                "RM-000813", "HD-3", None, "F6", 240, 300, 500, 2000,
                "2026-11-19 17:00", "",
            )
            .await
            .unwrap();
        db.set_hd_session_status(&s2.id, HdSessionStatus::Batal)
            .await
            .unwrap();
        assert_eq!(
            db.list_hd_machines()
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == "HD-3")
                .unwrap()
                .status,
            HdMachineStatus::Siap
        );
        assert!(matches!(
            db.set_hd_session_status("HD-20261119-999", HdSessionStatus::Selesai)
                .await,
            Err(WarasError::NotFound(_))
        ));
    }
}
