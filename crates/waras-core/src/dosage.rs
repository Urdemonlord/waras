//! Normalisasi dosis resep: kekuatan obat (mis. `500 mg`) hidup di
//! `drug_name` ("Amoxicillin 500 mg"), sedangkan `dosage` hanya frekuensi
//! ("1x1"). Tanpa normalisasi, tampilan `drug_name + dosage` mengulang
//! kekuatan: "Amoxicillin 500 mg 500 mg 1x1".
//!
//! Pure function — dipakai `waras-db::prescribe_as` saat resep ditulis,
//! jadi data tersimpan selalu kanonik walau form diisi "500 mg 1x1".
//! Tampilan di UI memakai aturan yang sama (`Waras.drugDose` di app.js)
//! untuk baris lama yang tersimpan sebelum normalisasi ini ada.

/// Satuan kekuatan obat yang dikenali di akhir nama obat.
const STRENGTH_UNITS: [&str; 7] = ["mg", "g", "mcg", "µg", "iu", "ml", "%"];

/// Ambil kekuatan di akhir nama obat, mis. `Some("500 mg")` untuk
/// "Amoxicillin 500 mg". `None` bila nama tidak berakhir pola angka+satuan.
pub fn trailing_strength(drug_name: &str) -> Option<String> {
    let tokens: Vec<&str> = drug_name.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }
    let unit = tokens[tokens.len() - 1].trim_end_matches(',');
    let amount = tokens[tokens.len() - 2].trim_end_matches(',');
    let unit_ok = STRENGTH_UNITS
        .iter()
        .any(|u| u.eq_ignore_ascii_case(unit));
    // Desimal Indonesia ("0,5") ikut diterima.
    let amount_ok = amount.replace(',', ".").parse::<f64>().is_ok();
    if unit_ok && amount_ok {
        Some(format!("{amount} {unit}"))
    } else {
        None
    }
}

/// Buang kekuatan di awal dosage bila nama obat sudah memuat kekuatan
/// yang sama. "Amoxicillin 500 mg" + "500 mg 1x1" -> "1x1";
/// "Amoxicillin 500 mg" + "1x1" -> "1x1" (tidak berubah).
/// Varian tanpa spasi ("500mg 1x1") ikut dinormalisasi.
pub fn normalize_dosage(drug_name: &str, dosage: &str) -> String {
    let dosage = dosage.trim();
    let Some(strength) = trailing_strength(drug_name) else {
        return dosage.to_string();
    };
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let want = compact(&strength).to_lowercase();
    let have = compact(dosage).to_lowercase();
    if !have.starts_with(&want) {
        return dosage.to_string();
    }
    // Potong awalan kekuatan dari dosage asli (tahan spasi "500 mg"/"500mg").
    let mut need = want.chars();
    let mut cur = need.next();
    let mut cut = 0;
    for (i, c) in dosage.char_indices() {
        if cur.is_none() {
            break;
        }
        if c.is_whitespace() {
            continue;
        }
        if c.to_ascii_lowercase() == cur.unwrap() {
            cut = i + c.len_utf8();
            cur = need.next();
        } else {
            return dosage.to_string();
        }
    }
    if cur.is_some() {
        return dosage.to_string();
    }
    dosage[cut..].trim().to_string()
}

/// Tampilan "nama + dosis" tanpa kekuatan dobel, mis.
/// "Amoxicillin 500 mg 1x1".
pub fn drug_dose_display(drug_name: &str, dosage: &str) -> String {
    let dose = normalize_dosage(drug_name, dosage);
    if dose.is_empty() {
        drug_name.trim().to_string()
    } else {
        format!("{} {dose}", drug_name.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_duplicated_strength() {
        assert_eq!(
            normalize_dosage("Amoxicillin 500 mg", "500 mg 1x1"),
            "1x1"
        );
        assert_eq!(
            normalize_dosage("Amoxicillin 500 mg", "500mg 1x1"),
            "1x1"
        );
        assert_eq!(
            normalize_dosage("Paracetamol 500 mg", "500 mg 3x1"),
            "3x1"
        );
        assert_eq!(normalize_dosage("Amlodipine 5 mg", "5 mg 1x1"), "1x1");
        assert_eq!(
            normalize_dosage("Cefixime 200 mg", "200 mg 2x1"),
            "2x1"
        );
    }

    #[test]
    fn normalize_keeps_canonical_and_unrelated_dosage() {
        assert_eq!(normalize_dosage("Amoxicillin 500 mg", "1x1"), "1x1");
        // Kekuatan berbeda tidak dibuang.
        assert_eq!(
            normalize_dosage("Amoxicillin 500 mg", "250 mg 1x1"),
            "250 mg 1x1"
        );
        // Nama tanpa kekuatan: dosage apa adanya.
        assert_eq!(normalize_dosage("Vitamin C", "500 mg 1x1"), "500 mg 1x1");
        assert_eq!(normalize_dosage("Amoxicillin 500 mg", "  1x1  "), "1x1");
    }

    #[test]
    fn drug_dose_display_never_repeats_strength() {
        assert_eq!(
            drug_dose_display("Amoxicillin 500 mg", "500 mg 1x1"),
            "Amoxicillin 500 mg 1x1"
        );
        assert_eq!(
            drug_dose_display("Azithromycin 500 mg", "1x1"),
            "Azithromycin 500 mg 1x1"
        );
    }
}
