//! Helper FHIR R4 minimal: referensi resource dan serialisasi JSON untuk
//! panel "FHIR R4 — resource & relasi" di Clinical Workspace serta endpoint
//! `GET /api/encounters/:id/fhir`. Bukan validator FHIR lengkap (itu Fase 2,
//! validasi sandbox SATUSEHAT); tujuannya satu: model data WARAS sudah
//! berbentuk resource FHIR sejak skema pertama.

use crate::types::{
    AllergyIntolerance, Condition, ConditionStatus, Criticality, Encounter,
    MedicationRequest, Observation, Patient, VitalCode,
};
use chrono::NaiveDateTime;
use serde_json::{json, Value};

/// Referensi resource FHIR, mis. `Patient/812`, `Encounter/ENC-2026-11-19-014`.
pub fn reference(resource: &str, id: &str) -> String {
    format!("{resource}/{id}")
}

/// Format dateTime FHIR (`2026-11-19T09:15:00`).
/// `NaiveDateTime::to_string` memakai spasi (`2026-11-19 09:15:00`) dan
/// itu BUKAN dateTime FHIR yang valid.
pub fn fhir_datetime(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
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
/// Tanpa `identifier` (dipakai panel FHIR + bundle tampilan).
pub fn encounter_resource(e: &Encounter) -> Value {
    encounter_resource_with_org(e, "")
}

/// Encounter WARAS -> JSON resource FHIR Encounter (subset) dengan
/// `identifier` resmi SATUSEHAT
/// (`http://sys-ids.kemkes.go.id/encounter/{org_id}`) bila `org_id`
/// tidak kosong. Dipakai adapter SATUSEHAT saat mengirim.
pub fn encounter_resource_with_org(e: &Encounter, org_id: &str) -> Value {
    let mut resource = json!({
        "resourceType": "Encounter",
        "id": e.id,
        "status": match e.status {
            crate::types::EncounterStatus::Aktif => "in-progress",
            crate::types::EncounterStatus::DalamPemeriksaan => "in-progress",
            crate::types::EncounterStatus::Selesai => "finished",
            crate::types::EncounterStatus::Dibatalkan => "cancelled",
        },
        "class": { "system": "http://terminology.hl7.org/CodeSystem/v3-ActCode", "code": "AMB", "display": "ambulatory" },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&e.patient_id)) },
        "period": { "start": e.date.to_string() },
    });
    if !org_id.trim().is_empty() {
        resource["identifier"] = json!([{
            "system": format!("http://sys-ids.kemkes.go.id/encounter/{}", org_id.trim()),
            "value": e.id,
        }]);
    }
    resource
}

/// "130/85" -> (130.0, 85.0). Dipakai komponen tekanan darah.
fn parse_blood_pressure(value: &str) -> Option<(f64, f64)> {
    let (sys, dia) = value.split_once('/')?;
    let sys: f64 = sys.trim().parse().ok()?;
    let dia: f64 = dia.trim().parse().ok()?;
    Some((sys, dia))
}

/// Observation (tanda vital) -> JSON resource FHIR Observation (subset),
/// mengikuti profil vital-signs SATUSEHAT: `category` vital-signs, kode
/// LOINC (`http://loinc.org`), `valueQuantity` UCUM; tekanan darah
/// memakai panel 85354-9 dengan komponen sistolik 8480-6 + diastolik
/// 8462-4. Nilai yang tidak bisa diparse angka jatuh ke `valueString`.
pub fn observation_resource(o: &Observation) -> Value {
    let mut resource = json!({
        "resourceType": "Observation",
        "id": o.id,
        "status": "final",
        "category": [{ "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "vital-signs", "display": "Vital Signs" }] }],
        "code": {
            "coding": [{ "system": "http://loinc.org", "code": o.code.loinc_code(), "display": o.code.label() }],
            "text": o.code.label(),
        },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&o.patient_id)) },
        "encounter": { "reference": reference("Encounter", &o.encounter_id) },
        "effectiveDateTime": fhir_datetime(o.measured_at),
    });
    match o.code {
        VitalCode::TekananDarah => {
            if let Some((sys, dia)) = parse_blood_pressure(&o.value) {
                resource["component"] = json!([
                    { "code": { "coding": [{ "system": "http://loinc.org", "code": "8480-6", "display": "Systolic blood pressure" }] },
                      "valueQuantity": { "value": sys, "unit": o.unit, "system": "http://unitsofmeasure.org", "code": "mm[Hg]" } },
                    { "code": { "coding": [{ "system": "http://loinc.org", "code": "8462-4", "display": "Diastolic blood pressure" }] },
                      "valueQuantity": { "value": dia, "unit": o.unit, "system": "http://unitsofmeasure.org", "code": "mm[Hg]" } },
                ]);
            } else {
                resource["valueString"] = json!(format!("{} {}", o.value, o.unit));
            }
        }
        _ => {
            if let Ok(n) = o.value.trim().parse::<f64>() {
                resource["valueQuantity"] = json!({
                    "value": n,
                    "unit": o.unit,
                    "system": "http://unitsofmeasure.org",
                    "code": o.code.ucum_code(),
                });
            } else {
                resource["valueString"] = json!(format!("{} {}", o.value, o.unit));
            }
        }
    }
    resource
}

/// Condition -> JSON resource FHIR Condition (subset).
/// `clinicalStatus` memakai coding resmi condition-clinical
/// (`active`/`resolved`, bukan teks "Aktif"), plus
/// `verificationStatus` confirmed dan `category` encounter-diagnosis.
/// Kondisi kronis tetap `active` secara klinis.
pub fn condition_resource(c: &Condition) -> Value {
    let (clinical_code, clinical_display) = match c.status {
        ConditionStatus::Aktif | ConditionStatus::Kronis => ("active", "Aktif"),
        ConditionStatus::Selesai => ("resolved", "Selesai"),
    };
    json!({
        "resourceType": "Condition",
        "id": c.id,
        "clinicalStatus": { "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/condition-clinical", "code": clinical_code, "display": clinical_display }] },
        "verificationStatus": { "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/condition-verification", "code": "confirmed", "display": "Confirmed" }] },
        "category": [{ "coding": [{ "system": "http://terminology.hl7.org/CodeSystem/condition-category", "code": "encounter-diagnosis", "display": "Encounter Diagnosis" }] }],
        "code": { "coding": [{ "system": "http://hl7.org/fhir/sid/icd-10", "code": c.code, "display": c.display }] },
        "subject": { "reference": reference("Patient", &crate::ids::fhir_patient_numeric_id(&c.patient_id)) },
        "encounter": c.encounter_id.as_ref().map(|id| json!({ "reference": reference("Encounter", id) })),
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
        "intent": "order",
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
        "recordedDate": fhir_datetime(a.recorded_at),
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

    fn seed_encounter(patient: &Patient) -> Encounter {
        Encounter {
            id: "ENC-2026-11-19-014".to_string(),
            patient_id: patient.id.clone(),
            poli: "Poli Umum".to_string(),
            doctor_id: Some("U-001".to_string()),
            doctor_name: "dr. Ahmad Hidayat".to_string(),
            date: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
            time: "09.15".to_string(),
            status: EncounterStatus::DalamPemeriksaan,
            created_at: NaiveDateTime::default(),
        }
    }

    fn seed_observation(code: VitalCode, value: &str) -> Observation {
        Observation {
            id: "OBS-0001".to_string(),
            encounter_id: "ENC-2026-11-19-014".to_string(),
            patient_id: "RM-000812".to_string(),
            code,
            value: value.to_string(),
            unit: "mmHg".to_string(),
            measured_at: NaiveDate::from_ymd_opt(2026, 11, 19)
                .unwrap()
                .and_hms_opt(9, 15, 0)
                .unwrap(),
            measured_by: "Ns. Sari Wijaya".to_string(),
            previous_value: None,
            previous_date: None,
        }
    }

    #[test]
    fn encounter_class_punya_system_dan_identifier_org() {
        let patient = seed_patient();
        let encounter = seed_encounter(&patient);
        let r = encounter_resource(&encounter);
        assert_eq!(
            r["class"]["system"],
            "http://terminology.hl7.org/CodeSystem/v3-ActCode"
        );
        assert_eq!(r["class"]["code"], "AMB");
        assert!(r.get("identifier").is_none(), "tanpa org id, tanpa identifier");
        let r2 = encounter_resource_with_org(&encounter, "10000001");
        assert_eq!(
            r2["identifier"][0]["system"],
            "http://sys-ids.kemkes.go.id/encounter/10000001"
        );
        assert_eq!(r2["identifier"][0]["value"], "ENC-2026-11-19-014");
    }

    #[test]
    fn observation_vital_signs_loinc_dan_value_quantity() {
        let mut o = seed_observation(VitalCode::Nadi, "80");
        o.unit = "x/mnt".to_string();
        let r = observation_resource(&o);
        assert_eq!(r["category"][0]["coding"][0]["code"], "vital-signs");
        assert_eq!(
            r["category"][0]["coding"][0]["system"],
            "http://terminology.hl7.org/CodeSystem/observation-category"
        );
        assert_eq!(r["code"]["coding"][0]["system"], "http://loinc.org");
        assert_eq!(r["code"]["coding"][0]["code"], "8867-4");
        assert_eq!(r["valueQuantity"]["value"], 80.0);
        assert_eq!(r["valueQuantity"]["code"], "/min");
        assert_eq!(r["valueQuantity"]["system"], "http://unitsofmeasure.org");
        assert_eq!(r["effectiveDateTime"], "2026-11-19T09:15:00");
        assert!(r.get("valueString").is_none());
    }

    #[test]
    fn observation_tekanan_darah_jadi_komponen_sistolik_diastolik() {
        let o = seed_observation(VitalCode::TekananDarah, "130/85");
        let r = observation_resource(&o);
        assert_eq!(r["code"]["coding"][0]["code"], "85354-9");
        assert!(r.get("valueQuantity").is_none(), "panel BP tanpa valueQuantity");
        let comp = r["component"].as_array().unwrap();
        assert_eq!(comp.len(), 2);
        assert_eq!(comp[0]["code"]["coding"][0]["code"], "8480-6");
        assert_eq!(comp[0]["valueQuantity"]["value"], 130.0);
        assert_eq!(comp[0]["valueQuantity"]["code"], "mm[Hg]");
        assert_eq!(comp[1]["code"]["coding"][0]["code"], "8462-4");
        assert_eq!(comp[1]["valueQuantity"]["value"], 85.0);
    }

    #[test]
    fn observation_nilai_tidak_angka_jatuh_ke_value_string() {
        let o = seed_observation(VitalCode::SpO2, "baik");
        let r = observation_resource(&o);
        assert_eq!(r["code"]["coding"][0]["code"], "59408-5");
        assert_eq!(r["valueString"], "baik mmHg");
        assert!(r.get("valueQuantity").is_none());
    }

    #[test]
    fn condition_status_memakai_coding_resmi() {
        let mut c = Condition {
            id: "COND-0001".to_string(),
            patient_id: "RM-000812".to_string(),
            encounter_id: Some("ENC-2026-11-19-014".to_string()),
            group: ConditionGroup::Utama,
            code: "J20.9".to_string(),
            display: "Bronkitis akut".to_string(),
            status: ConditionStatus::Aktif,
            clinician: "dr. Ahmad Hidayat".to_string(),
            recorded_at: NaiveDateTime::default(),
        };
        let r = condition_resource(&c);
        assert_eq!(
            r["clinicalStatus"]["coding"][0]["system"],
            "http://terminology.hl7.org/CodeSystem/condition-clinical"
        );
        assert_eq!(r["clinicalStatus"]["coding"][0]["code"], "active");
        assert_eq!(r["verificationStatus"]["coding"][0]["code"], "confirmed");
        assert_eq!(r["category"][0]["coding"][0]["code"], "encounter-diagnosis");
        c.status = ConditionStatus::Selesai;
        assert_eq!(
            condition_resource(&c)["clinicalStatus"]["coding"][0]["code"],
            "resolved"
        );
        c.status = ConditionStatus::Kronis;
        assert_eq!(
            condition_resource(&c)["clinicalStatus"]["coding"][0]["code"],
            "active"
        );
    }

    #[test]
    fn medication_request_punya_intent_order() {
        let rx = MedicationRequest {
            id: "RX-1041".to_string(),
            encounter_id: "ENC-2026-11-19-014".to_string(),
            patient_id: "RM-000812".to_string(),
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
        let r = medication_request_resource(&rx);
        assert_eq!(r["intent"], "order");
    }

    #[test]
    fn recorded_date_format_fhir_datetime() {
        let mut a = seed_allergy();
        a.recorded_at = NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(9, 15, 0)
            .unwrap();
        let r = allergy_intolerance_resource(&a);
        assert_eq!(r["recordedDate"], "2026-11-19T09:15:00");
    }
}
