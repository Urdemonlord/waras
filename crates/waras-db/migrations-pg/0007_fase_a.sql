-- WARAS migrasi Fase A (A4: keamanan produksi) — terjemahan
-- PostgreSQL dari crates/waras-db/migrations/0007_fase_a.sql,
-- nomor versi SAMA. INTEGER -> BIGINT (lihat 0001_init.sql).
-- PostgreSQL BISA menambah kolom nullable lewat ALTER biasa.
CREATE TABLE login_attempts (
  key TEXT PRIMARY KEY,               -- mis. "staff:127.0.0.1:ahmad"
  failed_count BIGINT NOT NULL DEFAULT 0,
  window_start TEXT NOT NULL,         -- YYYY-MM-DD HH:MM:SS, kegagalan pertama di jendela
  locked_until TEXT                   -- YYYY-MM-DD HH:MM:SS, NULL bila belum terkunci
);

CREATE TABLE api_rate_limits (
  key_id TEXT PRIMARY KEY,            -- id kunci API (AK-...)
  window_start TEXT NOT NULL,         -- YYYY-MM-DD HH:MM:SS, request pertama di jendela
  request_count BIGINT NOT NULL DEFAULT 0
);

ALTER TABLE patients ADD COLUMN portal_password_hash TEXT;
