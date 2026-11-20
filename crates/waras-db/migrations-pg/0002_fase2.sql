-- WARAS migrasi Fase 2 (Jembatan Nasional) — terjemahan PostgreSQL
-- dari crates/waras-db/migrations/0002_fase2.sql, nomor versi SAMA.
-- INTEGER -> BIGINT, sisanya identik (lihat 0001_init.sql soal aturan).

-- Outbox: satu baris = satu pengiriman. Idempotensi lewat UNIQUE.
CREATE TABLE IF NOT EXISTS outbox (
  id TEXT PRIMARY KEY,                    -- OBX-000001
  destination TEXT NOT NULL,              -- snake_case OutboxDestination
  resource_type TEXT NOT NULL,            -- Patient/Encounter/Observation/Condition/MedicationRequest/AllergyIntolerance/SEP/Rujukan/Kunjungan/Antrean/Klaim
  resource_id TEXT NOT NULL,              -- id lokal sumber
  idempotency_key TEXT NOT NULL UNIQUE,   -- {destination}:{resource_type}:{resource_id}:{event}
  payload TEXT NOT NULL,                  -- JSON
  status TEXT NOT NULL DEFAULT 'pending', -- snake_case OutboxStatus
  attempts BIGINT NOT NULL DEFAULT 0,
  max_attempts BIGINT NOT NULL DEFAULT 5,
  next_retry_at TEXT,                     -- ISO-8601, NULL = sekarang
  last_error TEXT,
  external_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_outbox_due ON outbox(status, next_retry_at, created_at);

-- SEP VClaim (satu baris per SEP; nomor SEP = id).
CREATE TABLE IF NOT EXISTS seps (
  id TEXT PRIMARY KEY,                    -- SEP-20261119-001
  sep_number TEXT NOT NULL UNIQUE,
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  bpjs_number TEXT NOT NULL,
  jenis TEXT NOT NULL,                    -- RawatJalan / RawatInap
  poli TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'aktif',   -- aktif / gagal / dibatalkan
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Rujukan keluar (VClaim).
CREATE TABLE IF NOT EXISTS referrals (
  id TEXT PRIMARY KEY,                    -- RUJ-20261119-001
  patient_id TEXT NOT NULL,
  from_poli TEXT NOT NULL,
  to_facility TEXT NOT NULL,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'menunggu', -- terkirim / menunggu / ditolak
  created_at TEXT NOT NULL
);

-- Kunjungan FKTP ke PCare.
CREATE TABLE IF NOT EXISTS pcare_visits (
  id TEXT PRIMARY KEY,                    -- PCV-20261119-001
  patient_id TEXT NOT NULL,
  visit_date TEXT NOT NULL,
  poli TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Tercatat',
  created_at TEXT NOT NULL
);

-- Booking antrean online Mobile JKN.
CREATE TABLE IF NOT EXISTS online_queue (
  id TEXT PRIMARY KEY,                    -- = booking_code
  booking_code TEXT NOT NULL UNIQUE,      -- JKN-881201
  patient_id TEXT NOT NULL,
  poli TEXT NOT NULL,
  estimasi TEXT NOT NULL,                 -- 07.30
  status TEXT NOT NULL DEFAULT 'Menunggu',-- Menunggu / Hadir
  local_queue_id TEXT,                    -- A-041 bila sudah check-in
  created_at TEXT NOT NULL
);

-- Cache ketersediaan bed (Aplicares).
CREATE TABLE IF NOT EXISTS bed_availability (
  class TEXT PRIMARY KEY,                 -- Kelas 1 / Kelas 2 / Kelas 3 / VIP / ICU
  total BIGINT NOT NULL,
  available BIGINT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Berkas klaim (E-Klaim; workbench Casemix penuh tetap Fase 3).
CREATE TABLE IF NOT EXISTS claims (
  id TEXT PRIMARY KEY,                    -- CLM-20261119-001
  invoice_id TEXT NOT NULL,
  sep_id TEXT,
  ina_cbg_code TEXT NOT NULL,
  amount BIGINT NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft',   -- draft / terkirim / diterima / ditolak
  submitted_at TEXT,
  created_at TEXT NOT NULL
);

-- Kolom baru di tabel Fase 1.
ALTER TABLE patients ADD COLUMN satusehat_id TEXT;
ALTER TABLE patients ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE encounters ADD COLUMN satusehat_id TEXT;
ALTER TABLE encounters ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE observations ADD COLUMN satusehat_id TEXT;
ALTER TABLE observations ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE medication_requests ADD COLUMN satusehat_id TEXT;
ALTER TABLE medication_requests ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE invoices ADD COLUMN sep_id TEXT;
