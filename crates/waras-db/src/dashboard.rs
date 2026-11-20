//! KPI dasbor — dihitung dari data nyata, BUKAN angka hardcode
//! (KONTRAK §4). Dua angka waktu tunggu adalah konstanta desain
//! (dari desain; dihitung dari antrean di Fase 1.1).
//!
//! Fase 3 (FASE3.md §4.1): `lab_pending`/`lab_critical`, `emergency_red`,
//! dan `beds_*` kini dihitung dari tabel nyata (lab_orders, igd_visits,
//! ward_beds), bukan `daily_stats`. Nilai seed berubah vs Fase 1/2:
//! lab_pending 26 → 3, lab_critical 2 → 1, emergency_red 3 → 1
//! (lihat laporan agen A: angka §8 FASE3.md — 4 dan 2 — tidak cocok
//! dengan baris seed §4.2; yang dipakai adalah hitungan nyata).
//! `patients_today`, revenue, dan `visits_7d` tetap dari `daily_stats`.

use crate::rows::DailyStatRow;
use crate::{db_err, Db};
use chrono::NaiveDate;
use waras_core::types::{DashboardKpis, VisitDay};
use waras_core::WarasError;

impl Db {
    pub async fn dashboard_kpis(&self, today: NaiveDate) -> Result<DashboardKpis, WarasError> {
        // -- antrean hari ini -------------------------------------------------
        let (waiting, in_consultation, done, no_show, total): (i64, i64, i64, i64, i64) = {
            let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
                "SELECT
                   COALESCE(SUM(CASE WHEN status = 'menunggu' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN status IN ('dipanggil', 'dalam_konsultasi') THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN status = 'selesai' THEN 1 ELSE 0 END), 0),
                   COALESCE(SUM(CASE WHEN status = 'tidak_hadir' THEN 1 ELSE 0 END), 0),
                   COUNT(*)
                 FROM queue_entries WHERE date = ?",
            )
            .bind(today.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
            row
        };
        let queue_active = waiting + in_consultation;

        // -- janji kontrol 7 hari ke depan ------------------------------------
        let end7 = today + chrono::Duration::days(7);
        let followups_7d: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM appointments
             WHERE status IN ('terkonfirmasi', 'menunggu')
               AND date >= ? AND date <= ?",
        )
        .bind(today.to_string())
        .bind(end7.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;

        // -- statistik harian: 7 hari terakhir + hari ini ---------------------
        let start7 = today - chrono::Duration::days(6);
        let stat_rows: Vec<DailyStatRow> = sqlx::query_as(
            "SELECT * FROM daily_stats WHERE date >= ? AND date <= ? ORDER BY date ASC",
        )
        .bind(start7.to_string())
        .bind(today.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut stats = Vec::new();
        for r in stat_rows {
            stats.push(r.into_domain()?);
        }
        let today_stat = stats.iter().find(|s| s.date == today);
        let (patients_today, revenue_today_idr, revenue_transactions) = match today_stat {
            Some(s) => (s.registrations, s.revenue_idr, s.transactions),
            None => (0, 0, 0),
        };

        // -- Fase 3 (§4.1): lab/IGD/bed dari tabel nyata, bukan daily_stats --
        // lab_pending = order hari ini Menunggu/Proses; lab_critical = Kritis.
        let (lab_pending, lab_critical) = self.lab_counts(&today.to_string()).await?;
        // emergency_red = IGD Merah aktif hari ini.
        let (emergency_red, _, _) = self.igd_counts(&today.to_string()).await?;
        // beds dari hunian bangsal; bila tabel Fase 3 belum di-seed (DB lama
        // sebelum seed_fase3 jalan), pakai agregat daily_stats.
        let (beds_occupied, beds_total) = {
            let occ = self.bed_occupancy().await?;
            let total: (i64, i64) = occ.iter().fold((0, 0), |a, (_, o, t)| (a.0 + o, a.1 + t));
            if total.1 == 0 {
                match today_stat {
                    Some(s) => (s.beds_occupied, s.beds_total),
                    None => (0, 0),
                }
            } else {
                total
            }
        };
        let visits_7d: Vec<VisitDay> = stats
            .iter()
            .map(|s| VisitDay {
                day: Self::day_name_id(s.date).to_string(),
                date: s.date,
                count: s.visits,
            })
            .collect();
        let visits_sum: i64 = stats.iter().map(|s| s.visits).sum();
        let visits_n = stats.len().max(1) as i64;
        let visits_avg_7d = if stats.is_empty() { 0 } else { visits_sum / visits_n };
        let visits_peak_7d = stats.iter().map(|s| s.visits).max().unwrap_or(0);

        // -- tagihan belum lunas hari ini --------------------------------------
        let (unpaid_idr, unpaid_count): (i64, i64) = {
            let row: (Option<i64>, i64) = sqlx::query_as(
                "SELECT COALESCE(SUM(amount), 0), COUNT(*) FROM invoices
                 WHERE status IN ('menunggu', 'cicilan')
                   AND substr(created_at, 1, 10) = ?",
            )
            .bind(today.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
            (row.0.unwrap_or(0), row.1)
        };

        Ok(DashboardKpis {
            patients_today,
            queue_active,
            queue_waiting: waiting,
            queue_in_consultation: in_consultation,
            queue_done: done,
            queue_no_show: no_show,
            queue_total: total,
            lab_pending,
            lab_critical,
            followups_7d,
            emergency_red,
            visits_7d,
            visits_avg_7d,
            visits_peak_7d,
            // Konstanta dari desain; dihitung dari antrean di Fase 1.1.
            avg_wait_minutes: 18,
            longest_wait_minutes: 45,
            beds_occupied,
            beds_total,
            revenue_today_idr,
            revenue_transactions,
            unpaid_invoices_idr: unpaid_idr,
            unpaid_invoices_count: unpaid_count,
        })
    }

    /// Satu baris daily_stats (dipakai API bila perlu angka mentah).
    pub async fn daily_stat(
        &self,
        date: NaiveDate,
    ) -> Result<Option<waras_core::types::DailyStat>, WarasError> {
        let row: Option<DailyStatRow> =
            sqlx::query_as("SELECT * FROM daily_stats WHERE date = ? LIMIT 1")
                .bind(date.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clinic_today;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn dashboard_fase3_kpis_are_real() {
        let db = seeded_db().await;
        let k = db.dashboard_kpis(clinic_today()).await.unwrap();
        // Hitungan nyata dari tabel Fase 3 (FASE3.md §4.1) — bukan agregat
        // daily_stats (26/2/3). Perhatikan: §8 FASE3.md menulis 4 dan 2,
        // tapi baris seed §4.2 hanya menghasilkan 3 dan 1.
        assert_eq!(k.lab_pending, 3);
        assert_eq!(k.lab_critical, 1);
        assert_eq!(k.emergency_red, 1);
        assert_eq!(k.beds_occupied, 86);
        assert_eq!(k.beds_total, 120);
        // Yang tetap dari daily_stats tidak berubah.
        assert_eq!(k.patients_today, 248);
        assert_eq!(k.revenue_today_idr, 18_400_000);
        assert_eq!(k.visits_7d.len(), 7);
    }
}
