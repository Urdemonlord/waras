//! Helper FHIR R4 minimal: referensi resource dan serialisasi JSON untuk
//! panel "FHIR R4 — resource & relasi" di Clinical Workspace serta endpoint
//! `GET /api/encounters/:id/fhir`. Bukan validator FHIR lengkap (itu Fase 2,
//! validasi sandbox SATUSEHAT); tujuannya satu: model data WARAS sudah
//! berbentuk resource FHIR sejak skema pertama.

use crate::types::{
    AllergyIntolerance, Condition, Criticality, Encounter, MedicationRequest, Observation,
    Patient,
};
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

/// AllergyIntolerance -> JSON resource FHIR AllergyIntolerance (subset).
/// `criticality` FHIR hanya mengenal `low`/`high`/`unable-to-assess`:
/// `Tinggi` -> `high`, `Sedang`/`Rendah` -> `low`.
pub fn allergy_intolerance_resource(a: &AllergyIntolerance) -> Value {
    let mut code = json!({ "text": a.substance });
    if let Some(class) = &a.substance_class {
        code["coding"] = json!([{ "system": "https://waras.meowlabs.id/id/drug-class", "code": class }]);
    }
    let mut resource = json!({
        "resourceType": "AllergyIntolerance",
        "id": a.id,
        "clinicalStatus": { "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical", "code": "active" }] },
        "verificationStatus": { "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/allergyintolerance-verification", "code": "confirmed" }] },
        "criticality": match a.criticality {
            Criticality::Tinggi => "high",
            Criticality::Sedang | Criticality::Rendah => "low",
        },
        "code": code,
        "patient": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&a.patient_id)) },
        "recordedDate": a.recorded_at.to_string(),
    });
    if let Some(note) = &a.note {
        resource["note"] = json!([{ "text": note }]);
    }
    resource
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ConditionGroup, ConditionStatus, EncounterStatus, Gender, Insurance,
        PrescriptionStatus, VitalCode,
    };
    use chrono::{NaiveDate, NaiveDateTime};

    fn seed_patient() -> Patient {
        Patient {
            id: "RM-000812".to_string(),
            nik: "3374021205850001".to_string(),
            name: "Siti Rahmawati".to_string(),
            birth_date: NaiveDate::from_ymd_opt(1985, 5, 12).unwrap(),
            gender: Gender::Perempuan,
            address: "Jl. Pandanaran No. 12, Semarang".to_string(),
            phone: None,
            blood_type: Some("O+".to_string()),
            bpjs_number: Some("0001234567890".to_string()),
            insurance: Insurance::Bpjs,
            created_at: NaiveDateTime::default(),
        }
    }

    fn seed_allergy() -> AllergyIntolerance {
        AllergyIntolerance {
            id: "ALG-000812-01".to_string(),
            patient_id: "RM-000812".to_string(),
            substance: "Penisilin".to_string(),
            substance_class: Some("penicillin".to_string()),
            criticality: Criticality::Tinggi,
            note: Some("Ruam + sesak".to_string()),
            recorded_at: NaiveDateTime::default(),
        }
    }

    #[test]
    fn allergy_intolerance_resource_memetakan_substance_dan_kritis() {
        let r = allergy_intolerance_resource(&seed_allergy());
        assert_eq!(r["resourceType"], "AllergyIntolerance");
        assert_eq!(r["id"], "ALG-000812-01");
        assert_eq!(r["code"]["text"], "Penisilin");
        assert_eq!(r["code"]["coding"][0]["code"], "penicillin");
        assert_eq!(r["criticality"], "high");
        assert_eq!(r["patient"]["reference"], "Patient/812");
        assert_eq!(r["note"][0]["text"], "Ruam + sesak");
    }

    #[test]
    fn allergy_intolerance_criticality_rendah_jadi_low_tanpa_coding() {
        let mut a = seed_allergy();
        a.criticality = Criticality::Rendah;
        a.substance_class = None;
        a.note = None;
        let r = allergy_intolerance_resource(&a);
        assert_eq!(r["criticality"], "low");
        assert!(r["code"].get("coding").is_none());
        assert!(r.get("note").is_none());
    }

    #[test]
    fn encounter_bundle_memuat_enam_jenis_resource() {
        let patient = seed_patient();
        let encounter = Encounter {
            id: "ENC-2026-11-19-014".to_string(),
            patient_id: patient.id.clone(),
            poli: "Poli Umum".to_string(),
            doctor_id: Some("U-001".to_string()),
            doctor_name: "dr. Ahmad Hidayat".to_string(),
            date: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
            time: "09.15".to_string(),
            status: EncounterStatus::DalamPemeriksaan,
            created_at: NaiveDateTime::default(),
        };
        let obs = Observation {
            id: "OBS-0001".to_string(),
            encounter_id: encounter.id.clone(),
            patient_id: patient.id.clone(),
            code: VitalCode::TekananDarah,
            value: "130/85".to_string(),
            unit: "mmHg".to_string(),
            measured_at: NaiveDateTime::default(),
            measured_by: "Ns. Sari Wijaya".to_string(),
            previous_value: Some("128/82".to_string()),
            previous_date: Some(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()),
        };
        let cond = Condition {
            id: "COND-0001".to_string(),
            patient_id: patient.id.clone(),
            encounter_id: Some(encounter.id.clone()),
            group: ConditionGroup::Utama,
            code: "J20.9".to_string(),
            display: "Bronkitis akut".to_string(),
            status: ConditionStatus::Aktif,
            clinician: "dr. Ahmad Hidayat".to_string(),
            recorded_at: NaiveDateTime::default(),
        };
        let rx = MedicationRequest {
            id: "RX-1041".to_string(),
            encounter_id: encounter.id.clone(),
            patient_id: patient.id.clone(),
            drug_name: "Azithromycin 500 mg".to_string(),
            drug_class: "macrolide".to_string(),
            dosage: "1x1".to_string(),
            duration: "3 hari".to_string(),
            status: PrescriptionStatus::MenungguKonfirmasi,
            doctor: "dr. Ahmad Hidayat".to_string(),
            blocked_reason: None,
            replaced_by: None,
            created_at: NaiveDateTime::default(),
        };
        let bundle = encounter_bundle(&patient, &encounter, &[obs], &[cond], &[rx]);
        assert_eq!(bundle["resourceType"], "Bundle");
        let types: Vec<&str> = bundle["entry"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["resource"]["resourceType"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            ["Patient", "Encounter", "Observation", "Condition", "MedicationRequest"]
        );
        // Alergi dikirim sebagai resource terpisah (bukan bagian bundle encounter).
        let allergy = allergy_intolerance_resource(&seed_allergy());
        assert_eq!(allergy["resourceType"], "AllergyIntolerance");
    }
}
