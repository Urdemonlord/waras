//! Obat & stok KFA.

use crate::db_err;
use crate::rows::DrugRow;
use crate::Db;
use waras_core::{types::Drug, WarasError};

impl Db {
    pub async fn list_drugs(&self) -> Result<Vec<Drug>, WarasError> {
        let rows: Vec<DrugRow> =
            sqlx::query_as("SELECT * FROM drugs ORDER BY name ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_drug(&self, kfa_code: &str) -> Result<Option<Drug>, WarasError> {
        let row: Option<DrugRow> =
            sqlx::query_as("SELECT * FROM drugs WHERE kfa_code = ? LIMIT 1")
                .bind(kfa_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Cari obat lewat nama persis (dipakai dispensing: `drug_name` resep
    /// sama dengan `name` di tabel drugs).
    pub(crate) async fn get_drug_by_name(
        &self,
        name: &str,
    ) -> Result<Option<Drug>, WarasError> {
        let row: Option<DrugRow> =
            sqlx::query_as("SELECT * FROM drugs WHERE name = ? LIMIT 1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }
}
