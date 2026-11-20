-- WARAS migrasi Fase 3 (Rumah Sakit) — docs/FASE3.md §1, persis.
-- IGD, rawat inap, kamar operasi & bersalin, lab, radiologi, plus
-- rebuild tabel claims (invoice_id boleh NULL + kolom grouping_note).
-- Rebuild mempertahankan SEMUA klaim lama (termasuk CLM-20261119-001).

-- IGD ---------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS igd_visits (
  id TEXT PRIMARY KEY,                    -- IGD-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,                     -- fast-track encounter (boleh NULL)
  complaint TEXT NOT NULL,
  triage TEXT NOT NULL,                   -- merah / kuning / hijau
  doctor TEXT NOT NULL,                   -- dokter jaga
  status TEXT NOT NULL DEFAULT 'menunggu',-- menunggu / observasi / tindakan / resusitasi / selesai / dirujuk
  arrived_at TEXT NOT NULL,               -- HH.MM
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS igd_observations (
  id TEXT PRIMARY KEY,                    -- IGO-000001
  igd_id TEXT NOT NULL,
  note TEXT NOT NULL,                     -- teks observasi (TTV + rencana)
  recorded_by TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);

-- Rawat inap ---------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wards (
  id TEXT PRIMARY KEY,                    -- Melati / Mawar / Anggrek / Cempaka / ICU
  name TEXT NOT NULL,                     -- Melati
  kind TEXT NOT NULL,                     -- Interna / Bedah / Anak / Obstetri / ICU
  floor TEXT NOT NULL                     -- Lantai 2
);
CREATE TABLE IF NOT EXISTS ward_beds (
  id TEXT PRIMARY KEY,                    -- 3A-01
  ward_id TEXT NOT NULL,
  room TEXT NOT NULL,                     -- 3A
  bed_no TEXT NOT NULL,                   -- 01
  status TEXT NOT NULL DEFAULT 'kosong',  -- terisi / kosong / dibersihkan
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS admissions (
  id TEXT PRIMARY KEY,                    -- ADM-20261119-001
  patient_id TEXT NOT NULL,
  bed_id TEXT NOT NULL,
  encounter_id TEXT,
  diagnosis_code TEXT NOT NULL,           -- ICD-10
  diagnosis_display TEXT NOT NULL,
  admitted_date TEXT NOT NULL,            -- YYYY-MM-DD
  dpjp TEXT NOT NULL,
  fall_risk INTEGER NOT NULL DEFAULT 0,   -- 1 = Risiko Jatuh
  status TEXT NOT NULL DEFAULT 'dirawat', -- dirawat / pulang / dirujuk
  discharged_at TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS care_notes (
  id TEXT PRIMARY KEY,                    -- CRN-000001
  admission_id TEXT NOT NULL,
  note TEXT NOT NULL,
  recorded_by TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);

-- Kamar operasi & bersalin --------------------------------------------------
CREATE TABLE IF NOT EXISTS surgeries (
  id TEXT PRIMARY KEY,                    -- OK-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  room TEXT NOT NULL,                     -- OK 1 / OK 2 / OK 3
  procedure TEXT NOT NULL,
  procedure_code TEXT NOT NULL,           -- ICD-9-CM, mis. 47.0
  operator TEXT NOT NULL,
  team TEXT NOT NULL DEFAULT '',          -- teks tim, mis. "dr. Budi, Sp.B · dr. Ani, Sp.An · 2 perawat"
  scheduled_at TEXT NOT NULL,             -- YYYY-MM-DD HH:MM
  status TEXT NOT NULL DEFAULT 'terjadwal',-- terjadwal / menunggu / siap / berjalan / selesai / batal
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS delivery_rooms (
  id TEXT PRIMARY KEY,                    -- VK 1
  status TEXT NOT NULL DEFAULT 'kosong'   -- terisi / kosong
);
CREATE TABLE IF NOT EXISTS deliveries (
  id TEXT PRIMARY KEY,                    -- VK-20261119-001
  patient_id TEXT NOT NULL,
  age_years INTEGER NOT NULL,
  gravida_para TEXT NOT NULL,             -- G2P1
  dilation_cm INTEGER NOT NULL,           -- pembukaan
  fetal_heart_rate INTEGER NOT NULL,      -- DJJ x/mnt
  midwife TEXT NOT NULL,
  doctor TEXT NOT NULL,
  plan TEXT NOT NULL,                     -- rencana persalinan
  room TEXT NOT NULL,                     -- VK 1..4
  status TEXT NOT NULL DEFAULT 'Aktif',   -- Aktif / Selesai / Dirujuk
  created_at TEXT NOT NULL
);

-- Laboratorium --------------------------------------------------------------
CREATE TABLE IF NOT EXISTS lab_orders (
  id TEXT PRIMARY KEY,                    -- LAB-2210 (tanpa tanggal, sesuai desain)
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  test_name TEXT NOT NULL,
  loinc TEXT NOT NULL,
  ordered_from TEXT NOT NULL,             -- Poli Umum / IGD / Mawar / ...
  ordered_by TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'menunggu',-- menunggu / proses / selesai / kritis
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS lab_results (
  id TEXT PRIMARY KEY,                    -- LBR-000001
  order_id TEXT NOT NULL,
  analyte TEXT NOT NULL,
  loinc TEXT,
  value_text TEXT NOT NULL,
  unit TEXT NOT NULL DEFAULT '',
  ref_text TEXT NOT NULL DEFAULT '',      -- teks nilai rujukan, mis. "13,0–17,0"
  ref_low REAL,
  ref_high REAL,
  critical_high REAL,
  critical_low REAL,
  flag TEXT NOT NULL,                     -- normal / tinggi / rendah / kritis
  created_at TEXT NOT NULL
);

-- Radiologi -----------------------------------------------------------------
CREATE TABLE IF NOT EXISTS rad_orders (
  id TEXT PRIMARY KEY,                    -- RAD-0911 (tanpa tanggal, sesuai desain)
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  modality TEXT NOT NULL,                 -- X-Ray / USG / CT Scan / MRI
  area TEXT NOT NULL,                     -- Thorax AP / Antebrachii dextra / ...
  ordered_from TEXT NOT NULL,
  ordered_by TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'menunggu',-- menunggu / proses / selesai
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS rad_reports (
  id TEXT PRIMARY KEY,                    -- RDR-000001
  order_id TEXT NOT NULL UNIQUE,
  findings TEXT NOT NULL,
  radiologist TEXT NOT NULL,
  reported_at TEXT NOT NULL
);

-- Casemix: klaim Fase 2 dipakai ulang; dua perubahan -------------------------
-- (a) claims.invoice_id boleh NULL (klaim grouping tidak selalu berangkat
--     dari tagihan), (b) kolom baru claims.grouping_note TEXT.
-- SQLite tidak bisa mengubah NOT NULL lewat ALTER, jadi rebuild tabel
-- (12-langkah SQLite: tabel baru -> salin -> hapus lama -> ganti nama).
-- Salinan memakai SELECT eksplisit supaya klaim lama (termasuk
-- CLM-20261119-001 + tautan INV-2026-1181) utuh.
CREATE TABLE claims_new (
  id TEXT PRIMARY KEY,
  invoice_id TEXT,
  sep_id TEXT,
  ina_cbg_code TEXT NOT NULL,
  amount INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft',
  submitted_at TEXT,
  created_at TEXT NOT NULL,
  grouping_note TEXT
);
INSERT INTO claims_new
  (id, invoice_id, sep_id, ina_cbg_code, amount, status, submitted_at,
   created_at, grouping_note)
  SELECT id, invoice_id, sep_id, ina_cbg_code, amount, status,
    submitted_at, created_at, NULL FROM claims;
DROP TABLE claims;
ALTER TABLE claims_new RENAME TO claims;
