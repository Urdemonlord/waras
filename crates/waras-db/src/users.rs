//! Pengguna & sesi.

use crate::convert::*;
use crate::rows::{PatientRow, UserRow};
use crate::{db_err, fmt_dt, hex_encode, now_naive, random_token_hex, Db};
use chrono::Duration;
use sha2::{Digest, Sha256};
use waras_core::{
    types::{Patient, User},
    WarasError,
};

impl Db {
    /// Buat user baru. ID `U-###` berikutnya; password di-hash Argon2.
    /// `create_user_once` memegang `Db::alloc_lock` selama hitung-max +
    /// INSERT; retry maks 5x di sini hanya jaring pengaman untuk
    /// tabrakan antar proses. Hash Argon2 dihitung di luar kunci supaya
    /// seksi kritis tetap pendek (retry tidak mengulang hash).
    pub async fn create_user(
        &self,
        nip: &str,
        username: &str,
        name: &str,
        role: waras_core::types::Role,
        password: &str,
    ) -> Result<User, WarasError> {
        let hash = Self::hash_password(password)?;
        let mut last_err: Option<WarasError> = None;
        for _ in 0..5 {
            match self
                .create_user_once(nip, username, name, role, &hash)
                .await
            {
                Ok(u) => return Ok(u),
                Err(WarasError::Database(ref m))
                    if crate::is_unique_violation_msg(m) =>
                {
                    last_err = Some(WarasError::Database(m.clone()));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| WarasError::Database("gagal alokasi id user".into())))
    }

    async fn create_user_once(
        &self,
        nip: &str,
        username: &str,
        name: &str,
        role: waras_core::types::Role,
        hash: &str,
    ) -> Result<User, WarasError> {
        // Kunci alokasi (lihat Db::alloc_lock): hitung-max + INSERT harus
        // atomik antar task dalam satu proses.
        let _alloc = self.alloc_lock.lock().await;
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM users")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        let id = format!("U-{:03}", crate::next_seq_from_ids(&ids, 1));
        let created = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO users (id, nip, username, name, role, active, password_hash, created_at)
             VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(nip)
        .bind(username)
        .bind(name)
        .bind(role_to_str(role))
        .bind(hash)
        .bind(&created)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.find_user_by_username_or_nip(username)
            .await?
            .ok_or_else(|| WarasError::Database("user baru tidak ditemukan".into()))
    }

    /// Cari user lewat username ATAU NIP (login menerima keduanya).
    pub async fn find_user_by_username_or_nip(
        &self,
        login: &str,
    ) -> Result<Option<User>, WarasError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT * FROM users WHERE username = ? OR nip = ? LIMIT 1",
        )
        .bind(login)
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Buat sesi: token 32 byte acak (64 hex), kedaluwarsa +12 jam.
    /// Mengembalikan tokennya.
    pub async fn create_session(&self, user_id: &str) -> Result<String, WarasError> {
        let token = random_token_hex();
        let now = now_naive();
        let expires = now + Duration::hours(12);
        sqlx::query(
            "INSERT INTO sessions (token, user_id, created_at, expires_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&token)
        .bind(user_id)
        .bind(fmt_dt(now))
        .bind(fmt_dt(expires))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(token)
    }

    /// User pemilik token, bila token ada dan belum kedaluwarsa.
    pub async fn find_user_by_token(&self, token: &str) -> Result<Option<User>, WarasError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT u.* FROM users u
             JOIN sessions s ON s.user_id = u.id
             WHERE s.token = ? AND s.expires_at > ? LIMIT 1",
        )
        .bind(token)
        .bind(fmt_dt(now_naive()))
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Hapus sesi (logout).
    pub async fn delete_session(&self, token: &str) -> Result<(), WarasError> {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Cek cepat: apakah token masih berlaku (dipakai guard API).
    pub async fn token_valid(&self, token: &str) -> Result<bool, WarasError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE token = ? AND expires_at > ?",
        )
        .bind(token)
        .bind(fmt_dt(now_naive()))
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(n > 0)
    }

    // -- Sesi pasien (portal, FASE4.md §4/§7) -------------------------------
    // Sesi pasien: `patient_id` terisi, `user_id` NULL. Token pasien
    // DITOLAK extractor staf (`find_user_by_token` JOIN ke users tidak
    // menemukan baris, karena user_id NULL).

    /// Buat sesi pasien: token 32 byte acak (64 hex), kedaluwarsa +12 jam.
    /// Mengembalikan tokennya.
    pub async fn create_patient_session(
        &self,
        patient_id: &str,
    ) -> Result<String, WarasError> {
        let token = random_token_hex();
        let now = now_naive();
        let expires = now + Duration::hours(12);
        sqlx::query(
            "INSERT INTO sessions (token, user_id, patient_id, created_at, expires_at)
             VALUES (?, NULL, ?, ?, ?)",
        )
        .bind(&token)
        .bind(patient_id)
        .bind(fmt_dt(now))
        .bind(fmt_dt(expires))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(token)
    }

    /// Pasien pemilik token pasien, bila token ada dan belum kedaluwarsa.
    pub async fn find_patient_by_token(
        &self,
        token: &str,
    ) -> Result<Option<Patient>, WarasError> {
        let row: Option<PatientRow> = sqlx::query_as(
            "SELECT p.* FROM patients p
             JOIN sessions s ON s.patient_id = p.id
             WHERE s.token = ? AND s.expires_at > ? LIMIT 1",
        )
        .bind(token)
        .bind(fmt_dt(now_naive()))
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Pasien lewat NIK (dipakai login portal: NIK + tanggal lahir).
    pub async fn find_patient_by_nik(
        &self,
        nik: &str,
    ) -> Result<Option<Patient>, WarasError> {
        let row: Option<PatientRow> =
            sqlx::query_as("SELECT * FROM patients WHERE nik = ? LIMIT 1")
                .bind(nik)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }
}

/// Hash SHA-256 hex (dipakai juga untuk token internal bila perlu).
#[allow(dead_code)]
pub(crate) fn sha256_hex(s: &str) -> String {
    hex_encode(Sha256::digest(s.as_bytes()))
}
