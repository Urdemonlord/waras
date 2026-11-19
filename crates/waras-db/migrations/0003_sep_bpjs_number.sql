-- 0003: nomor SEP yang diterbitkan BPJS (FASE2.md §9, perbaikan
-- double-submit SEP). `sep_number` tetap nomor lokal (= id); nomor
-- BPJS (`response.sep.noSep` dari `SEP/2.0/insert`) disimpan terpisah
-- supaya lookup/batal/klaim ke BPJS memakai nomor asli, bukan nomor
-- lokal. NULL untuk SEP seed lokal dan SEP yang insert-nya belum
-- berhasil.
ALTER TABLE seps ADD COLUMN bpjs_sep_number TEXT;
