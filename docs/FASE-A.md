# Fase A — Fondasi Kepercayaan (roadmap, 21 Nov 2026)

Branch `roadmap-fase-a` dari `main` @ `c23844c` (Fase 1–4).
Tujuan roadmap: apa pun yang diklaim di README bisa dibuktikan, dan
instalasi baru aman sejak menit pertama. Dokumen ini adalah ringkasan
integrasi; detail per item ada di dokumen masing-masing
(`FASE-A-A2.md`, `FASE-A-A4.md`, `BACKUP.md`).

## A1. Rilis yang rapi — SELESAI

- File `LICENSE` Apache-2.0 kanonis (diambil dari apache.org, 202 baris,
  terverifikasi) — sebelumnya README mengklaim Apache-2.0 tanpa file.
- `CHANGELOG.md` (format Keep a Changelog) dengan entri v0.1.0.
- README diperbarui: intro Fase 1–4 (sebelumnya masih menyebut Fase 3
  "belum merge"), demo live @ `c23844c`, lisensi menunjuk `LICENSE`.
- Semua crate sudah versi `0.1.0`; `/api/health` mengembalikan
  `version: "0.1.0"`.
- Tag `v0.1.0` dibuat lokal di commit integrasi Fase A (annotated,
  TIDAK di-push — push tag adalah keputusan terpisah).

## A2. Mode PostgreSQL dibuktikan — SELESAI

Detail: `docs/FASE-A-A2.md` (+ `docs/FASE3.md` §10.5).

- PostgreSQL 16.15 nyata di sandbox (apt, cluster 16/main, port 5432).
  Detail server + cara install ulang: `~/workspace/waras/.fase-a-scratch/pg-server.txt`
  dan `reinstall-pg.sh` (data dir `/var/lib/postgresql/16/main` TIDAK
  selamat dari pergantian VM — cek `pg_lsclusters` dulu).
- Migrasi 0001–0006 di PG nyata (termasuk 0005/0006 yang belum pernah
  dieksekusi di PG), seed, boot HTTP (`mode: "postgres"`), seluruh
  test suite hijau di kedua backend (lihat § Verifikasi).
- Bug nyata yang diperbaiki di PG: `SUM(amount)` → Numeric di
  dashboard/invoice/casemix (`CAST(... AS BIGINT)`); pencarian pasien
  case-sensitive (`LOWER(...) LIKE LOWER(?)`); race read-max-then-insert
  pada alokasi ID antrean/resep/outbox (`Db::alloc_lock` + retry 5×).
- Uji beban: 20 thread × 10 tulis = 200/200 dalam 5,0 dtk (40/dtk);
  antrean p50 281 ms p95 1462 ms; resep p50 506 ms p95 607 ms
  (angka di bawah kontensi CPU bersama, bukan kapasitas bersih).

## A3. Backup/restore otomatis — SELESAI

Detail + runbook: `docs/BACKUP.md`.

- `scripts/waras-backup.sh` — backup online (`sqlite3 .backup`, aman
  walau server berjalan), `integrity_check` sebelum finalisasi, sidecar
  `.sha256`, retensi 7, menolak bila mode Postgres atau direktori
  backup sama dengan direktori data.
- `scripts/waras-restore.sh` — satu perintah: verifikasi header +
  `integrity_check` + sha256 dulu, menolak saat server berjalan
  (kecuali `--force`), simpan salinan pre-restore, rename atomik.
- Penjadwalan: systemd timer `scripts/waras-backup.timer` (02:30
  harian, `Persistent=true`); alternatif cron di runbook.
- Latihan restore 21 Nov 2026: backup saat server jalan, DB dihapus,
  restore satu perintah, reboot, login seed jalan — sebelum/sesudah
  identik (patients 15, queue 45, resep 9, audit 10), **zero data loss**.
- Catatan: unit systemd memakai placeholder path (`/opt/waras`,
  `/var/backups/waras`, user `waras`) — cocokan dengan layout VPS
  sebelum timer diaktifkan. Salinan backup off-server adalah kerja
  Fase C, bukan A3. Untuk Postgres, backup yang benar adalah `pg_dump`
  (didokumentasikan di runbook).

## A4. Keamanan produksi — SELESAI

Detail: `docs/FASE-A-A4.md`.

- Login Portal Pasien kini kata sandi (Argon2, pola sama dengan staf):
  `POST /api/auth/patient-login` menerima `{nik, password}`; payload
  lama `{nik, birth_date}` → 400; tanggal lahir sebagai kata sandi →
  401. Migrasi 0007 (SQLite + PG): `patients.portal_password_hash`,
  `login_attempts`, `api_rate_limits`. Seed: semua pasien seed memakai
  hash `waras123` (didokumentasikan di README); `Db::backfill_portal_passwords`
  mengisi DB lama yang kolomnya NULL. `portal.html`/`portal.js`
  memakai field kata sandi.
- Brute-force di `/api/auth/login` dan patient-login: 5 gagal/15 menit
  → 429 + header `Retry-After` (900 dtk); sukses menghapus hitungan.
  IP dari `ConnectInfo` (sengaja bukan `X-Forwarded-For`).
- Rate limit API publik persisten (DB-backed, 100 req/60 dtk per kunci),
  terbukti bertahan setelah restart server.
- Header keamanan (tower-http `SetResponseHeaderLayer`): `nosniff`,
  `X-Frame-Options: DENY`, `Referrer-Policy` di semua respons; guard
  traversal eksplisit di `serve_asset`; `POST /api/auth/logout` baru
  (sebelumnya logout hanya menghapus localStorage).
- Pilihan yang didokumentasikan: kata sandi (bukan OTP) karena tidak ada
  kanal pengirim SMS/WA; token sesi mentah tetap di DB + localStorage
  dengan alasan tertulis di FASE-A-A4.md §8.

## Verifikasi integrasi (koordinator, 21 Nov 2026)

- `cargo test --offline` SQLite: **241 lolos, 0 gagal** (api 89, bridge
  35, core 32, db 82, web 3). Log:
  `~/workspace/waras/.fase-a-scratch/integration-test-sqlite.log`.
- `cargo test --offline` Postgres
  (`WARAS_TEST_PG_URL=postgres://waras:waras@127.0.0.1:5432/waras_pg_test`):
  **241 lolos, 0 gagal** (hitungan per-crate identik dengan SQLite).
  Log: `~/workspace/waras/.fase-a-scratch/integration-test-pg.log`.
- Migrasi 0007 di PG nyata: **terbukti** — seluruh 7 migrasi PG
  dijalankan lewat psql di DB `waras_integration` (tabel
  `login_attempts`, `api_rate_limits`, kolom
  `patients.portal_password_hash` ada); suite PG juga menjalankan
  migrasi 1–7 di tiap DB tesnya.
- Boot + curl kedua mode (binary release 15,7 MB, skrip
  `~/workspace/waras/.fase-a-scratch/integration-verify.py`):
  **20/20 PASS** — health mode benar per backend, login staf, KPI
  dasbor identik di kedua backend (248 pasien, antrean 44, lab 3,
  bed 86/120), portal kata sandi 200 + NIK tersensor, NIK+tanggal
  lahir → 400, tanggal lahir sebagai kata sandi → 401, brute-force
  4×401 lalu 429, Amoxicillin untuk Siti → 422 BLOCKED di PG,
  header keamanan ada, rate limit 100 OK → 101 429 → restart →
  tetap 429, backup script + restore satu perintah → login dan KPI
  sama (zero data loss).
- Playwright Chromium: halaman `/portal` (login kata sandi → data
  Siti tampil) + `/dashboard` (login staf), **0 console error, 0 page
  error, 0 request gagal, 0 HTTP ≥400**.
- `cargo build --release --offline`: OK (1m 52s, binary 15,7 MB).

## Yang masih terbuka (bukan Fase A)

- **Belum ada endpoint set/ganti kata sandi portal.** Pasien baru dari
  registrasi lahir dengan `portal_password_hash` NULL = tidak bisa login
  portal (`Db::set_portal_password` sudah tersedia; endpoint staf reset
  + endpoint pasien ganti sendiri belum ada). — SELESAI di
  `docs/FASE-A-FOLLOWUP.md` item 1 (branch `fase-a-followup`):
  `POST /api/patients/{id}/portal-password` (Admin) +
  `POST /api/portal/change-password` (pasien, kata sandi lama wajib).
- Race read-max-then-insert masih ada di jalur alokasi ID lain (pasien
  RM-######, encounter, invoice, klaim, admisi, order lab/radiologi,
  operasi, persalinan, HD, transfusi, staf, shift, kunci API) — baru
  antrean/resep/outbox yang dikunci `alloc_lock`; belum di-load-test.
- `persistent=false` di mode PG = satu Parse round-trip per query;
  biayanya belum diukur terpisah.
- CSP per halaman, HSTS (di nginx, bukan aplikasi), audit XSS penuh,
  dan ketepatan rate limit di bawah konkurensi (baca-lalu-tulis tanpa
  transaksi) adalah tindak lanjut (FASE-A-A4.md §8).
- Angka beban A2 adalah angka kontensi, bukan kapasitas bersih;
  batas kapasitas adalah kerja Fase C/C2.
- Fase B/C/D bukan bagian tugas ini.
