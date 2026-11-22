//! Berkas klaim E-Klaim + cache ketersediaan bed Aplicares (FASE2.md §2).
//!
//! Fase 2 hanya membangun alur submit/status klaim; workbench Casemix
//! penuh (grouping INA-CBG) tetap Fase 3.

use crate::{clinic_today, db_err, fmt_dt, parse_dt, Db};
use sqlx::FromRow;
use waras_core::{
    types::{BedAvailability, Claim, ClaimStatus},
    WarasError,
};

// --- enum <-> TEXT ----------------------------------------------------------

pub(crate) fn claim_status_to_str(s: ClaimStatus) -> &'static str {
    match s {
        ClaimStatus::Draft => "draft",
        ClaimStatus::Terkirim => "terkirim",
        ClaimStatus::Diterima => "diterima",
        ClaimStatus::Ditolak => "ditolak",
        ClaimStatus::Dibayar => "dibayar",
        ClaimStatus::Terverifikasi => "terverifikasi",
        ClaimStatus::Diajukan => "diajukan",
        ClaimStatus::Pending => "pending",
    }
}

pub(crate) fn claim_status_from_str(s: &str) -> Result<ClaimStatus, WarasError> {
    Ok(match s {
        "draft" => ClaimStatus::Draft,
        "terkirim" => ClaimStatus::Terkirim,
        "diterima" => ClaimStatus::Diterima,
        "ditolak" => ClaimStatus::Ditolak,
        "dibayar" => ClaimStatus::Dibayar,
        "terverifikasi" => ClaimStatus::Terverifikasi,
        "diajukan" => ClaimStatus::Diajukan,
        "pending" => ClaimStatus::Pending,
        _ => {
            return Err(WarasError::Database(format!(
                "nilai claim_status tidak dikenal di DB: {s}"
            )))
        }
    })
}

// --- rows -------------------------------------------------------------------

#[derive(Debug, FromRow)]
pub struct ClaimRow {
    pub id: String,
    /// Fase 3: boleh NULL (klaim grouping tidak selalu dari tagihan).
    pub invoice_id: Option<String>,
    /// Integrasi Fase 3 (migrasi 0005): pasien pemilik klaim.
    pub patient_id: Option<String>,
    pub sep_id: Option<String>,
    pub ina_cbg_code: String,
    pub amount: i64,
    pub status: String,
    pub submitted_at: Option<String>,
    pub created_at: String,
    /// Fase 3: catatan grouping lokal (boleh NULL).
    pub grouping_note: Option<String>,
}

impl ClaimRow {
    pub fn into_domain(self) -> Result<Claim, WarasError> {
        Ok(Claim {
            id: self.id,
            invoice_id: self.invoice_id,
            patient_id: self.patient_id,
            sep_id: self.sep_id,
            ina_cbg_code: self.ina_cbg_code,
            amount: self.amount,
            status: claim_status_from_str(&self.status)?,
            submitted_at: match self.submitted_at {
                Some(s) => Some(parse_dt(&s)?),
                None => None,
            },
            created_at: parse_dt(&self.created_at)?,
            grouping_note: self.grouping_note,
        })
    }
}

#[derive(Debug, FromRow)]
pub struct BedRow {
    pub class: String,
    pub total: i64,
    pub available: i64,
    pub updated_at: String,
}

impl BedRow {
    pub fn into_domain(self) -> Result<BedAvailability, WarasError> {
        Ok(BedAvailability {
            class: self.class,
            total: self.total,
            available: self.available,
            updated_at: parse_dt(&self.updated_at)?,
        })
    }
}

impl Db {
    /// Daftar bed per kelas, urut Kelas 1/2/3, VIP, ICU (bukan alfabetis).
    pub async fn list_beds(&self) -> Result<Vec<BedAvailability>, WarasError> {
        let rows: Vec<BedRow> = sqlx::query_as(
            "SELECT * FROM bed_availability
             ORDER BY CASE class
                 WHEN 'Kelas 1' THEN 1 WHEN 'Kelas 2' THEN 2 WHEN 'Kelas 3' THEN 3
                 WHEN 'VIP' THEN 4 WHEN 'ICU' THEN 5 ELSE 6 END, class",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Simpan cache bed dari Aplicares (insert baru atau timpa per kelas).
    pub async fn upsert_beds(&self, beds: &[BedAvailability]) -> Result<(), WarasError> {
        for b in beds {
            sqlx::query(
                "INSERT INTO bed_availability (class, total, available, updated_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(class) DO UPDATE SET
                   total = excluded.total,
                   available = excluded.available,
                   updated_at = excluded.updated_at",
            )
            .bind(&b.class)
            .bind(b.total)
            .bind(b.available)
            .bind(fmt_dt(b.updated_at))
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// Buat berkas klaim. Bila `c.id` kosong, isi `CLM-20261119-###`
    /// (tanggal klinik hari ini). `create_claim_once` memegang
    /// `Db::alloc_lock` selama hitung-max + INSERT; retry maks 5x di
    /// sini hanya jaring pengaman untuk tabrakan antar proses (untuk id
    /// yang dihitung; bila id diisi pemanggil dan sudah ada, retry
    /// gagal dengan pelanggaran yang sama).
    pub async fn create_claim(&self, c: &Claim) -> Result<Claim, WarasError> {
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self.create_claim_once(c).await {
                Ok(c) => return Ok(c),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id klaim".into())))
    }

    async fn create_claim_once(&self, c: &Claim) -> Result<Claim, WarasError> {
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let id = if c.id.trim().is_empty() {
            let date_part = clinic_today().format("%Y%m%d").to_string();
            let prefix = format!("CLM-{date_part}-");
            let ids: Vec<String> =
                sqlx::query_scalar("SELECT id FROM claims WHERE id LIKE ?")
                    .bind(format!("{prefix}%"))
                    .fetch_all(&self.pool)
                    .await
                    .map_err(db_err)?;
            let seq = crate::next_seq_from_ids(&ids, 1);
            format!("{prefix}{seq:03}")
        } else {
            c.id.clone()
        };
        let mut claim = c.clone();
        claim.id = id;
        self.insert_claim(&claim).await
    }

    /// INSERT satu klaim + baca ulang. Pemanggil HARUS sudah memegang
    /// `Db::alloc_lock` (dipakai `create_claim_once` dan
    /// `group_claim_for_encounter`, supaya grouping tidak mengunci dua
    /// kali).
    pub(crate) async fn insert_claim(&self, c: &Claim) -> Result<Claim, WarasError> {
        sqlx::query(
            "INSERT INTO claims (id, invoice_id, patient_id, sep_id, ina_cbg_code, amount,
                 status, submitted_at, created_at, grouping_note)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&c.id)
        .bind(&c.invoice_id)
        .bind(&c.patient_id)
        .bind(&c.sep_id)
        .bind(&c.ina_cbg_code)
        .bind(c.amount)
        .bind(claim_status_to_str(c.status))
        .bind(c.submitted_at.map(fmt_dt))
        .bind(fmt_dt(c.created_at))
        .bind(&c.grouping_note)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_claim(&c.id)
            .await?
            .ok_or_else(|| WarasError::Database("klaim baru tidak ditemukan".into()))
    }

    /// Daftar klaim, terbaru dulu.
    pub async fn list_claims(&self) -> Result<Vec<Claim>, WarasError> {
        let rows: Vec<ClaimRow> = sqlx::query_as(
            "SELECT * FROM claims ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_claim(&self, id: &str) -> Result<Option<Claim>, WarasError> {
        let row: Option<ClaimRow> = sqlx::query_as("SELECT * FROM claims WHERE id = ? LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Ubah status klaim; `submitted_at` diisi saat klaim dikirim
    /// (`Some` = stempel ISO, `None` = NULL).
    pub async fn set_claim_status(
        &self,
        id: &str,
        status: ClaimStatus,
        submitted_at: Option<&str>,
    ) -> Result<(), WarasError> {
        let n = sqlx::query("UPDATE claims SET status = ?, submitted_at = ? WHERE id = ?")
            .bind(claim_status_to_str(status))
            .bind(submitted_at)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("klaim {id} tidak ditemukan")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests()
            .await
            .expect("buka DB test");
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
    async fn seed_beds_and_claim() {
        let db = seeded_db().await;
        let beds = db.list_beds().await.unwrap();
        assert_eq!(beds.len(), 5);
        assert_eq!(beds[0].class, "Kelas 1");
        assert_eq!((beds[0].available, beds[0].total), (12, 16));
        assert_eq!((beds[1].available, beds[1].total), (24, 32));
        assert_eq!((beds[2].available, beds[2].total), (38, 48));
        assert_eq!((beds[3].available, beds[3].total), (6, 8));
        assert_eq!(beds[4].class, "ICU");
        assert_eq!((beds[4].available, beds[4].total), (0, 12));

        let claims = db.list_claims().await.unwrap();
        // 1 klaim Fase 2 + 8 klaim seed Fase 3.
        assert_eq!(claims.len(), 9);
        let c = db
            .get_claim("CLM-20261119-001")
            .await
            .unwrap()
            .expect("klaim Fase 2 ada");
        assert_eq!(c.id, "CLM-20261119-001");
        assert_eq!(c.invoice_id.as_deref(), Some("INV-2026-1181"));
        assert_eq!(c.sep_id.as_deref(), Some("SEP-20261119-001"));
        assert_eq!(c.ina_cbg_code, "J-4-17-I");
        assert_eq!(c.amount, 185_000);
        assert_eq!(c.status, ClaimStatus::Draft);
        assert_eq!(c.submitted_at, None);
        assert_eq!(c.grouping_note, None);
    }

    #[tokio::test]
    async fn upsert_beds_updates_in_place() {
        let db = seeded_db().await;
        db.upsert_beds(&[BedAvailability {
            class: "ICU".into(),
            total: 12,
            available: 2,
            updated_at: today_8(),
        }])
        .await
        .unwrap();
        let beds = db.list_beds().await.unwrap();
        assert_eq!(beds.len(), 5, "upsert tidak menambah baris");
        assert_eq!(beds[4].available, 2);
        // Kelas baru ikut masuk.
        db.upsert_beds(&[BedAvailability {
            class: "Isolasi".into(),
            total: 4,
            available: 4,
            updated_at: today_8(),
        }])
        .await
        .unwrap();
        assert_eq!(db.list_beds().await.unwrap().len(), 6);
    }

    #[tokio::test]
    async fn claim_roundtrip_and_status() {
        let db = seeded_db().await;
        let c = db
            .create_claim(&Claim {
                id: String::new(),
                invoice_id: Some("INV-2026-1182".into()),
                patient_id: Some("RM-000813".into()),
                sep_id: Some("SEP-20261119-002".into()),
                ina_cbg_code: "J-4-17-I".into(),
                amount: 120_000,
                status: ClaimStatus::Draft,
                submitted_at: None,
                created_at: today_8(),
                grouping_note: None,
            })
            .await
            .unwrap();
        assert_eq!(c.id, "CLM-20261119-002");
        assert_eq!(db.list_claims().await.unwrap().len(), 10);

        db.set_claim_status(&c.id, ClaimStatus::Terkirim, Some("2026-11-19 10:00:00"))
            .await
            .unwrap();
        let got = db.get_claim(&c.id).await.unwrap().unwrap();
        assert_eq!(got.status, ClaimStatus::Terkirim);
        assert!(got.submitted_at.is_some());
        assert!(db.get_claim("CLM-20261119-999").await.unwrap().is_none());
        assert!(matches!(
            db.set_claim_status("CLM-20261119-999", ClaimStatus::Ditolak, None)
                .await,
            Err(WarasError::NotFound(_))
        ));
    }
}
