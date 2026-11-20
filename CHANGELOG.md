# CHANGELOG — WARAS

Semua perubahan penting dicatat di sini. Format mengikuti
[Keep a Changelog](https://keepachangelog.com/id/1.1.0/).
Versi memakai Semantic Versioning.

## [v0.1.0] — 2026-11-21

Rilis pertama. Seluruh fitur Fase 1–4 + fondasi Fase A sudah merge ke
`main` (commit integrasi Fase A). Demo live: https://waras.meowlabs.id.

### Ditambahkan
- **Fase 1 — Inti Klinis:** registrasi, antrean, RME (SOAP + vitals),
  farmasi (aturan keamanan obat: Amoxicillin otomatis diblokir untuk
  pasien alergi penisilin), billing, dasbor KPI, audit trail append-only
  dengan rantai hash SHA-256.
- **Fase 2 — Jembatan Nasional:** outbox + worker, adapter SATUSEHAT
  (FHIR R4, terverifikasi live terhadap staging) dan BPJS (VClaim,
  PCare, Antrean Online, Aplicares; dites terhadap stub, belum
  live-proven), SEP single-send + guard anti-double-submit.
- **Fase 3 — Rumah Sakit:** IGD (triase), Rawat Inap (120 bed),
  Kamar Operasi & Bersalin, Laboratorium (LOINC), Radiologi,
  Casemix/INA-CBG workbench (grouping lokal; tanpa submit E-Klaim),
  validasi wrong-patient, RBAC per modul.
- **Fase 4 — Ekosistem:** Hemodialisa, Transfusi Darah, Gizi, Rekam
  Medis Gigi (odontogram), Portal Pasien, API Publik (kunci `wrs_`),
  Inventori, Kepegawaian, Laporan & SIRS, Pengaturan & Audit,
  Rawat Jalan. Sidebar: 0 "segera".
- **Fase A — Fondasi:** file `LICENSE` Apache-2.0; mode PostgreSQL
  dibuktikan di Postgres 16 nyata (migrasi 0001–0006, boot HTTP,
  uji beban 20 pengguna bersamaan); backup/restore SQLite otomatis
  + latihan restore; login Portal Pasien berbasis kata sandi;
  perlindungan brute-force di `/api/auth/login`; rate limit API
  publik persisten (DB-backed); header keamanan HTTP.

### Diketahui belum selesai
- BPJS belum pernah terbukti live (butuh kredensial Dev faskes mitra —
  lihat `docs/FASE2.md` §10). E-Klaim belum punya jalur submit.
- Login Portal Pasien memakai kata sandi seed yang didokumentasikan
  di README (demo); instalasi produksi wajib mengganti kata sandi
  portal tiap pasien.
- Retensi dan enkripsi berkas (UU PDP 27/2022, 25 tahun) belum dibangun.
- Akses keyboard dan layout responsif belum dibangun.
