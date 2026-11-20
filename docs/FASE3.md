# WARAS — Kontrak build Fase 3 (Rumah Sakit)

Dokumen ini SATU-SATUNYA sumber kebenaran untuk Fase 3. Baca sebelum
menulis kode. Bila kontrak dan desain bertentangan, kontrak ini yang
menang, dan penyimpangannya dicatat di README repo.

Sumber: paper v4 Bagian 4–7 (Fase 3 = IGD, Rawat Inap, Kamar Operasi
dan Bersalin, Lab, Radiologi, Casemix/INA-CBG, mode PostgreSQL),
FigCraft v16 `~/workspace/waras/figcraft/DESIGN.md` + layar
`IGD.png`, `Rawat Inap.png`, `Kamar Operasi & Bersalin.png`,
`Laboratorium.png`, `Radiologi.png`, `Casemix (INA-CBG).png`
di `exports-v16/`, dan `docs/CONTRACT.md` + `docs/FASE2.md`.

Aturan Fase 1/2 tetap berlaku: build offline (`cargo test --offline`,
`cargo build --release --offline`), SQLx hanya `query`/`query_as` +
`FromRow` (JANGAN macro `query!`), tipe domain hanya dari
`waras-core`, bahasa UI/pesan API Indonesia, tanggal klinik =
env `WARAS_TODAY` (default 2026-11-19), tidak ada kredensial asli di
repo. JANGAN push ke GitHub, JANGAN sentuh VPS, JANGAN deploy.
Branch: `fase-3-rumah-sakit` (dari main @ `447c547`).

## 0. Pembagian kerja (file TIDAK boleh tumpang tindih)

| Agen | Milik |
|---|---|
| A: core + DB + seed | `waras-core/src/types.rs`, `rbac.rs`, `error.rs` (tambah varian), `casemix.rs` (baru, grouper); `waras-db/migrations/0004_fase3.sql`; `waras-db/src/{igd,ranap,operasi,lab,radiologi,casemix}.rs` (baru), `seed.rs`, `dashboard.rs`, `rows.rs`, `claims.rs`; JANGAN sentuh `waras-db/src/lib.rs` dan `main.rs` (koordinator yang memasang `mod` + wiring seed) |
| B: API | `waras-api/src/fase3_handlers.rs` (baru), `waras-api/src/lib.rs` (route saja), `waras-api/src/handlers.rs` (hanya tambah cross-check pasien salah, lihat §6), `waras-api/src/tests.rs` (tambah tes); JANGAN sentuh `web.rs` |
| C: web | `waras-web/assets/{igd,ranap,operasi,lab,radiologi,casemix}.html` + `js/{igd,ranap,operasi,lab,radiologi,casemix}.js` (baru), edit sidebar di SEMUA html (6 item jadi tautan nyata), `waras-api/src/web.rs` (tambah `serve_*` saja); JANGAN sentuh `waras-api/src/lib.rs` (koordinator yang memasang route UI — C tulis baris route yang dibutuhkan di laporannya) |
| D: PostgreSQL | `Cargo.toml` (workspace, fitur `postgres` sqlx), `waras-db/src/lib.rs` (hanya `Db::connect`/`migrate`/`mode()` — JANGAN sentuh blok `mod`), `waras-db/migrations-pg/` (baru, 0001–0004), `waras/src/main.rs` (env `WARAS_DATABASE_URL`); JANGAN sentuh `handlers.rs` (koordinator yang memasang mode di `/api/health`) |

Semua repo method memakai `&self.pool` lewat `sqlx::query` /
`query_as` / `query_scalar` generik saja — JANGAN menyebut tipe pool
(`SqlitePool`/`AnyPool`) di file baru, supaya perubahan pool agen D
tidak merusak kompilasi agen A.

## 1. Skema DB (migrasi `0004_fase3.sql`)

Gaya Fase 1/2: TEXT + snake_case, tanpa FK ketat. ID mengikuti pola
yang tertulis. `created_at` TEXT `YYYY-MM-DD HH:MM:SS`.

```sql
-- IGD ---------------------------------------------------------------------
CREATE TABLE igd_visits (
  id TEXT PRIMARY KEY,                    -- IGD-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,                     -- fast-track encounter (boleh NULL)
  complaint TEXT NOT NULL,
  triage TEXT NOT NULL,                   -- Merah / Kuning / Hijau
  doctor TEXT NOT NULL,                   -- dokter jaga
  status TEXT NOT NULL DEFAULT 'Menunggu',-- Menunggu / Observasi / Tindakan / Resusitasi / Selesai / Dirujuk
  arrived_at TEXT NOT NULL,               -- HH.MM
  created_at TEXT NOT NULL
);
CREATE TABLE igd_observations (
  id TEXT PRIMARY KEY,                    -- IGO-000001
  igd_id TEXT NOT NULL,
  note TEXT NOT NULL,                     -- teks observasi (TTV + rencana)
  recorded_by TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);

-- Rawat inap ---------------------------------------------------------------
CREATE TABLE wards (
  id TEXT PRIMARY KEY,                    -- Melati / Mawar / Anggrek / Cempaka / ICU
  name TEXT NOT NULL,                     -- Melati
  kind TEXT NOT NULL,                     -- Interna / Bedah / Anak / Obstetri / ICU
  floor TEXT NOT NULL                     -- Lantai 2
);
CREATE TABLE ward_beds (
  id TEXT PRIMARY KEY,                    -- 3A-01
  ward_id TEXT NOT NULL,
  room TEXT NOT NULL,                     -- 3A
  bed_no TEXT NOT NULL,                   -- 01
  status TEXT NOT NULL DEFAULT 'Kosong',  -- Terisi / Kosong / Dibersihkan
  updated_at TEXT NOT NULL
);
CREATE TABLE admissions (
  id TEXT PRIMARY KEY,                    -- ADM-20261119-001
  patient_id TEXT NOT NULL,
  bed_id TEXT NOT NULL,
  encounter_id TEXT,
  diagnosis_code TEXT NOT NULL,           -- ICD-10
  diagnosis_display TEXT NOT NULL,
  admitted_date TEXT NOT NULL,            -- YYYY-MM-DD
  dpjp TEXT NOT NULL,
  fall_risk INTEGER NOT NULL DEFAULT 0,   -- 1 = Risiko Jatuh
  status TEXT NOT NULL DEFAULT 'Dirawat', -- Dirawat / Pulang / Dirujuk
  discharged_at TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE care_notes (
  id TEXT PRIMARY KEY,                    -- CRN-000001
  admission_id TEXT NOT NULL,
  note TEXT NOT NULL,
  recorded_by TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);

-- Kamar operasi & bersalin --------------------------------------------------
CREATE TABLE surgeries (
  id TEXT PRIMARY KEY,                    -- OK-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  room TEXT NOT NULL,                     -- OK 1 / OK 2 / OK 3
  procedure TEXT NOT NULL,
  procedure_code TEXT NOT NULL,           -- ICD-9-CM, mis. 47.0
  operator TEXT NOT NULL,
  team TEXT NOT NULL DEFAULT '',          -- teks tim, mis. "dr. Budi, Sp.B · dr. Ani, Sp.An · 2 perawat"
  scheduled_at TEXT NOT NULL,             -- YYYY-MM-DD HH:MM
  status TEXT NOT NULL DEFAULT 'Terjadwal',-- Terjadwal / Menunggu / Siap / Berjalan / Selesai / Batal
  created_at TEXT NOT NULL
);
CREATE TABLE delivery_rooms (
  id TEXT PRIMARY KEY,                    -- VK 1
  status TEXT NOT NULL DEFAULT 'Kosong'   -- Terisi / Kosong
);
CREATE TABLE deliveries (
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
CREATE TABLE lab_orders (
  id TEXT PRIMARY KEY,                    -- LAB-2210 (tanpa tanggal, sesuai desain)
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  test_name TEXT NOT NULL,
  loinc TEXT NOT NULL,
  ordered_from TEXT NOT NULL,             -- Poli Umum / IGD / Mawar / ...
  ordered_by TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Menunggu',-- Menunggu / Proses / Selesai / Kritis
  created_at TEXT NOT NULL
);
CREATE TABLE lab_results (
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
  flag TEXT NOT NULL,                     -- Normal / Tinggi / Rendah / Kritis
  created_at TEXT NOT NULL
);

-- Radiologi -----------------------------------------------------------------
CREATE TABLE rad_orders (
  id TEXT PRIMARY KEY,                    -- RAD-0911 (tanpa tanggal, sesuai desain)
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  modality TEXT NOT NULL,                 -- X-Ray / USG / CT Scan / MRI
  area TEXT NOT NULL,                     -- Thorax AP / Antebrachii dextra / ...
  ordered_from TEXT NOT NULL,
  ordered_by TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Menunggu',-- Menunggu / Proses / Selesai
  created_at TEXT NOT NULL
);
CREATE TABLE rad_reports (
  id TEXT PRIMARY KEY,                    -- RDR-000001
  order_id TEXT NOT NULL UNIQUE,
  findings TEXT NOT NULL,
  radiologist TEXT NOT NULL,
  reported_at TEXT NOT NULL
);

-- Casemix: klaim Fase 2 dipakai ulang; dua perubahan -------------------------
-- (a) claims.invoice_id boleh NULL (klaim grouping tidak selalu berangkat
--     dari tagihan; rebuild tabel SQLite 12-langkah di migrasi ini),
-- (b) kolom baru claims.grouping_note TEXT (catatan grouping lokal).
```

## 2. Tipe domain (`waras-core/src/types.rs` + `casemix.rs`)

Satu struct per tabel di §1 (serde Serialize/Deserialize, nama persis):
`TriageLevel` (Merah/Kuning/Hijau), `IgdStatus`, `IgdVisit`,
`IgdObservation`, `Ward`, `WardBed`, `BedStatus`, `Admission`,
`AdmissionStatus`, `CareNote`, `Surgery`, `SurgeryStatus`,
`Delivery`, `DeliveryRoom`, `LabOrder`, `LabOrderStatus`,
`LabResult`, `LabFlag` (Normal/Tinggi/Rendah/Kritis), `RadOrder`,
`RadOrderStatus`, `RadReport`.

`Claim` berubah: `invoice_id: Option<String>`,
`grouping_note: Option<String>`. `ClaimStatus` tambah
`Dibayar | Terverifikasi | Diajukan | Pending` (nilai DB sama,
snake_case `dibayar`/`terverifikasi`/`diajukan`/`pending`).

`waras-core/src/casemix.rs` (baru): `CasemixGroup { code, tariff_idr,
source: &'static str }` + `group_casemix(diagnosis_code, procedure_code)
-> Option<CasemixGroup>` — tabel LOKAL dari layar desain (tarif
CONTOH, bukan tarif resmi Kemenkes; `source = "lokal-contoh"`):
I20.9 → I-4-13-I 4.210.000; K35.8 → G-4-11-II 6.840.000;
J18.9 → J-4-12-I 3.150.000; S52.5 → H-4-15-I 5.420.000;
O80 → O-4-10-I 2.980.000; J06.9 → J-4-10-0 210.000;
J03.9 → J-4-10-0 185.000. Diagnosis lain / prosedur belum terpetakan
→ `None` (API menjawab 422 "belum terpetakan, perbaiki pengkodean" —
sesuai catatan desain CLM-5513).

`lab_flag(value: f64, ref_low, ref_high, crit_low, crit_high)
-> LabFlag`: Kritis bila melewati ambang kritis, Tinggi/Rendah bila
di luar rujukan, Normal bila di dalam. Nilai non-numerik (mis.
golongan darah) → Normal dengan `ref_low/high = None`.

LOINC panel lab (konstanta di core, dipakai seed + validasi order):
Darah lengkap 58410-2, Troponin I 10839-9, HbA1c 4548-4,
Urinalisis lengkap 24357-6, Golongan darah 883-9,
Elektrolit (Na/K) 2951-2, Gula darah puasa 1558-6.
(LOINC vital sudah ada di `VitalCode::loinc_code()`.)

## 3. RBAC (`waras-core/src/rbac.rs`) — aksi baru + matriks

```rust
ViewRumahSakit,   // lihat modul Fase 3 (semua peran login)
TriaseIgd,        // triase + ubah status IGD
KelolaRawatInap,  // admisi / mutasi / pulang / catatan perawatan
KelolaOperasi,    // jadwal operasi + persalinan
PesanLab, PesanRadiologi,   // buat order
HasilLab,         // isi hasil lab
LaporanRadiologi, // tulis laporan radiologi
GroupingCasemix,  // grouping + kelola workbench casemix
```
- DokterUmum: semua 9. Perawat: ViewRumahSakit, TriaseIgd,
  KelolaRawatInap, PesanLab, PesanRadiologi. Apoteker: ViewRumahSakit
  saja. Kasir: ViewRumahSakit, GroupingCasemix. Admin: semua.
- Label Indonesia mengikuti gaya yang ada. Test matriks wajib.

## 4. Repo `waras-db` (agen A) — method yang WAJIB ada

Semua `impl Db`, gaya Fase 1/2 (`sqlx::query` + `FromRow`,
error `WarasError::Database`). Nama method persis — agen B memanggil
nama-nama ini.

```rust
// -- Validasi pasien salah (§6) --------------------------------------------
pub async fn verify_encounter_patient(&self, encounter_id: &str, patient_id: &str) -> Result<(), WarasError>
// -- IGD ---------------------------------------------------------------------
pub async fn list_igd(&self, date: &str) -> Result<Vec<IgdVisit>, WarasError> // created_at LIKE date%
pub async fn get_igd(&self, id: &str) -> Result<Option<IgdVisit>, WarasError>
pub async fn create_igd_triage(&self, patient_id: &str, complaint: &str, triage: TriageLevel, doctor: &str, arrived_at: &str, encounter_id: Option<&str>) -> Result<IgdVisit, WarasError>
pub async fn set_igd_status(&self, id: &str, status: IgdStatus) -> Result<(), WarasError>
pub async fn add_igd_observation(&self, igd_id: &str, note: &str, by: &str) -> Result<IgdObservation, WarasError>
pub async fn list_igd_observations(&self, igd_id: &str) -> Result<Vec<IgdObservation>, WarasError>
pub async fn igd_counts(&self, date: &str) -> Result<(i64, i64, i64), WarasError> // (merah, kuning, hijau) status != Selesai/Dirujuk
// -- Rawat inap ---------------------------------------------------------------
pub async fn list_wards(&self) -> Result<Vec<Ward>, WarasError>
pub async fn list_ward_beds(&self, ward_id: Option<&str>) -> Result<Vec<WardBed>, WarasError>
pub async fn get_ward_bed(&self, id: &str) -> Result<Option<WardBed>, WarasError>
pub async fn set_bed_status(&self, id: &str, status: BedStatus) -> Result<(), WarasError>
pub async fn bed_occupancy(&self) -> Result<Vec<(String, i64, i64)>, WarasError> // (ward_id, terisi, total) per ward
pub async fn list_admissions(&self, status: Option<&str>) -> Result<Vec<Admission>, WarasError>
pub async fn get_admission(&self, id: &str) -> Result<Option<Admission>, WarasError>
pub async fn create_admission(&self, patient_id: &str, bed_id: &str, encounter_id: Option<&str>, diagnosis_code: &str, diagnosis_display: &str, dpjp: &str, fall_risk: bool) -> Result<Admission, WarasError> // gagal bila bed bukan Kosong; set bed Terisi dalam transaksi yang sama
pub async fn discharge_admission(&self, id: &str, by: &str) -> Result<Admission, WarasError> // status Pulang + bed Dibersihkan
pub async fn transfer_admission(&self, id: &str, bed_id: &str, by: &str) -> Result<Admission, WarasError> // bed lama Dibersihkan, bed baru Terisi; gagal bila bed baru bukan Kosong
pub async fn list_care_notes(&self, admission_id: &str) -> Result<Vec<CareNote>, WarasError>
pub async fn add_care_note(&self, admission_id: &str, note: &str, by: &str) -> Result<CareNote, WarasError>
// -- Operasi & bersalin ---------------------------------------------------------
pub async fn list_surgeries(&self, date: &str) -> Result<Vec<Surgery>, WarasError>
pub async fn get_surgery(&self, id: &str) -> Result<Option<Surgery>, WarasError>
pub async fn create_surgery(&self, patient_id: &str, encounter_id: Option<&str>, room: &str, procedure: &str, procedure_code: &str, operator: &str, team: &str, scheduled_at: &str) -> Result<Surgery, WarasError>
pub async fn set_surgery_status(&self, id: &str, status: SurgeryStatus) -> Result<(), WarasError>
pub async fn list_delivery_rooms(&self) -> Result<Vec<DeliveryRoom>, WarasError>
pub async fn list_deliveries(&self) -> Result<Vec<Delivery>, WarasError>
pub async fn get_delivery(&self, id: &str) -> Result<Option<Delivery>, WarasError>
pub async fn create_delivery(&self, patient_id: &str, age_years: i64, gravida_para: &str, dilation_cm: i64, fetal_heart_rate: i64, midwife: &str, doctor: &str, plan: &str, room: &str) -> Result<Delivery, WarasError> // gagal bila room bukan Kosong; set room Terisi
pub async fn set_delivery_status(&self, id: &str, status: &str) -> Result<(), WarasError> // Selesai -> room Kosong
// -- Laboratorium ---------------------------------------------------------------
pub async fn list_lab_orders(&self, date: &str, status: Option<&str>) -> Result<Vec<LabOrder>, WarasError>
pub async fn get_lab_order(&self, id: &str) -> Result<Option<LabOrder>, WarasError>
pub async fn create_lab_order(&self, patient_id: &str, encounter_id: Option<&str>, test_name: &str, loinc: &str, ordered_from: &str, ordered_by: &str) -> Result<LabOrder, WarasError>
pub async fn list_lab_results(&self, order_id: &str) -> Result<Vec<LabResult>, WarasError>
pub async fn add_lab_results(&self, order_id: &str, results: &[NewLabResult], by: &str) -> Result<LabOrder, WarasError> // hitung flag per analit (core::lab_flag); order -> Kritis bila ada analit Kritis, else Selesai; bila Kritis: buat notifikasi KRITIS + audit `lab.critical`
pub async fn lab_counts(&self, date: &str) -> Result<(i64, i64), WarasError> // (menunggu/proses, kritis) hari itu
// -- Radiologi -------------------------------------------------------------------
pub async fn list_rad_orders(&self, date: &str, status: Option<&str>) -> Result<Vec<RadOrder>, WarasError>
pub async fn get_rad_order(&self, id: &str) -> Result<Option<RadOrder>, WarasError>
pub async fn create_rad_order(&self, patient_id: &str, encounter_id: Option<&str>, modality: &str, area: &str, ordered_from: &str, ordered_by: &str) -> Result<RadOrder, WarasError>
pub async fn set_rad_order_status(&self, id: &str, status: RadOrderStatus) -> Result<(), WarasError>
pub async fn get_rad_report(&self, order_id: &str) -> Result<Option<RadReport>, WarasError>
pub async fn save_rad_report(&self, order_id: &str, findings: &str, radiologist: &str) -> Result<RadReport, WarasError> // upsert + order -> Selesai
// -- Casemix -----------------------------------------------------------------------
pub async fn list_casemix(&self) -> Result<Vec<Claim>, WarasError> // semua klaim, terbaru dulu
pub async fn group_claim_for_encounter(&self, encounter_id: &str, by: &str) -> Result<Claim, WarasError> // diagnosis utama encounter -> group_casemix; None -> 422; buat klaim Draft baru (id CLM-5518 dst) atau perbarui Draft yang sudah ada untuk encounter itu; tautkan invoice pasien bila ada; grouping_note = "Grouping lokal (perkiraan): {diag} -> {code}"
pub async fn casemix_kpis(&self) -> Result<(i64, i64, i64, i64), WarasError> // (total klaim, terverifikasi, dibayar_idr, pending/revisi)
```

`NewLabResult` (di `waras-db`, struct input): `{ analyte, loinc: Option<String>,
value_text, unit, ref_text, ref_low: Option<f64>, ref_high: Option<f64>,
critical_high: Option<f64>, critical_low: Option<f64> }` — flag dihitung
di `add_lab_results`, BUKAN dari klien.

### 4.1 Dasbor (`dashboard.rs`)

`dashboard_kpis` kini menghitung dari tabel nyata (bukan
`daily_stats` untuk hari ini):
- `lab_pending` = lab order hari ini status Menunggu/Proses;
  `lab_critical` = order hari ini status Kritis.
- `emergency_red` = IGD Merah aktif hari ini.
- `beds_occupied`/`beds_total` = dari `bed_occupancy()` (86/120 di seed).
`daily_stats` tetap dipakai untuk `patients_today`, revenue,
`visits_7d`, dan tanggal 13–18 Nov. Perubahan KPI vs Fase 1/2
(dokumentasikan di README + FASE3.md §8): lab_pending 26 → 3,
lab_critical 2 → 1, emergency_red 3 → 1 — karena sekarang dihitung
dari data nyata, bukan agregat seed.

### 4.2 Seed Fase 3 (`seed.rs`: `pub async fn seed_fase3(db) -> Result<bool>`)

Jalankan hanya bila tabel Fase 3 kosong. Tanggal klinik 2026-11-19.
Data dari layar desain v16 (lihat §8 untuk penyimpangan yang
didokumentasikan). Pasien baru (tambah ke seed Fase 1 — ubah
`seed_loads_exact_counts` jadi 12 pasien; `create_patient` berikutnya
jadi RM-000824):
- RM-000822 Maya Kusuma, NIK 3374031505930011, lahir 1993-05-15, P,
  Jl. Majapahit No. 9, Semarang, B+, BPJS 0001234567897, BPJS.
- RM-000823 Andi Wijaya, NIK 3374022204880012, lahir 1988-04-22, L,
  Jl. Hayam Wuruk No. 27, Semarang, O+, —, Umum.
- IGD 8 baris (jam, pasien, keluhan, triase, dokter jaga, status):
  08.02 Bambang Riyanto (RM-000820) "Nyeri dada, sesak" Merah
  Resusitasi dr. Andi Wijaya; 08.15 Joko Susilo (RM-000817)
  "Patah lengan kanan" Kuning Observasi dr. Sari Dewi; 08.21 Maya
  Kusuma (RM-000822) "Demam 39,5 °C" Kuning Observasi dr. Ahmad
  Hidayat; 08.33 Sri Wahyuni (RM-000821) "Luka robek dahi" Hijau
  Tindakan dr. Ahmad Hidayat; 08.47 Andi Wijaya (RM-000823)
  "Mual, muntah" Hijau Menunggu dr. Sari Dewi; 09.05 Rina Marlina
  (RM-000816) "Sesak ringan" Kuning Observasi dr. Andi Wijaya;
  09.18 Ahmad Fauzi (RM-000818) "Luka sayat tangan" Hijau Tindakan
  dr. Sari Dewi; 09.26 Putri Ayu (RM-000819) "Demam 38,8 °C" Hijau
  Menunggu dr. Ahmad Hidayat. Encounter fast-track: Bambang →
  ENC-2026-11-19-021, Joko → ENC-2026-11-19-022 (baru, status
  DalamPemeriksaan, poli IGD). Observasi seed (Ruang 2): Bambang
  "TD 150/95 · Nadi 104 · SpO2 96% · EKG: sinus takikardi. Rencana:
  rontgen thorax, troponin, observasi 4 jam."; Joko "TD 120/80 ·
  Nadi 88 · SpO2 99% · X-Ray: fraktur radius distal. Rencana: gips,
  kontrol ortopedi 1 minggu."
- Rawat inap — ward: Melati (Interna, Lantai 2, 32 bed: ruang 3A 12,
  3B 10, 3C 10), Mawar (Bedah, Lantai 2, 28 bed: 2A 14, 2B 14),
  Anggrek (Anak, Lantai 1, 24 bed: 1C 12, 1D 12), Cempaka (Obstetri,
  Lantai 4, 24 bed: 4A 12, 4B 12), ICU (Lantai 3, 12 bed: ICU-01..12).
  Hunian: Melati 28, Mawar 22, Anggrek 18, Cempaka 18, ICU 0
  (total 86/120 — sama dengan `daily_stats` 19 Nov). Status bed
  3A-01..12 persis desain (01 Terisi, 02 Terisi, 03 Terisi —
  koreksi: desain menggambar 3A-03 Kosong tapi tabel admisi
  menempatkan Ahmad Fauzi di 3A-03; tabel admisi yang menang, lihat
  §8 — 04 Terisi, 05 Dibersihkan, 06 Terisi, 07 Kosong, 08 Terisi,
  09 Terisi, 10 Kosong, 11 Dibersihkan, 12 Terisi); bed lain:
  isi Terisi sampai kuota hunian per ward tercapai (urut nomor),
  sisanya Kosong, 2 bed per ward Dibersihkan bila memungkinkan.
  Admisi 7 baris: Bambang Melati 3A-01 I20.9 Angina pektoris
  17 Nov dr. Rina, Sp.PD, fall_risk=1; Sri Wahyuni Mawar 2B-04
  K35.8 Apendisitis akut 18 Nov dr. Budi, Sp.B; Joko Susilo Mawar
  2B-07 S52.5 Fraktur radius 19 Nov dr. Budi, Sp.B; Dewi Lestari
  Anggrek 1C-02 J18.9 Pneumonia 18 Nov dr. Sari Dewi, Sp.A; Rina
  Marlina Cempaka 4A-03 O80 Persalinan normal 19 Nov dr. Maya,
  Sp.OG, fall_risk=1; Ahmad Fauzi Melati 3A-03 J18.9 Pneumonia
  19 Nov dr. Rina, Sp.PD; Putri Ayu Anggrek 1C-05 J06.9 ISPA atas
  19 Nov dr. Sari Dewi, Sp.A. Catatan perawatan seed: Bambang
  "Nyeri dada berkurang, TD 145/90, observasi ketat." (Ns. Sari
  Wijaya, 19 Nov 08.00); Rina "Pembukaan 6 cm, DJJ 140, observasi
  VK." (Bidan Sari, 19 Nov 09.00).
- Operasi 6 baris: 08.00 OK 1 Sri Wahyuni Apendektomi 47.0
  dr. Budi, Sp.B Selesai; 09.30 OK 2 Bambang Riyanto Kateterisasi
  jantung (tanpa kode ICD-9-CM di desain — pakai 37.22) dr. Rina,
  Sp.JP Berjalan; 11.00 OK 1 Agus Prasetyo Hernioplasti 53.0
  dr. Budi, Sp.B Siap; 13.00 OK 3 Rina Marlina Seksio sesarea 74.1
  dr. Maya, Sp.OG Siap; 14.30 OK 2 Joko Susilo ORIF radius 79.32
  dr. Budi, Sp.B Menunggu; 16.00 OK 3 Sri Wahyuni Histerektomi 68.4
  dr. Maya, Sp.OG Terjadwal. Tim: "dr. Budi, Sp.B · dr. Ani, Sp.An ·
  2 perawat" (OK 1), "dr. Rina, Sp.JP · dr. Ani, Sp.An · 2 perawat"
  (OK 2), "dr. Maya, Sp.OG · dr. Ani, Sp.An · 1 bidan" (OK 3).
  Persalinan 1 baris: Rina Marlina, 34 thn, G2P1, pembukaan 6 cm,
  DJJ 140, Bidan Sari, dr. Maya, Sp.OG, rencana "Persalinan
  pervaginam, siaga seksio sesarea", ruang VK 2, Aktif. Ruang
  bersalin VK 1..4: VK 2 Terisi, VK 1 Terisi, VK 3 Kosong,
  VK 4 Kosong ("2 terisi · 2 kosong · 1 rujukan masuk (VK 2)" —
  rujukan masuk dicatat sebagai teks di `plan`? TIDAK; cukup
  status ruang; lihat §8).
- Lab 8 order (LAB-2210..2217): 2210 Siti Rahmawati Darah lengkap
  58410-2 Poli Umum Selesai; 2211 Bambang Riyanto Troponin I
  10839-9 IGD Kritis; 2212 Dewi Lestari HbA1c 4548-4 Penyakit Dalam
  Selesai; 2213 Rina Marlina Urinalisis lengkap 24357-6 Cempaka
  Proses; 2214 Joko Susilo Golongan darah 883-9 IGD Selesai;
  2215 Sri Wahyuni Elektrolit (Na/K) 2951-2 Mawar Proses;
  2216 Ahmad Fauzi Gula darah puasa 1558-6 Poli Umum Proses;
  2217 Putri Ayu Darah lengkap 58410-2 Poli Anak Selesai.
  Hasil LAB-2211 (persis desain): Troponin I 0,42 ng/mL, rujukan
  "< 0,04 ng/mL", ref_high 0,04, critical_high 0,04 → KRITIS;
  Hemoglobin 13,8 g/dL, 13,0–17,0, Normal; Leukosit 11.200 /µL,
  4.000–10.000, Tinggi; Trombosit 245.000 /µL, 150.000–400.000,
  Normal; Kreatinin 1,1 mg/dL, 0,7–1,3, Normal. Hasil LAB-2210
  (seed, nilai normal): Hemoglobin 14,2 g/dL Normal; Leukosit
  8.900 /µL Normal; Trombosit 280.000 /µL Normal. (Nilai LAB-2210
  dibuat seed — bukan dari desain; catat di §8.)
- Radiologi 7 order (RAD-0911..0917): 0911 Bambang Riyanto X-Ray
  Thorax AP Selesai; 0912 Joko Susilo X-Ray Antebrachii dextra
  Selesai; 0913 Rina Marlina USG Obstetri trimester 3 Selesai;
  0914 Dewi Lestari CT Scan Abdomen kontras Menunggu; 0915 Sri
  Wahyuni X-Ray Thorax post-op Proses; 0916 Ahmad Fauzi USG
  Abdomen Menunggu; 0917 Putri Ayu X-Ray Thorax AP Menunggu.
  Laporan seed: 0912 "Fraktur radius distal, tampak garis fraktur
  transversal." dr. Andi Wijaya, Sp.Rad, 09.05 (persis desain);
  0911 "Tidak tampak infiltrat; cor dalam batas normal."
  dr. Andi Wijaya, Sp.Rad, 08.50 (seed); 0913 "Janin tunggal hidup,
  presentasi kepala, taksiran berat 2.900 g." dr. Maya, Sp.OG,
  09.10 (seed).
- Casemix 8 klaim (CLM-5510..5517, persis desain; `invoice_id`
  NULL — grouping tidak berangkat dari tagihan Fase 1; klaim Fase 2
  CLM-20261119-001 tetap menaut INV-2026-1181):
  5510 Bambang I20.9 Angina I-4-13-I 4.210.000 Dibayar;
  5511 Sri K35.8 Apendisitis G-4-11-II 6.840.000 Terverifikasi;
  5512 Dewi J18.9 Pneumonia J-4-12-I 3.150.000 Diajukan;
  5513 Joko S52.5 Fraktur radius H-4-15-I 5.420.000 Pending
  (grouping_note: "Kode prosedur ORIF (79.32) belum terpetakan ke
  grouping. Perbaiki pengkodean, lalu kirim ulang.");
  5514 Rina O80 Persalinan O-4-10-I 2.980.000 Diajukan;
  5515 Dewi J06.9 ISPA J-4-10-0 210.000 Dibayar;
  5516 Ahmad Fauzi J06.9 ISPA J-4-10-0 210.000 Diajukan;
  5517 Putri Ayu J03.9 Tonsilitis J-4-10-0 185.000 Diajukan.
  (Diagnosis utama disimpan di `grouping_note`? TIDAK — klaim
  tidak punya kolom diagnosis; diagnosis dibaca dari encounter/
  kondisi saat grouping. Untuk klaim seed, tulis diagnosis di
  `grouping_note` sebagai "Diagnosis utama: I20.9 Angina pektoris.
  Grouping lokal (perkiraan): I-4-13-I." — kecuali 5513 yang
  memakai catatan di atas.)

## 5. API (agen B) — endpoint baru

Prefix `/api`, bentuk respons/error mengikuti CONTRACT.md §3.
RBAC per §3. Semua POST tulis yang membawa `patient_id` +
`encounter_id` WAJIB lolos `verify_encounter_patient` dulu (§6).

| Method & path | Peran | Request | Response |
|---|---|---|---|
| `GET /api/igd?date=` | login | default WARAS_TODAY | `{counts:{merah,kuning,hijau}, visits:[IgdVisit...]}` |
| `POST /api/igd/triage` | TriaseIgd | `{patient_id, complaint, triage, doctor, encounter_id?}` | 201 IgdVisit (id IGD-... berikutnya) |
| `GET /api/igd/{id}` | login | — | IgdVisit + observations[] |
| `POST /api/igd/{id}/status` | TriaseIgd | `{status}` | IgdVisit |
| `POST /api/igd/{id}/observations` | TriaseIgd | `{note}` | IgdObservation |
| `GET /api/ranap/beds?ward=` | login | — | `[{ward_id, ward_name, kind, beds:[WardBed...]}]` |
| `GET /api/ranap/admissions?status=` | login | — | daftar Admission (+ nama pasien, bed) |
| `POST /api/ranap/admissions` | KelolaRawatInap | `{patient_id, bed_id, encounter_id?, diagnosis_code, diagnosis_display, dpjp, fall_risk}` | 201 Admission; 422 bila bed bukan Kosong |
| `GET /api/ranap/admissions/{id}` | login | — | Admission + care_notes[] |
| `POST /api/ranap/admissions/{id}/discharge` | KelolaRawatInap | — | Admission Pulang + bed Dibersihkan |
| `POST /api/ranap/admissions/{id}/transfer` | KelolaRawatInap | `{bed_id}` | Admission + bed lama Dibersihkan, bed baru Terisi |
| `POST /api/ranap/admissions/{id}/notes` | KelolaRawatInap | `{note}` | CareNote |
| `GET /api/operasi?date=` | login | default WARAS_TODAY | `{surgeries:[], deliveries:[], delivery_rooms:[]}` |
| `POST /api/operasi` | KelolaOperasi | `{patient_id, encounter_id?, room, procedure, procedure_code, operator, team?, scheduled_at}` | 201 Surgery |
| `POST /api/operasi/{id}/status` | KelolaOperasi | `{status}` | Surgery |
| `POST /api/bersalin` | KelolaOperasi | `{patient_id, age_years, gravida_para, dilation_cm, fetal_heart_rate, midwife, doctor, plan, room}` | 201 Delivery; 422 bila room bukan Kosong |
| `GET /api/bersalin` | login | — | `{deliveries:[], rooms:[]}` |
| `POST /api/bersalin/{id}/status` | KelolaOperasi | `{status}` | Delivery |
| `GET /api/lab/orders?date=&status=` | login | — | daftar LabOrder (+ nama pasien) |
| `POST /api/lab/orders` | PesanLab | `{patient_id, encounter_id?, test_name, loinc?, ordered_from}` | 201 LabOrder; `loinc` kosong → isi dari tabel LOINC §2 bila test dikenal, bila tidak 422 "LOINC tidak dikenal" |
| `GET /api/lab/orders/{id}` | login | — | LabOrder + results[] |
| `POST /api/lab/orders/{id}/results` | HasilLab | `{results:[NewLabResult...]}` | LabOrder (status diperbarui) + results[] |
| `POST /api/lab/orders/{id}/status` | HasilLab | `{status}` | LabOrder |
| `GET /api/radiologi/orders?date=&status=` | login | — | daftar RadOrder (+ nama pasien) |
| `POST /api/radiologi/orders` | PesanRadiologi | `{patient_id, encounter_id?, modality, area, ordered_from}` | 201 RadOrder |
| `GET /api/radiologi/orders/{id}` | login | — | RadOrder + report? |
| `POST /api/radiologi/orders/{id}/report` | LaporanRadiologi | `{findings, radiologist}` | RadReport + order Selesai |
| `POST /api/radiologi/orders/{id}/status` | PesanRadiologi | `{status}` | RadOrder |
| `GET /api/casemix` | login | — | `{kpis:{total,terverifikasi,dibayar_idr,pending}, claims:[Claim...]}` |
| `POST /api/casemix/group` | GroupingCasemix | `{encounter_id}` | Claim (Draft baru/diubah) + `{code, tariff_idr}`; 422 bila diagnosis belum terpetakan |
| `GET /api/casemix/{id}` | login | — | Claim |
| `GET /api/casemix/{id}/export` | GroupingCasemix | — | JSON berkas klaim untuk disalin ke aplikasi E-Klaim (lihat §7) |

## 6. Validasi pasien salah (agen A + B)

- `waras-core/src/error.rs`: tambah varian `WrongPatient(String)` →
  HTTP 422, code `WRONG_PATIENT`.
- `Db::verify_encounter_patient(encounter_id, patient_id)`: baca
  encounter; bila tidak ada → `NotFound`; bila
  `encounter.patient_id != patient_id` → `WrongPatient`
  ("Encounter {id} milik pasien {nama} ({rm}), bukan {nama2}
  ({rm2})"). Bila `patient_id` kosong → lewati (tidak ada yang
  dicocokkan).
- Agen B memakai ini di: `add_vital`, `add_condition`,
  `create_prescription` (sudah ada — tambah cross-check; encounter
  menentukan pasien, jadi bila body membawa `patient_id` dan tidak
  cocok → 422), dan SEMUA endpoint Fase 3 yang membawa
  `patient_id` + `encounter_id`.
- UI (agen C): form tulis Fase 3 menampilkan konfirmasi
  "Pasien: {nama} ({rm})" sebelum kirim (teks dari `GET
  /api/patients/{id}` atau dari baris yang dipilih) — bukan
  sekadar field kosong.

## 7. E-Klaim — yang SENGAJA tidak dibangun

Tidak ada endpoint BPJS-hosted untuk E-Klaim (FASE2.md §10.1:
aplikasi E-Klaim adalah `ws.php` lokal atau alur FHIR Claim).
Workbench Casemix HANYA: grouping lokal (perkiraan) + persiapan
berkas + export JSON. Tombol "Kirim ke E-Klaim" di UI membuka
penjelasan jujur ("Kirim lewat aplikasi E-Klaim; WARAS tidak
mengirim langsung") + tombol salin JSON (`GET
/api/casemix/{id}/export`). JANGAN buat endpoint submit, JANGAN
menebak URL E-Klaim. Tulis ini di README + FASE3.md §8.

## 8. Penyimpangan yang didokumentasikan (bukan bug)

1. Nomor RM desain Rawat Inap (RM-000701..707) TIDAK dipakai —
   build memakai RM seed Fase 1 (Bambang RM-000820 dst., §4.2).
2. Kartu triase desain (Merah 2 / Kuning 5 / Hijau 9 = 16 pasien)
   adalah agregat ilustrasi; 8 baris tabel yang di-seed menghasilkan
   hitungan nyata 1 / 3 / 4. UI menghitung dari data nyata.
3. Peta bed desain menggambar 3A-03 Kosong tapi tabel admisi
   menempatkan Ahmad Fauzi di 3A-03 — tabel admisi yang menang
   (3A-03 Terisi).
4. KPI dasbor berubah karena kini nyata (§4.1): lab_pending 26 → 3,
   lab_critical 2 → 1, emergency_red 3 → 1.
5. KPI Casemix desain (486 klaim, Rp 1,42 miliar) adalah agregat
   30 hari/ilustrasi; UI menghitung dari 9 klaim nyata (8 seed +
   1 Fase 2).
6. Nilai hasil LAB-2210 dan laporan RAD-0911/0913 adalah seed
   (bukan dari desain); nilai LAB-2211 dan laporan RAD-0912 persis
   desain.
7. Tarif INA-CBG di grouper adalah CONTOH dari layar desain,
   bukan tarif resmi Kemenkes; grouping lokal = perkiraan, bukan
   grouper resmi. Tertulis di UI ("Grouping lokal (perkiraan)") dan
   di `grouping_note`.
8. "1 rujukan masuk (VK 2)" di layar bersalin tidak dimodelkan
   sebagai baris — hanya status ruang (VK 2 Terisi).
9. Kateterisasi jantung seed memakai kode ICD-9-CM 37.22 (desain
   tidak menulis kodenya).
10. Modal component definition, keyboard accessibility, dan
    responsive layout (ditunda dari fase desain) TIDAK dibangun di
    Fase 3 — laporkan jujur bila dilewati.
11. Seed Fase 3 menambahkan diagnosis 'utama' (COND-011..014) ke
    empat encounter selesai — ENC-016 J18.9, ENC-018 I20.9,
    ENC-019 K35.8, ENC-020 O80 — supaya grouping casemix bisa
    didemokan dari data seed. Seed Fase 1 hanya memberi 'utama' ke
    ENC-014 (J20.9, tidak terpetakan grouper) dan ENC-015 (J06.9
    tapi grup Riwayat), jadi sebelum ini tidak ada satu pun
    encounter seed yang bisa di-group.

## 9. Yang tetap "segera" (Fase 4 — JANGAN disentuh)

Inventori, Kepegawaian, Laporan & SIRS, API Publik,
Pengaturan & Audit, Portal Pasien, Hemodialisa, Transfusi Darah,
Gizi, Rekam Medis Gigi, Rawat Jalan (tautan `#` + tag `segera`
tetap). Sidebar: hanya 6 item Fase 3 yang jadi tautan nyata.

## 10. Env vars (untuk README)

```
WARAS_DATABASE_URL=   # bila diisi postgres://... -> mode PostgreSQL (rumah sakit);
                      # bila kosong -> SQLite (WARAS_DB, default waras.db)
```

`/api/health` mengembalikan `mode: "sqlite" | "postgres"`.
Footer UI: "Mode Rumah Sakit · PostgreSQL" bila Postgres,
"Mode Klinik · SQLite" bila SQLite.
