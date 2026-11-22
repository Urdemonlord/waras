//! Kunci API publik (FASE4.md §1, §5.2).
//!
//! Tabel `api_keys` (migrasi 0006, agen A): kunci penuh TIDAK PERNAH
//! disimpan — hanya `key_prefix` (12 karakter pertama) + `key_hash`
//! (SHA-256 hex dari kunci penuh) + nama + scope + pembuat.
//! `scopes` disimpan sebagai TEXT dipisah koma
//! (`patients:read,queue:read,encounters:read,fhir:read`).
//! Baris mentah + konversi memakai `crate::rows::ApiKeyRow` (agen A).

use waras_core::{types::ApiKey, WarasError};

use crate::{db_err, fmt_dt, now_naive, rows::ApiKeyRow, Db};

impl Db {
    /// Buat kunci API baru. ID `AK-######` berikutnya. `scopes` sudah
    /// dalam bentuk dipisah koma (divalidasi handler sebelum memanggil).
    /// Pemanggil menyimpan HANYA prefix + hash, bukan kunci penuh.
    /// `create_api_key_once` memegang `Db::alloc_lock` selama hitung-max
    /// + INSERT; retry maks 5x di sini hanya jaring pengaman untuk
    /// tabrakan antar proses.
    pub async fn create_api_key(
        &self,
        name: &str,
        key_prefix: &str,
        key_hash: &str,
        scopes: &str,
        created_by: &str,
    ) -> Result<ApiKey, WarasError> {
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self
                .create_api_key_once(name, key_prefix, key_hash, scopes, created_by)
                .await
            {
                Ok(k) => return Ok(k),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id kunci API".into())))
    }

    async fn create_api_key_once(
        &self,
        name: &str,
        key_prefix: &str,
        key_hash: &str,
        scopes: &str,
        created_by: &str,
    ) -> Result<ApiKey, WarasError> {
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM api_keys")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let id = format!("AK-{:06}", crate::next_seq_from_ids(&ids, 1));
        let created = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO api_keys (id, name, key_prefix, key_hash, scopes,
                 created_by, created_at, last_used_at, revoked)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, 0)",
        )
        .bind(&id)
        .bind(name)
        .bind(key_prefix)
        .bind(key_hash)
        .bind(scopes)
        .bind(created_by)
        .bind(&created)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_api_key(&id)
            .await?
            .ok_or_else(|| WarasError::Database("kunci API baru tidak ditemukan".into()))
    }

    /// Daftar semua kunci (termasuk yang dicabut), terbaru dulu.
    pub async fn list_api_keys(&self) -> Result<Vec<ApiKey>, WarasError> {
        let rows: Vec<ApiKeyRow> = sqlx::query_as(
            "SELECT * FROM api_keys ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_api_key(&self, id: &str) -> Result<Option<ApiKey>, WarasError> {
        let row: Option<ApiKeyRow> =
            sqlx::query_as("SELECT * FROM api_keys WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Cabut kunci (`revoked = 1`). 404 bila id tidak ada.
    pub async fn revoke_api_key(&self, id: &str) -> Result<ApiKey, WarasError> {
        let n = sqlx::query("UPDATE api_keys SET revoked = 1 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(WarasError::NotFound(format!(
                "kunci API {id} tidak ditemukan"
            )));
        }
        self.get_api_key(id)
            .await?
            .ok_or_else(|| WarasError::Database("kunci API tidak ditemukan".into()))
    }

    /// Cari kunci lewat hash SHA-256 hex dari kunci penuh yang
    /// ditunjukkan (dipakai verifikasi `X-Api-Key` di `public_api`).
    /// Mengembalikan kunci apa pun statusnya; pemanggil yang menolak
    /// bila `revoked`.
    pub async fn find_api_key_by_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<ApiKey>, WarasError> {
        let row: Option<ApiKeyRow> =
            sqlx::query_as("SELECT * FROM api_keys WHERE key_hash = ? LIMIT 1")
                .bind(key_hash)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Perbarui `last_used_at` ke sekarang (best-effort: pemanggil di
    /// `public_api` mengabaikan error-nya, jangan gagalkan request).
    pub async fn touch_api_key(&self, id: &str) -> Result<(), WarasError> {
        sqlx::query("UPDATE api_keys SET last_used_at = ? WHERE id = ?")
            .bind(fmt_dt(now_naive()))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
