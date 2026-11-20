//! Laporan & SIRS (FASE4.md §4): agregat dari tabel nyata, tanpa tabel baru.
//!
//! - `report_visits`: (tanggal, kunjungan) dari `queue_entries` per tanggal.
//! - `report_top_diseases`: (kode ICD-10, display, jumlah) dari `conditions`
//!   grup Utama/Sekunder/Kronis.
//! - `report_sirs`: RL1 kunjungan hari ini + rawat jalan/inap; RL2 10 besar
//!   penyakit; RL3 hunian bed + lab menunggu — semua dari tabel nyata:
//!   kunjungan hari ini = antrean hari itu; rawat jalan = encounter hari
//!   itu; rawat inap = admisi berstatus Dirawat; lab menunggu = order lab
//!   hari itu Menunggu/Proses.

use crate::{db_err, Db};
use waras_core::{types::SirsSummary, WarasError};

impl Db {
    /// (tanggal, kunjungan) dari `queue_entries`, tanggal `from..=to`.
    /// Kunjungan per hari dalam rentang: dari `daily_stats.visits` (sama
    /// seperti `visits_7d` dasbor), dilengkapi hitungan antrean untuk
    /// tanggal yang belum ada di `daily_stats` (mis. hari berjalan di DB
    /// lama). Diubah koordinator dari kontrak awal (antrean saja) karena
    /// antrean hanya ada untuk hari ini — laporan 7 hari akan menampilkan
    /// satu batang saja.
    pub async fn report_visits(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(String, i64)>, WarasError> {
        let mut map: std::collections::BTreeMap<String, i64> = sqlx::query_as(
            "SELECT date, visits FROM daily_stats
             WHERE date >= ? AND date <= ?
             ORDER BY date ASC",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .collect();
        let queue_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT date, COUNT(*) FROM queue_entries
             WHERE date >= ? AND date <= ?
             GROUP BY date ORDER BY date ASC",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for (date, count) in queue_rows {
            map.entry(date).or_insert(count);
        }
        Ok(map.into_iter().collect())
    }

    /// (kode ICD-10, display, jumlah) dari `conditions` grup
    /// Utama/Sekunder/Kronis, terbanyak dulu.
    pub async fn report_top_diseases(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, i64)>, WarasError> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT code, display, COUNT(*) FROM conditions
             WHERE condition_group IN ('utama', 'sekunder', 'kronis')
             GROUP BY code, display
             ORDER BY COUNT(*) DESC, code ASC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows)
    }

    pub async fn report_sirs(&self) -> Result<SirsSummary, WarasError> {
        let today = crate::clinic_today().to_string();
        let visits_today: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM queue_entries WHERE date = ?",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let outpatient_today: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM encounters WHERE date = ?",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let inpatient_today: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM admissions WHERE status = 'dirawat'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let (beds_occupied, beds_total) = {
            let occ = self.bed_occupancy().await?;
            occ.iter().fold((0, 0), |a, (_, o, t)| (a.0 + o, a.1 + t))
        };
        let (lab_pending, _) = self.lab_counts(&today).await?;
        let top_diseases = self.report_top_diseases(10).await?;
        Ok(SirsSummary {
            visits_today,
            outpatient_today,
            inpatient_today,
            beds_occupied,
            beds_total,
            lab_pending,
            top_diseases,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clinic_today;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn report_visits_counts_queue_per_day() {
        let db = seeded_db().await;
        // 19 Nov: daily_stats.visits (210) menang atas antrean (44) —
        // sama seperti visits_7d dasbor.
        let rows = db
            .report_visits("2026-11-19", "2026-11-19")
            .await
            .unwrap();
        assert_eq!(rows, vec![("2026-11-19".to_string(), 210)]);
        // Rentang 7 hari: 7 baris dari daily_stats.
        let rows = db
            .report_visits("2026-11-13", "2026-11-19")
            .await
            .unwrap();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0], ("2026-11-13".to_string(), 162));
        // Rentang tanpa statistik maupun antrean -> kosong.
        assert!(db
            .report_visits("2026-11-20", "2026-11-21")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn report_top_diseases_groups_main_only() {
        let db = seeded_db().await;
        let top = db.report_top_diseases(10).await.unwrap();
        // Grup Utama/Sekunder/Kronis seed (9 baris, semua jumlah 1):
        // J20.9, E11.9 x2 display berbeda, I10, J18.9, I20.9, K35.8,
        // O80, N18.9 (Fase 4, Slamet).
        assert_eq!(top.len(), 9);
        assert!(top.iter().all(|(_, _, n)| *n == 1));
        // E11.9 muncul dua kali dengan display berbeda (sekunder vs kronis).
        let e119: i64 = top
            .iter()
            .filter(|(c, _, _)| c == "E11.9")
            .map(|(_, _, n)| *n)
            .sum();
        assert_eq!(e119, 2);
        assert!(top.iter().any(|(c, _, _)| c == "N18.9"));
        // Grup Riwayat (J06.9 x3) dan Tindakan medis (89.52) TIDAK masuk.
        assert!(!top.iter().any(|(c, _, _)| c == "J06.9"));
        assert!(!top.iter().any(|(c, _, _)| c == "89.52"));
        assert_eq!(db.report_top_diseases(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn report_sirs_from_real_tables() {
        let db = seeded_db().await;
        let s = db.report_sirs().await.unwrap();
        assert_eq!(s.visits_today, 44, "antrean 19 Nov");
        // Encounter 19 Nov: 014–022 = 9 baris.
        assert_eq!(s.outpatient_today, 9);
        assert_eq!(s.inpatient_today, 7, "admisi Dirawat seed Fase 3");
        assert_eq!(s.beds_occupied, 86);
        assert_eq!(s.beds_total, 120);
        assert_eq!(s.lab_pending, 3, "order lab 19 Nov Menunggu/Proses");
        assert_eq!(s.top_diseases.len(), 9);
        // Hari ini dari clinic_today (bukan tanggal sistem).
        let _ = clinic_today();
    }
}
