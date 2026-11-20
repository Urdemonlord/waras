-- WARAS migrasi Fase 4 (Ekosistem) — terjemahan PostgreSQL dari
-- crates/waras-db/migrations/0006_fase4.sql, nomor versi SAMA.
-- INTEGER -> BIGINT, REAL -> DOUBLE PRECISION (lihat 0001_init.sql).
--
-- Sesi pasien: PostgreSQL BISA mengubah NOT NULL lewat ALTER
-- (tidak perlu rebuild seperti SQLite) — user_id dilonggarkan +
-- kolom patient_id baru.

ALTER TABLE sessions ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE sessions ADD COLUMN patient_id TEXT;

-- Hemodialisa ---------------------------------------------------------------
CREATE TABLE hd_machines (
  id TEXT PRIMARY KEY,                    -- HD-1
  name TEXT NOT NULL,                     -- Mesin HD 1
  status TEXT NOT NULL DEFAULT 'Siap'    -- Siap / Terpakai / Perawatan
);
CREATE TABLE hd_sessions (
  id TEXT PRIMARY KEY,                    -- HD-20261119-001
  patient_id TEXT NOT NULL,
  machine_id TEXT NOT NULL,
  encounter_id TEXT,
  dialyzer TEXT NOT NULL DEFAULT '',     -- mis. F6
  duration_minutes BIGINT NOT NULL DEFAULT 240,
  blood_flow BIGINT NOT NULL DEFAULT 300,      -- mL/mnt
  dialysate_flow BIGINT NOT NULL DEFAULT 500,  -- mL/mnt
  uf_goal_ml BIGINT NOT NULL DEFAULT 2000,
  status TEXT NOT NULL DEFAULT 'Terjadwal',     -- Terjadwal / Berjalan / Selesai / Batal
  scheduled_at TEXT NOT NULL,             -- YYYY-MM-DD HH:MM
  started_at TEXT,
  finished_at TEXT,
  notes TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- Transfusi darah ------------------------------------------------------------
CREATE TABLE blood_stock (
  id TEXT PRIMARY KEY,                    -- BL-A-POS-001
  blood_type TEXT NOT NULL,               -- A+ / A- / B+ / B- / O+ / O- / AB+ / AB-
  component TEXT NOT NULL,                -- WB / PRC / TC / FFP
  units BIGINT NOT NULL DEFAULT 0,
  expires_at TEXT NOT NULL,               -- YYYY-MM-DD
  status TEXT NOT NULL DEFAULT 'Tersedia' -- Tersedia / Menipis / Habis
);
CREATE TABLE blood_requests (
  id TEXT PRIMARY KEY,                    -- TR-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  blood_type TEXT NOT NULL,
  component TEXT NOT NULL,
  units BIGINT NOT NULL,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Menunggu', -- Menunggu / Crossmatch / Siap / Diberikan / Batal
  requested_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE blood_crossmatches (
  id TEXT PRIMARY KEY,                    -- XM-000001
  request_id TEXT NOT NULL,
  bag_id TEXT NOT NULL,                   -- id blood_stock
  result TEXT NOT NULL,                   -- Cocok / Tidak Cocok
  tested_by TEXT NOT NULL,
  tested_at TEXT NOT NULL
);

-- Gizi ------------------------------------------------------------------------
CREATE TABLE nutrition_assessments (
  id TEXT PRIMARY KEY,                    -- GZ-000001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  weight_kg DOUBLE PRECISION NOT NULL,
  height_cm DOUBLE PRECISION NOT NULL,
  bmi DOUBLE PRECISION NOT NULL,
  imt_category TEXT NOT NULL,             -- Kurus / Normal / Gemuk / Obesitas
  risk TEXT NOT NULL DEFAULT 'Rendah',    -- Rendah / Sedang / Tinggi
  diet TEXT NOT NULL DEFAULT '',          -- mis. DM 1800 kkal
  notes TEXT NOT NULL DEFAULT '',
  assessed_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE nutrition_diets (
  id TEXT PRIMARY KEY,                    -- DT-000001
  patient_id TEXT NOT NULL,
  diet_type TEXT NOT NULL,                -- DM / Jantung / Rendah Garam / Tinggi Protein / Biasa
  calories BIGINT NOT NULL DEFAULT 0,
  notes TEXT NOT NULL DEFAULT '',
  start_date TEXT NOT NULL,               -- YYYY-MM-DD
  status TEXT NOT NULL DEFAULT 'Aktif'    -- Aktif / Selesai
);

-- Rekam medis gigi -------------------------------------------------------------
CREATE TABLE dental_records (
  id TEXT PRIMARY KEY,                    -- GG-000001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  tooth_number TEXT NOT NULL,             -- notasi FDI: 11..48
  diagnosis TEXT NOT NULL,
  diagnosis_code TEXT NOT NULL DEFAULT '',-- ICD-10, mis. K02.9
  treatment TEXT NOT NULL,
  treatment_code TEXT NOT NULL DEFAULT '',-- ICD-9-CM, mis. 23.2
  dentist TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Selesai',-- Rencana / Berjalan / Selesai
  created_at TEXT NOT NULL
);

-- Inventori ---------------------------------------------------------------------
CREATE TABLE inventory_items (
  id TEXT PRIMARY KEY,                    -- INV-ITM-001
  name TEXT NOT NULL,
  category TEXT NOT NULL,                 -- Obat / Alkes / Umum
  unit TEXT NOT NULL,                     -- pcs / box / botol
  stock BIGINT NOT NULL DEFAULT 0,
  min_stock BIGINT NOT NULL DEFAULT 0,
  location TEXT NOT NULL DEFAULT '',      -- Gudang A / Rak C-01
  expiry TEXT NOT NULL DEFAULT '',        -- YYYY-MM-DD (boleh kosong)
  created_at TEXT NOT NULL
);
CREATE TABLE inventory_moves (
  id TEXT PRIMARY KEY,                    -- INM-000001
  item_id TEXT NOT NULL,
  kind TEXT NOT NULL,                     -- Masuk / Keluar / Opname
  qty BIGINT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  by_name TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Kepegawaian ---------------------------------------------------------------------
CREATE TABLE staff (
  id TEXT PRIMARY KEY,                    -- STF-001
  name TEXT NOT NULL,
  nip TEXT NOT NULL DEFAULT '',
  role TEXT NOT NULL,                     -- Dokter Umum / Perawat / Apoteker / Kasir / Admin / Bidan / Dokter Gigi
  poli TEXT NOT NULL DEFAULT '',
  phone TEXT NOT NULL DEFAULT '',
  active BIGINT NOT NULL DEFAULT 1
);
CREATE TABLE shift_schedules (
  id TEXT PRIMARY KEY,                    -- SHF-20261119-001
  staff_id TEXT NOT NULL,
  date TEXT NOT NULL,                     -- YYYY-MM-DD
  shift TEXT NOT NULL,                    -- Pagi / Siang / Malam
  poli TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- Pengaturan ------------------------------------------------------------------------
CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,                   -- backup_otomatis
  value TEXT NOT NULL,                    -- on / off / angka / teks
  updated_at TEXT NOT NULL
);

-- API Publik (kunci API) --------------------------------------------------------------
CREATE TABLE api_keys (
  id TEXT PRIMARY KEY,                    -- AK-000001
  name TEXT NOT NULL,                     -- mis. "Aplikasi Mobile JKN"
  key_prefix TEXT NOT NULL,               -- 12 karakter pertama kunci, mis. wrs_9f2k...
  key_hash TEXT NOT NULL,                 -- SHA-256 hex dari kunci penuh (TIDAK PERNAH simpan kunci penuh)
  scopes TEXT NOT NULL DEFAULT '',        -- dipisah koma: patients:read,queue:read,encounters:read,fhir:read
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked BIGINT NOT NULL DEFAULT 0
);
