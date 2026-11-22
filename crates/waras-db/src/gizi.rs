//! Gizi (FASE4.md §4): asesmen + diet.
//! BMI + kategori IMT dihitung server (`waras_core::nutrition`),
//! bukan dari klien.

use crate::rows::{clinic_now_string, NutritionAssessmentRow, NutritionDietRow};
use crate::{db_err, Db};
use waras_core::{
    nutrition::{bmi, bmi_category},
    types::{NutritionAssessment, NutritionDiet},
    WarasError,
};

/// BMI dibulatkan 1 desimal sebelum disimpan (nilai seed FASE4.md §4.2
/// memakai 1 desimal: 24,2 / 27,0 / 23,2 / 27,1).
fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

impl Db {
    pub async fn list_nutrition_assessments(
        &self,
        patient_id: Option<&str>,
    ) -> Result<Vec<NutritionAssessment>, WarasError> {
        let rows: Vec<NutritionAssessmentRow> = match patient_id {
            Some(p) => {
                sqlx::query_as(
                    "SELECT * FROM nutrition_assessments WHERE patient_id = ?
                     ORDER BY created_at DESC, id ASC",
                )
                .bind(p)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM nutrition_assessments ORDER BY created_at DESC, id ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Asesmen baru: id `GZ-<seq>`, BMI + kategori dihitung server.
    /// `create_nutrition_assessment_once` memegang `Db::alloc_lock`
    /// selama hitung-max + INSERT; retry maks 5x di sini hanya jaring
    /// pengaman untuk tabrakan antar proses.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_nutrition_assessment(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        weight_kg: f64,
        height_cm: f64,
        risk: &str,
        diet: &str,
        notes: &str,
        assessed_by: &str,
    ) -> Result<NutritionAssessment, WarasError> {
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self
                .create_nutrition_assessment_once(
                    patient_id,
                    encounter_id,
                    weight_kg,
                    height_cm,
                    risk,
                    diet,
                    notes,
                    assessed_by,
                )
                .await
            {
                Ok(a) => return Ok(a),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id asesmen gizi".into())))
    }

    async fn create_nutrition_assessment_once(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        weight_kg: f64,
        height_cm: f64,
        risk: &str,
        diet: &str,
        notes: &str,
        assessed_by: &str,
    ) -> Result<NutritionAssessment, WarasError> {
        if !(weight_kg > 0.0) {
            return Err(WarasError::Validation(
                "berat badan harus lebih dari 0".into(),
            ));
        }
        if !(height_cm > 0.0) {
            return Err(WarasError::Validation(
                "tinggi badan harus lebih dari 0".into(),
            ));
        }
        let bmi_value = round1(bmi(weight_kg, height_cm));
        let category = bmi_category(bmi_value);
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM nutrition_assessments")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("GZ-{seq:06}");
        sqlx::query(
            "INSERT INTO nutrition_assessments (id, patient_id, encounter_id,
                 weight_kg, height_cm, bmi, imt_category, risk, diet, notes,
                 assessed_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(weight_kg)
        .bind(height_cm)
        .bind(bmi_value)
        .bind(category)
        .bind(risk)
        .bind(diet)
        .bind(notes)
        .bind(assessed_by)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<NutritionAssessmentRow> = sqlx::query_as(
            "SELECT * FROM nutrition_assessments WHERE id = ? LIMIT 1",
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("asesmen gizi baru tidak ditemukan".into()))
    }

    pub async fn list_nutrition_diets(
        &self,
        patient_id: Option<&str>,
    ) -> Result<Vec<NutritionDiet>, WarasError> {
        let rows: Vec<NutritionDietRow> = match patient_id {
            Some(p) => {
                sqlx::query_as(
                    "SELECT * FROM nutrition_diets WHERE patient_id = ?
                     ORDER BY start_date DESC, id ASC",
                )
                .bind(p)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM nutrition_diets ORDER BY start_date DESC, id ASC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Diet baru: id `DT-<seq>`, status Aktif.
    /// `create_nutrition_diet_once` memegang `Db::alloc_lock` selama
    /// hitung-max + INSERT; retry maks 5x di sini hanya jaring pengaman
    /// untuk tabrakan antar proses.
    pub async fn create_nutrition_diet(
        &self,
        patient_id: &str,
        diet_type: &str,
        calories: i64,
        notes: &str,
        start_date: &str,
    ) -> Result<NutritionDiet, WarasError> {
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self
                .create_nutrition_diet_once(patient_id, diet_type, calories, notes, start_date)
                .await
            {
                Ok(d) => return Ok(d),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id diet".into())))
    }

    async fn create_nutrition_diet_once(
        &self,
        patient_id: &str,
        diet_type: &str,
        calories: i64,
        notes: &str,
        start_date: &str,
    ) -> Result<NutritionDiet, WarasError> {
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM nutrition_diets")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("DT-{seq:06}");
        sqlx::query(
            "INSERT INTO nutrition_diets (id, patient_id, diet_type, calories,
                 notes, start_date, status)
             VALUES (?, ?, ?, ?, ?, ?, 'Aktif')",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(diet_type)
        .bind(calories)
        .bind(notes)
        .bind(start_date)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let row: Option<NutritionDietRow> =
            sqlx::query_as("SELECT * FROM nutrition_diets WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("diet baru tidak ditemukan".into()))
    }

    pub async fn set_nutrition_diet_status(
        &self,
        id: &str,
        status: &str,
    ) -> Result<NutritionDiet, WarasError> {
        if status != "Aktif" && status != "Selesai" {
            return Err(WarasError::Validation(format!(
                "status diet harus 'Aktif' atau 'Selesai', bukan {status:?}"
            )));
        }
        let n = sqlx::query("UPDATE nutrition_diets SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!("diet {id} tidak ditemukan")));
        }
        let row: Option<NutritionDietRow> =
            sqlx::query_as("SELECT * FROM nutrition_diets WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("diet tidak ditemukan setelah update".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_nutrition_assessments_and_diets() {
        let db = seeded_db().await;
        let list = db.list_nutrition_assessments(None).await.unwrap();
        assert_eq!(list.len(), 4);
        let siti = list.iter().find(|a| a.patient_id == "RM-000812").unwrap();
        assert_eq!(siti.weight_kg, 62.0);
        assert_eq!(siti.height_cm, 160.0);
        assert_eq!(siti.bmi, 24.2);
        assert_eq!(siti.imt_category, "Normal");
        assert_eq!(siti.risk, "Rendah");
        assert_eq!(siti.diet, "DM 1800 kkal");
        let bambang = list.iter().find(|a| a.patient_id == "RM-000820").unwrap();
        assert_eq!(bambang.bmi, 27.0);
        assert_eq!(bambang.imt_category, "Gemuk");
        assert_eq!(bambang.risk, "Sedang");
        let dewi = list.iter().find(|a| a.patient_id == "RM-000814").unwrap();
        // Layar Gizi menulis IMT 27,4 untuk Dewi — kontrak (71 kg / 162 cm)
        // yang menang: 27,1.
        assert_eq!(dewi.bmi, 27.1);
        assert_eq!(
            db.list_nutrition_assessments(Some("RM-000812"))
                .await
                .unwrap()
                .len(),
            1
        );
        let diets = db.list_nutrition_diets(None).await.unwrap();
        assert_eq!(diets.len(), 3);
        assert!(diets.iter().all(|d| d.status == "Aktif"));
        assert_eq!(diets[0].diet_type, "DM");
        assert_eq!(diets[0].calories, 1800);
    }

    #[tokio::test]
    async fn nutrition_assessment_computes_bmi_server_side() {
        let db = seeded_db().await;
        let a = db
            .create_nutrition_assessment(
                "RM-000813", None, 50.0, 170.0, "Rendah", "Biasa 2000 kkal", "", "Ns. Sari Wijaya",
            )
            .await
            .unwrap();
        assert_eq!(a.id, "GZ-000005");
        assert_eq!(a.bmi, 17.3);
        assert_eq!(a.imt_category, "Kurus");
        assert!(matches!(
            db.create_nutrition_assessment(
                "RM-000813", None, 0.0, 170.0, "Rendah", "", "", "Ns. Sari Wijaya",
            )
            .await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.create_nutrition_assessment(
                "RM-000813", None, 50.0, 0.0, "Rendah", "", "", "Ns. Sari Wijaya",
            )
            .await,
            Err(WarasError::Validation(_))
        ));
        let d = db
            .create_nutrition_diet("RM-000813", "Tinggi Protein", 2200, "", "2026-11-19")
            .await
            .unwrap();
        assert_eq!(d.id, "DT-000004");
        assert_eq!(d.status, "Aktif");
        let done = db
            .set_nutrition_diet_status(&d.id, "Selesai")
            .await
            .unwrap();
        assert_eq!(done.status, "Selesai");
        assert!(matches!(
            db.set_nutrition_diet_status(&d.id, "Batal").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_nutrition_diet_status("DT-999999", "Selesai").await,
            Err(WarasError::NotFound(_))
        ));
    }
}
