//! Aturan keamanan obat: resep DIBLOKIR bila pasien punya alergi terhadap
//! kelas obat yang diresepkan. Aturan ini murni (tidak menyentuh I/O) supaya
//! bisa diuji tanpa basis data, dan ditegakkan di lapisan API maupun di
//! layanan resep (`waras-db`/`waras-api`).
//!
//! Kasus kanonik dari desain: Siti Rahmawati alergi Penisilin.
//! Amoxicillin 500 mg (kelas penicillin) -> DIBLOKIR.
//! Azithromycin 500 mg (kelas macrolide) -> LULUS.

use crate::types::AllergyIntolerance;
use std::fmt;

/// Penolakan resep. Display-nya dipakai sebagai pesan alert merah di UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MedicationBlocked {
    /// Zat alergi pasien, mis. `Penisilin`.
    pub substance: String,
    /// Obat yang coba diresepkan, mis. `Amoxicillin 500 mg`.
    pub drug: String,
    /// Pesan lengkap untuk UI/audit.
    pub message: String,
}

impl fmt::Display for MedicationBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MedicationBlocked {}

/// Anggota kelas penisilin (nama generik, huruf kecil). Daftar ini yang
/// membuat `Amoxicillin` dikenali sebagai penisilin walau kelas obat di
/// input hanya berisi nama dagang/generik tanpa kelas.
pub fn penicillin_class_members() -> &'static [&'static str] {
    &[
        "amoxicillin",
        "ampicillin",
        "amoxiclav",
        "amoxicillin-clavulanate",
        "penicillin",
        "penisilin",
        "benzylpenicillin",
        "phenoxymethylpenicillin",
        "piperacillin",
        "ticarcillin",
        "oxacillin",
        "cloxacillin",
        "flucloxacillin",
        "dicloxacillin",
        "nafcillin",
        "carbenicillin",
    ]
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Apakah obat ini anggota kelas penisilin (dari nama atau kelasnya)?
pub fn is_penicillin_drug(drug_name: &str, drug_class: &str) -> bool {
    let class = norm(drug_class);
    if class == "penicillin" || class == "penisilin" || class == "beta-lactam" {
        return true;
    }
    let name = norm(drug_name);
    penicillin_class_members()
        .iter()
        .any(|m| name.contains(m))
}

/// Apakah alergi ini alergi kelas penisilin (dari zat atau kelasnya)?
pub fn is_penicillin_allergy(allergy: &AllergyIntolerance) -> bool {
    if let Some(class) = &allergy.substance_class {
        let c = norm(class);
        if c == "penicillin" || c == "penisilin" || c == "beta-lactam" {
            return true;
        }
    }
    let s = norm(&allergy.substance);
    s.contains("penisilin") || s.contains("penicillin")
}

/// Periksa satu resep terhadap seluruh alergi pasien.
///
/// Lolos (`Ok`) bila tidak ada alergi yang cocok. Gagal
/// (`Err(MedicationBlocked)`) pada alergi pertama yang cocok, dengan pesan
/// yang menjelaskan zat alergi dan obat yang diblokir.
pub fn check_prescription(
    allergies: &[AllergyIntolerance],
    drug_name: &str,
    drug_class: &str,
) -> Result<(), MedicationBlocked> {
    let drug_class_n = norm(drug_class);
    let drug_name_n = norm(drug_name);

    for allergy in allergies {
        // 1. Kelas obat sama dengan kelas alergi (mis. penicillin == penicillin).
        let class_match = allergy
            .substance_class
            .as_ref()
            .map(|c| norm(c) == drug_class_n)
            .unwrap_or(false);
        // 2. Nama zat alergi cocok dengan nama/kelas obat.
        let substance_match = norm(&allergy.substance) == drug_class_n
            || norm(&allergy.substance) == drug_name_n
            || drug_name_n.contains(&norm(&allergy.substance));
        // 3. Kasus khusus penisilin: nama obat anggota kelas penisilin dan
        //    pasien alergi penisilin (dari zat atau kelas alergi).
        let penicillin_match =
            is_penicillin_allergy(allergy) && is_penicillin_drug(drug_name, drug_class);

        if class_match || substance_match || penicillin_match {
            return Err(MedicationBlocked {
                substance: allergy.substance.clone(),
                drug: drug_name.to_string(),
                message: format!(
                    "DIBLOKIR: pasien alergi {} — {} mengandung {} dan tidak boleh diresepkan.",
                    allergy.substance,
                    drug_name,
                    allergy.substance.to_lowercase()
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Criticality;

    fn penicillin_allergy() -> AllergyIntolerance {
        AllergyIntolerance {
            id: "ALG-001".into(),
            patient_id: "RM-000812".into(),
            substance: "Penisilin".into(),
            substance_class: Some("penicillin".into()),
            criticality: Criticality::Tinggi,
            note: None,
            recorded_at: chrono::DateTime::from_timestamp(1_793_000_000, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[test]
    fn amoxicillin_blocked_for_penicillin_allergy() {
        let allergies = vec![penicillin_allergy()];
        let err = check_prescription(&allergies, "Amoxicillin 500 mg", "penicillin")
            .expect_err("amoxicillin harus diblokir untuk pasien alergi penisilin");
        assert!(err.message.contains("DIBLOKIR"));
        assert!(err.message.contains("Penisilin"));
    }

    #[test]
    fn ampicillin_blocked_by_name_even_without_class() {
        let allergies = vec![penicillin_allergy()];
        // Kelas kosong/salah pun tetap ketahuan dari nama obatnya.
        check_prescription(&allergies, "Ampicillin 500 mg", "antibiotic")
            .expect_err("ampicillin adalah penisilin");
    }

    #[test]
    fn azithromycin_allowed_for_penicillin_allergy() {
        let allergies = vec![penicillin_allergy()];
        check_prescription(&allergies, "Azithromycin 500 mg", "macrolide")
            .expect("azithromycin bukan penisilin, harus lolos");
    }

    #[test]
    fn no_allergy_allows_anything() {
        check_prescription(&[], "Amoxicillin 500 mg", "penicillin")
            .expect("tanpa alergi, resep apa pun lolos");
    }

    #[test]
    fn other_class_allergy_blocks_matching_class() {
        let allergy = AllergyIntolerance {
            substance: "Sulfonamida".into(),
            substance_class: Some("sulfonamide".into()),
            ..penicillin_allergy()
        };
        check_prescription(&[allergy], "Cotrimoxazole", "sulfonamide")
            .expect_err("kelas sulfonamide harus diblokir");
    }
}
