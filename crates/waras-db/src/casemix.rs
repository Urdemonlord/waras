//! Casemix / INA-CBG (FASE3.md §4): daftar klaim, grouping lokal per
//! encounter, KPI workbench. Grouping memakai
//! `waras_core::casemix::group_casemix` (tarif CONTOH lokal, bukan
//! grouper resmi Kemenkes).

use crate::rows::clinic_now_string;
use crate::{db_err, parse_dt, Db};
use serde_json::json;
use waras_core::{casemix::group_casemix, types::Claim, WarasError};

impl Db {
    /// Semua klaim, terbaru dulu (sama dengan `list_claims`).
    pub async fn list_casemix(&self) -> Result<Vec<Claim>, WarasError> {
        self.list_claims().await
    }

    /// Grouping lokal untuk satu encounter: diagnosis utama encounter →
    /// `group_casemix`. Bila diagnosis belum terpetakan → 422
    /// (`Unprocessable`, "belum terpetakan, perbaiki pengkodean").
    /// Buat klaim Draft baru (id `CLM-5518` dst) atau perbarui Draft
    /// yang sudah ada untuk encounter itu; tautkan invoice pasien bila
    /// ada (invoice encounter dulu, bila tidak ada invoice terbaru
    /// pasien); `grouping_note` =
    /// `"Grouping lokal (perkiraan): {diag} -> {code}"`.
    pub async fn group_claim_for_encounter(
        &self,
        encounter_id: &str,
        by: &str,
    ) -> Result<Claim, WarasError> {
        let encounter = self
            .get_encounter(encounter_id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("encounter {encounter_id} tidak ditemukan")))?;
        // Diagnosis utama encounter (grup Utama, tercatat paling awal).
        let primary: Option<(String, String)> = sqlx::query_as(
            "SELECT code, display FROM conditions
             WHERE encounter_id = ? AND condition_group = 'utama'
             ORDER BY recorded_at ASC, id ASC LIMIT 1",
        )
        .bind(encounter_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let (diag_code, _display) = match primary {
            Some(p) => p,
            None => {
                return Err(WarasError::Unprocessable(format!(
                    "encounter {encounter_id} belum punya diagnosis utama"
                )))
            }
        };
        // Kode prosedur (tindakan medis encounter, bila ada).
        let procedure: Option<String> = sqlx::query_scalar(
            "SELECT code FROM conditions
             WHERE encounter_id = ? AND condition_group = 'tindakan_medis'
             ORDER BY recorded_at ASC, id ASC LIMIT 1",
        )
        .bind(encounter_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let group = match group_casemix(&diag_code, procedure.as_deref()) {
            Some(g) => g,
            None => {
                return Err(WarasError::Unprocessable(format!(
                    "diagnosis {diag_code} belum terpetakan ke grouping INA-CBG, perbaiki pengkodean"
                )))
            }
        };
        // Invoice: milik encounter dulu, bila tidak ada invoice terbaru pasien.
        let invoice_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM invoices WHERE encounter_id = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(encounter_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let invoice_id = match invoice_id {
            Some(id) => Some(id),
            None => {
                sqlx::query_scalar(
                    "SELECT id FROM invoices WHERE patient_id = ? ORDER BY created_at DESC LIMIT 1",
                )
                .bind(&encounter.patient_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        let sep_id: Option<String> = match &invoice_id {
            Some(inv) => {
                sqlx::query_scalar("SELECT sep_id FROM invoices WHERE id = ? LIMIT 1")
                    .bind(inv)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(db_err)?
            }
            None => None,
        };
        let note = format!(
            "Grouping lokal (perkiraan): {diag_code} -> {}",
            group.code
        );
        // Draft yang sudah ada untuk encounter ini (klaim → invoice → encounter).
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT claims.id FROM claims
             JOIN invoices ON claims.invoice_id = invoices.id
             WHERE invoices.encounter_id = ? AND claims.status = 'draft'
             ORDER BY claims.created_at DESC LIMIT 1",
        )
        .bind(encounter_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        if let Some(id) = existing {
            sqlx::query(
                "UPDATE claims SET ina_cbg_code = ?, amount = ?, grouping_note = ?,
                     invoice_id = COALESCE(?, invoice_id), sep_id = COALESCE(?, sep_id)
                 WHERE id = ?",
            )
            .bind(group.code)
            .bind(group.tariff_idr)
            .bind(&note)
            .bind(&invoice_id)
            .bind(&sep_id)
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
            let claim = self
                .get_claim(&id)
                .await?
                .ok_or_else(|| WarasError::Database("klaim tidak ditemukan setelah grouping".into()))?;
            self.log_audit(
                by,
                "casemix.group",
                "claim",
                &claim.id,
                json!({"encounter_id": encounter_id, "code": group.code}),
            )
            .await?;
            return Ok(claim);
        }
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM claims")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 5518);
        let id = format!("CLM-{seq}");
        let claim = Claim {
            id,
            invoice_id,
            patient_id: Some(encounter.patient_id.clone()),
            sep_id,
            ina_cbg_code: group.code.to_string(),
            amount: group.tariff_idr,
            status: waras_core::types::ClaimStatus::Draft,
            submitted_at: None,
            created_at: parse_dt(&clinic_now_string())?,
            grouping_note: Some(note),
        };
        let claim = self.create_claim(&claim).await?;
        self.log_audit(
            by,
            "casemix.group",
            "claim",
            &claim.id,
            json!({"encounter_id": encounter_id, "code": group.code}),
        )
        .await?;
        Ok(claim)
    }

    /// (total klaim, terverifikasi, dibayar_rp, pending/revisi).
    /// Pending/revisi = status `pending` + `ditolak` (butuh perbaikan).
    pub async fn casemix_kpis(&self) -> Result<(i64, i64, i64, i64), WarasError> {
        let row: (i64, i64, Option<i64>, i64) = sqlx::query_as(
            "SELECT COUNT(*),
               COALESCE(SUM(CASE WHEN status = 'terverifikasi' THEN 1 ELSE 0 END), 0),
               CAST(COALESCE(SUM(CASE WHEN status = 'dibayar' THEN amount ELSE 0 END), 0) AS BIGINT),
               COALESCE(SUM(CASE WHEN status IN ('pending', 'ditolak') THEN 1 ELSE 0 END), 0)
             FROM claims",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok((row.0, row.1, row.2.unwrap_or(0), row.3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::types::ClaimStatus;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_casemix_counts_and_kpis() {
        let db = seeded_db().await;
        let claims = db.list_casemix().await.unwrap();
        // 8 seed Fase 3 + 1 Fase 2.
        assert_eq!(claims.len(), 9);
        let c5510 = db.get_claim("CLM-5510").await.unwrap().unwrap();
        assert_eq!(c5510.invoice_id, None);
        assert_eq!(c5510.ina_cbg_code, "I-4-13-I");
        assert_eq!(c5510.amount, 4_210_000);
        assert_eq!(c5510.status, ClaimStatus::Dibayar);
        assert!(c5510
            .grouping_note
            .as_deref()
            .unwrap()
            .contains("I20.9 Angina pektoris"));
        let c5513 = db.get_claim("CLM-5513").await.unwrap().unwrap();
        assert_eq!(c5513.status, ClaimStatus::Pending);
        assert!(c5513.grouping_note.as_deref().unwrap().contains("79.32"));
        // Klaim Fase 2 tetap menaut invoice.
        let old = db.get_claim("CLM-20261119-001").await.unwrap().unwrap();
        assert_eq!(old.invoice_id.as_deref(), Some("INV-2026-1181"));
        assert_eq!(old.grouping_note, None);
        // KPI: total 9, terverifikasi 1, dibayar 4.420.000, pending 1.
        assert_eq!(db.casemix_kpis().await.unwrap(), (9, 1, 4_420_000, 1));
    }

    #[tokio::test]
    async fn group_claim_creates_and_updates_draft() {
        let db = seeded_db().await;
        // ENC-...-016 (Dewi) punya diagnosis utama J18.9 dari seed Fase 3
        // (COND-011) -> J-4-12-I.
        // (Diagnosis utama seed ENC-...-014 adalah J20.9 Bronkitis — TIDAK
        // ada di tabel grouper §2, jadi sengaja tidak dipakai di sini;
        // lihat group_claim_unmapped_diagnosis_is_422.)
        let c = db
            .group_claim_for_encounter("ENC-2026-11-19-016", "Sinta Maharani")
            .await
            .unwrap();
        assert_eq!(c.id, "CLM-5518");
        assert_eq!(c.ina_cbg_code, "J-4-12-I");
        assert_eq!(c.amount, 3_150_000);
        assert_eq!(c.status, ClaimStatus::Draft);
        assert_eq!(
            c.grouping_note.as_deref(),
            Some("Grouping lokal (perkiraan): J18.9 -> J-4-12-I")
        );
        assert_eq!(c.invoice_id.as_deref(), Some("INV-2026-1183"));
        assert_eq!(c.sep_id.as_deref(), Some("SEP-20261119-004"));
        assert_eq!(db.list_casemix().await.unwrap().len(), 10);
        // Grouping ulang: draft yang sama diperbarui, bukan baris baru.
        let c2 = db
            .group_claim_for_encounter("ENC-2026-11-19-016", "Sinta Maharani")
            .await
            .unwrap();
        assert_eq!(c2.id, c.id);
        assert_eq!(db.list_casemix().await.unwrap().len(), 10);
    }

    #[tokio::test]
    async fn group_claim_new_draft_and_unmapped() {
        let db = seeded_db().await;
        // ENC-...-015 (Budi): hanya kondisi Riwayat, tanpa diagnosis Utama.
        let err = db
            .group_claim_for_encounter("ENC-2026-11-19-015", "Sinta Maharani")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        assert!(matches!(
            db.group_claim_for_encounter("ENC-2026-11-19-999", "x").await,
            Err(WarasError::NotFound(_))
        ));
        // Encounter baru dengan diagnosis Utama J03.9 -> klaim CLM-5518.
        // ENC-...-022 milik Sri (RM-000817); Sri tidak punya tagihan,
        // jadi klaim baru tanpa invoice.
        sqlx::query(
            "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
                 code, display, status, clinician, recorded_at)
             VALUES ('COND-T1', 'RM-000817', 'ENC-2026-11-19-022', 'utama',
                 'J03.9', 'Tonsilitis', 'aktif', 'dr. Sari Dewi', '2026-11-19 08:15:00')",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let c = db
            .group_claim_for_encounter("ENC-2026-11-19-022", "Sinta Maharani")
            .await
            .unwrap();
        assert_eq!(c.id, "CLM-5518");
        assert_eq!(c.ina_cbg_code, "J-4-10-0");
        assert_eq!(c.amount, 185_000);
        assert_eq!(c.status, ClaimStatus::Draft);
        assert_eq!(c.invoice_id, None);
    }

    #[tokio::test]
    async fn group_claim_unmapped_diagnosis_is_422() {
        let db = seeded_db().await;
        // J20.9 (Bronkitis akut, diagnosis utama kanonik ENC-...-014)
        // TIDAK ada di tabel grouper §2 (yang ada I20.9 Angina) -> 422.
        let err = db
            .group_claim_for_encounter("ENC-2026-11-19-014", "Sinta Maharani")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        sqlx::query(
            "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
                 code, display, status, clinician, recorded_at)
             VALUES ('COND-T2', 'RM-000813', 'ENC-2026-11-19-015', 'utama',
                 'Z99.9', 'Tidak dikenal', 'aktif', 'dr. Sari Dewi', '2026-11-19 10:15:00')",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let err = db
            .group_claim_for_encounter("ENC-2026-11-19-015", "Sinta Maharani")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
    }
}
