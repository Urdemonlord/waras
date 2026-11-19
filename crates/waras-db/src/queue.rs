//! Antrean harian.

use crate::convert::*;
use crate::rows::QueueRow;
use crate::{db_err, fmt_dt, new_uuid, now_naive, now_time_str, Db};
use chrono::NaiveDate;
use waras_core::{ids, types::{QueueEntry, QueueStatus}, WarasError};

/// Prefix nomor antrean per poli. Kontrak hanya menetapkan A = Poli Umum;
/// poli lain dipetakan berurutan (B Anak, C Penyakit Dalam, D Gigi, E Mata).
pub fn poli_prefix(poli: &str) -> &'static str {
    let p = poli.to_lowercase();
    if p.contains("umum") {
        "A"
    } else if p.contains("anak") {
        "B"
    } else if p.contains("dalam") {
        "C"
    } else if p.contains("gigi") {
        "D"
    } else if p.contains("mata") {
        "E"
    } else {
        "A"
    }
}

impl Db {
    /// Daftar antrean satu tanggal, urut nomor.
    pub async fn list_queue(&self, date: NaiveDate) -> Result<Vec<QueueEntry>, WarasError> {
        let rows: Vec<QueueRow> = sqlx::query_as(
            "SELECT * FROM queue_entries WHERE date = ? ORDER BY number ASC",
        )
        .bind(date.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Ambil nomor antrean baru: nomor berikutnya per prefix poli + tanggal.
    pub async fn take_queue(
        &self,
        patient_id: &str,
        poli: &str,
        doctor: &str,
        date: NaiveDate,
    ) -> Result<QueueEntry, WarasError> {
        let prefix = poli_prefix(poli);
        let numbers: Vec<String> = sqlx::query_scalar(
            "SELECT number FROM queue_entries WHERE date = ? AND number LIKE ?",
        )
        .bind(date.to_string())
        .bind(format!("{prefix}-%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let seq = numbers
            .iter()
            .filter_map(|n| n.trim_start_matches(prefix).trim_start_matches('-').parse::<u32>().ok())
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        let number = ids::queue_number(prefix, seq);
        let id = format!("Q-{date}-{number}");
        let time = now_time_str();
        sqlx::query(
            "INSERT INTO queue_entries (id, number, patient_id, poli, doctor, date, time, status, called_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'menunggu', NULL)",
        )
        .bind(&id)
        .bind(&number)
        .bind(patient_id)
        .bind(poli)
        .bind(doctor)
        .bind(date.to_string())
        .bind(&time)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(QueueEntry {
            id,
            number,
            patient_id: patient_id.to_string(),
            poli: poli.to_string(),
            doctor: doctor.to_string(),
            date,
            time,
            status: QueueStatus::Menunggu,
            called_at: None,
        })
    }

    /// Ubah status antrean (DalamKonsultasi / Selesai / TidakHadir).
    pub async fn set_queue_status(
        &self,
        id: &str,
        status: QueueStatus,
    ) -> Result<QueueEntry, WarasError> {
        let row: Option<QueueRow> =
            sqlx::query_as("SELECT * FROM queue_entries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        let Some(_) = row else {
            return Err(WarasError::NotFound(format!("antrean {id} tidak ditemukan")));
        };
        sqlx::query("UPDATE queue_entries SET status = ? WHERE id = ?")
            .bind(queue_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        self.get_queue(id)
            .await?
            .ok_or_else(|| WarasError::Database("antrean hilang setelah update".into()))
    }

    /// Panggil antrean: status -> Dipanggil, `called_at` diisi jam sekarang.
    pub async fn call_queue(&self, id: &str) -> Result<QueueEntry, WarasError> {
        let row: Option<QueueRow> =
            sqlx::query_as("SELECT * FROM queue_entries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        if row.is_none() {
            return Err(WarasError::NotFound(format!("antrean {id} tidak ditemukan")));
        }
        sqlx::query(
            "UPDATE queue_entries SET status = 'dipanggil', called_at = ? WHERE id = ?",
        )
        .bind(fmt_dt(now_naive()))
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_queue(id)
            .await?
            .ok_or_else(|| WarasError::Database("antrean hilang setelah update".into()))
    }

    pub async fn get_queue(&self, id: &str) -> Result<Option<QueueEntry>, WarasError> {
        let row: Option<QueueRow> =
            sqlx::query_as("SELECT * FROM queue_entries WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Daftar janji; bila `date` diisi hanya tanggal itu, bila None semua
    /// (urut tanggal + jam konsultasi).
    pub async fn list_appointments(
        &self,
        date: Option<NaiveDate>,
    ) -> Result<Vec<waras_core::types::Appointment>, WarasError> {
        use crate::rows::AppointmentRow;
        let rows: Vec<AppointmentRow> = match date {
            Some(d) => {
                sqlx::query_as(
                    "SELECT * FROM appointments WHERE date = ?
                     ORDER BY date ASC, consultation_time ASC",
                )
                .bind(d.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM appointments
                     ORDER BY date ASC, consultation_time ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }
}

// dipakai seed untuk id antrean yang deterministik
pub(crate) fn queue_seed_id(date: NaiveDate, number: &str) -> String {
    format!("Q-{date}-{number}")
}

#[allow(dead_code)]
pub(crate) fn _unused_uuid() -> String {
    new_uuid()
}
