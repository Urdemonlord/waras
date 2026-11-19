//! SEP VClaim, rujukan keluar, kunjungan PCare, dan antrean online
//! Mobile JKN (FASE2.md §2).
//!
//! Pola ID mengikuti `prescriptions.rs` (RX-####): nomor urut diambil
//! dari suffix numerik id yang sudah ada. SEP/rujukan/PCare memakai
//! prefix bertanggal (`SEP-20261119-001`, tanggal klinik `WARAS_TODAY`).

use crate::{clinic_today, db_err, fmt_dt, parse_date, parse_dt, Db};
use sqlx::FromRow;
use waras_core::{
    types::{OnlineBooking, PcareVisit, Referral, ReferralStatus, Sep, SepStatus},
    WarasError,
};

// --- enum <-> TEXT ----------------------------------------------------------

pub(crate) fn sep_status_to_str(s: SepStatus) -> &'static str {
    match s {
        SepStatus::Aktif => "aktif",
        SepStatus::Gagal => "gagal",
        SepStatus::Dibatalkan => "dibatalkan",
    }
}

pub(crate) fn sep_status_from_str(s: &str) -> Result<SepStatus, WarasError> {
    Ok(match s {
        "aktif" => SepStatus::Aktif,
        "gagal" => SepStatus::Gagal,
        "dibatalkan" => SepStatus::Dibatalkan,
        _ => return Err(WarasError::Database(format!("nilai sep_status tidak dikenal di DB: {s}"))),
    })
}

pub(crate) fn referral_status_to_str(s: ReferralStatus) -> &'static str {
    match s {
        ReferralStatus::Terkirim => "terkirim",
        ReferralStatus::Menunggu => "menunggu",
        ReferralStatus::Ditolak => "ditolak",
    }
}

pub(crate) fn referral_status_from_str(s: &str) -> Result<ReferralStatus, WarasError> {
    Ok(match s {
        "terkirim" => ReferralStatus::Terkirim,
        "menunggu" => ReferralStatus::Menunggu,
        "ditolak" => ReferralStatus::Ditolak,
        _ => {
            return Err(WarasError::Database(format!(
                "nilai referral_status tidak dikenal di DB: {s}"
            )))
        }
    })
}

// --- rows -------------------------------------------------------------------

#[derive(Debug, FromRow)]
pub struct SepRow {
    pub id: String,
    pub sep_number: String,
    pub patient_id: String,
    pub encounter_id: Option<String>,
    pub bpjs_number: String,
    pub jenis: String,
    pub poli: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
}

impl SepRow {
    pub fn into_domain(self) -> Result<Sep, WarasError> {
        Ok(Sep {
            id: self.id,
            sep_number: self.sep_number,
            patient_id: self.patient_id,
            encounter_id: self.encounter_id,
            bpjs_number: self.bpjs_number,
            jenis: self.jenis,
            poli: self.poli,
            status: sep_status_from_str(&self.status)?,
            created_by: self.created_by,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct ReferralRow {
    pub id: String,
    pub patient_id: String,
    pub from_poli: String,
    pub to_facility: String,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

impl ReferralRow {
    pub fn into_domain(self) -> Result<Referral, WarasError> {
        Ok(Referral {
            id: self.id,
            patient_id: self.patient_id,
            from_poli: self.from_poli,
            to_facility: self.to_facility,
            reason: self.reason,
            status: referral_status_from_str(&self.status)?,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct PcareVisitRow {
    pub id: String,
    pub patient_id: String,
    pub visit_date: String,
    pub poli: String,
    pub status: String,
    pub created_at: String,
}

impl PcareVisitRow {
    pub fn into_domain(self) -> Result<PcareVisit, WarasError> {
        Ok(PcareVisit {
            id: self.id,
            patient_id: self.patient_id,
            visit_date: parse_date(&self.visit_date)?,
            poli: self.poli,
            status: self.status,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct OnlineBookingRow {
    pub id: String,
    pub booking_code: String,
    pub patient_id: String,
    pub poli: String,
    pub estimasi: String,
    pub status: String,
    pub local_queue_id: Option<String>,
    pub created_at: String,
}

impl OnlineBookingRow {
    pub fn into_domain(self) -> Result<OnlineBooking, WarasError> {
        Ok(OnlineBooking {
            id: self.id,
            booking_code: self.booking_code,
            patient_id: self.patient_id,
            poli: self.poli,
            estimasi: self.estimasi,
            status: self.status,
            local_queue_id: self.local_queue_id,
            created_at: parse_dt(&self.created_at)?,
        })
    }
}

// --- helpers ----------------------------------------------------------------

/// Id berikutnya berpola `{PREFIX}-{YYYYMMDD}-{seq:03}` untuk tanggal
/// klinik hari ini, mis. `SEP-20261119-009`. `table` hanya diisi konstanta
/// internal (bukan input pengguna).
async fn next_dated_id(db: &Db, table: &str, prefix: &str) -> Result<String, WarasError> {
    let date_part = clinic_today().format("%Y%m%d").to_string();
    let full_prefix = format!("{prefix}-{date_part}-");
    let ids: Vec<String> =
        sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE id LIKE ?"))
            .bind(format!("{full_prefix}%"))
            .fetch_all(&db.pool)
            .await
            .map_err(db_err)?;
    let seq = crate::next_seq_from_ids(&ids, 1);
    Ok(format!("{full_prefix}{seq:03}"))
}

impl Db {
    /// Simpan SEP. Bila `sep.id` kosong, isi id + `sep_number` berikutnya
    /// (`SEP-20261119-###`); bila `sep_number` kosong tapi id terisi, pakai
    /// id sebagai nomor SEP (kontrak: nomor SEP = id).
    pub async fn create_sep(&self, sep: &Sep) -> Result<Sep, WarasError> {
        let id = if sep.id.trim().is_empty() {
            next_dated_id(self, "seps", "SEP").await?
        } else {
            sep.id.clone()
        };
        let sep_number = if sep.sep_number.trim().is_empty() {
            id.clone()
        } else {
            sep.sep_number.clone()
        };
        sqlx::query(
            "INSERT INTO seps (id, sep_number, patient_id, encounter_id, bpjs_number,
                 jenis, poli, status, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&sep_number)
        .bind(&sep.patient_id)
        .bind(&sep.encounter_id)
        .bind(&sep.bpjs_number)
        .bind(&sep.jenis)
        .bind(&sep.poli)
        .bind(sep_status_to_str(sep.status))
        .bind(&sep.created_by)
        .bind(fmt_dt(sep.created_at))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_sep(&id)
            .await?
            .ok_or_else(|| WarasError::Database("SEP baru tidak ditemukan".into()))
    }

    /// Daftar SEP yang `created_at`-nya diawali `date` (`YYYY-MM-DD`),
    /// urut kronologis.
    pub async fn list_seps(&self, date: &str) -> Result<Vec<Sep>, WarasError> {
        let rows: Vec<SepRow> = sqlx::query_as(
            "SELECT * FROM seps WHERE created_at LIKE ? ORDER BY created_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_sep(&self, id: &str) -> Result<Option<Sep>, WarasError> {
        let row: Option<SepRow> = sqlx::query_as("SELECT * FROM seps WHERE id = ? LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    pub async fn set_sep_status(&self, id: &str, status: SepStatus) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE seps SET status = ? WHERE id = ?")
            .bind(sep_status_to_str(status))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("SEP {id} tidak ditemukan")));
        }
        Ok(())
    }

    /// Tautkan tagihan ke SEP (kolom `invoices.sep_id` dari migrasi 0002).
    pub async fn link_invoice_sep(&self, invoice_id: &str, sep_id: &str) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE invoices SET sep_id = ? WHERE id = ?")
            .bind(sep_id)
            .bind(invoice_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "tagihan {invoice_id} tidak ditemukan"
            )));
        }
        Ok(())
    }

    /// Simpan rujukan keluar. Bila `r.id` kosong, isi `RUJ-20261119-###`.
    pub async fn create_referral(&self, r: &Referral) -> Result<Referral, WarasError> {
        let id = if r.id.trim().is_empty() {
            next_dated_id(self, "referrals", "RUJ").await?
        } else {
            r.id.clone()
        };
        sqlx::query(
            "INSERT INTO referrals (id, patient_id, from_poli, to_facility, reason,
                 status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&r.patient_id)
        .bind(&r.from_poli)
        .bind(&r.to_facility)
        .bind(&r.reason)
        .bind(referral_status_to_str(r.status))
        .bind(fmt_dt(r.created_at))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<ReferralRow> =
            sqlx::query_as("SELECT * FROM referrals WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("rujukan baru tidak ditemukan".into()))
    }

    /// Daftar rujukan yang `created_at`-nya diawali `date`, urut kronologis.
    pub async fn list_referrals(&self, date: &str) -> Result<Vec<Referral>, WarasError> {
        let rows: Vec<ReferralRow> = sqlx::query_as(
            "SELECT * FROM referrals WHERE created_at LIKE ? ORDER BY created_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Catat kunjungan PCare. Bila `v.id` kosong, isi `PCV-20261119-###`.
    pub async fn create_pcare_visit(&self, v: &PcareVisit) -> Result<PcareVisit, WarasError> {
        let id = if v.id.trim().is_empty() {
            next_dated_id(self, "pcare_visits", "PCV").await?
        } else {
            v.id.clone()
        };
        sqlx::query(
            "INSERT INTO pcare_visits (id, patient_id, visit_date, poli, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&v.patient_id)
        .bind(v.visit_date.to_string())
        .bind(&v.poli)
        .bind(&v.status)
        .bind(fmt_dt(v.created_at))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<PcareVisitRow> =
            sqlx::query_as("SELECT * FROM pcare_visits WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("kunjungan PCare baru tidak ditemukan".into()))
    }

    /// Daftar kunjungan PCare yang `created_at`-nya diawali `date`.
    pub async fn list_pcare_visits(&self, date: &str) -> Result<Vec<PcareVisit>, WarasError> {
        let rows: Vec<PcareVisitRow> = sqlx::query_as(
            "SELECT * FROM pcare_visits WHERE created_at LIKE ? ORDER BY created_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Daftar booking antrean online yang `created_at`-nya diawali `date`.
    pub async fn list_online_queue(&self, date: &str) -> Result<Vec<OnlineBooking>, WarasError> {
        let rows: Vec<OnlineBookingRow> = sqlx::query_as(
            "SELECT * FROM online_queue WHERE created_at LIKE ? ORDER BY created_at ASC, id ASC",
        )
        .bind(format!("{date}%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Ubah status booking (`Menunggu`/`Hadir`) berdasarkan `booking_code`.
    /// `local_queue_id`: `Some` = isi nomor antrean lokal (mis. `A-041`
    /// saat check-in), `None` = kosongkan (NULL).
    pub async fn set_online_queue_status(
        &self,
        booking_code: &str,
        status: &str,
        local_queue_id: Option<&str>,
    ) -> Result<(), WarasError> {
        let n = sqlx::query(
            "UPDATE online_queue SET status = ?, local_queue_id = ? WHERE booking_code = ?",
        )
        .bind(status)
        .bind(local_queue_id)
        .bind(booking_code)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "booking {booking_code} tidak ditemukan"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:")
            .await
            .expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    fn today_8() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    #[tokio::test]
    async fn seed_fase2_counts() {
        let db = seeded_db().await;
        assert_eq!(db.list_seps("2026-11-19").await.unwrap().len(), 8);
        assert_eq!(db.list_online_queue("2026-11-19").await.unwrap().len(), 7);
        assert_eq!(db.list_referrals("2026-11-19").await.unwrap().len(), 2);
        assert_eq!(db.list_pcare_visits("2026-11-19").await.unwrap().len(), 3);
        assert_eq!(db.list_outbox(None, None, 50).await.unwrap().len(), 0, "outbox seed kosong");
        let seps = db.list_seps("2026-11-19").await.unwrap();
        assert_eq!(seps[0].id, "SEP-20261119-001");
        assert_eq!(seps[0].patient_id, "RM-000812");
        assert_eq!(seps[4].status, SepStatus::Gagal, "SEP-005 Agus Gagal");
        assert_eq!(seps[4].patient_id, "RM-000815");
        let q = db.list_online_queue("2026-11-19").await.unwrap();
        assert_eq!(q[0].booking_code, "JKN-881201");
        assert_eq!(q[0].status, "Hadir");
        assert_eq!(q[0].local_queue_id.as_deref(), Some("A-041"));
        assert_eq!(q[2].status, "Menunggu");
    }

    #[tokio::test]
    async fn create_sep_generates_next_id() {
        let db = seeded_db().await;
        let sep = db
            .create_sep(&Sep {
                id: String::new(),
                sep_number: String::new(),
                patient_id: "RM-000813".into(),
                encounter_id: None,
                bpjs_number: "0001234567891".into(),
                jenis: "RawatJalan".into(),
                poli: "Poli Anak".into(),
                status: SepStatus::Aktif,
                created_by: "dr. Ahmad Hidayat".into(),
                created_at: today_8(),
            })
            .await
            .unwrap();
        assert_eq!(sep.id, "SEP-20261119-009");
        assert_eq!(sep.sep_number, "SEP-20261119-009");
        assert_eq!(db.list_seps("2026-11-19").await.unwrap().len(), 9);

        db.set_sep_status(&sep.id, SepStatus::Dibatalkan).await.unwrap();
        assert_eq!(
            db.get_sep(&sep.id).await.unwrap().unwrap().status,
            SepStatus::Dibatalkan
        );
        assert!(db.get_sep("SEP-20261119-999").await.unwrap().is_none());
        assert!(matches!(
            db.set_sep_status("SEP-20261119-999", SepStatus::Aktif).await,
            Err(WarasError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn link_invoice_sep_sets_column() {
        let db = seeded_db().await;
        db.link_invoice_sep("INV-2026-1181", "SEP-20261119-001")
            .await
            .unwrap();
        let sep_id: Option<String> =
            sqlx::query_scalar("SELECT sep_id FROM invoices WHERE id = ?")
                .bind("INV-2026-1181")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(sep_id.as_deref(), Some("SEP-20261119-001"));
        assert!(matches!(
            db.link_invoice_sep("INV-2099-9999", "SEP-20261119-001").await,
            Err(WarasError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn referral_and_pcare_roundtrip() {
        let db = seeded_db().await;
        let r = db
            .create_referral(&Referral {
                id: String::new(),
                patient_id: "RM-000814".into(),
                from_poli: "Penyakit Dalam".into(),
                to_facility: "RSUD Dr. Kariadi".into(),
                reason: "Bedah Saraf".into(),
                status: ReferralStatus::Menunggu,
                created_at: today_8(),
            })
            .await
            .unwrap();
        assert_eq!(r.id, "RUJ-20261119-003");
        assert_eq!(db.list_referrals("2026-11-19").await.unwrap().len(), 3);

        let v = db
            .create_pcare_visit(&PcareVisit {
                id: String::new(),
                patient_id: "RM-000815".into(),
                visit_date: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
                poli: "Gigi".into(),
                status: "Tercatat".into(),
                created_at: today_8(),
            })
            .await
            .unwrap();
        assert_eq!(v.id, "PCV-20261119-004");
        assert_eq!(db.list_pcare_visits("2026-11-19").await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn set_online_queue_status_checkin() {
        let db = seeded_db().await;
        db.set_online_queue_status("JKN-881203", "Hadir", Some("A-043"))
            .await
            .unwrap();
        let q = db
            .list_online_queue("2026-11-19")
            .await
            .unwrap()
            .into_iter()
            .find(|b| b.booking_code == "JKN-881203")
            .unwrap();
        assert_eq!(q.status, "Hadir");
        assert_eq!(q.local_queue_id.as_deref(), Some("A-043"));
        assert!(matches!(
            db.set_online_queue_status("JKN-000000", "Hadir", None).await,
            Err(WarasError::NotFound(_))
        ));
    }
}
