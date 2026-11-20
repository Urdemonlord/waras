-- 0003: nomor SEP yang diterbitkan BPJS — terjemahan PostgreSQL dari
-- crates/waras-db/migrations/0003_sep_bpjs_number.sql, nomor versi SAMA.
-- `sep_number` tetap nomor lokal (= id); nomor BPJS disimpan terpisah.
ALTER TABLE seps ADD COLUMN bpjs_sep_number TEXT;
