//! Pengaturan aplikasi (FASE4.md §4.1).
//! Kunci yang diizinkan: `backup_otomatis`, `sinkron_satusehat`,
//! `mode_offline` (nilai `on`/`off`), `retensi_audit_hari` (angka 1..3650).
//! Kunci lain → 400 "kunci pengaturan tidak dikenal".

use crate::rows::{clinic_now_string, AppSettingRow};
use crate::{db_err, Db};
use waras_core::{types::AppSetting, WarasError};

/// (kunci, jenis nilai): `onoff` = hanya `on`/`off`; `number` = angka.
const ALLOWED_SETTINGS: &[(&str, &str)] = &[
    ("backup_otomatis", "onoff"),
    ("sinkron_satusehat", "onoff"),
    ("mode_offline", "onoff"),
    ("retensi_audit_hari", "number"),
];

impl Db {
    pub async fn list_settings(&self) -> Result<Vec<AppSetting>, WarasError> {
        let rows: Vec<AppSettingRow> =
            sqlx::query_as("SELECT * FROM app_settings ORDER BY key ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<AppSetting, WarasError> {
        let kind = ALLOWED_SETTINGS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, kind)| *kind)
            .ok_or_else(|| {
                WarasError::Validation(format!("kunci pengaturan tidak dikenal: {key}"))
            })?;
        match kind {
            "onoff" => {
                if value != "on" && value != "off" {
                    return Err(WarasError::Validation(format!(
                        "nilai {key} harus 'on' atau 'off', bukan {value:?}"
                    )));
                }
            }
            "number" => {
                let n: i64 = value.parse().map_err(|_| {
                    WarasError::Validation(format!(
                        "nilai {key} harus angka 1..3650, bukan {value:?}"
                    ))
                })?;
                if !(1..=3650).contains(&n) {
                    return Err(WarasError::Validation(format!(
                        "nilai {key} harus angka 1..3650, bukan {value:?}"
                    )));
                }
            }
            _ => unreachable!(),
        }
        let now = clinic_now_string();
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if exists > 0 {
            sqlx::query("UPDATE app_settings SET value = ?, updated_at = ? WHERE key = ?")
                .bind(value)
                .bind(&now)
                .bind(key)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
        } else {
            sqlx::query(
                "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?)",
            )
            .bind(key)
            .bind(value)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        }
        let row: Option<AppSettingRow> =
            sqlx::query_as("SELECT * FROM app_settings WHERE key = ? LIMIT 1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("pengaturan tidak ditemukan setelah simpan".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_settings_and_validation() {
        let db = seeded_db().await;
        let list = db.list_settings().await.unwrap();
        assert_eq!(list.len(), 4);
        let get = |k: &str| list.iter().find(|s| s.key == k).unwrap().value.clone();
        assert_eq!(get("backup_otomatis"), "on");
        assert_eq!(get("sinkron_satusehat"), "on");
        assert_eq!(get("mode_offline"), "off");
        assert_eq!(get("retensi_audit_hari"), "365");
        // Ubah nilai yang diizinkan.
        let s = db.set_setting("mode_offline", "on").await.unwrap();
        assert_eq!(s.value, "on");
        let s = db.set_setting("retensi_audit_hari", "180").await.unwrap();
        assert_eq!(s.value, "180");
        // Kunci tidak dikenal -> 400.
        let err = db.set_setting("tema_warna", "biru").await.unwrap_err();
        assert!(matches!(err, WarasError::Validation(_)));
        assert_eq!(err.http_status(), 400);
        // Nilai tidak valid -> 400.
        assert!(matches!(
            db.set_setting("backup_otomatis", "ya").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_setting("retensi_audit_hari", "tiga ratus").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_setting("retensi_audit_hari", "0").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.set_setting("retensi_audit_hari", "3651").await,
            Err(WarasError::Validation(_))
        ));
        assert_eq!(db.list_settings().await.unwrap().len(), 4);
    }
}
