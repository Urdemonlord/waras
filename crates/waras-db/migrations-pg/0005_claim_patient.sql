-- Fase 3 integrasi (PostgreSQL): sama seperti 0005 SQLite —
-- klaim membawa pasien pemiliknya.
ALTER TABLE claims ADD COLUMN patient_id TEXT;
