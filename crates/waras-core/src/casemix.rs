//! Casemix / INA-CBG: grouper lokal (perkiraan) + flag hasil lab.
//! Lihat docs/FASE3.md §2.
//!
//! Tarif di tabel ini adalah CONTOH dari layar desain, BUKAN tarif resmi
//! Kemenkes; grouping lokal = perkiraan, bukan grouper resmi. Tertulis di
//! UI ("Grouping lokal (perkiraan)") dan di `Claim.grouping_note`.

use crate::types::LabFlag;

/// Satu hasil grouping: kode INA-CBG + tarif contoh (rupiah).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CasemixGroup {
    pub code: &'static str,
    pub tariff_idr: i64,
    /// Selalu `"lokal-contoh"` — menandai tarif ini contoh lokal.
    pub source: &'static str,
}

/// Tabel grouping lokal (FASE3.md §2), dikunci diagnosis ICD-10.
/// `procedure_code` diterima untuk pemakaian mendatang, tapi tabel saat
/// ini hanya memetakan diagnosis — prosedur belum mengubah grouping.
/// Diagnosis lain / prosedur belum terpetakan → `None` (API menjawab
/// 422 "belum terpetakan, perbaiki pengkodean").
pub fn group_casemix(diagnosis_code: &str, _procedure_code: Option<&str>) -> Option<CasemixGroup> {
    const LOCAL: &str = "lokal-contoh";
    let (code, tariff_idr) = match diagnosis_code.trim() {
        "I20.9" => ("I-4-13-I", 4_210_000),
        "K35.8" => ("G-4-11-II", 6_840_000),
        "J18.9" => ("J-4-12-I", 3_150_000),
        "S52.5" => ("H-4-15-I", 5_420_000),
        "O80" => ("O-4-10-I", 2_980_000),
        "J06.9" => ("J-4-10-0", 210_000),
        "J03.9" => ("J-4-10-0", 185_000),
        _ => return None,
    };
    Some(CasemixGroup {
        code,
        tariff_idr,
        source: LOCAL,
    })
}

/// Panel LOINC lab (FASE3.md §2): nama pemeriksaan → kode LOINC.
/// Dipakai seed + validasi order (API: `loinc` kosong → isi dari tabel
/// ini bila test dikenal, bila tidak 422 "LOINC tidak dikenal").
pub const LAB_TEST_LOINC: &[(&str, &str)] = &[
    ("Darah lengkap", "58410-2"),
    ("Troponin I", "10839-9"),
    ("HbA1c", "4548-4"),
    ("Urinalisis lengkap", "24357-6"),
    ("Golongan darah", "883-9"),
    ("Elektrolit (Na/K)", "2951-2"),
    ("Gula darah puasa", "1558-6"),
];

/// Kode LOINC untuk nama pemeriksaan (cocok tanpa peduli huruf besar /
/// kecil dan spasi ujung). `None` bila test tidak dikenal.
pub fn loinc_for_test(test_name: &str) -> Option<&'static str> {
    let want = test_name.trim().to_lowercase();
    LAB_TEST_LOINC
        .iter()
        .find(|(name, _)| name.to_lowercase() == want)
        .map(|(_, loinc)| *loinc)
}

/// Flag hasil lab dari nilai numerik terhadap rujukan + ambang kritis.
///
/// - Kritis bila nilai melewati ambang kritis (`<= crit_low` atau
///   `>= crit_high` — di ambang pun sudah kritis);
/// - Tinggi bila di atas rujukan (`> ref_high`), Rendah bila di bawah
///   rujukan (`< ref_low`); Normal bila di dalam rujukan.
/// - Bila tidak ada rujukan sama sekali (`None`/`None`), hasilnya
///   Normal. Nilai non-numerik (mis. golongan darah) TIDAK masuk sini:
///   pemanggil memakai `parse_lab_value` dulu; bila `None`, flag
///   langsung Normal.
pub fn lab_flag(
    value: f64,
    ref_low: Option<f64>,
    ref_high: Option<f64>,
    crit_low: Option<f64>,
    crit_high: Option<f64>,
) -> LabFlag {
    if let Some(cl) = crit_low {
        if value <= cl {
            return LabFlag::Kritis;
        }
    }
    if let Some(ch) = crit_high {
        if value >= ch {
            return LabFlag::Kritis;
        }
    }
    if let Some(lo) = ref_low {
        if value < lo {
            return LabFlag::Rendah;
        }
    }
    if let Some(hi) = ref_high {
        if value > hi {
            return LabFlag::Tinggi;
        }
    }
    LabFlag::Normal
}

/// Parse nilai lab dari teks Indonesia.
///
/// - Desimal koma: `"0,42"` → `0.42`, `"13,8"` → `13.8`.
/// - Pemisah ribuan titik: `"11.200"` → `11200.0`, `"280.000"` → `280000.0`.
/// - Campuran: `"1.234,5"` → `1234.5`.
/// - Tanpa pemisah: `"142"` → `142.0`, `"0.42"` → `0.42`.
/// - Non-numerik (mis. `"A+"`, `"Negatif"`) → `None` (flag Normal).
pub fn parse_lab_value(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    // Hanya angka, pemisah ribuan/desimal, dan tanda yang diizinkan.
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+' || c == ' ')
    {
        return None;
    }
    let t = t.replace(' ', "");
    let normalized = if t.contains(',') {
        // Koma = desimal; titik sebelumnya = ribuan.
        t.replace('.', "").replace(',', ".")
    } else if t.matches('.').count() == 1 && t.split('.').nth(1).map(|f| f.len()) == Some(3) {
        // Satu titik + tepat 3 digit di belakang = ribuan ("11.200").
        // Ambigu dengan desimal 3 digit ("0.123"), tapi nilai lab
        // desimal 3 digit praktis tidak muncul tanpa koma di seed/desain.
        t.replace('.', "")
    } else {
        t
    };
    normalized.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouper_maps_all_design_rows() {
        let cases: [(&str, &str, i64); 7] = [
            ("I20.9", "I-4-13-I", 4_210_000),
            ("K35.8", "G-4-11-II", 6_840_000),
            ("J18.9", "J-4-12-I", 3_150_000),
            ("S52.5", "H-4-15-I", 5_420_000),
            ("O80", "O-4-10-I", 2_980_000),
            ("J06.9", "J-4-10-0", 210_000),
            ("J03.9", "J-4-10-0", 185_000),
        ];
        for (diag, code, tariff) in cases {
            let g = group_casemix(diag, None).unwrap_or_else(|| panic!("{diag} terpetakan"));
            assert_eq!(g.code, code);
            assert_eq!(g.tariff_idr, tariff);
            assert_eq!(g.source, "lokal-contoh");
        }
    }

    #[test]
    fn grouper_unknown_returns_none() {
        assert!(group_casemix("Z99.9", None).is_none());
        assert!(group_casemix("", None).is_none());
        assert!(group_casemix("I20.9 ", None).is_some(), "spasi ujung ditoleransi");
        // Prosedur saja tidak cukup: diagnosis tak dikenal tetap None.
        assert!(group_casemix("Z99.9", Some("47.0")).is_none());
    }

    #[test]
    fn lab_flag_cases() {
        // Troponin I seed: 0,42 vs rujukan <0,04, kritis 0,04.
        assert_eq!(
            lab_flag(0.42, None, Some(0.04), None, Some(0.04)),
            LabFlag::Kritis
        );
        // Tepat di ambang kritis = kritis.
        assert_eq!(
            lab_flag(0.04, None, Some(0.04), None, Some(0.04)),
            LabFlag::Kritis
        );
        // Leukosit 11.200 vs 4.000–10.000.
        assert_eq!(
            lab_flag(11_200.0, Some(4_000.0), Some(10_000.0), None, None),
            LabFlag::Tinggi
        );
        // Hemoglobin 13,8 di dalam 13,0–17,0.
        assert_eq!(
            lab_flag(13.8, Some(13.0), Some(17.0), None, None),
            LabFlag::Normal
        );
        // Di bawah rujukan.
        assert_eq!(
            lab_flag(12.0, Some(13.0), Some(17.0), None, None),
            LabFlag::Rendah
        );
        // Tepat di batas rujukan = Normal.
        assert_eq!(
            lab_flag(13.0, Some(13.0), Some(17.0), None, None),
            LabFlag::Normal
        );
        assert_eq!(
            lab_flag(17.0, Some(13.0), Some(17.0), None, None),
            LabFlag::Normal
        );
        // Ambang kritis bawah.
        assert_eq!(
            lab_flag(2.5, Some(3.5), Some(5.0), Some(3.0), None),
            LabFlag::Kritis
        );
        // Tanpa rujukan sama sekali = Normal.
        assert_eq!(lab_flag(99.0, None, None, None, None), LabFlag::Normal);
    }

    #[test]
    fn parse_lab_value_indonesian_numbers() {
        assert_eq!(parse_lab_value("0,42"), Some(0.42));
        assert_eq!(parse_lab_value("13,8"), Some(13.8));
        assert_eq!(parse_lab_value("11.200"), Some(11_200.0));
        assert_eq!(parse_lab_value("280.000"), Some(280_000.0));
        assert_eq!(parse_lab_value("1.234,5"), Some(1234.5));
        assert_eq!(parse_lab_value("142"), Some(142.0));
        assert_eq!(parse_lab_value("0.42"), Some(0.42));
        assert_eq!(parse_lab_value("A+"), None);
        assert_eq!(parse_lab_value("Negatif"), None);
        assert_eq!(parse_lab_value(""), None);
    }

    #[test]
    fn loinc_panel_lookup() {
        assert_eq!(loinc_for_test("Darah lengkap"), Some("58410-2"));
        assert_eq!(loinc_for_test("troponin i"), Some("10839-9"));
        assert_eq!(loinc_for_test("  HbA1c  "), Some("4548-4"));
        assert_eq!(loinc_for_test("Elektrolit (Na/K)"), Some("2951-2"));
        assert_eq!(loinc_for_test("Rontgen thorax"), None);
        assert_eq!(LAB_TEST_LOINC.len(), 7);
    }
}
