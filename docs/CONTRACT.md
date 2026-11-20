# WARAS — Kontrak build Fase 1

Dokumen ini SATU-SATUNYA sumber kebenaran untuk kontrak antar-crate.
Baca sebelum menulis kode. Bila kontrak dan desain bertentangan, kontrak ini
yang menang, dan penyimpangannya dicatat di README repo.

Sumber desain: paper `~/workspace/your_files/waras-paper-simrs-modern/waras-paper-simrs-modern.pdf`
(v4, full Rust, DSR, Fase 1 = inti klinis) dan FigCraft v16
`~/workspace/waras/figcraft/DESIGN.md` + `exports-v16/` (27 frame, Swiss
clinical, teal #0E7490, ikon Lucide, bahasa Indonesia).

## 0. Aturan main

- Satu workspace, satu binary `waras`. Lokasi: `~/workspace/waras/waras/`.
- JANGAN push ke GitHub, JANGAN sentuh VPS, JANGAN deploy. Build lokal saja.
- TIDAK ada secret/token/.env berisi kredensial asli di repo.
- Build offline HARUS jalan: `cargo test --offline`, `cargo build --release --offline`.
  Semua dep sudah ada di cache registry sandbox (axum 0.8.9, sqlx 0.8.6,
  tokio 1.54.1, serde 1.0.229, argon2 0.5.3, rust-embed 8.13, chrono 0.4.45).
  JANGAN tambah dep baru tanpa cek cache dulu (`ls ~/.cargo/registry/cache/*/ | grep <nama>`).
- SQLx: pakai `sqlx::query` / `query_as` + `FromRow` SAJA. JANGAN pakai
  macro `query!`/`query_as!` (butuh DB saat compile / file metadata offline).
- Tipe domain HANYA dari `waras-core` (sudah jadi, 9 test lolos). Jangan
  definisikan ulang Patient/Encounter/dll di crate lain.
- Bahasa UI dan pesan API: Indonesia. Kode dan komentar boleh Inggris/Indonesia.
- Tanggal klinik: "hari ini" = env `WARAS_TODAY`, default `2026-11-19`
  (tanggal di desain). Semua seed dan KPI "hari ini" memakai tanggal ini,
  supaya angka dasbor deterministik dan cocok dengan desain. Transaksi baru
  juga memakai tanggal ini (bukan tanggal sistem).

## 1. Peta crate

| Crate | Isi |
|---|---|
| `waras-core` | SELESAI (jangan diubah kecuali bug). Tipe domain FHIR R4, aturan keamanan obat (`safety::check_prescription`), RBAC (`rbac::can`), error, ID, helper FHIR JSON. |
| `waras-db` | SQLx SQLite, migrasi, repository, seed, audit trail append-only + rantai hash. |
| `waras-api` | Axum: router, auth (token sesi), guard RBAC, handler, DTO, error mapping ke JSON. |
| `waras-web` | Aset statis (HTML/CSS/JS + ikon Lucide PNG + logo WARAS) via rust-embed. |
| `waras` (bin) | `main.rs`: config, buka DB, jalankan migrasi, seed bila kosong, serve API + UI. |

## 2. Skema DB (SQLite, file `waras.db`)

Satu tabel per tipe domain di `waras-core::types`, kolom snake_case.
Relasi kunci: `patients.id` (RM-...), `encounters.patient_id`,
`observations.encounter_id`, `conditions.patient_id + encounter_id`,
`medication_requests.encounter_id`, `queue_entries.patient_id`,
`appointments.patient_id`, `invoices.patient_id`, `payments.invoice_id`,
`allergies.patient_id`, `soap_notes.encounter_id` (UNIQUE),
`soap_versions.soap_id`, `users.username` UNIQUE, `sessions.token` UNIQUE,
`audit_log` append-only (tidak ada UPDATE/DELETE; `prev_hash`/`entry_hash`
rantai SHA-256, entri pertama `prev_hash = "GENESIS"`),
`daily_stats.date` UNIQUE, `notifications`.

Migrasi: file SQL di `crates/waras-db/migrations/`, dijalankan via
`sqlx::migrate!` (embedded) saat boot. Versi migrasi: `0001_init.sql`
(semua tabel Fase 1 sekaligus — cukup, karena ini repo baru).

Password: hash Argon2 (`argon2` crate) saat seed dan saat buat user.
Token sesi: 32 byte acak hex, disimpan di tabel `sessions`
(token, user_id, created_at, expires_at = +12 jam).

## 3. API contract (JSON, prefix `/api`)

Semua respons sukses: JSON objek. Error: `{ "error": { "code": "NOT_FOUND", "message": "..." } }`
dengan status HTTP dari `WarasError::http_status` (Blocked -> 422).
Auth: header `Authorization: Bearer <token>` kecuali `/api/health` dan
`/api/auth/login`. Guard RBAC pakai `waras_core::rbac::can` per aksi.

| Method & path | Peran | Request | Response / catatan |
|---|---|---|---|
| `GET /api/health` | publik | — | `{status:"ok", version:"0.1.0", mode:"sqlite", today:"2026-11-19"}` |
| `POST /api/auth/login` | publik | `{username, password}` | `{token, user:{id,nip,username,name,role}}`. Username boleh NIP atau username. |
| `GET /api/auth/me` | login | — | user dari token. |
| `GET /api/patients?search=` | login | cari nama/NIK/RM | daftar pasien (urut nama). |
| `POST /api/patients` | Dokter/Perawat/Admin | Patient tanpa id/created_at | pasien baru, id `RM-######` berikutnya. |
| `GET /api/patients/:id` | login | — | pasien + alergi + kondisi + encounter terakhir. |
| `GET /api/patients/:id/fhir` | login | — | resource FHIR Patient JSON (`fhir::patient_resource`). |
| `GET /api/queue?date=YYYY-MM-DD` | login | default WARAS_TODAY | daftar antrean hari itu (urut nomor). |
| `POST /api/queue/take` | Dokter/Perawat/Admin | `{patient_id, poli, doctor}` | antrean baru, nomor berikutnya per prefix poli (A=Poli Umum). |
| `POST /api/queue/:id/call` | Dokter/Perawat/Admin | — | status -> Dipanggil, `called_at` diisi. |
| `POST /api/queue/:id/status` | Dokter/Perawat/Admin | `{status}` | ubah status (DalamKonsultasi/Selesai/TidakHadir). |
| `GET /api/encounters?patient_id=` | login | — | daftar encounter. |
| `POST /api/encounters` | Dokter/Admin | `{patient_id, poli, doctor_name, doctor_id?}` | encounter baru `ENC-<tanggal>-<seq>`, status Aktif. |
| `GET /api/encounters/:id` | login | — | encounter + SOAP + vitals + kondisi + resep + FHIR bundle. |
| `GET /api/encounters/:id/fhir` | login | — | FHIR Bundle (`fhir::encounter_bundle`). |
| `GET /api/encounters/:id/soap` | login | — | SOAP + `versions[]` (riwayat). |
| `PUT /api/encounters/:id/soap` | Dokter/Admin | `{subjective, objective, assessment, plan}` | simpan draft: version+1, baris baru di `soap_versions`, `draft_saved_at` diisi, status Draft kecuali plan lengkap -> Lengkap. TIDAK pernah menimpa versi lama. |
| `GET /api/encounters/:id/vitals` | login | — | daftar Observation. |
| `POST /api/encounters/:id/vitals` | Dokter/Perawat/Admin | `{code, value, measured_by, previous_value?, previous_date?}` | unit diisi dari `VitalCode::unit()`. |
| `GET /api/patients/:id/conditions` | login | — | daftar Condition (urut grup). |
| `POST /api/encounters/:id/conditions` | Dokter/Admin | `{group, code, display, status, clinician}` | Condition baru, `encounter_id` = encounter ini. |
| `GET /api/prescriptions?status=&patient_id=` | login | — | daftar resep. |
| `POST /api/prescriptions` | Dokter/Admin | `NewPrescription` | **WAJIB lewati `safety::check_prescription` dulu**: bila diblokir -> 422 + simpan resep status Diblokir + `blocked_reason` + audit `prescription.blocked`. Bila lolos -> resep Aktif, id `RX-####` berikutnya. |
| `POST /api/prescriptions/:id/confirm` | Dokter/Admin | — | MenungguKonfirmasi -> Aktif. |
| `POST /api/prescriptions/:id/dispense` | Apoteker/Admin | — | Aktif -> Diberikan, stok obat berkurang sesuai dosis (jumlah tablet dari `dosage`, mis. `1x1, 3 hari` = 3 tablet; hitung sederhana: angka hari x frekuensi per hari). |
| `GET /api/drugs` | login | — | daftar obat + `stock_status` dari `Drug::stock_status()`. |
| `GET /api/dashboard` | login | — | `DashboardKpis` (lihat §4). |
| `GET /api/invoices?status=` | login | — | daftar tagihan. |
| `POST /api/invoices` | Kasir/Admin | `{patient_id, encounter_id?, payer, service, amount, ina_cbg_code?}` | tagihan baru `INV-<tahun>-<seq>`, status Menunggu. |
| `GET /api/invoices/:id` | login | — | tagihan + pembayaran. |
| `POST /api/invoices/:id/pay` | Kasir/Admin | `{method, amount}` | catat Payment; bila total bayar >= amount -> status Lunas. |
| `GET /api/audit?limit=` | Admin/Dokter | default 50 | daftar AuditEntry terbaru dulu. |
| `GET /api/notifications` | login | — | notifikasi hari ini. |

## 4. Dasbor (`GET /api/dashboard`)

Dihitung dari data nyata, BUKAN angka hardcode:
- `patients_today` = `daily_stats.registrations` pada WARAS_TODAY (248).
- `queue_*` dari `queue_entries` tanggal WARAS_TODAY:
  `queue_waiting` = status Menunggu; `queue_in_consultation` = Dipanggil +
  DalamKonsultasi; `queue_done` = Selesai; `queue_no_show` = TidakHadir;
  `queue_total` = semua; `queue_active` = waiting + in_consultation.
- `lab_pending`/`lab_critical`, `emergency_red`, `beds_*` dari
  `daily_stats` hari itu (Lab bukan modul Fase 1; agregat di-seed).
- `followups_7d` = jumlah Appointment status Terkonfirmasi/Menunggu dengan
  tanggal WARAS_TODAY..+7 hari (31 di seed).
- `visits_7d` = `daily_stats.visits` 7 hari terakhir (urut tanggal naik),
  `visits_avg_7d` = rata-rata (178), `visits_peak_7d` = maks (210).
- `avg_wait_minutes` = 18, `longest_wait_minutes` = 45 (konstanta seed di
  `daily_stats`? TIDAK ADA kolomnya — hitung sederhana: simpan sebagai
  konstanta di handler dengan komentar "dari desain; dihitung dari antrean
  di Fase 1.1". JANGAN bikin kolom baru demi dua angka ini.)
- `revenue_today_idr`/`revenue_transactions` dari `daily_stats`
  (18.400.000 / 142). `unpaid_invoices_*` dari tabel invoices
  (status Menunggu/Cicilan, hari ini).

## 5. Seed data (wajib persis, dari desain v16)

Tanggal klinik: 2026-11-19 (Kamis). Semua jam WIB.

### 5.1 Pengguna (password semua: `waras123`, hash Argon2 saat seed)

| id | nama | NIP | username | peran |
|---|---|---|---|---|
| U-001 | dr. Ahmad Hidayat | 198505122010011001 | ahmad | Dokter Umum |
| U-002 | Ns. Sari Wijaya | 199001152015032002 | sari | Perawat |
| U-003 | dr. Rina, Sp.PD | 198208102008122003 | rina | Dokter Umum |
| U-004 | apt. Dina Puspita | 199303252019032004 | dina | Apoteker |
| U-005 | Sinta Maharani | 199507082021012005 | sinta | Kasir |
| U-006 | Admin Sistem | 199001012015011006 | admin | Admin |

### 5.2 Pasien (10 orang)

| RM | Nama | NIK | Lahir | Kelamin | Alamat | Gol. darah | BPJS | Penjamin |
|---|---|---|---|---|---|---|---|---|
| RM-000812 | Siti Rahmawati | 3374021205850001 | 1985-05-12 | P | Jl. Pandanaran No. 12, Semarang | O+ | 0001234567890 | BPJS |
| RM-000813 | Budi Santoso | 3374011503900002 | 1990-03-15 | L | Jl. Gajah Mada No. 45, Semarang | A+ | 0001234567891 | BPJS |
| RM-000814 | Dewi Lestari | 3374032207900003 | 1979-07-22 | P | Jl. Pemuda No. 88, Semarang | B+ | — | Umum |
| RM-000815 | Agus Prasetyo | 3374020310880004 | 1988-10-03 | L | Jl. Siliwangi No. 21, Semarang | O+ | 0001234567892 | BPJS |
| RM-000816 | Rina Marlina | 3374051402920005 | 1992-02-14 | P | Jl. Ahmad Yani No. 7, Semarang | AB+ | 0001234567893 | BPJS |
| RM-000817 | Joko Susilo | 3374022505810006 | 1981-05-25 | L | Jl. Diponegoro No. 33, Semarang | A+ | — | Umum |
| RM-000818 | Ahmad Fauzi | 3374031103890007 | 1989-11-03 | L | Jl. Kartini No. 19, Semarang | B+ | 0001234567894 | BPJS |
| RM-000819 | Putri Ayu | 3374052807940008 | 1994-07-28 | P | Jl. Sudirman No. 56, Semarang | O+ | 0001234567895 | BPJS |
| RM-000820 | Bambang Riyanto | 3374011806750009 | 1975-06-18 | L | Jl. Veteran No. 64, Semarang | A+ | — | Umum |
| RM-000821 | Sri Wahyuni | 3374033009800010 | 1980-09-30 | P | Jl. Pahlawan No. 102, Semarang | B+ | 0001234567896 | BPJS |

Alamat pasien 813+ adalah data sintetis seed (desain hanya memberi alamat Siti).
Alergi: Siti — Penisilin, kelas `penicillin`, kritis Tinggi.
Kondisi Siti: Utama J20.9 Bronkitis akut (Aktif, ENC-2026-11-19-014,
dr. Ahmad Hidayat); Sekunder E11.9 Diabetes melitus tipe 2 tanpa komplikasi
(Kronis, encounter ini, dr. Ahmad Hidayat); Kronis E11.9 DM tipe 2 + I10
Hipertensi esensial (Kronis, dr. Rina, Sp.PD, tanpa encounter); Riwayat J45.9
Asma (2024); Riwayat M54.5 Nyeri punggung bawah (2025); Tindakan medis 89.52
Elektrokardigram (EKG) (Selesai, encounter ini, dr. Ahmad Hidayat).
Kondisi lain: Budi J06.9 ISPA atas; Ahmad Fauzi J06.9; Putri Ayu J06.9
(semua grup Riwayat, status Selesai).

### 5.3 Encounter Siti (kanonik)

- `ENC-2026-11-19-014`, pasien RM-000812, Poli Umum, dr. Ahmad Hidayat
  (U-001), 2026-11-19 09.15, status DalamPemeriksaan.
- Encounter lalu Siti: `ENC-2026-11-12-008` Kontrol DM tipe 2, dr. Rina,
  2026-11-12, Selesai; `ENC-2026-10-28-005` Lab darah lengkap, 2026-10-28,
  Selesai; `ENC-2026-09-15-003` Rawat jalan Asma (J45.9), dr. Ahmad Hidayat,
  2026-09-15, Selesai.
- Encounter resep lain (2026-11-19, semua Selesai kecuali Agus Aktif):
  Budi ENC-...-015 Poli Anak dr. Sari Dewi; Dewi ENC-...-016 Penyakit Dalam
  dr. Rina; Agus ENC-...-017 Gigi drg. Maya K. (DalamPemeriksaan); Rina
  ENC-...-018 Poli Umum dr. Ahmad Hidayat; Joko ENC-...-019 Mata dr. Budi,
  Sp.B; Sri ENC-...-020 Poli Umum dr. Rina. (Dokter tanpa akun: `doctor_id`
  None, nama saja.)

### 5.4 SOAP Siti (ENC-...-014, versi 3, status Draft)

- S (Ditulis 09.18, dr. Ahmad H.): "Batuk berdahak 3 hari. Demam malam hari.
  Nyeri dada ringan saat batuk."
- O (Diperiksa 09.20, dr. Ahmad H.): "Paru: ronki basal kanan. THT: faring
  hiperemis. Suhu 37,2 °C, SpO2 98%."
- A (Dinilai 09.21, dr. Ahmad H.): "Bronkitis akut (J20.9). DM tipe 2
  terkontrol (E11.9)."
- P: "Azithromycin 500 mg 1x1, 3 hari. Kontrol 3 hari lagi. Edukasi hidrasi
  & istirahat." + catatan "Belum lengkap — resep menunggu konfirmasi".
- `draft_saved_at` 09.22. Riwayat versi: 3 versi tersimpan (v1 09.18 S saja,
  v2 09.20 S+O, v3 09.21 lengkap).

### 5.5 Tanda vital Siti (09.15, Ns. Sari Wijaya; "Sebelumnya" 12 Nov)

| Parameter | Hasil | Sebelumnya |
|---|---|---|
| Tekanan darah | 130/85 mmHg | 128/82 |
| Nadi | 82 x/mnt | 80 |
| Suhu | 37,2 °C | 37,0 |
| SpO2 | 98% | 99% |
| Gula darah | 142 mg/dL | 138 |

### 5.6 Resep (e-resep)

| ID | Pasien | Obat | Dosis | Dokter | Status |
|---|---|---|---|---|---|
| RX-1040 | Siti Rahmawati | Amoxicillin 500 mg | 1x1, 3 hari | dr. Ahmad H. | Diblokir (alasan: alergi Penisilin; `replaced_by` RX-1041) |
| RX-1041 | Siti Rahmawati | Azithromycin 500 mg | 1x1, 3 hari | dr. Ahmad H. | MenungguKonfirmasi |
| RX-1042 | Budi Santoso | Paracetamol 500 mg | 3x1, 3 hari | dr. Sari Dewi | Aktif |
| RX-1043 | Dewi Lestari | Amlodipine 5 mg | 5 mg 1x1, 30 hari | dr. Rina | Aktif |
| RX-1044 | Agus Prasetyo | Cefixime 200 mg | 200 mg 2x1, 5 hari | drg. Maya K. | Aktif |
| RX-1045 | Rina Marlina | Metformin 500 mg | 3x1, 30 hari | dr. Ahmad H. | Aktif |
| RX-1046 | Joko Susilo | Ibuprofen 400 mg | 400 mg 3x1, 3 hari | dr. Budi, Sp.B | Aktif |
| RX-1047 | Sri Wahyuni | Omeprazole 20 mg | 20 mg 1x1, 14 hari | dr. Rina | Aktif |

Kelas obat: Amoxicillin `penicillin`, Azithromycin `macrolide`,
Paracetamol `analgesic`, Amlodipine `ccb`, Cefixime `cephalosporin`,
Metformin `antidiabetic`, Ibuprofen `nsaid`, Omeprazole `ppi`.

### 5.7 Obat & stok KFA

| KFA | Obat | Kelas | Bentuk | Stok | Lokasi | Exp |
|---|---|---|---|---|---|---|
| KFA-9201 | Amoxicillin 500 mg | penicillin | Tablet | 12 | Rak A-02 | 2027-03-31 |
| KFA-9202 | Paracetamol 500 mg | analgesic | Tablet | 1240 | Rak A-01 | 2027-11-30 |
| KFA-9203 | Omeprazole 20 mg | ppi | Kapsul | 15 | Rak B-03 | 2027-01-31 |
| KFA-9204 | Salbutamol 100 mcg | bronchodilator | Inhaler | 3 | Rak C-01 | 2027-06-30 |
| KFA-9205 | Metformin 500 mg | antidiabetic | Tablet | 860 | Rak A-04 | 2027-09-30 |
| KFA-9206 | Ibuprofen 400 mg | analgesic | Tablet | 540 | Rak A-02 | 2027-08-31 |
| KFA-9207 | Cefixime 200 mg | cephalosporin | Kapsul | 22 | Rak B-01 | 2027-05-31 |
| KFA-9208 | Azithromycin 500 mg | macrolide | Tablet | 320 | Rak A-03 | 2027-08-31 |
| KFA-9209 | Amlodipine 5 mg | ccb | Tablet | 145 | Rak B-02 | 2027-05-31 |

### 5.8 Antrean 2026-11-19 (Poli Umum, prefix A)

44 entri A-001..A-044. Status: A-001..A-010 Selesai; A-011, A-012 Tidak
hadir; A-013..A-027 Dalam konsultasi; A-028..A-040 Menunggu; A-041 Siti
Rahmawati Dipanggil 07.12; A-042 Budi Santoso Menunggu 07.18; A-043 Dewi
Lestari Menunggu 07.24; A-044 Agus Prasetyo Dalam konsultasi 07.31.
Jam check-in A-001..A-040: 07.00 + 2 menit per nomor (07.00, 07.02, ...).
Pasien A-001..A-040: putar 10 pasien seed berurutan.
CATATAN PENYIMPANGAN (sengaja, didokumentasikan): desain menulis total
donut 42 dan "nomor berikutnya A-045", tapi baris antrean yang terlihat
sudah sampai A-044 — keduanya tidak bisa benar sekaligus. Seed memakai 44
entri supaya baris A-041..A-044 persis seperti desain; KPI dasbor dihitung
dari data nyata sehingga total jadi 44 dan antrean aktif 32 (bukan 42/30).
Ini dicatat juga di README.

### 5.9 Janji (31 total)

Hari ini (2026-11-19): 07.30 Siti Rahmawati Poli Umum 09.30 Terkonfirmasi;
08.00 Budi Santoso Poli Anak 10.15 Terkonfirmasi; 08.30 Dewi Lestari
Penyakit Dalam 11.00 Menunggu; 09.00 Agus Prasetyo Gigi 12.00
Terkonfirmasi; 09.15 Rina Marlina Poli Umum 13.30 Menunggu; 09.45 Joko
Susilo Mata 14.00 Menunggu. + 25 janji kontrol ulang 2026-11-20..26
(pasien seed bergiliran, poli bergiliran, jam 08.00–14.00, Terkonfirmasi).

### 5.10 Tagihan (8)

| ID | Pasien | Penjamin | Layanan | Jumlah | Status |
|---|---|---|---|---|---|
| INV-2026-1181 | Siti Rahmawati | BPJS | Poli Umum + Lab | 185.000 | Lunas |
| INV-2026-1182 | Budi Santoso | BPJS | Poli Anak | 120.000 | Lunas |
| INV-2026-1183 | Dewi Lestari | Umum | Penyakit Dalam + EKG | 450.000 | Menunggu |
| INV-2026-1184 | Agus Prasetyo | BPJS | Gigi + tambal | 275.000 | Lunas |
| INV-2026-1185 | Bambang Riyanto | Umum | Rawat inap 3 hari | 4.850.000 | Cicilan |
| INV-2026-1186 | Sri Wahyuni | BPJS | Radiologi X-Ray | 310.000 | Menunggu |
| INV-2026-1187 | Ahmad Fauzi | Umum | Poli Umum + Lab | 265.000 | Menunggu |
| INV-2026-1188 | Putri Ayu | BPJS | Poli Anak | 95.000 | Lunas |

`ina_cbg_code`: kosong di semua (Fase 1). Pembayaran seed: INV-1181, 1182,
1184, 1188 lunas tunai/QRIS (catat satu Payment per tagihan lunas);
INV-1185 cicilan: satu Payment Rp 2.000.000 tunai.

### 5.11 Daily stats

| Tanggal | Registrasi | Kunjungan | Pendapatan | Transaksi | Lab menunggu | Lab kritis | IGD merah | Bed |
|---|---|---|---|---|---|---|---|---|
| 2026-11-13 | 231 | 162 | 16.100.000 | 128 | 21 | 1 | 2 | 81/120 |
| 2026-11-14 | 240 | 174 | 17.200.000 | 135 | 24 | 1 | 1 | 83/120 |
| 2026-11-15 | 219 | 168 | 15.800.000 | 121 | 19 | 0 | 2 | 79/120 |
| 2026-11-16 | 252 | 182 | 18.900.000 | 148 | 27 | 2 | 4 | 88/120 |
| 2026-11-17 | 244 | 176 | 17.600.000 | 139 | 23 | 1 | 2 | 84/120 |
| 2026-11-18 | 238 | 174 | 17.900.000 | 141 | 25 | 2 | 3 | 85/120 |
| 2026-11-19 | 248 | 210 | 18.400.000 | 142 | 26 | 2 | 3 | 86/120 |

Rata-rata kunjungan 7 hari = 178, tertinggi Rabu 210. (Nilai 13–18 Nov
sintetis seed, konsisten dengan rata-rata desain.)

### 5.12 Notifikasi (2026-11-19)

- INFO 09.15 "Hasil lab Siti Rahmawati siap."
- INFO 08.50 "Antrean baru dari JKN-881206."
- KRITIS 08.40 "Stok Amoxicillin kritis: 12 tablet."

### 5.13 Audit seed (6 entri, berantai hash)

Login dr. Ahmad Hidayat 07.00; pasien Siti terdaftar 07.12; antrean A-041
dipanggil 09.10; resep Amoxicillin DIBLOKIR 09.21 (alergi Penisilin);
resep Azithromycin dibuat 09.21 (pengganti RX-1040); SOAP Siti disimpan
draft v3 09.22.

## 6. UI (waras-web) — ringkas

- Halaman: `/` Login, `/dashboard`, `/registrasi`, `/rme?pasien=RM-000812`,
  `/farmasi`, `/billing`. Sidebar + topbar sama di semua halaman (kecuali
  Login). Footer sidebar: "RSUD Waras · Semarang".
- Token desain (dari DESIGN.md): bg `#F7F8F9`, surface `#FFFFFF`, border
  `#E3E6E9`, ink `#1A2027`, ink-2 `#374151`, muted `#667085`, aksen
  `#0E7490`, teks aksen `#0B7285`, aktif `#00D9FF` (hanya pill nav aktif),
  sukses `#16A34A`, warning `#B45309`, danger `#DC2626`, sidebar `#1F2937`.
  Status: hijau `#15803D`, kuning `#B45309`, teal `#0B7285`, merah
  `#DC2626`, netral `#374151`, biru `#1D4ED8`. Radius kartu 8, tombol 6.
  Font: Inter bila ada, fallback system-ui.
- Ikon: PNG Lucide di `~/workspace/waras/figcraft/assets/lucide/` (98 file,
  nama `<ikon>-<HEX>.png`) — salin yang dipakai ke `waras-web/assets/icons/`.
  Logo: `~/workspace/waras/figcraft/assets/waras-tile-sharp.png` (sidebar +
  Login) — salin ke `waras-web/assets/`. Logo MeowLabs TIDAK dipakai di UI
  klinik (hanya Login, bila mau: `meowlabs-mark-paper.png`).
- SEMUA data halaman dari API (`fetch`), bukan hardcode di HTML — kecuali
  label statis. Auth: token di `localStorage`, guard redirect ke `/` bila
  401. Search topbar memanggil `GET /api/patients?search=`.
- Footer Login/UI: "v1.0 · Mode Klinik · SQLite · Audit trail aktif"
  (BUKAN "Mode Rumah Sakit · PostgreSQL" seperti di desain — itu mode
  Fase 3; yang berjalan sekarang SQLite).

## 7.5 Fase 2 (Jembatan Nasional) — lihat `docs/FASE2.md`

Fase 2 (branch `fase-2-jembatan`) menambah: outbox + worker latar,
adapter SATUSEHAT + BPJS (`waras-bridge`), endpoint jembatan
(`/api/bridges`, `/api/outbox`, `/api/seps`, `/api/referrals`,
`/api/pcare/visits`, `/api/antrean-online`, `/api/beds`, `/api/claims`,
`/api/satusehat/resources`), endpoint baru `POST
/api/encounters/{id}/close` (status -> Selesai + enqueue), aksi RBAC
`ViewBridges/ManageOutbox/CreateSep/CreateReferral/CreateClaim/
SyncBridges`, dan kolom `satusehat_id`/`satusehat_status` +
`invoices.sep_id`. FASE2.md adalah sumber kebenaran Fase 2; dokumen
ini tetap sumber kebenaran Fase 1.

## 7.6 Fase 3 (Rumah Sakit) — lihat `docs/FASE3.md`

Fase 3 (branch `fase-3-rumah-sakit`, belum merge/deploy) menambah:
modul IGD (triase + observasi), Rawat Inap (ward/bed/admisi/mutasi/
pulang/catatan perawatan), Kamar Operasi & Bersalin (jadwal + tim +
persalinan), Laboratorium (order + hasil LOINC + flag kritis),
Radiologi (order + laporan), Casemix/INA-CBG workbench (grouping
lokal + export JSON untuk E-Klaim — TIDAK ada submit E-Klaim),
mode PostgreSQL (`WARAS_DATABASE_URL`, DDL `migrations-pg/`),
validasi pasien salah (422 `WRONG_PATIENT`), aksi RBAC
`ViewRumahSakit/TriaseIgd/KelolaRawatInap/KelolaOperasi/PesanLab/
HasilLab/PesanRadiologi/LaporanRadiologi/GroupingCasemix`, kolom
`claims.patient_id` (migrasi 0005), dan KPI dasbor lab/IGD/bed yang
kini dihitung dari tabel domain. FASE3.md adalah sumber kebenaran
Fase 3.

## 7. Yang SENGAJA tidak dibangun (Fase 2–4)

SATUSEHAT/FHIR sinkronisasi, BPJS (VClaim/PCare/Aplicares/Antrean Online/
E-Klaim), IGD, Rawat Inap, OK & Bersalin, Lab, Radiologi, Casemix,
Inventori, Kepegawaian, Laporan/SIRS, Hemodialisa, Transfusi, Gizi, Gigi,
Portal Pasien, API Publik. JANGAN buat stub endpoint untuk itu — cukup
tulis di README. Seam yang disiapkan: tipe FHIR di core, `fhir::` helpers,
kolom `ina_cbg_code` di invoices, dan struktur crate per modul.
