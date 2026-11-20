-- Fase 3 integrasi: klaim membawa pasien pemiliknya, supaya tabel
-- Casemix selalu bisa menampilkan nama pasien walaupun klaim tidak
-- menaut tagihan/SEP (klaim grouping seed CLM-5510..5517).
-- NULL untuk klaim lama (sebelum migrasi ini); seed Fase 3 mengisi
-- patient_id untuk klaim baru, group_claim_for_encounter mengisinya
-- dari encounter.
ALTER TABLE claims ADD COLUMN patient_id TEXT;
