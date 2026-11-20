//! Gizi (FASE4.md §2): IMT (BMI) + kategori.
//!
//! `bmi = berat_kg / (tinggi_m^2)`, kategori: <18,5 Kurus;
//! 18,5–24,9 Normal; 25–29,9 Gemuk; >=30 Obesitas.
//! Dipakai seed + repo (`create_nutrition_assessment`), bukan dihitung
//! klien.

/// BMI dari berat (kg) dan tinggi (cm).
pub fn bmi(weight_kg: f64, height_cm: f64) -> f64 {
    let height_m = height_cm / 100.0;
    weight_kg / (height_m * height_m)
}

/// Kategori IMT dari nilai BMI (batas FASE4.md §2).
pub fn bmi_category(bmi: f64) -> &'static str {
    if bmi < 18.5 {
        "Kurus"
    } else if bmi < 25.0 {
        "Normal"
    } else if bmi < 30.0 {
        "Gemuk"
    } else {
        "Obesitas"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmi_seed_cases() {
        // Nilai seed FASE4.md §4.2 (dibulatkan 1 desimal di repo).
        assert!((bmi(62.0, 160.0) - 24.2).abs() < 0.05);
        assert!((bmi(78.0, 170.0) - 27.0).abs() < 0.05);
        assert!((bmi(58.0, 158.0) - 23.2).abs() < 0.05);
        assert!((bmi(71.0, 162.0) - 27.1).abs() < 0.05);
    }

    #[test]
    fn bmi_category_boundaries() {
        assert_eq!(bmi_category(18.4), "Kurus");
        assert_eq!(bmi_category(18.5), "Normal");
        assert_eq!(bmi_category(24.9), "Normal");
        assert_eq!(bmi_category(25.0), "Gemuk");
        assert_eq!(bmi_category(29.9), "Gemuk");
        assert_eq!(bmi_category(30.0), "Obesitas");
        assert_eq!(bmi_category(35.0), "Obesitas");
    }
}
