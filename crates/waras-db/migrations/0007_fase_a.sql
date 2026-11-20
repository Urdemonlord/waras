-- WARAS migrasi Fase A (A4: keamanan produksi).
--
-- 1. `patients.portal_password_hash`: hash Argon2 kata sandi portal
--    pasien. NULL = pasien belum punya kata sandi portal (tidak bisa
--    login portal). Hash diisi seed (pasien seed: `waras123`, sama
--    seperti staf) + backfill saat boot untuk DB lama yang sudah
--    ter-seed sebelum migrasi ini (`Db::backfill_portal_passwords`).
--    Kata sandi TIDAK PERNAH disimpan mentah.
-- 2. `login_attempts`: pelacak brute-force per kunci `ip:pengguna`
--    (staf: `staff:<ip>:<username/nip>`, portal: `patient:<ip>:<nik>`).
--    5 gagal dalam jendela 15 menit -> `locked_until` terisi (+15 mnt
--    dari kegagalan ke-5). Login sukses menghapus barisnya.
--    Semua stempel waktu TEXT `YYYY-MM-DD HH:MM:SS` (UTC, lihat
--    `Db::now_naive`); logika jendela dihitung di Rust, BUKAN fungsi
--    tanggal SQL, supaya SQL-nya identik di SQLite dan PostgreSQL.
-- 3. `api_rate_limits`: jendela rate limit API publik per kunci API
--    (100 request / 60 detik). Persisten di DB: selamat dari restart
--    server (pengganti HashMap in-memory di `public_api.rs`).
CREATE TABLE login_attempts (
  key TEXT PRIMARY KEY,               -- mis. "staff:127.0.0.1:ahmad"
  failed_count INTEGER NOT NULL DEFAULT 0,
  window_start TEXT NOT NULL,         -- YYYY-MM-DD HH:MM:SS, kegagalan pertama di jendela
  locked_until TEXT                   -- YYYY-MM-DD HH:MM:SS, NULL bila belum terkunci
);

CREATE TABLE api_rate_limits (
  key_id TEXT PRIMARY KEY,            -- id kunci API (AK-...)
  window_start TEXT NOT NULL,         -- YYYY-MM-DD HH:MM:SS, request pertama di jendela
  request_count INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE patients ADD COLUMN portal_password_hash TEXT;
