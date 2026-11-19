//! Resep (e-resep) — penegakan aturan keamanan obat ada di sini.

use crate::convert::*;
use crate::rows::PrescriptionRow;
use crate::{db_err, fmt_dt, now_naive, Db, PrescriptionFilter};
use serde_json::json;
use waras_core::{
    ids,
    safety::check_prescription,
    types::{MedicationRequest, NewPrescription, PrescriptionStatus},
    WarasError,
};

impl Db {
    /// Resepkan obat. WAJIB lewat `safety::check_prescription` (alergi
    /// pasien dimuat dulu):
    /// - diblokir -> resep TETAP disimpan (status Diblokir +
    ///   `blocked_reason`) + audit `prescription.blocked`, lalu
    ///   `Err(WarasError::Blocked(...))`;
    /// - lolos -> status Aktif (default) + audit `prescription.created`.
    /// `actor` adalah nama dokter peresep (kolom `doctor`).
    pub async fn prescribe(
        &self,
        actor: &str,
        input: NewPrescription,
    ) -> Result<MedicationRequest, WarasError> {
        self.prescribe_as(actor, input, PrescriptionStatus::Aktif)
            .await
    }

    /// Seperti `prescribe`, tapi status awal bisa diminta
    /// (mis. MenungguKonfirmasi untuk resep pengganti). Default pemanggil
    /// biasa tetap Aktif lewat `prescribe`.
    pub async fn prescribe_as(
        &self,
        actor: &str,
        input: NewPrescription,
        requested: PrescriptionStatus,
    ) -> Result<MedicationRequest, WarasError> {
        let encounter = self
            .get_encounter(&input.encounter_id)
            .await?
            .ok_or_else(|| {
                WarasError::NotFound(format!("encounter {} tidak ditemukan", input.encounter_id))
            })?;
        let patient_id = encounter.patient_id.clone();
        let allergies = self.get_allergies(&patient_id).await?;

        // Kanonik: dosage hanya frekuensi ("1x1"); kekuatan hidup di
        // drug_name. Tanpa ini, tampilan mengulang kekuatan
        // ("Amoxicillin 500 mg 500 mg 1x1").
        let dosage = waras_core::dosage::normalize_dosage(&input.drug_name, &input.dosage);

        let ids_all: Vec<String> = sqlx::query_scalar("SELECT id FROM medication_requests")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids_all, 1000);
        let id = ids::prescription_id(seq);
        let created = fmt_dt(now_naive());

        match check_prescription(&allergies, &input.drug_name, &input.drug_class) {
            Err(blocked) => {
                let reason = blocked.message.clone();
                sqlx::query(
                    "INSERT INTO medication_requests (id, encounter_id, patient_id, drug_name,
                         drug_class, dosage, duration, status, doctor, blocked_reason,
                         replaced_by, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, 'diblokir', ?, ?, NULL, ?)",
                )
                .bind(&id)
                .bind(&input.encounter_id)
                .bind(&patient_id)
                .bind(&input.drug_name)
                .bind(&input.drug_class)
                .bind(&dosage)
                .bind(&input.duration)
                .bind(actor)
                .bind(&reason)
                .bind(&created)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                self.log_audit(
                    actor,
                    "prescription.blocked",
                    "medication_request",
                    &id,
                    json!({"drug": input.drug_name, "reason": reason, "patient_id": patient_id}),
                )
                .await?;
                Err(WarasError::Blocked(blocked))
            }
            Ok(()) => {
                let status = match requested {
                    PrescriptionStatus::MenungguKonfirmasi => PrescriptionStatus::MenungguKonfirmasi,
                    _ => PrescriptionStatus::Aktif,
                };
                sqlx::query(
                    "INSERT INTO medication_requests (id, encounter_id, patient_id, drug_name,
                         drug_class, dosage, duration, status, doctor, blocked_reason,
                         replaced_by, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)",
                )
                .bind(&id)
                .bind(&input.encounter_id)
                .bind(&patient_id)
                .bind(&input.drug_name)
                .bind(&input.drug_class)
                .bind(&dosage)
                .bind(&input.duration)
                .bind(prescription_status_to_str(status))
                .bind(actor)
                .bind(&created)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                self.log_audit(
                    actor,
                    "prescription.created",
                    "medication_request",
                    &id,
                    json!({"drug": input.drug_name, "patient_id": patient_id}),
                )
                .await?;
                self.get_prescription(&id)
                    .await?
                    .ok_or_else(|| WarasError::Database("resep baru tidak ditemukan".into()))
            }
        }
    }

    /// MenungguKonfirmasi -> Aktif. Status lain ditolak (Validation).
    pub async fn confirm_prescription(
        &self,
        id: &str,
    ) -> Result<MedicationRequest, WarasError> {
        let rx = self
            .get_prescription(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("resep {id} tidak ditemukan")))?;
        if rx.status != PrescriptionStatus::MenungguKonfirmasi {
            return Err(WarasError::Validation(format!(
                "resep {id} berstatus {:?}, bukan MenungguKonfirmasi",
                rx.status
            )));
        }
        sqlx::query("UPDATE medication_requests SET status = 'aktif' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        self.log_audit(
            &rx.doctor,
            "prescription.confirmed",
            "medication_request",
            id,
            json!({"drug": rx.drug_name}),
        )
        .await?;
        self.get_prescription(id)
            .await?
            .ok_or_else(|| WarasError::Database("resep hilang setelah konfirmasi".into()))
    }

    /// Serahkan obat: Aktif -> Diberikan, stok obat berkurang sesuai
    /// jumlah tablet (`dose_count` dari dosage + duration). Bila obat
    /// tidak ada di tabel drugs, resep tetap diserahkan tanpa pengurangan
    /// stok. Audit `prescription.dispensed`.
    pub async fn dispense_prescription(
        &self,
        id: &str,
        actor: &str,
    ) -> Result<MedicationRequest, WarasError> {
        let rx = self
            .get_prescription(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("resep {id} tidak ditemukan")))?;
        if rx.status != PrescriptionStatus::Aktif {
            return Err(WarasError::Validation(format!(
                "resep {id} berstatus {:?}, hanya resep Aktif yang bisa diserahkan",
                rx.status
            )));
        }
        let count = Self::dose_count(&rx.dosage, &rx.duration);
        if let Some(drug) = self.get_drug_by_name(&rx.drug_name).await? {
            let new_stock = (drug.stock - count).max(0);
            sqlx::query("UPDATE drugs SET stock = ? WHERE kfa_code = ?")
                .bind(new_stock)
                .bind(&drug.kfa_code)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
        }
        sqlx::query("UPDATE medication_requests SET status = 'diberikan' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        self.log_audit(
            actor,
            "prescription.dispensed",
            "medication_request",
            id,
            json!({"drug": rx.drug_name, "tablets": count, "patient_id": rx.patient_id}),
        )
        .await?;
        self.get_prescription(id)
            .await?
            .ok_or_else(|| WarasError::Database("resep hilang setelah dispensing".into()))
    }

    pub async fn list_prescriptions(
        &self,
        filter: PrescriptionFilter,
    ) -> Result<Vec<MedicationRequest>, WarasError> {
        let rows: Vec<PrescriptionRow> = match (&filter.status, &filter.patient_id) {
            (Some(status), Some(pid)) => {
                sqlx::query_as(
                    "SELECT * FROM medication_requests WHERE status = ? AND patient_id = ?
                     ORDER BY created_at DESC, id DESC",
                )
                .bind(prescription_status_to_str(*status))
                .bind(pid)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (Some(status), None) => {
                sqlx::query_as(
                    "SELECT * FROM medication_requests WHERE status = ?
                     ORDER BY created_at DESC, id DESC",
                )
                .bind(prescription_status_to_str(*status))
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (None, Some(pid)) => {
                sqlx::query_as(
                    "SELECT * FROM medication_requests WHERE patient_id = ?
                     ORDER BY created_at DESC, id DESC",
                )
                .bind(pid)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            (None, None) => {
                sqlx::query_as(
                    "SELECT * FROM medication_requests ORDER BY created_at DESC, id DESC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_prescription(
        &self,
        id: &str,
    ) -> Result<Option<MedicationRequest>, WarasError> {
        let row: Option<PrescriptionRow> =
            sqlx::query_as("SELECT * FROM medication_requests WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Resep-resep satu encounter (untuk detail encounter / FHIR bundle).
    pub async fn list_prescriptions_for_encounter(
        &self,
        encounter_id: &str,
    ) -> Result<Vec<MedicationRequest>, WarasError> {
        let rows: Vec<PrescriptionRow> = sqlx::query_as(
            "SELECT * FROM medication_requests WHERE encounter_id = ?
             ORDER BY created_at ASC, id ASC",
        )
        .bind(encounter_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }
}
