//! Keamanan produksi Fase A (A4): kata sandi portal pasien,
//! pelacak brute-force login, dan rate limit API publik persisten.
//!
//! Semua stempel waktu disimpan sebagai TEXT `YYYY-MM-DD HH:MM:SS`
//! (UTC, `now_naive`) dan semua logika jendela dihitung di Rust —
//! TIDAK ADA fungsi tanggal khas-SQLite (`strftime`, `datetime`,
//! dsb.), supaya query yang sama jalan di SQLite maupun PostgreSQL
//! (placeholder `?` ditulis ulang `DbPool` di mode Postgres).

use chrono::{Duration, NaiveDateTime};

use crate::{db_err, fmt_dt, now_naive, parse_dt, Db};
use waras_core::WarasError;

/// Brute-force login: 5 kegagalan dalam jendela ini -> kunci 15 menit.
pub const LOGIN_MAX_ATTEMPTS: i64 = 5;
pub const LOGIN_WINDOW_SECS: i64 = 900; // 15 menit
pub const LOGIN_LOCKOUT_SECS: i64 = 900; // 15 menit dari kegagalan ke-5

/// Rate limit API publik: 100 request per 60 detik per kunci API.
pub const API_RATE_LIMIT_PER_MINUTE: i64 = 100;
pub const API_RATE_WINDOW_SECS: i64 = 60;

/// Hasil cek rate limit API publik.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiRateLimit {
    /// `true` = request boleh jalan (hitungan sudah ditambah).
    pub allowed: bool,
    /// Bila ditolak: detik sampai jendela baru (untuk `Retry-After`).
    /// Bila lolos: 0.
    pub retry_after_secs: u64,
}

fn remaining_secs(now: NaiveDateTime, until: NaiveDateTime) -> i64 {
    (until - now).num_seconds().max(1)
}

impl Db {
    // -- Kata sandi portal pasien ------------------------------------------

    /// Hash Argon2 kata sandi portal pasien, bila sudah diset.
    pub async fn get_portal_password_hash(
        &self,
        patient_id: &str,
    ) -> Result<Option<String>, WarasError> {
        let hash: Option<Option<String>> = sqlx::query_scalar(
            "SELECT portal_password_hash FROM patients WHERE id = ? LIMIT 1",
        )
        .bind(patient_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(hash.flatten())
    }

    /// Set (atau ganti) kata sandi portal pasien. Disimpan sebagai
    /// hash Argon2, tidak pernah mentah.
    pub async fn set_portal_password(
        &self,
        patient_id: &str,
        password: &str,
    ) -> Result<(), WarasError> {
        let hash = Self::hash_password(password)?;
        sqlx::query("UPDATE patients SET portal_password_hash = ? WHERE id = ?")
            .bind(&hash)
            .bind(patient_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Backfill untuk DB yang sudah ter-seed SEBELUM migrasi 0007
    /// (kolom baru NULL): beri semua pasien tanpa hash kata sandi
    /// portal demo `waras123` (sama seperti kata sandi seed staf).
    /// Idempotent — hanya menyentuh baris yang hash-nya masih NULL.
    /// Mengembalikan jumlah pasien yang di-backfill.
    pub async fn backfill_portal_passwords(&self) -> Result<u64, WarasError> {
        let hash = Self::hash_password("waras123")?;
        let r = sqlx::query(
            "UPDATE patients SET portal_password_hash = ? WHERE portal_password_hash IS NULL",
        )
        .bind(&hash)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(r.rows_affected())
    }

    // -- Brute-force login ---------------------------------------------------

    /// Sisa detik penguncian untuk `key` (`staff:<ip>:<login>` atau
    /// `patient:<ip>:<nik>`), bila kunci itu sedang terkunci.
    /// `None` = tidak terkunci (termasuk kunci yang belum pernah gagal
    /// dan kunci yang jendelanya sudah kedaluwarsa).
    pub async fn check_login_lockout(
        &self,
        key: &str,
    ) -> Result<Option<i64>, WarasError> {
        let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT failed_count, window_start, locked_until FROM login_attempts WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some((_count, _start, locked_until)) = row else {
            return Ok(None);
        };
        let Some(locked_until) = locked_until else {
            return Ok(None);
        };
        let until = parse_dt(&locked_until)?;
        let now = now_naive();
        if until > now {
            Ok(Some(remaining_secs(now, until)))
        } else {
            Ok(None)
        }
    }

    /// Catat satu kegagalan login untuk `key`. Mengembalikan
    /// `Some(detik)` bila kegagalan ini MENGUNCI akun (kegagalan ke-5
    /// dalam jendela) — pemanggil membalas 429 dengan `Retry-After`
    /// selama itu; `None` bila belum terkunci (pemanggil balas 401).
    /// Jendela yang sudah kedaluwarsa di-reset (hitungan mulai dari 1).
    pub async fn record_login_failure(
        &self,
        key: &str,
    ) -> Result<Option<i64>, WarasError> {
        let now = now_naive();
        let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT failed_count, window_start, locked_until FROM login_attempts WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        match row {
            None => {
                sqlx::query(
                    "INSERT INTO login_attempts (key, failed_count, window_start, locked_until)
                     VALUES (?, 1, ?, NULL)",
                )
                .bind(key)
                .bind(fmt_dt(now))
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                Ok(None)
            }
            Some((count, window_start, locked_until)) => {
                let start = parse_dt(&window_start)?;
                // Sudah terkunci dan kunci masih berlaku: jangan
                // tambah hitungan (kunci tidak diperpanjang tiap
                // percobaan — penyerang tidak bisa memperpanjang
                // lockout orang lain selamanya).
                if let Some(until) = locked_until {
                    if parse_dt(&until)? > now {
                        return Ok(Some(remaining_secs(now, parse_dt(&until)?)));
                    }
                }
                // Jendela kedaluwarsa -> mulai jendela baru.
                if now - start >= Duration::seconds(LOGIN_WINDOW_SECS) {
                    sqlx::query(
                        "UPDATE login_attempts SET failed_count = 1, window_start = ?, locked_until = NULL WHERE key = ?",
                    )
                    .bind(fmt_dt(now))
                    .bind(key)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                    return Ok(None);
                }
                let count = count + 1;
                if count >= LOGIN_MAX_ATTEMPTS {
                    let until = now + Duration::seconds(LOGIN_LOCKOUT_SECS);
                    sqlx::query(
                        "UPDATE login_attempts SET failed_count = ?, window_start = ?, locked_until = ? WHERE key = ?",
                    )
                    .bind(count)
                    .bind(fmt_dt(start))
                    .bind(fmt_dt(until))
                    .bind(key)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                    Ok(Some(LOGIN_LOCKOUT_SECS))
                } else {
                    sqlx::query(
                        "UPDATE login_attempts SET failed_count = ? WHERE key = ?",
                    )
                    .bind(count)
                    .bind(key)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                    Ok(None)
                }
            }
        }
    }

    /// Hapus pelacak kegagalan untuk `key` (dipanggil saat login
    /// sukses — satu login benar membersihkan hitungan).
    pub async fn clear_login_attempts(&self, key: &str) -> Result<(), WarasError> {
        sqlx::query("DELETE FROM login_attempts WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    // -- Rate limit API publik (persisten) ------------------------------------

    /// Cek + hitung satu request API publik untuk `key_id`.
    /// Jendela 60 detik dihitung dari request pertama di jendela itu;
    /// request ke-101 dalam satu jendela ditolak (`allowed = false`,
    /// `retry_after_secs` = detik sampai jendela baru).
    /// Baris bertahan di DB, jadi batas tetap berlaku setelah server
    /// restart (tidak seperti HashMap in-memory sebelumnya).
    pub async fn check_api_rate_limit(
        &self,
        key_id: &str,
    ) -> Result<ApiRateLimit, WarasError> {
        let now = now_naive();
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT window_start, request_count FROM api_rate_limits WHERE key_id = ?",
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        match row {
            None => {
                sqlx::query(
                    "INSERT INTO api_rate_limits (key_id, window_start, request_count)
                     VALUES (?, ?, 1)",
                )
                .bind(key_id)
                .bind(fmt_dt(now))
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                Ok(ApiRateLimit {
                    allowed: true,
                    retry_after_secs: 0,
                })
            }
            Some((window_start, count)) => {
                let start = parse_dt(&window_start)?;
                let elapsed = (now - start).num_seconds();
                if elapsed >= API_RATE_WINDOW_SECS {
                    sqlx::query(
                        "UPDATE api_rate_limits SET window_start = ?, request_count = 1 WHERE key_id = ?",
                    )
                    .bind(fmt_dt(now))
                    .bind(key_id)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                    Ok(ApiRateLimit {
                        allowed: true,
                        retry_after_secs: 0,
                    })
                } else if count < API_RATE_LIMIT_PER_MINUTE {
                    sqlx::query(
                        "UPDATE api_rate_limits SET request_count = ? WHERE key_id = ?",
                    )
                    .bind(count + 1)
                    .bind(key_id)
                    .execute(&self.pool)
                    .await
                    .map_err(db_err)?;
                    Ok(ApiRateLimit {
                        allowed: true,
                        retry_after_secs: 0,
                    })
                } else {
                    Ok(ApiRateLimit {
                        allowed: false,
                        retry_after_secs: (API_RATE_WINDOW_SECS - elapsed).max(1) as u64,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
        db
    }

    #[tokio::test]
    async fn portal_password_seed_terisi_untuk_semua_pasien_seed() {
        let db = seeded_db().await;
        // 12 pasien seed Fase 1 (+ 2 Fase 3 = 14 di seed utama; Fase 4
        // menambah 2 lagi lewat seed_fase4 — di sini hanya seed utama).
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM patients WHERE portal_password_hash IS NULL",
        )
        .fetch_one(&db.pool)
        .await
        .expect("hitung");
        assert_eq!(n, 0, "semua pasien seed harus punya hash portal");
        let hash = db
            .get_portal_password_hash("RM-000812")
            .await
            .expect("baca hash")
            .expect("hash Siti ada");
        assert!(Db::verify_password(&hash, "waras123"));
        assert!(!Db::verify_password(&hash, "1985-05-12"));
        assert!(!Db::verify_password(&hash, "salah"));
    }

    #[tokio::test]
    async fn backfill_mengisi_hash_yang_null_dan_idempotent() {
        let db = seeded_db().await;
        // Simulasi DB lama: hapus hash semua pasien, lalu backfill.
        sqlx::query("UPDATE patients SET portal_password_hash = NULL")
            .execute(&db.pool)
            .await
            .expect("hapus hash");
        let n = db.backfill_portal_passwords().await.expect("backfill");
        assert_eq!(n, 14, "14 pasien seed utama harus ter-backfill");
        let n2 = db.backfill_portal_passwords().await.expect("backfill 2");
        assert_eq!(n2, 0, "backfill kedua tidak mengubah apa-apa");
        let hash = db
            .get_portal_password_hash("RM-000813")
            .await
            .expect("baca")
            .expect("ada");
        assert!(Db::verify_password(&hash, "waras123"));
    }

    #[tokio::test]
    async fn brute_force_lima_gagal_mengunci_lalu_sukses_membersihkan() {
        let db = seeded_db().await;
        let key = "staff:127.0.0.1:ahmad";
        assert_eq!(db.check_login_lockout(key).await.unwrap(), None);
        for i in 1..5 {
            assert_eq!(
                db.record_login_failure(key).await.unwrap(),
                None,
                "gagal ke-{i} belum mengunci"
            );
        }
        // Kegagalan ke-5 -> terkunci 900 detik.
        assert_eq!(
            db.record_login_failure(key).await.unwrap(),
            Some(900),
            "gagal ke-5 harus mengunci"
        );
        let remaining = db.check_login_lockout(key).await.unwrap().expect("terkunci");
        assert!((1..=900).contains(&remaining), "sisa {remaining}");
        // Percobaan gagal saat terkunci TIDAK memperpanjang kunci.
        assert_eq!(
            db.record_login_failure(key).await.unwrap(),
            Some(remaining),
            "kunci tidak boleh diperpanjang tiap percobaan"
        );
        // Login sukses membersihkan -> tidak terkunci lagi.
        db.clear_login_attempts(key).await.unwrap();
        assert_eq!(db.check_login_lockout(key).await.unwrap(), None);
        // Satu gagal setelah dibersihkan mulai dari 1 lagi.
        assert_eq!(db.record_login_failure(key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn brute_force_jendela_kedaluwarsa_mereset_hitungan() {
        let db = seeded_db().await;
        let key = "staff:127.0.0.1:sari";
        for _ in 0..4 {
            assert_eq!(db.record_login_failure(key).await.unwrap(), None);
        }
        // Majukan jendela 16 menit ke masa lalu (simulasi waktu lewat):
        // hitungan harus reset, bukan mengunci di gagal ke-5.
        let old = fmt_dt(now_naive() - Duration::seconds(960));
        sqlx::query("UPDATE login_attempts SET window_start = ? WHERE key = ?")
            .bind(&old)
            .bind(key)
            .execute(&db.pool)
            .await
            .expect("majukan jendela");
        assert_eq!(db.check_login_lockout(key).await.unwrap(), None);
        assert_eq!(
            db.record_login_failure(key).await.unwrap(),
            None,
            "jendela baru: gagal pertama tidak mengunci"
        );
        // Kunci yang sudah kedaluwarsa juga tidak lagi berlaku.
        for _ in 0..4 {
            db.record_login_failure(key).await.unwrap();
        }
        assert!(db.check_login_lockout(key).await.unwrap().is_some());
        let past = fmt_dt(now_naive() - Duration::seconds(60));
        sqlx::query("UPDATE login_attempts SET locked_until = ? WHERE key = ?")
            .bind(&past)
            .bind(key)
            .execute(&db.pool)
            .await
            .expect("kedaluwarsakan kunci");
        assert_eq!(
            db.check_login_lockout(key).await.unwrap(),
            None,
            "kunci kedaluwarsa tidak berlaku"
        );
    }

    #[tokio::test]
    async fn api_rate_limit_100_lolos_101_ditolak_dan_reset_setelah_jendela() {
        let db = seeded_db().await;
        for i in 1..=100 {
            let r = db.check_api_rate_limit("AK-000001").await.unwrap();
            assert!(r.allowed, "request ke-{i} harus lolos");
        }
        let r = db.check_api_rate_limit("AK-000001").await.unwrap();
        assert!(!r.allowed, "request ke-101 harus ditolak");
        assert!((1..=60).contains(&r.retry_after_secs), "retry {}", r.retry_after_secs);
        // Kunci lain tidak kena batas kunci ini.
        assert!(db.check_api_rate_limit("AK-000002").await.unwrap().allowed);
        // Majukan jendela 61 detik: hitungan mulai lagi dari 1.
        let old = fmt_dt(now_naive() - Duration::seconds(61));
        sqlx::query("UPDATE api_rate_limits SET window_start = ? WHERE key_id = ?")
            .bind(&old)
            .bind("AK-000001")
            .execute(&db.pool)
            .await
            .expect("majukan jendela");
        assert!(db.check_api_rate_limit("AK-000001").await.unwrap().allowed);
    }
}
