//! WARAS persistence: SQLx, migrasi, repository, seed, audit trail
//! append-only + rantai hash SHA-256.
//!
//! Dua backend, satu kode query: SQLite (mode klinik, default) dan
//! PostgreSQL (mode rumah sakit, `WARAS_DATABASE_URL`). Pool-nya
//! `DbPool` di bawah — pembungkus `sqlx::any::AnyPool` yang menulis
//! ulang placeholder `?` menjadi `$1, $2, ...` di mode Postgres, karena
//! driver `Any` sqlx TIDAK menerjemahkan placeholder (SQL diteruskan
//! verbatim ke Postgres, dan `?` di sana adalah operator JSON, bukan
//! placeholder). Semua `sqlx::query` / `query_as` / `query_scalar` di
//! crate ini tetap memakai `?` dan tidak tahu soal backend.
//!
//! Satu-satunya titik masuk: [`Db`]. Semua tipe domain berasal dari
//! `waras-core`; crate ini hanya memetakan baris SQLite <-> tipe domain
//! (lihat `rows.rs` / `convert.rs`) dan menegakkan aturan bisnis yang
//! butuh data (mis. `safety::check_prescription` di `prescribe`).
//!
//! SQLx dipakai lewat `sqlx::query` / `query_as` + `FromRow` saja.
//! Tidak ada macro `query!`/`query_as!` (butuh DB saat compile).

mod api_keys;
mod audit;
mod casemix;
mod claims;
mod convert;
mod dashboard;
mod drugs;
mod encounters;
mod fhir_data;
mod gigi;
mod gizi;
mod hemodialisa;
mod igd;
mod inventori;
mod invoices;
mod kepegawaian;
mod lab;
mod laporan;
mod notifications;
mod operasi;
mod outbox;
mod patients;
mod pengaturan;
mod prescriptions;
mod queue;
mod radiologi;
mod ranap;
mod rawat_jalan;
mod rows;
mod seed;
mod seps;
mod transfusi;
mod users;

pub use lab::NewLabResult;
pub use seed::seed;
pub use waras_core::types::NewPrescription;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use futures_core::future::BoxFuture;
use futures_core::stream::{BoxStream, Stream};
use sha2::{Digest, Sha256};
use sqlx::any::{
    Any, AnyArguments, AnyPoolOptions, AnyQueryResult, AnyRow, AnyStatement, AnyTypeInfo,
};
use sqlx::{Describe, Either, Execute, Executor, Transaction, AnyPool};
use waras_core::types::{Gender, Insurance, PrescriptionStatus};
use waras_core::WarasError;

/// Ganti segmen nama database dalam URL Postgres
/// (`postgres://user:pass@host:port/db_lama?query` -> `.../db_baru?query`).
/// Dipakai `connect_for_tests` (tanpa crate `url` baru). `None` bila
/// URL tidak punya skema `://` atau tidak punya segmen path.
fn replace_db_in_url(url: &str, new_db: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let path_start = url[scheme_end..].find('/')? + scheme_end;
    let after_path = &url[path_start + 1..];
    let db_end = after_path
        .find(|c| c == '?' || c == '#')
        .map(|i| i + path_start + 1)
        .unwrap_or(url.len());
    let mut out = String::with_capacity(url.len() + new_db.len());
    out.push_str(&url[..path_start + 1]);
    out.push_str(new_db);
    out.push_str(&url[db_end..]);
    Some(out)
}

/// Tanggal klinik ("hari ini"): env `WARAS_TODAY`, default `2026-11-19`
/// (tanggal desain, KONTRAK §0). Semua seed dan KPI memakai tanggal ini.
pub fn clinic_today() -> NaiveDate {
    if let Ok(s) = std::env::var("WARAS_TODAY") {
        if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
            return d;
        }
    }
    NaiveDate::from_ymd_opt(2026, 11, 19).expect("tanggal default valid")
}

/// Input pasien baru (tanpa id/created_at; id `RM-######` diisi DB).
#[derive(Debug, Clone)]
pub struct NewPatient {
    pub nik: String,
    pub name: String,
    pub birth_date: NaiveDate,
    pub gender: Gender,
    pub address: String,
    pub phone: Option<String>,
    pub blood_type: Option<String>,
    pub bpjs_number: Option<String>,
    pub insurance: Insurance,
}

/// Filter daftar resep.
#[derive(Debug, Clone, Default)]
pub struct PrescriptionFilter {
    pub status: Option<PrescriptionStatus>,
    pub patient_id: Option<String>,
}

/// Pool database backend-agnostik (SQLite atau PostgreSQL).
///
/// Pembungkus `sqlx::any::AnyPool` + satu flag backend. Semua repo
/// memakai `&self.pool` (atau `&mut tx`) sebagai `Executor<Database =
/// Any>`, jadi tidak ada satu pun call site query yang berubah.
///
/// Di mode Postgres, SQL ditulis ulang `?` -> `$n` (lihat
/// `DbPool::rewritten_sql`): sqlx `Any` TIDAK menerjemahkan
/// placeholder, dan Postgres menolak `?` sebagai placeholder.
#[derive(Clone)]
pub(crate) struct DbPool {
    inner: AnyPool,
    postgres: bool,
    /// Cache intern hasil rewrite per string SQL unik (`&'static`,
    /// dibocorkan sekali per string — jumlah query di repo terbatas,
    /// jadi totalnya beberapa KB). Perlu supaya stream hasil
    /// `fetch_many` bisa meminjam SQL hasil rewrite tanpa meminjam
    /// `String` lokal yang sudah di-drop.
    cache: Arc<Mutex<HashMap<String, &'static str>>>,
}

impl std::fmt::Debug for DbPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbPool")
            .field("postgres", &self.postgres)
            .finish_non_exhaustive()
    }
}

/// Transaksi atas `DbPool`.
///
/// Sengaja TIDAK mengimplementasikan `Deref` ke koneksi: supaya
/// `&mut tx` di repo tetap berarti `&mut DbTx` (bukan
/// `&mut AnyConnection`) dan query di dalam transaksi ikut ditulis
/// ulang placeholder-nya di mode Postgres.
pub(crate) struct DbTx<'a> {
    tx: Transaction<'a, Any>,
    postgres: bool,
    cache: Arc<Mutex<HashMap<String, &'static str>>>,
}

impl std::fmt::Debug for DbTx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbTx")
            .field("postgres", &self.postgres)
            .finish_non_exhaustive()
    }
}

impl<'a> DbTx<'a> {
    /// Commit transaksi (drop tanpa commit = rollback, seperti biasa).
    pub(crate) async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await
    }
}

// CATATAN transaksi: repo memakai `&mut tx` (bukan `&mut *tx`) untuk
// `DbTx` — `DbTx` sengaja TIDAK mengimplementasikan `Deref`, supaya
// `&mut tx` langsung berarti `&mut DbTx` dan query transaksi lewat
// impl `Executor` di bawah (rewrite placeholder tetap jalan di mode
// Postgres). `Deref<Target = Self>` dicoba dan DITOLAK compiler
// (E0055, auto-deref rekursif); `Deref` ke `AnyConnection` juga
// ditolak desain (query transaksi akan lolos tanpa rewrite).

/// Tulis ulang placeholder `?` menjadi `$1, $2, ...` (Postgres).
/// `?` di dalam literal string (`'...'`, `''` = escape), identifier
/// (`"..."`), dan komentar `-- ...` dibiarkan apa adanya.
fn rewrite_placeholders(sql: &str) -> String {
    #[derive(PartialEq)]
    enum St {
        Normal,
        Str,
        Ident,
        Comment,
    }
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0u32;
    let mut chars = sql.chars().peekable();
    let mut st = St::Normal;
    while let Some(c) = chars.next() {
        match st {
            St::Normal => match c {
                '\'' => {
                    st = St::Str;
                    out.push(c);
                }
                '"' => {
                    st = St::Ident;
                    out.push(c);
                }
                '-' if chars.peek() == Some(&'-') => {
                    st = St::Comment;
                    out.push(c);
                }
                '?' => {
                    n += 1;
                    out.push_str(&format!("${n}"));
                }
                _ => out.push(c),
            },
            St::Str => {
                out.push(c);
                if c == '\'' {
                    if chars.peek() == Some(&'\'') {
                        out.push(chars.next().unwrap());
                    } else {
                        st = St::Normal;
                    }
                }
            }
            St::Ident => {
                out.push(c);
                if c == '"' {
                    st = St::Normal;
                }
            }
            St::Comment => {
                out.push(c);
                if c == '\n' {
                    st = St::Normal;
                }
            }
        }
    }
    out
}

/// Stream satu-item untuk kegagalan `take_arguments` di `fetch_many`
/// (praktis tidak pernah terjadi untuk tipe argumen yang dipakai repo).
struct OnceError {
    err: Option<sqlx::Error>,
}

impl Stream for OnceError {
    type Item = Result<Either<AnyQueryResult, AnyRow>, sqlx::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.err.take() {
            Some(e) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

impl DbPool {
    /// Pasang driver `Any` (SQLite + Postgres) sekali per proses.
    fn install_drivers() {
        sqlx::any::install_default_drivers();
    }

    async fn connect_sqlite(url: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        Self::install_drivers();
        let inner = AnyPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await?;
        Ok(Self {
            inner,
            postgres: false,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn connect_postgres(url: &str) -> Result<Self, sqlx::Error> {
        Self::install_drivers();
        let inner = AnyPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        Ok(Self {
            inner,
            postgres: true,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn is_postgres(&self) -> bool {
        self.postgres
    }

    /// Mulai transaksi. Kueri di dalamnya dieksekusi lewat `DbTx`
    /// (bukan koneksi mentah) supaya rewrite placeholder tetap jalan.
    pub(crate) async fn begin(&self) -> Result<DbTx<'_>, sqlx::Error> {
        let tx = self.inner.begin().await?;
        Ok(DbTx {
            tx,
            postgres: self.postgres,
            cache: Arc::clone(&self.cache),
        })
    }

    /// SQL yang akan dieksekusi: di mode Postgres, `?` ditulis ulang
    /// menjadi `$1, $2, ...`; di mode SQLite SQL dipakai apa adanya.
    fn rewritten_sql<'s>(&self, sql: &'s str) -> &'s str {
        if !self.postgres {
            return sql;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = cache.get(sql) {
            return hit;
        }
        let rewritten = rewrite_placeholders(sql);
        let leaked: &'static str = Box::leak(rewritten.into_boxed_str());
        cache.insert(sql.to_string(), leaked);
        leaked
    }

    /// Ambil SQL + argumen + flag persistent dari query masuk; SQL
    /// ditulis ulang bila perlu. `statement()` yang sudah di-prepare
    /// diabaikan — repo ini tidak pernah memakai `.prepare()` eksplisit.
    /// (Bentuk tuple, bukan `Query`, karena tipe `Query` tidak punya
    /// path publik dari crate `sqlx`.)
    ///
    /// Mode Postgres MEMAKSA `persistent = false`. Alasannya bug di
    /// sqlx 0.8.6 (`sqlx-core/src/any/arguments.rs`, `convert_to`):
    /// `NULL` dari `Option<f64>` dikonversi ke `Option::<f32>::None`
    /// (Real/Double tertukar), jadi eksekusi pertama sebuah statement
    /// yang mengikat NULL di kolom float mendeklarasikan parameter itu
    /// sebagai float4 di Parse; statement yang di-cache per koneksi
    /// itu lalu menolak eksekusi berikut yang mengikat `Some(f64)`
    /// ("incorrect binary data format in bind parameter N"). Buktinya
    /// non-deterministik: seed `lab_results` gagal di baris 6 atau 7
    /// tergantung koneksi pool mana yang menyiapkan statement-nya.
    /// Tanpa cache (UNNAMED statement), tiap eksekusi me-Parse ulang
    /// dengan tipe argumennya sendiri dan konsisten. Biayanya satu
    /// round-trip Parse per query di mode Postgres; SQLite tidak kena
    /// (tipenya dinamis) dan tetap memakai cache.
    fn query_parts<'q, E>(
        &self,
        mut query: E,
    ) -> Result<(&'q str, Option<AnyArguments<'q>>, bool), sqlx::Error>
    where
        E: Execute<'q, Any>,
    {
        let sql = self.rewritten_sql(query.sql());
        let persistent = query.persistent() && !self.postgres;
        let args = query.take_arguments().map_err(sqlx::Error::Encode)?;
        Ok((sql, args, persistent))
    }
}

impl<'c> Executor<'c> for &'c DbPool {
    type Database = Any;

    fn fetch_many<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxStream<'e, Result<Either<AnyQueryResult, AnyRow>, sqlx::Error>>
    where
        'c: 'e,
        E: 'q + Execute<'q, Any>,
    {
        match self.query_parts(query) {
            Ok((sql, args, persistent)) => {
                let q = match args {
                    Some(a) => sqlx::query_with(sql, a).persistent(persistent),
                    None => sqlx::query(sql).persistent(persistent),
                };
                (&self.inner).fetch_many(q)
            }
            Err(e) => Box::pin(OnceError { err: Some(e) }),
        }
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<AnyRow>, sqlx::Error>>
    where
        'c: 'e,
        E: 'q + Execute<'q, Any>,
    {
        let pool = &self.inner;
        Box::pin(async move {
            let (sql, args, persistent) = self.query_parts(query)?;
            let q = match args {
                Some(a) => sqlx::query_with(sql, a).persistent(persistent),
                None => sqlx::query(sql).persistent(persistent),
            };
            pool.fetch_optional(q).await
        })
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        sql: &'q str,
        parameters: &'e [AnyTypeInfo],
    ) -> BoxFuture<'e, Result<AnyStatement<'q>, sqlx::Error>>
    where
        'c: 'e,
    {
        let sql = self.rewritten_sql(sql);
        let pool = &self.inner;
        Box::pin(async move { pool.prepare_with(sql, parameters).await })
    }

    fn describe<'e, 'q: 'e>(
        self,
        sql: &'q str,
    ) -> BoxFuture<'e, Result<Describe<Any>, sqlx::Error>>
    where
        'c: 'e,
    {
        let sql = self.rewritten_sql(sql);
        let pool = &self.inner;
        Box::pin(async move { pool.describe(sql).await })
    }
}

impl<'c, 'a> Executor<'c> for &'c mut DbTx<'a> {
    type Database = Any;

    fn fetch_many<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxStream<'e, Result<Either<AnyQueryResult, AnyRow>, sqlx::Error>>
    where
        'c: 'e,
        E: 'q + Execute<'q, Any>,
    {
        // `&mut *self.tx` = `&mut AnyConnection` (Transaction deref ke
        // koneksi); eksekutor koneksi itulah yang dipakai, bukan
        // `&mut Transaction` (tidak ada impl Executor untuknya).
        match self.query_parts(query) {
            Ok((sql, args, persistent)) => {
                let q = match args {
                    Some(a) => sqlx::query_with(sql, a).persistent(persistent),
                    None => sqlx::query(sql).persistent(persistent),
                };
                (&mut *self.tx).fetch_many(q)
            }
            Err(e) => Box::pin(OnceError { err: Some(e) }),
        }
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<AnyRow>, sqlx::Error>>
    where
        'c: 'e,
        E: 'q + Execute<'q, Any>,
    {
        Box::pin(async move {
            let (sql, args, persistent) = self.query_parts(query)?;
            let q = match args {
                Some(a) => sqlx::query_with(sql, a).persistent(persistent),
                None => sqlx::query(sql).persistent(persistent),
            };
            (&mut *self.tx).fetch_optional(q).await
        })
    }

    fn prepare_with<'e, 'q: 'e>(
        self,
        sql: &'q str,
        parameters: &'e [AnyTypeInfo],
    ) -> BoxFuture<'e, Result<AnyStatement<'q>, sqlx::Error>>
    where
        'c: 'e,
    {
        let sql = self.rewritten_sql(sql);
        Box::pin(async move { (&mut *self.tx).prepare_with(sql, parameters).await })
    }

    fn describe<'e, 'q: 'e>(
        self,
        sql: &'q str,
    ) -> BoxFuture<'e, Result<Describe<Any>, sqlx::Error>>
    where
        'c: 'e,
    {
        let sql = self.rewritten_sql(sql);
        Box::pin(async move { (&mut *self.tx).describe(sql).await })
    }
}

impl<'a> DbTx<'a> {
    fn rewritten_sql<'s>(&self, sql: &'s str) -> &'s str {
        if !self.postgres {
            return sql;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = cache.get(sql) {
            return hit;
        }
        let rewritten = rewrite_placeholders(sql);
        let leaked: &'static str = Box::leak(rewritten.into_boxed_str());
        cache.insert(sql.to_string(), leaked);
        leaked
    }

    /// Sama seperti `DbPool::query_parts`: mode Postgres memaksa
    /// `persistent = false` (lihat penjelasan di sana — cache statement
    /// per koneksi sqlx 0.8.6 meracuni tipe parameter NULL float).
    fn query_parts<'q, E>(
        &self,
        mut query: E,
    ) -> Result<(&'q str, Option<AnyArguments<'q>>, bool), sqlx::Error>
    where
        E: Execute<'q, Any>,
    {
        let sql = self.rewritten_sql(query.sql());
        let persistent = query.persistent() && !self.postgres;
        let args = query.take_arguments().map_err(sqlx::Error::Encode)?;
        Ok((sql, args, persistent))
    }
}

#[derive(Debug, Clone)]
pub struct Db {
    pool: DbPool,
}

impl Db {
    /// Buka pool SQLite. `path` boleh path file (`waras.db`), URL
    /// `sqlite:...`, atau `sqlite::memory:` (untuk test).
    pub async fn connect(path: &str) -> Result<Self, WarasError> {
        let url = if path.contains("://") {
            path.to_string()
        } else {
            format!("sqlite:{path}?mode=rwc")
        };
        let in_memory = url.contains(":memory:");
        let pool = DbPool::connect_sqlite(&url, if in_memory { 1 } else { 5 })
            .await
            .map_err(|e| WarasError::Database(format!("gagal membuka DB {path}: {e}")))?;
        Ok(Self { pool })
    }

    /// Buka pool PostgreSQL (mode rumah sakit). `url` bentuk
    /// `postgres://user:pass@host:5432/db` (skema `postgresql://`
    /// juga diterima driver). Driver + migrasi dipilih dari skema URL.
    pub async fn connect_postgres(url: &str) -> Result<Self, WarasError> {
        let pool = DbPool::connect_postgres(url)
            .await
            .map_err(|e| WarasError::Database(format!("gagal membuka DB postgres: {e}")))?;
        Ok(Self { pool })
    }

    /// Buka DB untuk test (Fase A, A2): bila env `WARAS_TEST_PG_URL`
    /// diisi (`postgres://user:pass@host:5432/db_template`), buat satu
    /// database Postgres FRESH per pemanggilan (nama unik
    /// `waras_t<pid>_<n>`, dibuat lewat koneksi admin ke database
    /// `postgres`) lalu connect ke sana. Bila env kosong/tidak diisi,
    /// SQLite in-memory seperti biasa.
    ///
    /// Satu database per test supaya test boleh jalan paralel tanpa
    /// berebut skema/seed. Database test TIDAK di-drop otomatis (test
    /// tidak tahu kapan pool terakhir dipakai); bersihkan berkala:
    /// `psql -c "SELECT 'DROP DATABASE \"' || datname || '\";' FROM
    /// pg_database WHERE datname LIKE 'waras_t%'"` lalu jalankan
    /// hasilnya. Nama dibuat dari pid + counter atomik saja (tanpa
    /// crate `url` baru — parsing manual di bawah).
    pub async fn connect_for_tests() -> Result<Self, WarasError> {
        let url = match std::env::var("WARAS_TEST_PG_URL") {
            Ok(u) if !u.trim().is_empty() => u.trim().to_string(),
            _ => return Db::connect("sqlite::memory:").await,
        };
        static NEXT_TEST_DB: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let n = NEXT_TEST_DB.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = format!("waras_t{}_{}", std::process::id(), n);
        let admin_url = replace_db_in_url(&url, "postgres").ok_or_else(|| {
            WarasError::Database(format!(
                "WARAS_TEST_PG_URL tidak bisa diparse: {url}"
            ))
        })?;
        let admin = DbPool::connect_postgres(&admin_url).await.map_err(|e| {
            WarasError::Database(format!("gagal membuka DB postgres (admin): {e}"))
        })?;
        sqlx::query(&format!("CREATE DATABASE \"{name}\""))
            .execute(&admin.inner)
            .await
            .map_err(|e| {
                WarasError::Database(format!("gagal membuat database test {name}: {e}"))
            })?;
        drop(admin);
        let test_url = replace_db_in_url(&url, &name).ok_or_else(|| {
            WarasError::Database(format!(
                "WARAS_TEST_PG_URL tidak bisa diparse: {url}"
            ))
        })?;
        Db::connect_postgres(&test_url).await
    }

    /// Backend yang dipakai koneksi ini: `"sqlite"` atau `"postgres"`.
    /// Dipakai `/api/health` (koordinator) + log startup + footer UI.
    pub fn mode(&self) -> &'static str {
        if self.pool.is_postgres() {
            "postgres"
        } else {
            "sqlite"
        }
    }

    /// Jalankan migrasi embedded. Direktori dipilih dari backend:
    /// `migrations/` (SQLite) atau `migrations-pg/` (PostgreSQL),
    /// nomor versi sama (0001–0004).
    pub async fn migrate(&self) -> Result<(), WarasError> {
        if self.pool.is_postgres() {
            sqlx::migrate!("./migrations-pg")
                .run(&self.pool.inner)
                .await
                .map_err(|e| WarasError::Database(format!("migrasi gagal: {e}")))
        } else {
            sqlx::migrate!("./migrations")
                .run(&self.pool.inner)
                .await
                .map_err(|e| WarasError::Database(format!("migrasi gagal: {e}")))
        }
    }

    /// Seed bila tabel `users` kosong. Idempotent; mengembalikan `true`
    /// bila seed dijalankan, `false` bila DB sudah berisi data.
    pub async fn seed_if_empty(&self) -> Result<bool, WarasError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if n > 0 {
            return Ok(false);
        }
        seed::seed(self).await?;
        Ok(true)
    }

    // -- helpers password (Argon2) ----------------------------------------

    /// Hash Argon2 untuk password baru / seed.
    pub fn hash_password(password: &str) -> Result<String, WarasError> {
        use argon2::password_hash::SaltString;
        // Salt 16 byte acak dari rand (bukan OsRng password-hash: rand_core
        // 0.6 di argon2 tidak mengaktifkan fitur getrandom).
        let salt_bytes: [u8; 16] = rand::random();
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|e| WarasError::Internal(format!("salt gagal: {e}")))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| WarasError::Internal(format!("hash password gagal: {e}")))
    }

    /// Verifikasi password terhadap hash Argon2 yang tersimpan.
    pub fn verify_password(password_hash: &str, password: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    /// Hash rantai audit: sha256 hex atas gabungan
    /// `prev_hash + timestamp + actor + action + resource_type +
    /// resource_id + details_json` (tanpa pemisah).
    pub fn audit_entry_hash(
        prev_hash: &str,
        timestamp: NaiveDateTime,
        actor: &str,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        details: &serde_json::Value,
    ) -> String {
        let payload = format!(
            "{prev_hash}{timestamp}{actor}{action}{resource_type}{resource_id}{details}"
        );
        hex_encode(Sha256::digest(payload.as_bytes()))
    }

    /// Hitung jumlah tablet dari dosis + durasi, aturan KONTRAK §3:
    /// angka hari x frekuensi per hari. Mis. dosage `1x1` +
    /// duration `3 hari` = 3 tablet; `3x1, 30 hari` = 90.
    /// Format lama `500 mg 1x1` tetap diparse (kekuatan diabaikan),
    /// karena `dosage` kanonik kini hanya frekuensi
    /// (lihat `waras_core::dosage::normalize_dosage`).
    /// Bila tidak bisa diparse, kembalikan 1 (jangan gagalkan dispensing).
    pub fn dose_count(dosage: &str, duration: &str) -> i64 {
        let freq = dosage
            .split_whitespace()
            .find(|t| t.contains('x'))
            .and_then(|t| t.split('x').next())
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        let days = duration
            .split_whitespace()
            .find_map(|t| t.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1);
        freq * days
    }

    /// Nama hari Indonesia untuk grafik kunjungan 7 hari.
    pub fn day_name_id(date: NaiveDate) -> &'static str {
        match date.weekday() {
            chrono::Weekday::Mon => "Senin",
            chrono::Weekday::Tue => "Selasa",
            chrono::Weekday::Wed => "Rabu",
            chrono::Weekday::Thu => "Kamis",
            chrono::Weekday::Fri => "Jumat",
            chrono::Weekday::Sat => "Sabtu",
            chrono::Weekday::Sun => "Minggu",
        }
    }
}

// -- helper internal --------------------------------------------------------

pub(crate) fn db_err(e: sqlx::Error) -> WarasError {
    WarasError::Database(e.to_string())
}

/// Apakah pesan error DB ini pelanggaran unique/PK (A2)? Dipakai retry
/// alokasi id baca-max-lalu-insert: di Postgres dua penulis bersamaan
/// bisa menghitung id yang sama dan yang kedua gagal `duplicate key
/// value violates unique constraint`; di SQLite pesannya `UNIQUE
/// constraint failed`. Hanya pola itu yang di-retry (maks 5x) — error
/// lain diteruskan apa adanya.
pub(crate) fn is_unique_violation_msg(msg: &str) -> bool {
    msg.contains("duplicate key value violates unique constraint")
        || msg.contains("UNIQUE constraint failed")
}

pub(crate) fn fmt_dt(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Jam sekarang (waktu lokal, WIB di sandbox) sebagai `NaiveDateTime`.
pub(crate) fn now_naive() -> NaiveDateTime {
    Utc::now().naive_utc()
}

/// Jam sekarang sebagai string `HH.MM` (format jam di UI/desain).
pub(crate) fn now_time_str() -> String {
    chrono::Local::now().format("%H.%M").to_string()
}

pub(crate) fn parse_date(s: &str) -> Result<NaiveDate, WarasError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| WarasError::Database(format!("tanggal tidak valid {s:?}: {e}")))
}

pub(crate) fn parse_dt(s: &str) -> Result<NaiveDateTime, WarasError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map_err(|e| WarasError::Database(format!("timestamp tidak valid {s:?}: {e}")))
}

pub(crate) fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Token sesi: 32 byte acak -> 64 karakter hex.
pub(crate) fn random_token_hex() -> String {
    let bytes: [u8; 32] = rand::random();
    hex_encode(bytes)
}

pub(crate) fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Ambil angka terbesar dari suffix numerik id-id yang ada
/// (mis. `RX-1041` -> 1041), lalu +1. Bila tidak ada, `start`.
pub(crate) fn next_seq_from_ids(ids: &[String], start: u32) -> u32 {
    ids.iter()
        .filter_map(|id| {
            id.rsplit('-')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .max()
        .map(|m| m + 1)
        .unwrap_or(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::types::DashboardKpis;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests()
            .await
            .expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[test]
    fn dose_count_parses_contract_example() {
        assert_eq!(Db::dose_count("1x1", "3 hari"), 3);
        assert_eq!(Db::dose_count("3x1", "30 hari"), 90);
        assert_eq!(Db::dose_count("2x1", "5 hari"), 10);
        // Format lama (kekuatan di dosage) tetap diparse.
        assert_eq!(Db::dose_count("500 mg 1x1", "3 hari"), 3);
        assert_eq!(Db::dose_count("tanpa pola", "7 hari"), 7);
    }

    #[test]
    fn clinic_today_defaults_to_design_date() {
        std::env::remove_var("WARAS_TODAY");
        assert_eq!(
            clinic_today(),
            NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
        );
    }

    #[test]
    fn replace_db_in_url_swaps_database_segment() {
        assert_eq!(
            replace_db_in_url("postgres://waras:pass@127.0.0.1:5432/waras_pg_test", "waras_t1_0"),
            Some("postgres://waras:pass@127.0.0.1:5432/waras_t1_0".to_string())
        );
        assert_eq!(
            replace_db_in_url(
                "postgres://u:p@127.0.0.1:5432/db?sslmode=disable",
                "waras_t1_1"
            ),
            Some("postgres://u:p@127.0.0.1:5432/waras_t1_1?sslmode=disable".to_string())
        );
        assert_eq!(replace_db_in_url("postgres://127.0.0.1:5432", "x"), None);
        assert_eq!(replace_db_in_url("not-a-url", "x"), None);
    }

    #[test]
    fn rewrite_placeholders_numbers_params() {
        assert_eq!(
            rewrite_placeholders("SELECT * FROM users WHERE username = ? OR nip = ?"),
            "SELECT * FROM users WHERE username = $1 OR nip = $2"
        );
        // `?` di dalam literal string dibiarkan.
        assert_eq!(
            rewrite_placeholders("SELECT * FROM t WHERE a = '?' AND b = ?"),
            "SELECT * FROM t WHERE a = '?' AND b = $1"
        );
        // Escape '' di dalam string tidak mengakhiri literal.
        assert_eq!(
            rewrite_placeholders("SELECT 'it''s ?' FROM t WHERE x = ?"),
            "SELECT 'it''s ?' FROM t WHERE x = $1"
        );
        assert_eq!(rewrite_placeholders("SELECT 1"), "SELECT 1");
        assert_eq!(
            rewrite_placeholders(
                "INSERT INTO outbox (id) VALUES (?) ON CONFLICT(id) DO NOTHING"
            ),
            "INSERT INTO outbox (id) VALUES ($1) ON CONFLICT(id) DO NOTHING"
        );
    }

    #[tokio::test]
    async fn db_mode_is_sqlite_by_default() {
        // Sengaja SQLite langsung (bukan connect_for_tests): test ini
        // membuktikan default-nya sqlite walau env PG diisi.
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        assert_eq!(db.mode(), "sqlite");
    }

    #[tokio::test]
    async fn connect_postgres_refused_is_error_not_panic() {
        // Tanpa server Postgres: konstruktor harus Err, bukan panic.
        let r = Db::connect_postgres("postgres://127.0.0.1:1/waras").await;
        assert!(r.is_err());
    }

    /// Smoke test jalur Postgres end-to-end (migrasi PG + seed + mode +
    /// transaksi + id audit). Hanya jalan bila env `WARAS_TEST_PG_URL`
    /// diisi, mis. `postgres://waras:pass@127.0.0.1:5432/waras_pg_test`;
    /// bila kosong, test di-skip (pass) supaya `cargo test` biasa tetap
    /// jalan tanpa server. DB harus KOSONG (fresh) — migrasi sqlx
    /// menganggap skema yang dibuat manual tanpa `_sqlx_migrations`
    /// sebagai belum dimigrasi dan ALTER 0002 akan gagal.
    #[tokio::test]
    async fn postgres_smoke_via_env() {
        let Ok(url) = std::env::var("WARAS_TEST_PG_URL") else {
            eprintln!("SKIP postgres_smoke_via_env (WARAS_TEST_PG_URL tidak diisi)");
            return;
        };
        let db = Db::connect_postgres(&url)
            .await
            .expect("buka postgres");
        assert_eq!(db.mode(), "postgres");
        db.migrate().await.expect("migrasi PG");
        db.seed_if_empty().await.expect("seed PG");
        let patients = db.list_patients(None).await.expect("list pasien PG");
        assert_eq!(patients.len(), 14, "seed Fase 1 + 2 pasien Fase 3 + 2 pasien Fase 4");
        // Transaksi + placeholder `?` di mode Postgres: klaim batch
        // outbox (kosong) — membuktikan begin/commit + rewrite jalan.
        let jobs = db
            .claim_outbox_batch_atomic(10, "2026-11-19 00:00:00")
            .await
            .expect("claim batch PG");
        assert!(jobs.is_empty());
        // Audit: id hasil insert harus id nyata (> 0), bukan 0.
        let entry = db
            .log_audit(
                "test-pg",
                "test.smoke",
                "test",
                "T-PG",
                serde_json::json!({"x": 1}),
            )
            .await
            .expect("audit PG");
        assert!(entry.id > 0, "id audit PG harus nyata, dapat {}", entry.id);
    }

    #[tokio::test]
    async fn seed_loads_exact_counts() {
        let db = seeded_db().await;
        let patients = db.list_patients(None).await.unwrap();
        assert_eq!(patients.len(), 14);
        let queue = db.list_queue(clinic_today()).await.unwrap();
        assert_eq!(queue.len(), 44);
        let rx = db
            .list_prescriptions(PrescriptionFilter::default())
            .await
            .unwrap();
        assert_eq!(rx.len(), 8);
        let drugs = db.list_drugs().await.unwrap();
        assert_eq!(drugs.len(), 9);
        let invoices = db.list_invoices(None).await.unwrap();
        assert_eq!(invoices.len(), 8);
        let appointments = db.list_appointments(None).await.unwrap();
        assert_eq!(appointments.len(), 31);
        let notifs = db.list_notifications(clinic_today()).await.unwrap();
        assert_eq!(notifs.len(), 3);
        let allergies = db.get_allergies("RM-000812").await.unwrap();
        assert_eq!(allergies.len(), 1);
        assert_eq!(allergies[0].substance, "Penisilin");
        let conditions = db.get_conditions("RM-000812").await.unwrap();
        assert_eq!(conditions.len(), 7);
        // 31 janji: hitung lewat KPI followup (6 hari ini + 25 kontrol ulang).
        let kpis = db.dashboard_kpis(clinic_today()).await.unwrap();
        assert_eq!(kpis.followups_7d, 31);
        // 7 baris daily_stats -> visits_7d berisi 7 hari.
        assert_eq!(kpis.visits_7d.len(), 7);
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let db = seeded_db().await;
        assert!(!db.seed_if_empty().await.unwrap(), "seed kedua harus skip");
        assert_eq!(db.list_patients(None).await.unwrap().len(), 14);
    }

    #[tokio::test]
    async fn seed_users_can_login() {
        let db = seeded_db().await;
        let user = db
            .find_user_by_username_or_nip("ahmad")
            .await
            .unwrap()
            .expect("ahmad ada");
        assert_eq!(user.id, "U-001");
        assert!(Db::verify_password(&user.password_hash, "waras123"));
        assert!(!Db::verify_password(&user.password_hash, "salah"));
        // Login lewat NIP juga bisa.
        let by_nip = db
            .find_user_by_username_or_nip("198505122010011001")
            .await
            .unwrap()
            .expect("NIP ahmad ada");
        assert_eq!(by_nip.username, "ahmad");
    }

    #[tokio::test]
    async fn sessions_roundtrip() {
        let db = seeded_db().await;
        let token = db.create_session("U-001").await.unwrap();
        assert_eq!(token.len(), 64, "32 byte acak -> 64 hex");
        let user = db.find_user_by_token(&token).await.unwrap().unwrap();
        assert_eq!(user.username, "ahmad");
        db.delete_session(&token).await.unwrap();
        assert!(db.find_user_by_token(&token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prescribe_blocks_amoxicillin_allows_azithromycin() {
        let db = seeded_db().await;
        let blocked = db
            .prescribe(
                "dr. Ahmad Hidayat",
                NewPrescription {
                    encounter_id: "ENC-2026-11-19-014".into(),
                    drug_name: "Amoxicillin 500 mg".into(),
                    drug_class: "penicillin".into(),
                    dosage: "1x1".into(),
                    duration: "3 hari".into(),
                },
            )
            .await;
        assert!(matches!(blocked, Err(WarasError::Blocked(_))));
        // Resep yang diblokir tetap tersimpan dengan status Diblokir.
        let rx = db
            .list_prescriptions(PrescriptionFilter {
                status: Some(PrescriptionStatus::Diblokir),
                patient_id: Some("RM-000812".into()),
            })
            .await
            .unwrap();
        assert_eq!(rx.len(), 2, "seed RX-1040 + yang baru diblokir");
        assert!(rx.iter().all(|r| r.blocked_reason.is_some()));

        let ok = db
            .prescribe(
                "dr. Ahmad Hidayat",
                NewPrescription {
                    encounter_id: "ENC-2026-11-19-014".into(),
                    drug_name: "Azithromycin 500 mg".into(),
                    drug_class: "macrolide".into(),
                    dosage: "1x1".into(),
                    duration: "3 hari".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(ok.status, PrescriptionStatus::Aktif);
    }

    #[tokio::test]
    async fn dashboard_kpis_match_contract() {
        let db = seeded_db().await;
        let k: DashboardKpis = db.dashboard_kpis(clinic_today()).await.unwrap();
        assert_eq!(k.queue_total, 44);
        assert_eq!(k.queue_active, 32);
        assert_eq!(k.queue_waiting, 15);
        assert_eq!(k.queue_in_consultation, 17);
        assert_eq!(k.queue_done, 10);
        assert_eq!(k.queue_no_show, 2);
        assert_eq!(k.followups_7d, 31);
        assert_eq!(k.visits_avg_7d, 178);
        assert_eq!(k.visits_peak_7d, 210);
        assert_eq!(k.patients_today, 248);
        assert_eq!(k.revenue_today_idr, 18_400_000);
        assert_eq!(k.revenue_transactions, 142);
        assert_eq!(k.avg_wait_minutes, 18);
        assert_eq!(k.longest_wait_minutes, 45);
        assert_eq!(k.unpaid_invoices_count, 4);
        assert_eq!(k.unpaid_invoices_idr, 5_875_000);
    }

    #[tokio::test]
    async fn audit_hash_chain_verifies() {
        let db = seeded_db().await;
        // Tambah satu entri, lalu verifikasi seluruh rantai dari awal.
        db.log_audit(
            "tester",
            "test.cek",
            "test",
            "T-1",
            serde_json::json!({"x": 1}),
        )
        .await
        .unwrap();
        let entries = db.list_audit(100).await.unwrap();
        assert_eq!(entries.len(), 7, "6 seed + 1 baru");
        // list_audit mengembalikan terbaru dulu; balik untuk verifikasi rantai.
        let mut prev = "GENESIS".to_string();
        for e in entries.iter().rev() {
            assert_eq!(e.prev_hash, prev, "prev_hash entri {}", e.id);
            let recomputed = Db::audit_entry_hash(
                &e.prev_hash,
                e.timestamp,
                &e.actor,
                &e.action,
                &e.resource_type,
                &e.resource_id,
                &e.details,
            );
            assert_eq!(recomputed, e.entry_hash, "entry_hash entri {}", e.id);
            prev = e.entry_hash.clone();
        }
    }

    #[tokio::test]
    async fn soap_versioning_keeps_old_versions() {
        let db = seeded_db().await;
        let soap = db.get_soap("ENC-2026-11-19-014").await.unwrap().unwrap();
        assert_eq!(soap.version, 3);
        let versions = db.get_soap_versions(&soap.id).await.unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].subjective.contains("Batuk"), true);
        assert!(versions[0].objective.is_empty(), "v1 hanya S");
        assert!(!versions[2].plan.is_empty(), "v3 lengkap");

        let saved = db
            .save_soap(
                "ENC-2026-11-19-014",
                "Batuk berdahak 3 hari. Demam malam hari.",
                "Paru: ronki basal kanan.",
                "Bronkitis akut (J20.9).",
                "Azithromycin 500 mg 1x1, 3 hari. Kontrol 3 hari lagi.",
                "dr. Ahmad Hidayat",
            )
            .await
            .unwrap();
        assert_eq!(saved.version, 4);
        let versions = db.get_soap_versions(&soap.id).await.unwrap();
        assert_eq!(versions.len(), 4, "versi lama tidak ditimpa");
        assert_eq!(versions[0].version, 1);
        assert_eq!(versions[3].version, 4);
    }

    #[tokio::test]
    async fn queue_take_assigns_next_number() {
        let db = seeded_db().await;
        let entry = db
            .take_queue("RM-000813", "Poli Umum", "dr. Ahmad Hidayat", clinic_today())
            .await
            .unwrap();
        assert_eq!(entry.number, "A-045");
        let called = db.call_queue(&entry.id).await.unwrap();
        assert_eq!(called.status, waras_core::types::QueueStatus::Dipanggil);
        assert!(called.called_at.is_some());
    }

    #[tokio::test]
    async fn dispense_decrements_stock() {
        let db = seeded_db().await;
        // RX-1042 Paracetamol 500 mg 3x1, 3 hari = 9 tablet; stok 1240 -> 1231.
        let before = db.get_drug("KFA-9202").await.unwrap().unwrap();
        assert_eq!(before.stock, 1240);
        db.dispense_prescription("RX-1042", "apt. Dina Puspita")
            .await
            .unwrap();
        let after = db.get_drug("KFA-9202").await.unwrap().unwrap();
        assert_eq!(after.stock, 1231);
        let rx = db.get_prescription("RX-1042").await.unwrap().unwrap();
        assert_eq!(rx.status, PrescriptionStatus::Diberikan);
    }

    #[tokio::test]
    async fn pay_invoice_marks_lunas() {
        let db = seeded_db().await;
        // INV-2026-1183 Dewi 450.000 Menunggu; bayar penuh -> Lunas.
        let inv = db
            .pay_invoice(
                "INV-2026-1183",
                waras_core::types::PaymentMethod::Tunai,
                450_000,
                "Sinta Maharani",
            )
            .await
            .unwrap();
        assert_eq!(inv.status, waras_core::types::InvoiceStatus::Lunas);
        let payments = db.get_payments("INV-2026-1183").await.unwrap();
        assert_eq!(payments.len(), 1);
        assert_eq!(payments[0].amount, 450_000);
    }

    #[tokio::test]
    async fn create_patient_assigns_next_rm() {
        let db = seeded_db().await;
        let p = db
            .create_patient(NewPatient {
                nik: "3374010101900001".into(),
                name: "Test Pasien".into(),
                birth_date: NaiveDate::from_ymd_opt(1990, 1, 1).unwrap(),
                gender: Gender::LakiLaki,
                address: "Jl. Test No. 1".into(),
                phone: None,
                blood_type: None,
                bpjs_number: None,
                insurance: Insurance::Umum,
            })
            .await
            .unwrap();
        assert_eq!(p.id, "RM-000826");
        let found = db.get_patient("RM-000826").await.unwrap().unwrap();
        assert_eq!(found.name, "Test Pasien");
    }

    #[tokio::test]
    async fn encounter_bundle_data_loads() {
        let db = seeded_db().await;
        let (patient, encounter, vitals, conditions, rx) = db
            .encounter_bundle_data("ENC-2026-11-19-014")
            .await
            .unwrap()
            .expect("bundle ada");
        assert_eq!(patient.id, "RM-000812");
        assert_eq!(encounter.id, "ENC-2026-11-19-014");
        assert_eq!(vitals.len(), 5);
        assert!(!conditions.is_empty());
        assert_eq!(rx.len(), 2, "RX-1040 + RX-1041");
    }
}
