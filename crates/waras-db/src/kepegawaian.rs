//! Kepegawaian (FASE4.md §4): data staf + jadwal shift.
//! Satu staf hanya boleh punya satu shift per tanggal (bila tidak 422).

use crate::rows::{clinic_now_string, ShiftScheduleRow, StaffRow};
use crate::{clinic_today, db_err, Db};
use waras_core::{
    types::{ShiftSchedule, Staff},
    WarasError,
};

impl Db {
    pub async fn list_staff(&self) -> Result<Vec<Staff>, WarasError> {
        let rows: Vec<StaffRow> =
            sqlx::query_as("SELECT * FROM staff ORDER BY name ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Staf baru: id `STF-<seq>`, aktif.
    pub async fn create_staff(
        &self,
        name: &str,
        nip: &str,
        role: &str,
        poli: &str,
        phone: &str,
    ) -> Result<Staff, WarasError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM staff")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("STF-{seq:03}");
        sqlx::query(
            "INSERT INTO staff (id, name, nip, role, poli, phone, active)
             VALUES (?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(&id)
        .bind(name)
        .bind(nip)
        .bind(role)
        .bind(poli)
        .bind(phone)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<StaffRow> =
            sqlx::query_as("SELECT * FROM staff WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("staf baru tidak ditemukan".into()))
    }

    pub async fn set_staff_active(
        &self,
        id: &str,
        active: bool,
    ) -> Result<Staff, WarasError> {
        let n = sqlx::query("UPDATE staff SET active = ? WHERE id = ?")
            .bind(if active { 1 } else { 0 })
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("staf {id} tidak ditemukan")));
        }
        let row: Option<StaffRow> =
            sqlx::query_as("SELECT * FROM staff WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("staf tidak ditemukan setelah update".into()))
    }

    /// Jadwal shift satu tanggal, urut id (urutan seed).
    pub async fn list_shifts(&self, date: &str) -> Result<Vec<ShiftSchedule>, WarasError> {
        let rows: Vec<ShiftScheduleRow> = sqlx::query_as(
            "SELECT * FROM shift_schedules WHERE date = ? ORDER BY id ASC",
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Shift baru: id `SHF-<tanggal klinik>-<seq>`. Gagal 422 bila staf
    /// sudah punya shift di tanggal itu.
    pub async fn create_shift(
        &self,
        staff_id: &str,
        date: &str,
        shift: &str,
        poli: &str,
    ) -> Result<ShiftSchedule, WarasError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM shift_schedules WHERE staff_id = ? AND date = ?",
        )
        .bind(staff_id)
        .bind(date)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        if n > 0 {
            return Err(WarasError::Unprocessable(format!(
                "staf {staff_id} sudah punya shift pada {date}"
            )));
        }
        let today = clinic_today();
        let date_part = today.format("%Y%m%d").to_string();
        let prefix = format!("SHF-{date_part}-");
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM shift_schedules WHERE id LIKE ?")
                .bind(format!("{prefix}%"))
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("{prefix}{seq:03}");
        sqlx::query(
            "INSERT INTO shift_schedules (id, staff_id, date, shift, poli, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(staff_id)
        .bind(date)
        .bind(shift)
        .bind(poli)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<ShiftScheduleRow> =
            sqlx::query_as("SELECT * FROM shift_schedules WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("shift baru tidak ditemukan".into()))
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
    async fn seed_staff_and_shifts() {
        let db = seeded_db().await;
        let staff = db.list_staff().await.unwrap();
        // 6 pengguna seed + drg. Maya + Bidan Sari + dr. Andi Wijaya
        // (dokter jaga IGD — lihat catatan seed_fase4: kontrak §4.2
        // menulis 8 staf tapi shift ke-8 butuh baris staf Andi).
        assert_eq!(staff.len(), 9);
        let maya = staff.iter().find(|s| s.id == "STF-007").unwrap();
        assert_eq!(maya.name, "drg. Maya Kusuma");
        assert_eq!(maya.role, "Dokter Gigi");
        assert_eq!(maya.poli, "Gigi");
        assert!(maya.active);
        let bidan = staff.iter().find(|s| s.id == "STF-008").unwrap();
        assert_eq!(bidan.name, "Bidan Sari Handayani");
        assert_eq!(bidan.role, "Bidan");
        let date = clinic_today().to_string();
        let shifts = db.list_shifts(&date).await.unwrap();
        assert_eq!(shifts.len(), 8);
        assert_eq!(shifts[0].staff_id, "STF-001");
        assert_eq!(shifts[0].shift, "Pagi");
        assert_eq!(shifts[2].shift, "Siang");
        assert_eq!(shifts[7].staff_id, "STF-009");
        assert_eq!(shifts[7].shift, "Malam");
        assert_eq!(shifts[7].poli, "IGD");
    }

    #[tokio::test]
    async fn shift_duplicate_is_422_and_staff_active_toggle() {
        let db = seeded_db().await;
        let date = clinic_today().to_string();
        // STF-001 sudah Pagi 19 Nov -> shift kedua tanggal itu 422.
        let err = db
            .create_shift("STF-001", &date, "Malam", "IGD")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        assert_eq!(db.list_shifts(&date).await.unwrap().len(), 8);
        // Tanggal lain boleh.
        let s = db
            .create_shift("STF-001", "2026-11-20", "Pagi", "Poli Umum")
            .await
            .unwrap();
        assert_eq!(s.id, "SHF-20261119-009");
        let off = db.set_staff_active("STF-001", false).await.unwrap();
        assert!(!off.active);
        let on = db.set_staff_active("STF-001", true).await.unwrap();
        assert!(on.active);
        assert!(matches!(
            db.set_staff_active("STF-999", false).await,
            Err(WarasError::NotFound(_))
        ));
        let st = db
            .create_staff("Budi Santoso", "", "Perawat", "IGD", "")
            .await
            .unwrap();
        assert_eq!(st.id, "STF-010");
        assert!(st.active);
    }
}
