//! Data mentah untuk FHIR bundle satu encounter. API memanggil
//! `waras_core::fhir::encounter_bundle` dengan hasil ini.

use crate::Db;
use waras_core::{
    types::{Condition, Encounter, MedicationRequest, Observation, Patient},
    WarasError,
};

impl Db {
    /// (Patient, Encounter, vitals, conditions terikat encounter ini,
    /// resep encounter ini). `None` bila encounter tidak ditemukan.
    pub async fn encounter_bundle_data(
        &self,
        encounter_id: &str,
    ) -> Result<
        Option<(
            Patient,
            Encounter,
            Vec<Observation>,
            Vec<Condition>,
            Vec<MedicationRequest>,
        )>,
        WarasError,
    > {
        let Some(encounter) = self.get_encounter(encounter_id).await? else {
            return Ok(None);
        };
        let Some(patient) = self.get_patient(&encounter.patient_id).await? else {
            return Err(WarasError::Database(format!(
                "pasien {} untuk encounter {encounter_id} tidak ditemukan",
                encounter.patient_id
            )));
        };
        let vitals = self.list_vitals(encounter_id).await?;
        let conditions = self.list_conditions_for_encounter(encounter_id).await?;
        let prescriptions = self.list_prescriptions_for_encounter(encounter_id).await?;
        Ok(Some((patient, encounter, vitals, conditions, prescriptions)))
    }
}
