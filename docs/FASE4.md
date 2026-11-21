# WARAS — Kontrak build Fase 4 (Ekosistem: semua sisa "segera")

Dokumen ini SATU-SATUNYA sumber kebenaran untuk Fase 4. Baca sebelum
menulis kode. Bila kontrak dan desain bertentangan, kontrak ini yang
menang, dan penyimpangannya dicatat di README repo.

Sumber: paper v4 (Fase 4 = hemodialisa, transfusi darah, gizi, rekam
medis gigi, portal pasien, API publik; modul 12a–12c = inventori,
kepegawaian, laporan/SIRS; modul 3 = Rawat Jalan), FigCraft v16
`~/workspace/waras/figcraft/DESIGN.md` + layar `Hemodialisa.png`,
`Transfusi Darah.png`, `Gizi.png`, `Rekam Medis Gigi.png`,
`Portal Pasien (Mobile).png`, `API Publik.png`, `Inventori.png`,
`Kepegawaian.png`, `Laporan & SIRS.png`, `Pengaturan & Audit.png`,
`Rawat Jalan.png` di `~/workspace/waras/figcraft/exports-v16/`, dan
`docs/CONTRACT.md` + `docs/FASE2.md` + `docs/FASE3.md`.

Aturan Fase 1–3 tetap berlaku: build offline (`cargo test --offline`,
`cargo build --release --offline`), SQLx hanya `query`/`query_as` +
`FromRow` (JANGAN macro `query!`), tipe domain hanya dari
`waras-core`, bahasa UI/pesan API Indonesia, tanggal klinik =
env `WARAS_TODAY` (default 2026-11-19), tidak ada kredensial asli di
repo. JANGAN push ke GitHub, JANGAN sentuh VPS, JANGAN deploy.
Branch: `fase-4-ekosistem` (dari main @ `eb588d2`).

Tujuan akhir: NOL item sidebar "segera". Setelah Fase 4, setiap item
sidebar adalah tautan nyata ke halaman nyata. Satu-satunya kata
"Segera" yang BOLEH tersisa adalah label status stok
(`Drug::stock_status()` — Kritis/Segera/Aman, artinya "segera restock",
bukan "segera hadir") di Farmasi/Inventori. Itu label klinis, bukan
tag coming-soon, dan wajib didokumentasikan di laporan akhir.

## 0. Pembagian kerja (file TIDAK boleh tumpang tindih)

| Agen | Milik |
|---|---|
| A: core + DB + seed | `waras-core/src/types.rs`, `rbac.rs`, `error.rs` (tambah varian bila perlu); `waras-db/migrations/0006_fase4.sql`, `waras-db/migrations-pg/0006_fase4.sql`; `waras-db/src/{hemodialisa,transfusi,gizi,gigi,inventori,kepegawaian,laporan,pengaturan,rawat_jalan}.rs` (baru), `waras-db/src/users.rs` (hanya tambah method sesi pasien §7), `waras-db/src/seed.rs`, `waras-db/src/rows.rs` (bila perlu); JANGAN sentuh `waras-db/src/lib.rs` dan `waras/src/main.rs` (koordinator yang memasang `mod` + wiring seed) |
| B: API modul | `waras-api/src/fase4_handlers.rs` (baru), `waras-api/src/lib.rs` (route modul Fase 4 + UI saja — lihat §5.4), `waras-api/src/tests.rs` (tambah tes); JANGAN sentuh `web.rs`, `public_api.rs`, `portal_handlers.rs`, `api_key_handlers.rs` |
| C: web UI | `waras-web/assets/{hemodialisa,transfusi,gizi,gigi,portal,api_publik,inventori,kepegawaian,laporan,pengaturan,rawat_jalan}.html` + `js/{...}.js` (baru), edit sidebar di SEMUA html, `waras-api/src/web.rs` (tambah `serve_*` saja), `waras-web/assets/js/app.js` (hanya tambah helper modal + hapus penanganan `.soon`), `waras-web/assets/css/app.css` (hanya TAMBAH gaya modal di akhir file), `waras-web/assets/rme.html` (perbaiki 4 kartu "segera", lihat §6.3), `waras-web/assets/antrean.html` (tautan "Lihat panduan", lihat §6.3); JANGAN sentuh `waras-api/src/lib.rs` (koordinator yang memasang route UI — C tulis baris route yang dibutuhkan di laporannya) |
| D: API Publik + Portal (backend) | `waras-db/src/api_keys.rs` (baru), `waras-api/src/public_api.rs` (baru), `waras-api/src/portal_handlers.rs` (baru), `waras-api/src/api_key_handlers.rs` (baru); JANGAN sentuh `waras-api/src/lib.rs` dan `web.rs` (koordinator yang memasang route dari laporan D) |

Semua repo method memakai `&self.pool` lewat `sqlx::query` /
`query_as` / `query_scalar` generik saja — JANGAN menyebut tipe pool
(`SqlitePool`/`AnyPool`) di file baru.

## 1. Skema DB (migrasi `0006_fase4.sql`, SQLite + PG)

Gaya Fase 1–3: TEXT + snake_case, tanpa FK ketat. ID mengikuti pola
yang tertulis. `created_at` TEXT `YYYY-MM-DD HH:MM:SS`. Migrasi PG
(`migrations-pg/0006_fase4.sql`) = DDL yang sama dengan penyesuaian
minimal yang sudah berlaku di 0001–0005 (koordinator Fase 3 menulis
ulang placeholder; agen A menulis DDL PG mengikuti pola file
`migrations-pg/0005_claim_patient.sql`).

```sql
-- Sesi pasien (portal): kolom baru di tabel sessions ----------------------
ALTER TABLE sessions ADD COLUMN patient_id TEXT;   -- NULL = sesi staf

-- Hemodialisa ---------------------------------------------------------------
CREATE TABLE hd_machines (
  id TEXT PRIMARY KEY,                    -- HD-1
  name TEXT NOT NULL,                     -- Mesin HD 1
  status TEXT NOT NULL DEFAULT 'Siap'    -- Siap / Terpakai / Perawatan
);
CREATE TABLE hd_sessions (
  id TEXT PRIMARY KEY,                    -- HD-20261119-001
  patient_id TEXT NOT NULL,
  machine_id TEXT NOT NULL,
  encounter_id TEXT,
  dialyzer TEXT NOT NULL DEFAULT '',     -- mis. F6
  duration_minutes INTEGER NOT NULL DEFAULT 240,
  blood_flow INTEGER NOT NULL DEFAULT 300,      -- mL/mnt
  dialysate_flow INTEGER NOT NULL DEFAULT 500,  -- mL/mnt
  uf_goal_ml INTEGER NOT NULL DEFAULT 2000,
  status TEXT NOT NULL DEFAULT 'Terjadwal',     -- Terjadwal / Berjalan / Selesai / Batal
  scheduled_at TEXT NOT NULL,             -- YYYY-MM-DD HH:MM
  started_at TEXT,
  finished_at TEXT,
  notes TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- Transfusi darah ------------------------------------------------------------
CREATE TABLE blood_stock (
  id TEXT PRIMARY KEY,                    -- BL-A-POS-001
  blood_type TEXT NOT NULL,               -- A+ / A- / B+ / B- / O+ / O- / AB+ / AB-
  component TEXT NOT NULL,                -- WB / PRC / TC / FFP
  units INTEGER NOT NULL DEFAULT 0,
  expires_at TEXT NOT NULL,               -- YYYY-MM-DD
  status TEXT NOT NULL DEFAULT 'Tersedia' -- Tersedia / Menipis / Habis
);
CREATE TABLE blood_requests (
  id TEXT PRIMARY KEY,                    -- TR-20261119-001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  blood_type TEXT NOT NULL,
  component TEXT NOT NULL,
  units INTEGER NOT NULL,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Menunggu', -- Menunggu / Crossmatch / Siap / Diberikan / Batal
  requested_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE blood_crossmatches (
  id TEXT PRIMARY KEY,                    -- XM-000001
  request_id TEXT NOT NULL,
  bag_id TEXT NOT NULL,                   -- id blood_stock
  result TEXT NOT NULL,                   -- Cocok / Tidak Cocok
  tested_by TEXT NOT NULL,
  tested_at TEXT NOT NULL
);

-- Gizi ------------------------------------------------------------------------
CREATE TABLE nutrition_assessments (
  id TEXT PRIMARY KEY,                    -- GZ-000001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  weight_kg REAL NOT NULL,
  height_cm REAL NOT NULL,
  bmi REAL NOT NULL,
  imt_category TEXT NOT NULL,             -- Kurus / Normal / Gemuk / Obesitas
  risk TEXT NOT NULL DEFAULT 'Rendah',    -- Rendah / Sedang / Tinggi
  diet TEXT NOT NULL DEFAULT '',          -- mis. DM 1800 kkal
  notes TEXT NOT NULL DEFAULT '',
  assessed_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE nutrition_diets (
  id TEXT PRIMARY KEY,                    -- DT-000001
  patient_id TEXT NOT NULL,
  diet_type TEXT NOT NULL,                -- DM / Jantung / Rendah Garam / Tinggi Protein / Biasa
  calories INTEGER NOT NULL DEFAULT 0,
  notes TEXT NOT NULL DEFAULT '',
  start_date TEXT NOT NULL,               -- YYYY-MM-DD
  status TEXT NOT NULL DEFAULT 'Aktif'    -- Aktif / Selesai
);

-- Rekam medis gigi -------------------------------------------------------------
CREATE TABLE dental_records (
  id TEXT PRIMARY KEY,                    -- GG-000001
  patient_id TEXT NOT NULL,
  encounter_id TEXT,
  tooth_number TEXT NOT NULL,             -- notasi FDI: 11..48
  diagnosis TEXT NOT NULL,
  diagnosis_code TEXT NOT NULL DEFAULT '',-- ICD-10, mis. K02.9
  treatment TEXT NOT NULL,
  treatment_code TEXT NOT NULL DEFAULT '',-- ICD-9-CM, mis. 23.2
  dentist TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Selesai',-- Rencana / Berjalan / Selesai
  created_at TEXT NOT NULL
);

-- Inventori ---------------------------------------------------------------------
CREATE TABLE inventory_items (
  id TEXT PRIMARY KEY,                    -- INV-ITM-001
  name TEXT NOT NULL,
  category TEXT NOT NULL,                 -- Obat / Alkes / Umum
  unit TEXT NOT NULL,                     -- pcs / box / botol
  stock INTEGER NOT NULL DEFAULT 0,
  min_stock INTEGER NOT NULL DEFAULT 0,
  location TEXT NOT NULL DEFAULT '',      -- Gudang A / Rak C-01
  expiry TEXT NOT NULL DEFAULT '',        -- YYYY-MM-DD (boleh kosong)
  created_at TEXT NOT NULL
);
CREATE TABLE inventory_moves (
  id TEXT PRIMARY KEY,                    -- INM-000001
  item_id TEXT NOT NULL,
  kind TEXT NOT NULL,                     -- Masuk / Keluar / Opname
  qty INTEGER NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  by_name TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Kepegawaian ---------------------------------------------------------------------
CREATE TABLE staff (
  id TEXT PRIMARY KEY,                    -- STF-001
  name TEXT NOT NULL,
  nip TEXT NOT NULL DEFAULT '',
  role TEXT NOT NULL,                     -- Dokter Umum / Perawat / Apoteker / Kasir / Admin / Bidan / Dokter Gigi
  poli TEXT NOT NULL DEFAULT '',
  phone TEXT NOT NULL DEFAULT '',
  active INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE shift_schedules (
  id TEXT PRIMARY KEY,                    -- SHF-20261119-001
  staff_id TEXT NOT NULL,
  date TEXT NOT NULL,                     -- YYYY-MM-DD
  shift TEXT NOT NULL,                    -- Pagi / Siang / Malam
  poli TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- Pengaturan ------------------------------------------------------------------------
CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,                   -- backup_otomatis
  value TEXT NOT NULL,                    -- on / off / angka / teks
  updated_at TEXT NOT NULL
);

-- API Publik (kunci API) --------------------------------------------------------------
CREATE TABLE api_keys (
  id TEXT PRIMARY KEY,                    -- AK-000001
  name TEXT NOT NULL,                     -- mis. "Aplikasi Mobile JKN"
  key_prefix TEXT NOT NULL,               -- 12 karakter pertama kunci, mis. wrs_9f2k...
  key_hash TEXT NOT NULL,                 -- SHA-256 hex dari kunci penuh (TIDAK PERNAH simpan kunci penuh)
  scopes TEXT NOT NULL DEFAULT '',        -- dipisah koma: patients:read,queue:read,encounters:read,fhir:read
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked INTEGER NOT NULL DEFAULT 0
);
```

## 2. Tipe domain (`waras-core/src/types.rs`)

Satu struct per tabel di §1 (serde Serialize/Deserialize, nama persis):
`HdMachine`, `HdMachineStatus`, `HdSession`, `HdSessionStatus`,
`BloodStock`, `BloodRequest`, `BloodRequestStatus`, `BloodCrossmatch`,
`NutritionAssessment`, `NutritionDiet`, `DentalRecord`,
`InventoryItem`, `InventoryMove`, `Staff`, `ShiftSchedule`,
`AppSetting`, `ApiKey` (TIDAK PERNAH menyerialisasi `key_hash` —
`#[serde(skip_serializing)]` seperti `password_hash` di `User`).

`Role` tambah varian `Pasien` (label "Pasien"). `User` tidak berubah.

BMI: `bmi = weight_kg / (height_m^2)`, kategori: <18,5 Kurus;
18,5–24,9 Normal; 25–29,9 Gemuk; >=30 Obesitas. Helper di core:
`nutrition::bmi_category(bmi) -> &'static str` (fungsi kecil di
`waras-core/src/nutrition.rs` baru, milik agen A) — dipakai seed +
repo, bukan dihitung klien.

Status stok inventori: PAKAI ULANG `Drug::stock_status()` yang sudah
ada (stok <=12 Kritis, <=30 Segera, else Aman) — agen A menulis
helper `inventory_stock_status(stock: i64) -> StockStatus` yang
memanggil logika yang sama (bila `Drug::stock_status` terikat ke
struct Drug, duplikasi ambang 12/30 dengan komentar "ambang sama
dengan Farmasi"). Label "Segera" di sini artinya segera restock.

## 3. RBAC (`waras-core/src/rbac.rs`) — aksi baru + matriks

```rust
ViewEkosistem,     // lihat modul Fase 4 (semua peran staf login)
KelolaHemodialisa, // jadwal + status sesi HD
KelolaTransfusi,   // permintaan + crossmatch + stok darah
KelolaGizi,        // asesmen + diet
KelolaGigi,        // rekam gigi
KelolaInventori,   // item + mutasi stok
KelolaKepegawaian, // data staf + jadwal shift
ViewLaporan,       // laporan + SIRS
KelolaPengaturan,  // pengaturan aplikasi
ManageApiKeys,     // buat/cabut kunci API
ViewPortal,        // portal pasien (hanya peran Pasien; staf memakai ViewPatient)
```
- DokterUmum: ViewEkosistem, KelolaHemodialisa, KelolaTransfusi,
  KelolaGizi, KelolaGigi, KelolaInventori, KelolaKepegawaian,
  ViewLaporan. (TIDAK KelolaPengaturan, TIDAK ManageApiKeys.)
- Perawat: ViewEkosistem, KelolaHemodialisa, KelolaTransfusi,
  KelolaGizi, KelolaGigi, ViewLaporan.
- Apoteker: ViewEkosistem, KelolaInventori, ViewLaporan.
- Kasir: ViewEkosistem, ViewLaporan.
- Admin: semua (termasuk KelolaPengaturan + ManageApiKeys).
- Pasien: HANYA ViewPortal (tidak ada aksi staf lain).
- Label Indonesia mengikuti gaya yang ada. Test matriks wajib.

## 4. Repo `waras-db` (agen A) — method yang WAJIB ada

Semua `impl Db`, gaya Fase 1–3 (`sqlx::query` + `FromRow`,
error `WarasError::Database`). Nama method persis — agen B/D
memanggil nama-nama ini.

```rust
// -- Sesi pasien (§7, di users.rs) -------------------------------------------
pub async fn create_patient_session(&self, patient_id: &str) -> Result<String, WarasError>
pub async fn find_patient_by_token(&self, token: &str) -> Result<Option<Patient>, WarasError>
pub async fn find_patient_by_nik(&self, nik: &str) -> Result<Option<Patient>, WarasError>
// -- Hemodialisa ---------------------------------------------------------------
pub async fn list_hd_machines(&self) -> Result<Vec<HdMachine>, WarasError>
pub async fn list_hd_sessions(&self, date: &str) -> Result<Vec<HdSession>, WarasError>
pub async fn get_hd_session(&self, id: &str) -> Result<Option<HdSession>, WarasError>
pub async fn create_hd_session(&self, patient_id: &str, machine_id: &str, encounter_id: Option<&str>, dialyzer: &str, duration_minutes: i64, blood_flow: i64, dialysate_flow: i64, uf_goal_ml: i64, scheduled_at: &str, notes: &str) -> Result<HdSession, WarasError> // gagal 422 bila mesin bukan Siap; set mesin Terpakai
pub async fn set_hd_session_status(&self, id: &str, status: HdSessionStatus) -> Result<HdSession, WarasError> // Selesai/Batal -> mesin Siap
// -- Transfusi -------------------------------------------------------------------
pub async fn list_blood_stock(&self) -> Result<Vec<BloodStock>, WarasError>
pub async fn get_blood_stock(&self, id: &str) -> Result<Option<BloodStock>, WarasError>
pub async fn set_blood_stock_units(&self, id: &str, units: i64) -> Result<BloodStock, WarasError> // status ikut dihitung: 0 Habis, <=2 Menipis, else Tersedia
pub async fn list_blood_requests(&self, status: Option<&str>) -> Result<Vec<BloodRequest>, WarasError>
pub async fn get_blood_request(&self, id: &str) -> Result<Option<BloodRequest>, WarasError>
pub async fn create_blood_request(&self, patient_id: &str, encounter_id: Option<&str>, blood_type: &str, component: &str, units: i64, reason: &str, requested_by: &str) -> Result<BloodRequest, WarasError>
pub async fn set_blood_request_status(&self, id: &str, status: BloodRequestStatus) -> Result<BloodRequest, WarasError>
pub async fn list_crossmatches(&self, request_id: &str) -> Result<Vec<BloodCrossmatch>, WarasError>
pub async fn add_crossmatch(&self, request_id: &str, bag_id: &str, result: &str, tested_by: &str) -> Result<BloodCrossmatch, WarasError> // hasil Cocok -> request Crossmatch
// -- Gizi -------------------------------------------------------------------------
pub async fn list_nutrition_assessments(&self, patient_id: Option<&str>) -> Result<Vec<NutritionAssessment>, WarasError>
pub async fn create_nutrition_assessment(&self, patient_id: &str, encounter_id: Option<&str>, weight_kg: f64, height_cm: f64, risk: &str, diet: &str, notes: &str, assessed_by: &str) -> Result<NutritionAssessment, WarasError> // BMI + kategori dihitung server (core::nutrition)
pub async fn list_nutrition_diets(&self, patient_id: Option<&str>) -> Result<Vec<NutritionDiet>, WarasError>
pub async fn create_nutrition_diet(&self, patient_id: &str, diet_type: &str, calories: i64, notes: &str, start_date: &str) -> Result<NutritionDiet, WarasError>
pub async fn set_nutrition_diet_status(&self, id: &str, status: &str) -> Result<NutritionDiet, WarasError>
// -- Gigi --------------------------------------------------------------------------
pub async fn list_dental_records(&self, patient_id: Option<&str>) -> Result<Vec<DentalRecord>, WarasError>
pub async fn create_dental_record(&self, patient_id: &str, encounter_id: Option<&str>, tooth_number: &str, diagnosis: &str, diagnosis_code: &str, treatment: &str, treatment_code: &str, dentist: &str) -> Result<DentalRecord, WarasError> // tooth_number wajib pola FDI 11..48, bila tidak 400
pub async fn set_dental_record_status(&self, id: &str, status: &str) -> Result<DentalRecord, WarasError>
// -- Inventori -----------------------------------------------------------------------
pub async fn list_inventory_items(&self) -> Result<Vec<InventoryItem>, WarasError>
pub async fn get_inventory_item(&self, id: &str) -> Result<Option<InventoryItem>, WarasError>
pub async fn create_inventory_item(&self, name: &str, category: &str, unit: &str, stock: i64, min_stock: i64, location: &str, expiry: &str) -> Result<InventoryItem, WarasError>
pub async fn list_inventory_moves(&self, item_id: Option<&str>) -> Result<Vec<InventoryMove>, WarasError>
pub async fn add_inventory_move(&self, item_id: &str, kind: &str, qty: i64, note: &str, by_name: &str) -> Result<InventoryMove, WarasError> // Masuk: stok += qty; Keluar: stok -= qty (gagal 422 bila stok tidak cukup); Opname: stok = qty
// -- Kepegawaian -----------------------------------------------------------------------
pub async fn list_staff(&self) -> Result<Vec<Staff>, WarasError>
pub async fn create_staff(&self, name: &str, nip: &str, role: &str, poli: &str, phone: &str) -> Result<Staff, WarasError>
pub async fn set_staff_active(&self, id: &str, active: bool) -> Result<Staff, WarasError>
pub async fn list_shifts(&self, date: &str) -> Result<Vec<ShiftSchedule>, WarasError>
pub async fn create_shift(&self, staff_id: &str, date: &str, shift: &str, poli: &str) -> Result<ShiftSchedule, WarasError> // gagal 422 bila staf sudah punya shift di tanggal itu
// -- Laporan (agregat, tanpa tabel baru) -------------------------------------------------
pub async fn report_visits(&self, from: &str, to: &str) -> Result<Vec<(String, i64)>, WarasError> // (tanggal, kunjungan) dari `daily_stats.visits` (sama seperti `visits_7d` dasbor), dilengkapi hitungan antrean untuk tanggal yang belum ada di `daily_stats` — DIUBAH koordinator dari kontrak awal (antrean saja), lihat §8 butir 7
pub async fn report_top_diseases(&self, limit: i64) -> Result<Vec<(String, String, i64)>, WarasError> // (kode ICD-10, display, jumlah) dari conditions grup Utama/Sekunder/Kronis
pub async fn report_sirs(&self) -> Result<SirsSummary, WarasError> // RL1: kunjungan hari ini + rawat jalan/inap; RL2: 10 besar penyakit; RL3: bed occupancy + lab pending — hitung dari tabel nyata
// -- Pengaturan ----------------------------------------------------------------------------
pub async fn list_settings(&self) -> Result<Vec<AppSetting>, WarasError>
pub async fn set_setting(&self, key: &str, value: &str) -> Result<AppSetting, WarasError> // kunci hanya yang terdaftar di §4.1 (bila tidak 400)
// -- Rawat jalan (agregat, tanpa tabel baru) ---------------------------------------------------
pub async fn list_outpatient_visits(&self, date: &str) -> Result<Vec<OutpatientVisit>, WarasError> // gabung queue hari itu + encounter hari itu + appointment hari itu (satu baris per pasien, lihat §4.2)
```

`SirsSummary` (di `waras-core`, struct baru): `{ visits_today: i64,
outpatient_today: i64, inpatient_today: i64, beds_occupied: i64,
beds_total: i64, lab_pending: i64, top_diseases: Vec<(String, String,
i64)> }` — nama field persis (agen B mengembalikan struct ini apa
adanya sebagai JSON).

`OutpatientVisit` (di `waras-core`, struct baru): `{ patient_id,
patient_name, rm, poli, doctor, time, source, status }` — `source` =
"Antrean" / "Encounter" / "Janji"; dedup per pasien+poli (utamakan
baris Antrean bila ada).

### 4.1 Kunci pengaturan yang diizinkan

`backup_otomatis` (on/off), `sinkron_satusehat` (on/off),
`mode_offline` (on/off), `retensi_audit_hari` (angka, default 365).
Kunci lain → 400 "kunci pengaturan tidak dikenal". Nilai `on`/`off`
hanya untuk tiga kunci pertama; `retensi_audit_hari` wajib angka
1..3650.

### 4.2 Seed Fase 4 (`seed.rs`: `pub async fn seed_fase4(db) -> Result<bool>`)

Jalankan hanya bila tabel Fase 4 kosong. Tanggal klinik 2026-11-19.
Data dari layar desain v16 (agen A WAJIB melihat PNG layar di
`exports-v16/` dan mengambil baris yang digambar di sana; bila layar
tidak memberi angka, pakai seed di bawah ini dan catat di §8).
Pasien baru (tambah ke seed Fase 1 — ubah hitungan pasien jadi 14;
`create_patient` berikutnya jadi RM-000826):
- RM-000824 Slamet Riyadi, NIK 3374031005720013, lahir 1972-05-10, L,
  Jl. Tlogosari No. 18, Semarang, O+, BPJS 0001234567898, BPJS.
  Kondisi: N18.9 Gagal ginjal kronik (Kronis, dr. Rina, Sp.PD).
- RM-000825 Ratna Sari, NIK 3374042505850014, lahir 1985-05-25, P,
  Jl. Lamper Tengah No. 5, Semarang, A+, —, Umum.
- Mesin HD 4: HD-1 Terpakai, HD-2 Terpakai, HD-3 Siap, HD-4 Perawatan.
  Sesi HD 3: Slamet HD-1 dialyzer F6 240 mnt, flow darah 300,
  dialisat 500, UF 2000 mL, Berjalan 07.00; Dewi Lestari HD-2
  dialyzer F8 240 mnt Selesai 06.00; Agus Prasetyo HD-3 dialyzer F6
  240 mnt Terjadwal 13.00.
- Stok darah 8 baris (komponen PRC, 2 kantong tiap golongan, exp
  2026-12-19; O+ 1 kantong → Menipis; AB- 0 kantong → Habis):
  A+ 2, A- 2, B+ 2, B- 2, O+ 1, O- 2, AB+ 2, AB- 0.
  Permintaan 2: Bambang Riyanto PRC O+ 2 kantong "Pasca kateterisasi
  jantung" Menunggu; Sri Wahyuni PRC B+ 1 kantong "Pasca apendektomi"
  Crossmatch. Crossmatch 1: permintaan Sri, kantong BL-B-POS-001,
  Cocok, Ns. Sari Wijaya.
- Asesmen gizi 4: Siti 62 kg / 160 cm (BMI 24,2 Normal, risiko Rendah,
  diet "DM 1800 kkal"); Bambang 78 kg / 170 cm (BMI 27,0 Gemuk, risiko
  Sedang, diet "Jantung 2000 kkal"); Rina Marlina 58 kg / 158 cm
  (BMI 23,2 Normal, risiko Rendah, diet "Nifas 2200 kkal"); Dewi
  71 kg / 162 cm (BMI 27,1 Gemuk, risiko Sedang, diet "DM 1700 kkal").
  Diet 3: Siti "DM" 1800 kkal Aktif; Bambang "Jantung" 2000 kkal
  Aktif; Dewi "DM" 1700 kkal Aktif.
- Rekam gigi 5: Agus Prasetyo gigi 36 K02.9 Karies dentin, Tambal
  komposit 23.2, drg. Maya K., Selesai; Siti Rahmawati gigi 11 K03.0
  Atrisi email, Scaling 89.42, drg. Maya K., Selesai; Budi Santoso
  gigi 46 K02.9 Karies dentin, Tambal komposit 23.2, drg. Maya K.,
  Rencana; Putri Ayu gigi 21 K00.6 Gigi supernumerary, Observasi,
  drg. Maya K., Selesai; Joko Susilo gigi 16 K02.9 Karies dentin,
  Pencabutan 23.1, drg. Maya K., Selesai.
- Inventori 10 item (stok mengikuti ambang Farmasi 12/30): Infus set
  240 pcs (Aman), Sarung tangan 180 box (Aman), Masker bedah 320 box
  (Aman), Alkohol 70% 45 botol (Aman), Kasa steril 90 pcs (Aman),
  Spuit 5 mL 150 pcs (Aman), Benang jahit 3-0 28 pcs (Segera),
  Kateter urine 15 pcs (Segera), Oksigen tabung 8 tabung (Kritis),
  Handscoon steril 5 box (Kritis). Mutasi 3: Infus set Masuk +100
  "Pengadaan November"; Masker bedah Keluar -40 "Distribusi poli";
  Oksigen tabung Masuk +4 "Isi ulang".
- Staf 8 (6 pengguna seed + 2): drg. Maya Kusuma (Dokter Gigi, Gigi),
  Bidan Sari Handayani (Bidan, Cempaka). (6 pengguna seed Fase 1
  sudah ada sebagai staf: Ahmad Hidayat, Sari Wijaya, Rina, Dina
  Puspita, Sinta Maharani, Admin Sistem.)
  Shift 19 Nov 8 baris: Ahmad Pagi Poli Umum; Sari Pagi Poli Umum;
  Rina Siang Penyakit Dalam; Maya Pagi Gigi; Bidan Sari Pagi Cempaka;
  Dina Pagi Farmasi; Sinta Pagi Kasir; Andi Wijaya Malam IGD.
  ("Andi Wijaya" di sini adalah dokter jaga IGD dari seed Fase 3,
  bukan pasien Andi RM-000823 — nama sama, orang berbeda; catat di §8
  bila membingungkan, JANGAN ubah seed Fase 3.)
- Pengaturan seed: backup_otomatis=on, sinkron_satusehat=on,
  mode_offline=off, retensi_audit_hari=365.
- API keys: TIDAK ADA seed (kunci dibuat admin saat runtime, §7).

## 5. API (agen B + D) — endpoint baru

Prefix `/api`, bentuk respons/error mengikuti CONTRACT.md §3.
RBAC per §3. Semua POST tulis yang membawa `patient_id` +
`encounter_id` WAJIB lolos `verify_encounter_patient` dulu
(FASE3.md §6).

### 5.1 Modul (agen B, `fase4_handlers.rs`)

| Method & path | Peran | Request | Response |
|---|---|---|---|
| `GET /api/hemodialisa?date=` | login | default WARAS_TODAY | `{machines:[], sessions:[]}` |
| `POST /api/hemodialisa/sessions` | KelolaHemodialisa | `{patient_id, machine_id, encounter_id?, dialyzer?, duration_minutes?, blood_flow?, dialysate_flow?, uf_goal_ml?, scheduled_at, notes?}` | 201 HdSession; 422 bila mesin bukan Siap |
| `POST /api/hemodialisa/sessions/{id}/status` | KelolaHemodialisa | `{status}` | HdSession |
| `GET /api/transfusi/stock` | login | — | daftar BloodStock |
| `GET /api/transfusi/requests?status=` | login | — | daftar BloodRequest (+ nama pasien) |
| `POST /api/transfusi/requests` | KelolaTransfusi | `{patient_id, encounter_id?, blood_type, component, units, reason}` | 201 BloodRequest |
| `POST /api/transfusi/requests/{id}/crossmatch` | KelolaTransfusi | `{bag_id, result, tested_by?}` | BloodCrossmatch |
| `POST /api/transfusi/requests/{id}/status` | KelolaTransfusi | `{status}` | BloodRequest |
| `GET /api/gizi/assessments?patient_id=` | login | — | daftar NutritionAssessment |
| `POST /api/gizi/assessments` | KelolaGizi | `{patient_id, encounter_id?, weight_kg, height_cm, risk?, diet?, notes?}` | 201 (BMI + kategori dihitung server) |
| `GET /api/gizi/diets?patient_id=` | login | — | daftar NutritionDiet |
| `POST /api/gizi/diets` | KelolaGizi | `{patient_id, diet_type, calories, notes?, start_date?}` | 201 NutritionDiet |
| `POST /api/gizi/diets/{id}/status` | KelolaGizi | `{status}` | NutritionDiet |
| `GET /api/gigi/records?patient_id=` | login | — | daftar DentalRecord |
| `POST /api/gigi/records` | KelolaGigi | `{patient_id, encounter_id?, tooth_number, diagnosis, diagnosis_code?, treatment, treatment_code?, dentist}` | 201; 400 bila nomor gigi bukan FDI |
| `POST /api/gigi/records/{id}/status` | KelolaGigi | `{status}` | DentalRecord |
| `GET /api/inventori/items` | login | — | daftar InventoryItem (+ `stock_status`) |
| `POST /api/inventori/items` | KelolaInventori | `{name, category, unit, stock, min_stock, location?, expiry?}` | 201 InventoryItem |
| `GET /api/inventori/moves?item_id=` | login | — | daftar InventoryMove |
| `POST /api/inventori/items/{id}/moves` | KelolaInventori | `{kind, qty, note?}` | InventoryMove + item (stok diperbarui) |
| `GET /api/kepegawaian/staff` | login | — | daftar Staff |
| `POST /api/kepegawaian/staff` | KelolaKepegawaian | `{name, nip?, role, poli?, phone?}` | 201 Staff |
| `POST /api/kepegawaian/staff/{id}/active` | KelolaKepegawaian | `{active}` | Staff |
| `GET /api/kepegawaian/shifts?date=` | login | default WARAS_TODAY | daftar ShiftSchedule (+ nama staf) |
| `POST /api/kepegawaian/shifts` | KelolaKepegawaian | `{staff_id, date?, shift, poli?}` | 201; 422 bila staf sudah punya shift tanggal itu |
| `GET /api/laporan/kunjungan?from=&to=` | ViewLaporan | default 7 hari terakhir s.d. WARAS_TODAY | `[{date, visits}]` |
| `GET /api/laporan/penyakit?limit=` | ViewLaporan | default 10 | `[{code, display, count}]` |
| `GET /api/laporan/sirs` | ViewLaporan | — | SirsSummary |
| `GET /api/pengaturan` | login | — | daftar AppSetting |
| `PUT /api/pengaturan/{key}` | KelolaPengaturan | `{value}` | AppSetting; 400 bila kunci tidak dikenal / nilai tidak valid |
| `GET /api/rawat-jalan?date=` | login | default WARAS_TODAY | `{visits:[OutpatientVisit...]}` |

### 5.2 API Publik + Portal (agen D)

**Kunci API** (`api_key_handlers.rs`, prefix `/api`, auth staf biasa):
| Method & path | Peran | Request | Response |
|---|---|---|---|
| `GET /api/api-keys` | ManageApiKeys | — | daftar ApiKey (TANPA hash; tampilkan `key_prefix` + `last_used_at` + `revoked`) |
| `POST /api/api-keys` | ManageApiKeys | `{name, scopes:[...]}` | 201 `{key: <kunci penuh, HANYA SEKALI DITAMPILKAN>, api_key: {...}}`; kunci penuh TIDAK PERNAH tersimpan / dikembalikan lagi |
| `POST /api/api-keys/{id}/revoke` | ManageApiKeys | — | ApiKey (revoked=1) |

Format kunci: `wrs_` + 32 karakter heks acak (total 36). Hash:
SHA-256 hex dari kunci penuh. Prefix yang disimpan = 12 karakter
pertama. Scope yang diizinkan: `patients:read`, `queue:read`,
`encounters:read`, `fhir:read`. Scope di luar daftar → 400.

**Portal pasien** (`portal_handlers.rs`, prefix `/api`):
| Method & path | Auth | Request | Response |
|---|---|---|---|
| `POST /api/auth/patient-login` | publik | `{nik, birth_date}` (YYYY-MM-DD) | `{token, patient:{id, name, rm}}`; NIK/tanggal tidak cocok → 401 (pesan generik, jangan bocorkan mana yang salah) |
| `GET /api/portal/me` | token pasien | — | data pasien pemilik token (TANPA NIK lengkap — sensor tengah, mis. 337402******0001) |
| `GET /api/portal/appointments` | token pasien | — | janji milik pasien |
| `GET /api/portal/queue` | token pasien | — | antrean milik pasien hari ini |
| `GET /api/portal/lab` | token pasien | — | hasil lab milik pasien (order + results) |
| `GET /api/portal/prescriptions` | token pasien | — | e-resep milik pasien |

Token pasien: sesi 12 jam seperti sesi staf, disimpan di `sessions`
dengan `patient_id` terisi dan `user_id` NULL. Token pasien DITOLAK
oleh extractor staf (`AuthUser` — JOIN ke users tidak menemukan
baris). Extractor baru `PortalPatient` (di `portal_handlers.rs`):
baca Bearer, panggil `Db::find_patient_by_token`, 401 bila tidak ada
/ kedaluwarsa. JANGAN pernah kembalikan data pasien lain: semua
query portal memfilter `patient_id = token.patient_id`.

**API publik eksternal** (`public_api.rs`): router terpisah
`pub fn public_router() -> axum::Router<AppState>` — TANPA auth
staf; tiap request wajib header `X-Api-Key: wrs_...`. Kunci
diverifikasi: SHA-256(presented) == `key_hash`, `revoked = 0`,
scope cocok. Gagal → 401 (kunci salah) / 403 (scope tidak cukup).
`last_used_at` diperbarui tiap panggilan sukses (best-effort, jangan
gagalkan request bila update gagal). Rate limit: di memori per kunci,
100 request/menit; lewat → 429 (struktur `Mutex<HashMap<String,
(Instant, u32)>>` di dalam `public_api.rs`; JANGAN simpan state di
DB). Endpoint (read-only semua):
| Method & path | Scope | Response |
|---|---|---|
| `GET /public/v1/patients?search=` | patients:read | daftar pasien (TANPA NIK lengkap — sensor tengah) |
| `GET /public/v1/patients/{id}` | patients:read | pasien + alergi (NIK tersensor) |
| `GET /public/v1/queue?date=` | queue:read | antrean hari itu |
| `GET /public/v1/encounters?patient_id=` | encounters:read | daftar encounter |
| `GET /public/v1/fhir/Patient/{id}` | fhir:read | resource FHIR Patient JSON (pakai `fhir::patient_resource` yang sudah ada) |

### 5.3 UI routes (agen C melapor, koordinator memasang)

Halaman: `/hemodialisa`, `/transfusi`, `/gizi`, `/gigi`, `/portal`,
`/api-publik`, `/inventori`, `/kepegawaian`, `/laporan`,
`/pengaturan`, `/rawat-jalan`. Fungsi `serve_*` di `web.rs`
(mengikuti pola `serve_igd` dkk.). Aset: `/css/app.css`,
`/js/<nama>.js`, `/icons/<ikon>-CBD5E1.png` (+ varian 0B7285 bila
dipakai sebagai ikon aksen).

### 5.4 Pemasangan route (agen B, `lib.rs`)

Agen B memasang route §5.1 + merge router publik D:
`.merge(public_api::public_router())` — D WAJIB menyediakan
`pub fn public_router() -> axum::Router<AppState>` dengan nama
persis itu di `waras-api/src/public_api.rs`, dan D memasang
`pub mod public_api; pub mod portal_handlers; pub mod
api_key_handlers;` di `lib.rs`? TIDAK — `lib.rs` milik agen B;
D menulis baris `mod` yang dibutuhkan di laporannya, B yang
menambahkannya. Koordinator memasang route UI §5.3 + route
portal/api-keys D dari laporan D.

## 6. UI (agen C) — ringkas

- Ikuti pola halaman Fase 3 (sidebar + topbar sama, data dari API
  lewat fetch, label statis di HTML). Lihat PNG layar desain di
  `exports-v16/` untuk tiap halaman dan ambil susunannya dari sana.
- Sidebar (SEMUA html): 6 item "segera" jadi tautan nyata
  (Rawat Jalan → `/rawat-jalan`, Inventori → `/inventori`,
  Kepegawaian → `/kepegawaian`, Laporan & SIRS → `/laporan`,
  API Publik → `/api-publik`, Pengaturan & Audit →
  `/pengaturan`), DITAMBAH 5 item baru di grup PELAYANAN setelah
  Radiologi: Hemodialisa → `/hemodialisa`, Transfusi Darah →
  `/transfusi`, Gizi → `/gizi`, Rekam Medis Gigi → `/gigi`,
  Portal Pasien → `/portal`. Ikon (Lucide, pola v16): Hemodialisa
  `droplets`, Transfusi `droplet`, Gizi `apple`, Gigi `smile`
  (Lucide tidak punya ikon gigi), Portal `circle-user-round`
  (sudah ada), Inventori PINDAH ke `layout-grid` (sudah ada;
  Casemix tetap memakai `copy` — ini memperbaiki catatan Fase 3
  soal duplikasi ikon). Ikon baru (droplets, droplet, apple,
  smile): unduh SVG dari
  `https://cdn.jsdelivr.net/npm/lucide-static@0.469.0/icons/<nama>.svg`,
  render PNG 96×96 via `rsvg-convert` dengan stroke warna sidebar
  `#CBD5E1` (dan `#0B7285` bila dipakai sebagai ikon aksen),
  mengikuti pola v16. Bila unduhan gagal, pakai ulang ikon yang ada
  dan catat di laporan.
- Portal Pasien (`/portal`): halaman sempit ala mobile (maksimal
  lebar ~480px, mengikuti layar desain 390×844): bila belum login
  sebagai pasien, tampilkan form login NIK + kata sandi portal
  (Fase A menggantikan desain awal NIK + tanggal lahir, lihat
  `docs/FASE-A-A4.md`; token disimpan di localStorage kunci TERPISAH
  `waras-portal-token`, JANGAN menimpa token staf); bila sudah
  login, tampilkan janji, antrean, hasil lab, e-resep pasien itu
  (dari `/api/portal/*`). Bila dibuka staf dengan
  `?pasien=RM-...`, tampilkan data pasien itu read-only dari API
  staf biasa (label "Mode staf — lihat sebagai").
- API Publik (`/api-publik`): daftar kunci (nama, prefix, scope,
  dibuat, terakhir dipakai, status), form buat kunci (nama +
  checkbox scope) — kunci penuh yang dikembalikan HANYA SEKALI
  ditampilkan di modal (lihat di bawah) dengan tombol salin +
  peringatan "tidak bisa dilihat lagi". Tombol cabut (revoke) pakai
  konfirmasi modal.
- Modal component (baru, dipakai ≥2 tempat): helper
  `Waras.modal({title, bodyHtml, actions:[{label, kind, onClick}]})`
  di `app.js` + gaya `.modal-*` di akhir `app.css`; Esc menutup;
  klik backdrop menutup; dipakai di: tampil kunci API sekali +
  konfirmasi cabut kunci. Bila sempat, pakai juga untuk konfirmasi
  hapus/nonaktifkan staf.
- Odontogram (halaman Gigi): gambar 32 gigi (notasi FDI 11–18,
  21–28, 31–38, 41–48) sebagai grid; gigi yang punya rekam
  diwarnai menurut status (Rencana kuning, Berjalan teal, Selesai
  hijau); klik gigi → filter daftar rekam ke gigi itu. JANGAN
  gambar SVG gigi realistis — kotak berlabel nomor cukup (sesuai
  gaya Swiss klinis).

### 6.1 Halaman lain (ringkas)

- Hemodialisa: kartu mesin (status) + tabel sesi hari ini + form
  sesi baru (pasien, mesin, dialyzer, durasi, flow, UF, jadwal).
- Transfusi: kartu stok per golongan (dengan status Menipis/Habis)
  + tabel permintaan + form permintaan + crossmatch per permintaan.
- Gizi: form asesmen (berat, tinggi → BMI otomatis tampil sebelum
  kirim, dihitung JS dari rumus yang sama) + tabel asesmen + diet
  aktif per pasien.
- Inventori: tabel item + `stock_status` (pakai warna yang sama
  dengan Farmasi) + form item baru + mutasi per item.
- Kepegawaian: tabel staf + form staf + jadwal shift hari ini.
- Laporan: pemilih rentang tanggal + grafik kunjungan (bar sederhana
  dari div, bukan library chart) + 10 besar penyakit + kartu SIRS
  (RL1/RL2/RL3 dari `/api/laporan/sirs`).
- Pengaturan: toggle on/off untuk 3 kunci + input angka retensi +
  kartu mode (SQLite/PostgreSQL dari `/api/health`) + TABEL AUDIT
  (dari `GET /api/audit`, read-only, terbaru dulu, dengan rantai
  hash terlihat sebagai 8 karakter pertama `entry_hash`).
- Rawat Jalan: tabel kunjungan hari ini (pasien, poli, dokter, jam,
  sumber, status) + filter tanggal.

### 6.2 Sidebar — susunan akhir grup PELAYANAN

Dashboard, Registrasi & Antrean, Rekam Medis (RME), Rawat Jalan,
IGD, Rawat Inap, Kamar Operasi & Bersalin, Farmasi, Laboratorium,
Radiologi, Hemodialisa, Transfusi Darah, Gizi, Rekam Medis Gigi,
Portal Pasien. (KEUANGAN, OPERASIONAL, INTEGRASI, SISTEM tetap.)

### 6.3 Bersihkan sisa "segera" di luar sidebar

- `rme.html`: 4 kartu "segera" — Laboratorium dan Radiologi kini
  modul nyata: ubah jadi tautan ke `/lab` dan `/radiologi` dengan
  ringkasan nyata (hasil lab terakhir / laporan terakhir pasien
  dari API yang sudah ada). "Tindakan" → teks tanpa "segera":
  "Tindakan medis tercatat sebagai diagnosis kelompok 'Tindakan
  medis' di tab Encounter." "Dokumen" → teks tanpa "segera":
  "Surat rujukan dibuat dari halaman BPJS; hasil cetak tagihan
  dari Billing & Kasir."
- `antrean.html`: tautan "Lihat panduan" (`data-soon`) → jadikan
  anchor nyata ke kartu alur di halaman yang sama (beri `id`
  pada kartu alur).
- `app.js`: hapus penanganan klik `.soon` + toast "segera hadir
  (Fase 2–4)" (tidak ada lagi item `.soon`).

## 7. Portal pasien — keputusan desain (dicatat jujur)

Login pasien memakai NIK + kata sandi portal (hash Argon2, lihat
`docs/FASE-A-A4.md`). Desain awal Fase 4 memakai NIK + tanggal lahir,
tetapi Fase A menggantikannya: NIK + tanggal lahir bisa diketahui
orang lain dan bukan autentikasi kuat. Untuk produksi, opsi yang masih
terbuka adalah OTP (SMS/WA); kata sandi portal yang dibuat saat
registrasi sudah dipakai sekarang, tetapi belum ada endpoint set/ganti
kata sandi (pasien baru dari registrasi belum bisa login portal).
JANGAN pernah menulis NIK lengkap di respons API publik/portal (sensor
tengah). Token pasien tidak bisa mengakses endpoint staf apa pun.

## 8. Penyimpangan yang didokumentasikan (bukan bug)

1. Tarif/angka di layar desain yang tidak tertulis di kontrak ini:
   agen A mengambil dari PNG layar; bila layar dan kontrak berbeda,
   kontrak ini yang menang dan selisihnya dicatat di sini oleh
   koordinator.
2. "Andi Wijaya" muncul dua kali (dokter jaga IGD seed Fase 3 dan
   pasien RM-000823) — orang berbeda, nama sama; tidak diubah.
3. Pasien HD seed (Slamet, Dewi, Agus) tidak semuanya punya kondisi
   N18.9 — hanya Slamet yang diberi kondisi kronik N18.9; sesi HD
   Dewi/Agus adalah sesi terjadwal/selesai dari riwayat (catat bila
   layar desain berkata lain).
4. E-Klaim tetap tidak punya endpoint submit (FASE3.md §7).
5. Rate limit API publik 100/menit per kunci, di memori — hilang
   saat restart; cukup untuk demo, catat di README.
6. Modal component, keyboard accessibility, dan responsive layout
   (ditunda dari fase desain): modal DIBANGUN di Fase 4; keyboard
   accessibility + responsive layout tetap nice-to-have — laporkan
   jujur bila dilewati.
7. `report_visits` diubah koordinator dari kontrak awal (hitung
   antrean per hari) menjadi `daily_stats.visits` + fallback antrean:
   antrean hanya ada untuk hari ini, jadi laporan 7 hari dari antrean
   saja hanya menampilkan satu batang. Angka kini sama dengan
   `visits_7d` dasbor (19 Nov = 210, bukan 44).
8. Halaman API Publik mengecek peran lewat `GET /api/auth/me` dulu
   dan HANYA menembak `GET /api/api-keys` bila peran = Admin; peran
   lain melihat catatan "Hanya Admin…" tanpa tembakan itu — tembakan
   403 yang tertangkap tetap tercatat sebagai console error di
   browser. Endpoint `/api/api-keys` sendiri tetap menegakkan
   ManageApiKeys (dokter → 403).

## 9. Yang tetap "segera": TIDAK ADA

Setelah Fase 4, grep `tag-segera` / `nav-item soon` di
`crates/waras-web/assets/*.html` harus 0. Koordinator memverifikasi
ini sebelum melapor. (Pengecualian: label status stok "Segera" —
lihat pembuka dokumen ini.)

Terverifikasi 2026-11-20: `tag-segera` = 0, `nav-item soon` = 0,
"segera hadir" = 0 di HTML/JS. Tersisa 16 tautan `[data-soon]` di
HTML (tautan fitur kecil tanpa halaman sendiri, BUKAN item
sidebar) — semuanya ditangkap handler di `app.js` yang menampilkan
toast "Fitur ini belum tersedia." (tanpa kata "segera").

## 10. Env vars

Tidak ada env baru. `WARAS_DATABASE_URL` tetap mengatur mode
PostgreSQL (Fase 3). Migrasi 0006 berjalan di kedua backend.

## 11. Verifikasi (koordinator, 2026-11-20, branch fase-4-ekosistem)

- `cargo test --offline`: **227 lolos, 0 gagal** (api 82, bridge 35,
  core 32, db 75, web 3). Catatan: satu run sempat gagal 1 tes
  (`static_pages_and_fallback`, `/` → 404) karena rlib waras-web
  basi — rust-embed membekukan daftar aset saat kompilasi; sembuh
  dengan `cargo clean -p waras-web -p waras-api`.
- Release build OK (1m41 dtk, biner 15,7 MB).
- Curl di DB segar (port 3101): **96/96 lolos** — 26 halaman 200,
  login dokter/admin, KPI dasbor (lab 3, bed 86/120), satu alur
  tulis penuh per modul baru (HD, transfusi, gizi, gigi, inventori,
  staf, laporan, pengaturan, rawat jalan, portal pasien, kunci API
  + panggilan publik), regresi Amoxicillin → 422. Dari 10 kegagalan
  awal, 9 adalah ekspektasi skrip yang salah (bukan bug aplikasi);
  1 nyata: `report_visits` (lihat §8 butir 7) — diperbaiki, lalu
  FHIR publik dicek ulang terpisah: 200, NIK termaskir, NIK penuh
  tidak ada di respons.
- Playwright Chromium sandbox: **25 halaman × 2 peran (dokter +
  admin) = 50 muat, 0 console error, 0 page error, 0 request gagal,
  0 HTTP ≥400**. Satu console error 403 di run pertama (halaman
  API Publik menembak `/api/api-keys` sebagai dokter) diperbaiki
  lewat gate peran (lihat §8 butir 8) dan hilang di run ulang.
- Cek upgrade migrasi 0006: biner Fase 3 (`eb588d2`) di-boot di DB
  temp, login (membuat baris sesi), lalu biner Fase 4 di-boot di
  SALINAN DB itu — **token sesi lama tetap berlaku**
  (`GET /api/auth/me` 200), login ulang 200, pasien 12 → 14,
  bed 86/120 utuh, mesin HD 4, halaman `/hemodialisa` 200.
- Migrasi PostgreSQL 0006 BELUM dieksekusi di Postgres nyata
  (sama seperti 0005 di Fase 3) — jangan aktifkan mode Postgres
  sebelum boot Postgres dibuktikan.
