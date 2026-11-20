//! Kamar operasi & bersalin (FASE3.md §4): jadwal operasi, ruang
//! bersalin, persalinan.

use crate::rows::{
    clinic_now_string, surgery_status_to_str, DeliveryRoomRow, DeliveryRow, SurgeryRow,
};
use crate::{clinic_today, db_err, fmt_dt, Db};
use chrono::NaiveDateTime;
use waras_core::{
    types::{BedStatus, Delivery, DeliveryRoom, Surgery, SurgeryStatus},
    WarasError,
};

impl Db {
    pub async fn list_surgeries(&self, date: &str) -> Result<Vec<Surgery>, WarasError> {
        let rows: Vec<SurgeryRow> = sqlx::query_as(
            "SELECT * FROM surgeries WHERE substr(scheduled_at, 1, 10) = ?
             ORDER BY scheduled_at ASC, id ASC",
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_surgery(&self, id: &str) -> Result<Option<Surgery>, WarasError> {
        let row: Option<SurgeryRow> =
            sqlx::query_as("SELECT * FROM surgeries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    pub async fn create_surgery(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        room: &str,
        procedure: &str,
        procedure_code: &str,
        operator: &str,
        team: &str,
        scheduled_at: &str,
    ) -> Result<Surgery, WarasError> {
        // Validasi format jadwal ("YYYY-MM-DD HH:MM" atau dengan detik).
        let sched = NaiveDateTime::parse_from_str(scheduled_at, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(scheduled_at, "%Y-%m-%d %H:%M"))
            .map_err(|_| {
                WarasError::Validation(format!("jadwal tidak valid: {scheduled_at}"))
            })?;
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("OK-{date_part}-");
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM surgeries WHERE id LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        sqlx::query(
            "INSERT INTO surgeries (id, patient_id, encounter_id, room, procedure,
                 procedure_code, operator, team, scheduled_at, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'terjadwal', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(room)
        .bind(procedure)
        .bind(procedure_code)
        .bind(operator)
        .bind(team)
        .bind(fmt_dt(sched))
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_surgery(&id)
            .await?
            .ok_or_else(|| WarasError::Database("operasi baru tidak ditemukan".into()))
    }

    pub async fn set_surgery_status(
        &self,
        id: &str,
        status: SurgeryStatus,
    ) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE surgeries SET status = ? WHERE id = ?")
            .bind(surgery_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("operasi {id} tidak ditemukan")));
        }
        Ok(())
    }

    pub async fn list_delivery_rooms(&self) -> Result<Vec<DeliveryRoom>, WarasError> {
        let rows: Vec<DeliveryRoomRow> =
            sqlx::query_as("SELECT * FROM delivery_rooms ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn list_deliveries(&self) -> Result<Vec<Delivery>, WarasError> {
        let rows: Vec<DeliveryRow> = sqlx::query_as(
            "SELECT * FROM deliveries ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_delivery(&self, id: &str) -> Result<Option<Delivery>, WarasError> {
        let row: Option<DeliveryRow> =
            sqlx::query_as("SELECT * FROM deliveries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Persalinan baru: gagal (422) bila ruang bukan Kosong; ruang jadi
    /// Terisi dalam transaksi yang sama.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_delivery(
        &self,
        patient_id: &str,
        age_years: i64,
        gravida_para: &str,
        dilation_cm: i64,
        fetal_heart_rate: i64,
        midwife: &str,
        doctor: &str,
        plan: &str,
        room: &str,
    ) -> Result<Delivery, WarasError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let room_row: Option<DeliveryRoomRow> =
            sqlx::query_as("SELECT * FROM delivery_rooms WHERE id = ? LIMIT 1")
                .bind(room)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
        let room_row = match room_row {
            Some(r) => r.into_domain()?,
            None => {
                return Err(WarasError::NotFound(format!(
                    "ruang bersalin {room} tidak ditemukan"
                )))
            }
        };
        if room_row.status != BedStatus::Kosong {
            return Err(WarasError::Unprocessable(format!(
                "ruang bersalin {room} tidak kosong (status: {})",
                room_row.status.label()
            )));
        }
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("VK-{date_part}-");
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM deliveries WHERE id LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        sqlx::query(
            "INSERT INTO deliveries (id, patient_id, age_years, gravida_para,
                 dilation_cm, fetal_heart_rate, midwife, doctor, plan, room,
                 status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'Aktif', ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(age_years)
        .bind(gravida_para)
        .bind(dilation_cm)
        .bind(fetal_heart_rate)
        .bind(midwife)
        .bind(doctor)
        .bind(plan)
        .bind(room)
        .bind(clinic_now_string())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE delivery_rooms SET status = 'terisi' WHERE id = ?")
            .bind(room)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        self.get_delivery(&id)
            .await?
            .ok_or_else(|| WarasError::Database("persalinan baru tidak ditemukan".into()))
    }

    /// Ubah status persalinan. `Selesai` membebaskan ruang (→ Kosong).
    /// Status lain yang dikenal: `Aktif`, `Dirujuk`.
    pub async fn set_delivery_status(&self, id: &str, status: &str) -> Result<(), WarasError> {
        let status = status.trim();
        if !matches!(
            status.to_lowercase().as_str(),
            "aktif" | "selesai" | "dirujuk"
        ) {
            return Err(WarasError::Validation(format!(
                "status persalinan tidak dikenal: {status}"
            )));
        }
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let row: Option<DeliveryRow> =
            sqlx::query_as("SELECT * FROM deliveries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
        let delivery = match row {
            Some(r) => r.into_domain()?,
            None => {
                return Err(WarasError::NotFound(format!(
                    "persalinan {id} tidak ditemukan"
                )))
            }
        };
        sqlx::query("UPDATE deliveries SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        if status.eq_ignore_ascii_case("selesai") {
            sqlx::query("UPDATE delivery_rooms SET status = 'kosong' WHERE id = ?")
                .bind(&delivery.room)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_dt;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_surgeries_and_delivery() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let ops = db.list_surgeries(&date).await.unwrap();
        assert_eq!(ops.len(), 6);
        assert_eq!(ops[0].procedure, "Apendektomi");
        assert_eq!(ops[0].procedure_code, "47.0");
        assert_eq!(ops[0].status, SurgeryStatus::Selesai);
        assert_eq!(ops[1].procedure_code, "37.22");
        assert_eq!(ops[1].status, SurgeryStatus::Berjalan);
        assert!(ops[0].team.contains("dr. Ani, Sp.An"));
        let rooms = db.list_delivery_rooms().await.unwrap();
        assert_eq!(rooms.len(), 4);
        assert_eq!(rooms[0].id, "VK 1");
        assert_eq!(rooms[0].status, BedStatus::Terisi);
        assert_eq!(rooms[1].status, BedStatus::Terisi);
        assert_eq!(rooms[2].status, BedStatus::Kosong);
        assert_eq!(rooms[3].status, BedStatus::Kosong);
        let dels = db.list_deliveries().await.unwrap();
        assert_eq!(dels.len(), 1);
        assert_eq!(dels[0].patient_id, "RM-000816");
        assert_eq!(dels[0].gravida_para, "G2P1");
        assert_eq!(dels[0].dilation_cm, 6);
        assert_eq!(dels[0].fetal_heart_rate, 140);
        assert_eq!(dels[0].room, "VK 2");
        assert_eq!(dels[0].status, "Aktif");
    }

    #[tokio::test]
    async fn surgery_roundtrip_and_delivery_room_states() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        let op = db
            .create_surgery(
                "RM-000813",
                None,
                "OK 1",
                "Appendektomi",
                "47.0",
                "dr. Budi, Sp.B",
                "dr. Budi, Sp.B · dr. Ani, Sp.An · 2 perawat",
                "2026-11-19 17:00",
            )
            .await
            .unwrap();
        assert_eq!(op.id, "OK-20261119-007");
        assert_eq!(op.status, SurgeryStatus::Terjadwal);
        assert_eq!(op.scheduled_at, parse_dt("2026-11-19 17:00:00").unwrap());
        db.set_surgery_status(&op.id, SurgeryStatus::Siap)
            .await
            .unwrap();
        assert_eq!(db.list_surgeries(&date).await.unwrap().len(), 7);
        // Ruang terisi -> 422.
        let err = db
            .create_delivery("RM-000814", 30, "G1P0", 4, 140, "Bidan Sari", "dr. Maya, Sp.OG", "Observasi", "VK 2")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        // Ruang kosong -> Terisi; Selesai -> Kosong lagi.
        let d = db
            .create_delivery("RM-000814", 30, "G1P0", 4, 140, "Bidan Sari", "dr. Maya, Sp.OG", "Observasi", "VK 3")
            .await
            .unwrap();
        assert_eq!(d.id, "VK-20261119-002");
        assert_eq!(
            db.list_delivery_rooms().await.unwrap()[2].status,
            BedStatus::Terisi
        );
        db.set_delivery_status(&d.id, "Selesai").await.unwrap();
        assert_eq!(
            db.list_delivery_rooms().await.unwrap()[2].status,
            BedStatus::Kosong
        );
        assert_eq!(
            db.get_delivery(&d.id).await.unwrap().unwrap().status,
            "Selesai"
        );
        assert!(matches!(
            db.set_delivery_status(&d.id, "Ngawur").await,
            Err(WarasError::Validation(_))
        ));
    }
}
