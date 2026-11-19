//! Helper FHIR R4 minimal: referensi resource dan serialisasi JSON untuk
//! panel "FHIR R4 — resource & relasi" di Clinical Workspace serta endpoint
//! `GET /api/encounters/:id/fhir`. Bukan validator FHIR lengkap (itu Fase 2,
//! validasi sandbox SATUSEHAT); tujuannya satu: model data WARAS sudah
//! berbentuk resource FHIR sejak skema pertama.

use crate::types::{Condition, Encounter, MedicationRequest, Observation, Patient};
use serde_json::{json, Value};

/// Referensi resource FHIR, mis. `Patient/812`, `Encounter/ENC-2026-11-19-014`.
pub fn reference(resource: &str, id: &str) -> String {
    format!("{resource}/{id}")
}

/// Patient WARAS -> JSON resource FHIR Patient (subset field yang dipakai).
pub fn patient_resource(p: &Patient) -> Value {
    json!({
        "resourceType": "Patient",
        "id": crate::ids::fhir_patient_numeric_id(&p.id),
        "identifier": [
            { "system": "https://fhir.kemkes.go.id/id/nik", "value": p.nik },
            { "system": "https://waras.meowlabs.id/id/rm", "value": p.id }
        ],
        "name": [{ "text": p.name }],
        "gender": match p.gender {
            crate::types::Gender::LakiLaki => "male",
            crate::types::Gender::Perempuan => "female",
        },
        "birthDate": p.birth_date.to_string(),
        "address": [{ "text": p.address }],
    })
}

/// Encounter WARAS -> JSON resource FHIR Encounter (subset).
pub fn encounter_resource(e: &Encounter) -> Value {
    json!({
        "resourceType": "Encounter",
        "id": e.id,
        "status": match e.status {
            crate::types::EncounterStatus::Aktif => "in-progress",
            crate::types::EncounterStatus::DalamPemeriksaan => "in-progress",
            crate::types::EncounterStatus::Selesai => "finished",
            crate::types::EncounterStatus::Dibatalkan => "cancelled",
        },
        "class": { "code": "AMB", "display": "ambulatory" },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&e.patient_id)) },
        "period": { "start": e.date.to_string() },
    })
}

/// Observation (tanda vital) -> JSON resource FHIR Observation (subset).
pub fn observation_resource(o: &Observation) -> Value {
    json!({
        "resourceType": "Observation",
        "id": o.id,
        "status": "final",
        "code": { "text": o.code.label() },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&o.patient_id)) },
        "encounter": { "reference": reference("Encounter", &o.encounter_id) },
        "effectiveDateTime": o.measured_at.to_string(),
        "valueString": format!("{} {}", o.value, o.unit),
    })
}

/// Condition -> JSON resource FHIR Condition (subset).
pub fn condition_resource(c: &Condition) -> Value {
    json!({
        "resourceType": "Condition",
        "id": c.id,
        "code": { "coding": [{ "system": "http://hl7.org/fhir/sid/icd-10", "code": c.code, "display": c.display }] },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&c.patient_id)) },
        "encounter": c.encounter_id.as_ref().map(|id| json!({ "reference": reference("Encounter", id) })),
        "clinicalStatus": { "text": format!("{:?}", c.status) },
    })
}

/// MedicationRequest -> JSON resource FHIR MedicationRequest (subset).
pub fn medication_request_resource(m: &MedicationRequest) -> Value {
    json!({
        "resourceType": "MedicationRequest",
        "id": m.id,
        "status": match m.status {
            crate::types::PrescriptionStatus::Aktif => "active",
            crate::types::PrescriptionStatus::MenungguKonfirmasi => "draft",
            crate::types::PrescriptionStatus::Diblokir => "cancelled",
            crate::types::PrescriptionStatus::Diberikan => "completed",
            crate::types::PrescriptionStatus::Dibatalkan => "cancelled",
        },
        "medicationCodeableConcept": { "text": m.drug_name },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&m.patient_id)) },
        "encounter": { "reference": reference("Encounter", &m.encounter_id) },
        "dosageInstruction": [{ "text": format!("{} · {}", m.dosage, m.duration) }],
    })
}

/// Bundle relasi satu encounter: Patient -> Encounter -> Observation ·
/// Condition · MedicationRequest. Dipakai panel FHIR di UI.
pub fn encounter_bundle(
    patient: &Patient,
    encounter: &Encounter,
    observations: &[Observation],
    conditions: &[Condition],
    prescriptions: &[MedicationRequest],
) -> Value {
    let mut entries: Vec<Value> = vec![
        json!({ "resource": patient_resource(patient) }),
        json!({ "resource": encounter_resource(encounter) }),
    ];
    for o in observations {
        entries.push(json!({ "resource": observation_resource(o) }));
    }
    for c in conditions {
        entries.push(json!({ "resource": condition_resource(c) }));
    }
    for m in prescriptions {
        entries.push(json!({ "resource": medication_request_resource(m) }));
    }
    json!({
        "resourceType": "Bundle",
        "type": "collection",
        "entry": entries,
    })
}
