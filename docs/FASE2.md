# WARAS — Kontrak build Fase 2 (Jembatan Nasional)

Dokumen ini SATU-SATUNYA sumber kebenaran untuk Fase 2. Baca sebelum
menulis kode. Bila kontrak dan desain bertentangan, kontrak ini yang menang,
dan penyimpangannya dicatat di README repo.

Sumber: paper v4 Bagian 4.2 (pengiriman asinkron), Bagian 6 (Fase 2 =
FHIR SATUSEHAT + VClaim/PCare + Aplicare + antrean online; **E-Klaim
INA-CBG penuh + modul Casemix adalah Fase 3** — Fase 2 hanya membangun
adapter + alur submit/status klaim), Bagian 7 (pemetaan FHIR + adapter
BPJS), dan FigCraft v16 `~/workspace/waras/figcraft/DESIGN.md` + layar
`SATUSEHAT (FHIR)`, `BPJS · VClaim _ PCare`, `Antrean Online & Aplicares`.

Aturan Fase 1 tetap berlaku (CONTRACT.md §0): build offline
(`cargo test --offline`, `cargo build --release --offline`), SQLx hanya
`query`/`query_as` + `FromRow`, tipe domain hanya dari `waras-core`,
bahasa UI/pesan API Indonesia, tanggal klinik = env `WARAS_TODAY`,
tidak ada kredensial asli di repo. JANGAN push ke GitHub, JANGAN sentuh
VPS, JANGAN deploy. Build lokal di branch `fase-2-jembatan` saja.

## 0. Peta crate (Fase 2)

| Crate | Tambahan Fase 2 | Pemilik |
|---|---|---|
| `waras-core` | `bridge.rs` (SUDAH ADA di base commit: `OutboxDestination`, `OutboxStatus`, `BridgeHealth`, `SyncStatus`, `OutboxJob` + `backoff_minutes`); `types.rs` (SUDAH ADA: `Sep`, `SepStatus`, `Referral`, `ReferralStatus`, `PcareVisit`, `OnlineBooking`, `BedAvailability`, `Claim`, `ClaimStatus`); `rbac.rs` (tambah aksi Fase 2, lihat §5) | A: types sudah ada; C: rbac |
| `waras-bridge` | (SUDAH ADA kerangka: trait `Bridge`, `BridgeRegistry`, `BridgeConfig::from_env`, `BridgeError`, `SendOutcome`, `DisabledBridge`, `build_registry`, `build_http_client`, `bpjs_headers` + 1 test) — agen bridge mengisi `satusehat.rs` + `bpjs.rs` | B |
| `waras-db` | Migrasi `0002_fase2.sql`, `outbox.rs`, `seps.rs`, `claims.rs`, `beds.rs`, seed Fase 2 | A |
| `waras-api` | Handler jembatan + `worker.rs` + route (§5); `main.rs` (binary) menjalankan worker | C |
| `waras-web` | `satusehat.html` + `satusehat.js`, `bpjs.html` + `bpjs.js`, `antrean.html` + `antrean.js`; sidebar 3 item Fase 2 jadi tautan nyata di SEMUA halaman | D |

Ketergantungan kompilasi: `waras-db` dan `waras-bridge` hanya butuh
`waras-core` (sudah di base commit). `waras-api` butuh `waras-db` +
`waras-bridge` (keduanya stabil dari base commit + kontrak ini).
`waras-web` hanya butuh kontrak endpoint §5.

## 1. Skema DB (migrasi `0002_fase2.sql`)

Semua kolom baru mengikuti gaya Fase 1 (TEXT + snake_case, tanpa FK
ketat). Tabel baru:

```sql
-- Outbox: satu baris = satu pengiriman. Idempotensi lewat UNIQUE.
CREATE TABLE outbox (
  id TEXT PRIMARY KEY,                    -- OBX-000001
  destination TEXT NOT NULL,              -- snake_case OutboxDestination
  resource_type TEXT NOT NULL,            -- Patient/Encounter/Observation/Condition/MedicationRequest/AllergyIntolerance/SEP/Rujukan/Kunjungan/Antrean/Klaim
  resource_id TEXT NOT NULL,              -- id lokal sumber
  idempotency_key TEXT NOT NULL UNIQUE,   -- {destination}:{resource_type}:{resource_id}:{event}
  payload TEXT NOT NULL,                  -- JSON
  status TEXT NOT NULL DEFAULT 'pending', -- snake_case OutboxStatus
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 5,
  next_retry_at TEXT,                     -- ISO-8601, NULL = sekarang
  last_error TEXT,
  external_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_outbox_due ON outbox(status, next_retry_at, created_at);

-- SEP VClaim (satu baris per SEP; nomor SEP = id).
CREATE TABLE seps (
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
CREATE TABLE referrals (
  id TEXT PRIMARY KEY,                    -- RUJ-20261119-001
  patient_id TEXT NOT NULL,
  from_poli TEXT NOT NULL,
  to_facility TEXT NOT NULL,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'menunggu', -- terkirim / menunggu / ditolak
  created_at TEXT NOT NULL
);

-- Kunjungan FKTP ke PCare.
CREATE TABLE pcare_visits (
  id TEXT PRIMARY KEY,                    -- PCV-20261119-001
  patient_id TEXT NOT NULL,
  visit_date TEXT NOT NULL,
  poli TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'Tercatat',
  created_at TEXT NOT NULL
);

-- Booking antrean online Mobile JKN.
CREATE TABLE online_queue (
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
CREATE TABLE bed_availability (
  class TEXT PRIMARY KEY,                 -- Kelas 1 / Kelas 2 / Kelas 3 / VIP / ICU
  total INTEGER NOT NULL,
  available INTEGER NOT NULL,
  updated_at TEXT NOT NULL
);

-- Berkas klaim (E-Klaim; workbench Casemix penuh tetap Fase 3).
CREATE TABLE claims (
  id TEXT PRIMARY KEY,                    -- CLM-20261119-001
  invoice_id TEXT NOT NULL,
  sep_id TEXT,
  ina_cbg_code TEXT NOT NULL,
  amount INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft',   -- draft / terkirim / diterima / ditolak
  submitted_at TEXT,
  created_at TEXT NOT NULL
);
```

Kolom baru di tabel Fase 1 (ALTER TABLE):

```sql
ALTER TABLE patients ADD COLUMN satusehat_id TEXT;
ALTER TABLE patients ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE encounters ADD COLUMN satusehat_id TEXT;
ALTER TABLE encounters ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE observations ADD COLUMN satusehat_id TEXT;
ALTER TABLE observations ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE medication_requests ADD COLUMN satusehat_id TEXT;
ALTER TABLE medication_requests ADD COLUMN satusehat_status TEXT NOT NULL DEFAULT 'belum_dikirim';
ALTER TABLE invoices ADD COLUMN sep_id TEXT;
```

Nilai `satusehat_status`: `belum_dikirim` / `terkirim` / `gagal`
(`SyncStatus`, snake_case di DB).

## 2. Repo `waras-db` (agen A) — method yang WAJIB ada

Semua di `impl Db`, gaya Fase 1 (`sqlx::query` + `FromRow`, error
`WarasError::Database`). Nama method persis seperti di bawah — agen C
memanggil nama-nama ini.

```rust
// -- Outbox ---------------------------------------------------------------
// Enqueue idempotent: INSERT OR IGNORE atas idempotency_key; kembalikan
// true bila baris baru dibuat, false bila kunci sudah ada.
pub async fn enqueue_outbox(&self, job: &OutboxJob) -> Result<bool, WarasError>
// Ambil batch jatuh tempo: status='pending' DAN (next_retry_at IS NULL
// ATAU next_retry_at <= now_iso), urut created_at, LIMIT n.
pub async fn claim_outbox_batch(&self, limit: i64, now_iso: &str) -> Result<Vec<OutboxJob>, WarasError>
// Klaim batch SECARA ATOMIK: dalam satu transaksi, ubah status batch
// pending -> sending lalu kembalikan barisnya. (Worker memanggil ini,
// bukan claim + update terpisah.)
pub async fn claim_outbox_batch_atomic(&self, limit: i64, now_iso: &str) -> Result<Vec<OutboxJob>, WarasError>
// Tandai hasil kirim: status + attempts + next_retry_at + last_error +
// external_id + updated_at. Bila status Acked dan job adalah SATUSEHAT,
// perbarui juga satusehat_id/satusehat_status baris sumber (lihat §2.1).
pub async fn mark_outbox_sent(&self, id: &str, outcome: &SendOutcome, attempts: i64, next_retry_at: Option<String>, last_error: Option<String>) -> Result<(), WarasError>
// Hitung per (destination, status): dipakai layar status jembatan.
pub async fn outbox_counts(&self) -> Result<Vec<(String, String, i64)>, WarasError>
// Daftar + filter, terbaru dulu. limit default 50.
pub async fn list_outbox(&self, status: Option<&str>, destination: Option<&str>, limit: i64) -> Result<Vec<OutboxJob>, WarasError>
pub async fn get_outbox(&self, id: &str) -> Result<Option<OutboxJob>, WarasError>

// -- SEP / rujukan / PCare / antrean online / bed / klaim -----------------
pub async fn create_sep(&self, sep: &Sep) -> Result<Sep, WarasError>
pub async fn list_seps(&self, date: &str) -> Result<Vec<Sep>, WarasError> // created_at LIKE date%
pub async fn get_sep(&self, id: &str) -> Result<Option<Sep>, WarasError>
pub async fn set_sep_status(&self, id: &str, status: SepStatus) -> Result<(), WarasError>
pub async fn link_invoice_sep(&self, invoice_id: &str, sep_id: &str) -> Result<(), WarasError>
pub async fn create_referral(&self, r: &Referral) -> Result<Referral, WarasError>
pub async fn list_referrals(&self, date: &str) -> Result<Vec<Referral>, WarasError>
pub async fn create_pcare_visit(&self, v: &PcareVisit) -> Result<PcareVisit, WarasError>
pub async fn list_pcare_visits(&self, date: &str) -> Result<Vec<PcareVisit>, WarasError>
pub async fn list_online_queue(&self, date: &str) -> Result<Vec<OnlineBooking>, WarasError>
pub async fn set_online_queue_status(&self, booking_code: &str, status: &str, local_queue_id: Option<&str>) -> Result<(), WarasError>
pub async fn list_beds(&self) -> Result<Vec<BedAvailability>, WarasError>
pub async fn upsert_beds(&self, beds: &[BedAvailability]) -> Result<(), WarasError>
pub async fn create_claim(&self, c: &Claim) -> Result<Claim, WarasError>
pub async fn list_claims(&self) -> Result<Vec<Claim>, WarasError>
pub async fn get_claim(&self, id: &str) -> Result<Option<Claim>, WarasError>
pub async fn set_claim_status(&self, id: &str, status: ClaimStatus, submitted_at: Option<&str>) -> Result<(), WarasError>
```

### 2.1 Sinkronisasi `satusehat_*` dari outbox

`mark_outbox_sent` dengan `outcome.status == Acked` dan
`job.destination == Satusehat` memperbarui baris sumber sesuai
`resource_type`: `Patient` -> `patients` (by `resource_id` = RM id),
`Encounter` -> `encounters`, `Observation` -> `observations`,
`MedicationRequest` -> `medication_requests`, `Condition` ->
`conditions` (kolom `satusehat_*` TIDAK ada di conditions — lewati
update baris, cukup external_id di outbox; dokumentasikan ini),
`AllergyIntolerance` -> `allergies` (kolom tidak ada — sama, lewati).
Set `satusehat_id = outcome.external_id`, `satusehat_status =
'terkirim'`. Bila status akhir Dead dan destination Satusehat, set
`satusehat_status = 'gagal'` (tanpa id). Semua dalam transaksi yang sama
dengan update outbox.

### 2.2 Seed Fase 2 (tambah ke `seed.rs`, idempotent)

Jalankan hanya bila tabel-tabel Fase 2 kosong. Data dari layar desain
v16 (tanggal klinik 2026-11-19):

- SEP `SEP-20261119-001..008`: 001 Siti Rahmawati 0001234567890 RawatJalan
  Poli Umum Aktif; 002 Budi Santoso 0001234567891 RawatJalan Poli Anak
  Aktif; 003 Bambang Riyanto 0001234567892 RawatInap Melati Aktif; 004
  Dewi Lestari 0001234567893 RawatJalan Penyakit Dalam Aktif; 005 Agus
  Prasetyo 0001234567894 RawatJalan Gigi **Gagal**; 006 Rina Marlina
  0001234567895 RawatInap Cempaka Aktif; 007 Ahmad Fauzi 0001234567896
  RawatJalan Poli Umum Aktif; 008 Putri Ayu 0001234567897 RawatJalan
  Poli Anak Aktif. `created_by` = `dr. Ahmad Hidayat`.
- `online_queue` JKN-881201..207: 201 Siti Poli Umum 07.30 Hadir
  (local A-041); 202 Budi Poli Anak 08.00 Hadir; 203 Dewi Penyakit
  Dalam 08.30 Menunggu; 204 Agus Gigi 09.00 Menunggu; 205 Rina Poli
  Umum 09.15 Menunggu; 206 Ahmad Fauzi Poli Umum 09.30 Menunggu; 207
  Putri Ayu Poli Anak 09.45 Menunggu.
- `bed_availability`: Kelas 1 12/16, Kelas 2 24/32, Kelas 3 38/48,
  VIP 6/8, ICU 0/12.
- `referrals` 2 baris: Rina Marlina -> RSUD Dr. Kariadi (Bedah Saraf),
  Budi Santoso -> RSUD Dr. Kariadi (Bedah Saraf). Status Menunggu.
- `pcare_visits`: 3 baris hari ini (Siti, Budi, Dewi, poli masing-masing).
- `claims`: 1 baris Draft contoh: CLM-20261119-001, invoice
  INV-2026-1181, SEP-20261119-001, ina_cbg_code `J-4-17-I`, amount
  185000. (Kode INA-CBG contoh — bukan grouping sungguhan; tulis itu di
  README.)
- Outbox: KOSONG saat seed (tidak ada kiriman palsu).

### 2.3 Penyimpangan KPI desain (sengaja, didokumentasikan)

Angka agregat di layar desain (SATUSEHAT 12.480 terkirim, BPJS 96 SEP
hari ini, 182 kunjungan PCare) adalah agregat 30 hari/ilustrasi dan
TIDAK bisa diturunkan dari 8 baris seed. UI Fase 2 menghitung KPI dari
tabel nyata (outbox/seps/referrals/pcare_visits) — angkanya akan kecil
(8 SEP, 3 kunjungan PCare). Itu jujur, bukan bug. Catat di README.

## 3. Adapter `waras-bridge` (agen B)

### 3.1 HTTP client

`reqwest 0.12`, `default-features = false`, features `rustls-tls` +
`json` (sudah di workspace deps base commit). **Verifikasi dulu**:
`cargo check -p waras-bridge --offline` harus lolos dari cache. Bila
closure reqwest tidak lengkap di cache, fallback ke `hyper-util 0.1`
legacy Client + `hyper-rustls` (keduanya sudah di lock via axum/sqlx)
dan catat di laporan — jangan `cargo update` online.

### 3.2 SATUSEHAT (`satusehat.rs`)

- OAuth2 client credentials: POST `{auth_url}/accesstoken`, form
  `grant_type=client_credentials`, header Basic base64
  (`client_id:client_secret`). Respons: `access_token`, `expires_in`
  (detik), `token_type`. Cache token + expiry di struct (Mutex);
  refresh bila kedaluwarsa atau 401 (coba ulang sekali setelah refresh).
- Kirim resource: POST `{base_url}/{ResourceType}` (mis. `Patient`,
  `Encounter`, `Observation`, `Condition`, `MedicationRequest`,
  `AllergyIntolerance`), header `Authorization: Bearer <token>`,
  body = `job.payload` (JSON FHIR dari `waras-core::fhir`). Sukses
  2xx: `external_id` = `id` dari respons; `status` = Acked.
- Validasi SEBELUM kirim (tanpa HTTP): field wajib profil SATUSEHAT
  per resource — Patient wajib NIK + nama; Encounter wajib subject;
  Observation wajib code + subject; Condition wajib code ICD-10;
  MedicationRequest wajib drug + subject; AllergyIntolerance wajib
  substance + patient. Bila tidak lengkap -> `BridgeError::Validation`
  dengan NAMA FIELD yang bermasalah (pesan dipakai UI).
- `Bundle` (encounter_bundle): bila `resource_type == "Bundle"`, POST
  ke `{base_url}` (root) sebagai satu Bundle. (Dipakai bila worker
  mengirim bundle encounter utuh.)
- Error mapping: 401 -> Auth (refresh sekali, lalu Auth); 400/422 ->
  Validation (body dipotong 500 karakter di pesan); 429/5xx/transport
  -> Http (retryable); lainnya -> Rejected{status, body}.
- Tambah di `waras-core/src/fhir.rs`: `allergy_intolerance_resource`
  (dari `AllergyIntolerance`: substance + criticality + patient
  reference) + test. Builder lain sudah ada dan dipakai ulang.

### 3.3 BPJS (`bpjs.rs`) — satu struct, lima layanan

- Header: pakai `bpjs_headers` (SUDAH ADA di base commit, teruji):
  `X-cons-id`, `X-timestamp`, `X-signature`, `user_key`.
  Timestamp = detik Unix UTC saat request.
- Envelope respons: `{ "metaData": {"code": "200", "message": "OK"},
  "response": ... }`. `code == "200"` = sukses; selain itu ->
  `Rejected` (bila 4xx dari BPJS) atau `Validation` (bila code BPJS
  menandakan data salah, mis. peserta tidak ditemukan -> Validation
  "NIK tidak ditemukan").
- Dekripsi respons: respons BPJS VClaim/PCare terenkripsi AES-256-CBC
  (kunci = cons_id + secret_key + timestamp). Implementasikan BILA
  crate `aes 0.8` + `cbc` resolve offline dari cache; bila tidak,
  parse envelope saja dan catat dekripsi sebagai kekurangan yang
  didokumentasikan untuk langkah live. JANGAN menebak isi plaintext.
- Path endpoint: pakai pola bridging resmi yang umum (mis. VClaim
  `Peserta/nik/{nik}/tglSEP/{date}`, `SEP/2.0/insert`, `SEP/{noSep}`,
  `SEP/delete`; PCare `peserta/{noKartu}`, `kunjungan`; Antrean
  `antrean/add`, `antrean/get`, `antrean/update`, `antrean/batal`;
  Aplicares `bed/get`; E-Klaim `klaim/submit`, `klaim/status/{id}`).
  **Labeli di README sebagai "pola bridging; verifikasi ulang ke
  dokumen bridging resmi saat kredensial live tersedia"** — jangan
  klaim path ini sudah diverifikasi ke BPJS (tidak ada akses live).
- Dispatch `send(job)`: pilih layanan dari `job.destination`, lalu
  operasi dari `job.resource_type`: `SEP` -> insert SEP; `SEP Batal` ->
  delete; `Peserta` -> cek eligibilitas (GET, tanpa efek samping);
  `Rujukan` -> insert rujukan; `Kunjungan` -> submit kunjungan PCare;
  `Antrean` -> tambah antrean; `Antrean Batal` -> batal; `Klaim` ->
  submit klaim. `external_id` = nomor SEP / kode booking / id klaim
  dari `response`.

### 3.4 Stub server (test saja, di `#[cfg(test)]`)

Pakai `axum` (dev-dep, sudah di Cargo.toml skeleton) + `tokio::net::
TcpListener` di port ephemeral (port 0): tiru bentuk respons sandbox
SATUSEHAT (FHIR resource + `id`) dan BPJS (envelope `metaData` +
`response`). Stub HARUS bisa: (a) sukses, (b) gagal 500 N kali lalu
sukses (untuk uji retry), (c) merekam header + body yang diterima
(untuk uji signature + idempotensi), (d) menolak payload tidak valid
dengan 400 + pesan field. Tidak ada stub di kode produksi.

### 3.5 Test yang wajib ada (agen B)

- FHIR bundle per resource (Patient/Encounter/Observation/Condition/
  MedicationRequest/AllergyIntolerance) dari data seed.
- OAuth2 token cache: token dipakai ulang; 401 -> refresh sekali.
- Signature header hadir + bisa dihitung ulang (sudah ada 1 test di
  base commit — tambah uji end-to-end via stub yang memeriksa header).
- Retry: stub 500 dua kali lalu 200 -> adapter tetap mengembalikan
  sukses SETELAH worker-level retry (uji di level adapter: panggil
  `send` langsung 3x? TIDAK — retry adalah tugas worker; uji adapter
  hanya: 500 -> `BridgeError::Http`, 400 validasi -> `Validation`).
- Idempotensi: kirim job yang sama 2x ke stub yang menghitung hit ->
  adapter mengirim 2x HTTP (idempotensi dijamin outbox, BUKAN adapter);
  dokumentasikan itu di laporan. (Uji idempotensi outbox ada di agen A/C.)

## 4. Worker + wiring API (agen C)

### 4.1 Worker (`waras-api/src/worker.rs`, baru)

```rust
// Dijalankan binary sebagai task Tokio bila WARAS_WORKER_ENABLED=true.
// Loop: tidur interval -> claim_outbox_batch_atomic(batch, now) ->
// untuk tiap job: dispatch ke registry -> mark_outbox_sent / tandai
// Failed+backoff / Dead (Validation atau attempts >= max) -> audit.
// TIDAK PERNAH panic: semua error ditangkap + log, loop lanjut.
pub async fn run_worker(db: Arc<Db>, registry: Arc<BridgeRegistry>, config: &BridgeConfig)
```

Perilaku per hasil:
- `Ok(SendOutcome{Acked,..})` -> status Acked, external_id tersimpan,
  audit `bridge.acked`.
- `Ok(Sent)` -> status Sent (tetap diproses? TIDAK — Sent dianggap
  selesai tanpa ack; worker tidak mengirim ulang. Dokumentasikan.)
- `Err(NotConfigured)` -> biarkan Pending, JANGAN tambah attempts,
  JANGAN ubah next_retry_at, log debug sekali per loop.
- `Err(Validation)` -> status Dead, last_error = pesan, audit
  `bridge.failed_validation`.
- `Err(lain)` -> attempts+1; bila attempts >= max_attempts -> Dead;
  bila belum -> Failed + `next_retry_at` = now + `backoff_minutes
  (attempts)` (dari `OutboxJob::backoff_minutes`), last_error = pesan.
- Audit: setiap percobaan kirim dicatat (`bridge.send_attempt`,
  detail: job id, destination, status akhir, attempts).

### 4.2 Enqueue di handler Fase 1 (tambah di `handlers.rs`, JANGAN ubah perilaku Fase 1)

Setelah operasi sukses + audit Fase 1, enqueue (INSERT OR IGNORE;
abaikan bila kunci sudah ada — jangan gagalkan request klinis bila
enqueue gagal; log warning):

| Event | Destination | resource_type | idempotency_key | payload |
|---|---|---|---|---|
| `create_patient` sukses | Satusehat | `Patient` | `satusehat:Patient:{rm}:created` | `fhir::patient_resource` |
| `add_vital` sukses | Satusehat | `Observation` | `satusehat:Observation:{obs_id}:created` | `fhir::observation_resource` |
| `add_condition` sukses | Satusehat | `Condition` | `satusehat:Condition:{cond_id}:created` | `fhir::condition_resource` |
| `create_prescription` sukses (lolos safety) | Satusehat | `MedicationRequest` | `satusehat:MedicationRequest:{rx}:created` | `fhir::medication_request_resource` |
| `close_encounter` (BARU, §5) | Satusehat | `Encounter` | `satusehat:Encounter:{enc}:closed` | `fhir::encounter_bundle` (Patient+Encounter+Obs+Cond+Rx) |
| `close_encounter` (BARU) | Satusehat | `AllergyIntolerance` per alergi pasien | `satusehat:AllergyIntolerance:{allergy_id}:closed` | `fhir::allergy_intolerance_resource` |
| SEP dibuat via API (§5) | VClaim | `SEP` | `vclaim:SEP:{sep_no}:created` | body SEP JSON |
| Rujukan dibuat | VClaim | `Rujukan` | `vclaim:Rujukan:{ruj_id}:created` | body rujukan |
| Kunjungan PCare dicatat | PCare | `Kunjungan` | `pcare:Kunjungan:{pcv_id}:created` | body kunjungan |
| Klaim dibuat | EKlaim | `Klaim` | `eklaim:Klaim:{clm_id}:created` | body klaim |

Payload FHIR dibangun dari baris DB yang baru saja ditulis (baca
ulang bila perlu). `close_encounter` juga mengubah status encounter
-> Selesai (endpoint baru, bukan perubahan endpoint lama).

### 4.3 RBAC (tambah di `waras-core/src/rbac.rs`)

```rust
ViewBridges,      // lihat status jembatan + outbox (semua peran login)
ManageOutbox,     // retry/cancel outbox (Admin)
CreateSep,        // buat/batalkan SEP (DokterUmum, Perawat, Admin)
CreateReferral,   // buat rujukan (DokterUmum, Admin)
CreateClaim,      // buat klaim (Kasir, Admin)
SyncBridges,      // picu sinkronisasi manual antrean/bed (Admin)
```
Label Indonesia mengikuti gaya yang ada. Admin = semua (seperti
pola yang ada). DokterUmum dapat ViewBridges + CreateSep +
CreateReferral. Perawat dapat ViewBridges + CreateSep. Apoteker
hanya ViewBridges. Kasir dapat ViewBridges + CreateClaim.

## 5. API (agen C) — endpoint baru

Prefix `/api`, bentuk respons/error mengikuti CONTRACT.md §3.
Bila bridge belum dikonfigurasi dan endpoint MEMBUTUHKAN panggilan
live -> 503 `{error:{code:"BRIDGE_NOT_CONFIGURED", message:"..."}}`.
Endpoint yang hanya membaca DB lokal tetap jalan.

| Method & path | Peran | Request | Response |
|---|---|---|---|
| `POST /api/encounters/{id}/close` | Dokter/Admin | — | encounter status Selesai + enqueue §4.2 |
| `GET /api/bridges` | login | — | array 6 bridge: `{destination, name, health, configured, counts:{pending,sent,acked,failed,dead}, last_success_at}` (counts dari outbox; last_success_at dari outbox updated_at terakhir yang Acked) |
| `GET /api/bridges/{destination}` | login | — | satu bridge + 10 pekerjaan terakhir |
| `GET /api/outbox?status=&destination=&limit=` | Admin/Dokter | — | daftar OutboxJob (terbaru dulu) |
| `POST /api/outbox/{id}/retry` | Admin | — | status -> Pending, attempts=0, next_retry_at=NULL |
| `POST /api/outbox/{id}/cancel` | Admin | — | status -> Dead (keputusan operator) |
| `GET /api/seps?date=` | login | default WARAS_TODAY | daftar SEP hari itu |
| `POST /api/seps` | Dokter/Perawat/Admin | `{patient_id, encounter_id?, bpjs_number, jenis, poli}` | 503 bila BPJS belum dikonfigurasi; bila terkonfigurasi: cek eligibilitas via VClaim dulu (NIK tidak ditemukan -> 422), lalu insert SEP via adapter (stub di test), simpan + enqueue, `201` + SEP |
| `GET /api/seps/{id}` | login | — | SEP + invoice terkait (bila ada) |
| `POST /api/seps/{id}/cancel` | Dokter/Admin | — | batal via VClaim (atau 503 bila belum dikonfigurasi) |
| `GET /api/bpjs/peserta?nik=&tanggal=` | login | — | hasil cek eligibilitas VClaim (atau 503) |
| `POST /api/referrals` | Dokter/Admin | `{patient_id, from_poli, to_facility, reason}` | rujukan + enqueue VClaim |
| `GET /api/referrals?date=` | login | — | daftar rujukan hari itu |
| `POST /api/pcare/visits` | Dokter/Perawat/Admin | `{patient_id, poli}` | kunjungan PCare + enqueue |
| `GET /api/pcare/visits?date=` | login | — | daftar kunjungan hari itu |
| `GET /api/antrean-online?date=` | login | — | daftar OnlineBooking hari itu |
| `POST /api/antrean-online/sync` | Admin | — | enqueue semua booking Menunggu hari ini ke Antrean Online (atau 503) |
| `GET /api/beds` | login | — | daftar BedAvailability (cache lokal) |
| `POST /api/beds/refresh` | Admin | — | tarik dari Aplicares via adapter (atau 503), upsert cache |
| `GET /api/claims` | login | — | daftar klaim |
| `POST /api/claims` | Kasir/Admin | `{invoice_id, ina_cbg_code}` | klaim Draft + enqueue E-Klaim (SEP diambil dari invoice bila ada) |
| `GET /api/claims/{id}` | login | — | klaim + status |
| `GET /api/satusehat/resources` | login | — | per resource FHIR: `{resource, sent, failed, pending}` dari outbox + `satusehat_status` |

`AppState` tambah field `bridges: Arc<BridgeRegistry>` dan
`bridge_config: BridgeConfig` (dibangun di `main.rs` dari env;
`build_registry` dari `waras-bridge`). `main.rs`: jalankan
`worker::run_worker` sebagai task Tokio bila enabled (jangan blokir
serve). Worker memakai `Db` + `Arc<BridgeRegistry>` yang SAMA dengan
AppState.

## 6. UI (agen D) — 3 halaman baru + sidebar

- Route UI baru (tambah di `waras-api/src/web.rs` + `lib.rs`,
  mengikuti pola `serve_dashboard`): `/satusehat` ->
  `satusehat.html`, `/bpjs` -> `bpjs.html`, `/antrean-online` ->
  `antrean.html`. JS: `satusehat.js`, `bpjs.js`, `antrean.js`
  (pakai helper `Waras.api` dari `app.js`, guard 401 sama).
- Sidebar: di SEMUA halaman (dashboard, registrasi, rme, farmasi,
  billing + 3 baru), ubah 3 item Fase 2 dari `soon` (href `#`) jadi
  tautan nyata dengan `data-nav` + ikon yang SUDAH dipakai di desain
  (SATUSEHAT `rotate-cw`, BPJS `flag`, Antrean `clock`): SATUSEHAT
  (FHIR) -> `/satusehat`, BPJS · VClaim / PCare -> `/bpjs`, Antrean
  Online & Aplicares -> `/antrean-online`. JANGAN sentuh item
  `API Publik` (Fase 4, tetap `segera`) dan item `segera` lainnya.
- Layar SATUSEHAT (`/satusehat`): judul `SATUSEHAT — FHIR R4`,
  subjudul `Pengiriman resource · Validasi sandbox · Antrian sinkron`;
  pil status 6 jembatan (dari `GET /api/bridges`); 4 KPI dari data
  nyata (resource terkirim = outbox Acked 30 hari, gagal validasi =
  outbox Dead, antrean tertunda = outbox Pending, resource terpetakan
  = jumlah resource FHIR yang diimplementasikan / 57 — tulis
  "Target Kemenkes"); tabel resource FHIR prioritas (dari
  `GET /api/satusehat/resources`); log validasi (dari
  `GET /api/outbox?status=dead&limit=10`, tampilkan `last_error`);
  panel mode offline (teks statis dari desain: antrean lokal +
  kirim otomatis saat online).
- Layar BPJS (`/bpjs`): judul `BPJS Kesehatan — VClaim & PCare`;
  4 KPI dari data nyata (SEP terbit hari ini, rujukan keluar,
  kunjungan PCare, SEP gagal — dari tabel, BUKAN angka desain 96/14/
  182); tabel SEP hari ini (dari `GET /api/seps`, kolom No. SEP,
  Pasien, No. Kartu BPJS, Jenis, Poli, Status); tombol Terbitkan SEP
  -> form (pasien + jenis + poli; panggil `POST /api/seps`; bila
  503 tampilkan pesan "BPJS belum dikonfigurasi"); panel Rujukan &
  PCare (dari `GET /api/referrals` + `GET /api/pcare/visits`).
- Layar Antrean (`/antrean-online`): judul `Antrean Online &
  Aplicares`, subjudul `Mobile JKN · Ketersediaan tempat tidur real
  time`; tabel antrean online (dari `GET /api/antrean-online`,
  kolom Kode booking, Pasien, Poli, Estimasi, Status); tombol
  Sinkronkan (`POST /api/antrean-online/sync`); panel Aplicares
  (dari `GET /api/beds`, per kelas: tersedia/total + pil Tersedia/
  Penuh) + tombol Perbarui (`POST /api/beds/refresh`); panel alur
  antrean online (teks statis 4 langkah dari desain).
- SEMUA angka dari API, bukan hardcode (kecuali label statis +
  teks penjelasan). Gaya mengikuti Fase 1 (token desain CONTRACT
  §6, ikon Lucide PNG yang sudah ada di `waras-web/assets/icons/`).

## 7. Env vars (lengkap — untuk README)

```
SATUSEHAT_ENABLED=false
SATUSEHAT_BASE_URL=https://api-satusehat.kemkes.go.id/fhir-r4/v1
SATUSEHAT_AUTH_URL=https://api-satusehat.kemkes.go.id/oauth2/v1
SATUSEHAT_CLIENT_ID=            # contoh PALSU di README: CONTOH-CLIENT-ID
SATUSEHAT_CLIENT_SECRET=        # contoh PALSU di README: CONTOH-CLIENT-SECRET
SATUSEHAT_ORG_ID=               # contoh PALSU di README: CONTOH-ORG-ID
BPJS_ENABLED=false
BPJS_CONS_ID=                   # contoh PALSU: 12345
BPJS_SECRET_KEY=                # contoh PALSU: CONTOH-SECRET-KEY
BPJS_USER_KEY=                  # contoh PALSU: CONTOH-USER-KEY
BPJS_VCLAIM_URL=https://apijkn.bpjs-kesehatan.go.id/vclaim-rest/
BPJS_PCARE_URL=https://apijkn.bpjs-kesehatan.go.id/pcare-rest-v3.0/
BPJS_ANTREAN_URL=https://apijkn.bpjs-kesehatan.go.id/antrean-rs/
BPJS_APLICARES_URL=https://new-api.bpjs-kesehatan.go.id/aplicaresws/
BPJS_KODE_PPK=               # kode PPK faskes, dipakai Aplicares bed/read
BPJS_EKLAIM_URL=https://apijkn.bpjs-kesehatan.go.id/eklaim/
WARAS_WORKER_ENABLED=true
WARAS_WORKER_INTERVAL_SECS=10
WARAS_WORKER_BATCH=10
WARAS_OUTBOX_MAX_ATTEMPTS=5
```

URL sandbox/dev (untuk pengujian sebelum produksi; pola URL sama,
host berbeda): VClaim `https://apijkn-dev.bpjs-kesehatan.go.id/vclaim-rest-dev/`,
PCare `https://apijkn-dev.bpjs-kesehatan.go.id/pcare-rest-dev-v3.0/`
(belum terverifikasi — lihat §9), Antrean
`https://apijkn-dev.bpjs-kesehatan.go.id/antreanrs_dev/`, Aplicares
`https://dvlp.bpjs-kesehatan.go.id:8888/aplicaresws/`, SATUSEHAT staging
`https://api-satusehat-stg.dto.kemkes.go.id/fhir-r4/v1` (auth
`https://api-satusehat-stg.dto.kemkes.go.id/oauth2/v1`).

## 8. Yang SENGAJA tidak dibangun (Fase 3–4)

Modul Casemix penuh (grouping INA-CBG, workbench pengkodean), migrasi
PostgreSQL, IGD/Rawat Inap/Lab/Radiologi terstruktur, Portal Pasien,
API Publik, Apotek Online BPJS (hanya disebut di desain; adapter
menyusul bila dibutuhkan). JANGAN buat stub endpoint untuk itu —
cukup tulis di README.

## 9. Catatan integrasi (ditulis koordinator saat merge, 19 Nov 2026)

- **Percobaan ulang otomatis diperbaiki.** Kontrak §2 menulis klaim
  hanya mengambil `Pending`, sementara §4.1 menulis job `Failed`
  dijadwalkan ulang lewat `next_retry_at` — keduanya tidak bisa benar
  sekaligus: job Failed tidak pernah diklaim lagi. Perbaikan:
  `claim_outbox_batch_atomic` + `claim_outbox_batch` mengambil
  `status IN ('pending','failed')` yang `next_retry_at`-nya sudah
  lewat. Test `failed_job_is_reclaimed_once_retry_time_is_due`
  membuktikannya (gagal -> Failed, tidak diklaim sebelum waktunya,
  diklaim lagi sesudahnya). Test lama yang menegaskan perilaku macet
  (`failed_job_is_not_reclaimed_until_operator_retry`) diganti.
- **Encounter ditutup mengirim satu resource Encounter, bukan Bundle**
  (§4.2 dikoreksi): adapter SATUSEHAT memvalidasi payload `Encounter`
  butuh `subject` tingkat atas (Bundle tidak punya -> Validation ->
  Dead), dan sync-back `satusehat_id` hanya memetakan resource
  `Encounter`. Kunci idempotensi tetap
  `satusehat:Encounter:{enc}:closed`. Bundle lengkap tetap tersedia di
  `GET /api/encounters/{id}/fhir`.
- **`Invoice.sep_id` akhirnya diserialisasi.** Kolom sudah ada sejak
  migrasi 0002, tapi tipe `Invoice` tidak membawanya, jadi handler
  mencocokkan SEP-tagihan lewat encounter. Sekarang: field ada di
  `waras-core::types::Invoice` + `InvoiceRow`, seed menautkan
  `invoices.sep_id` lewat encounter, `GET /api/seps/{id}` dan
  `POST /api/claims` memakai tautan langsung.
- **SEP bisa terkirim dua kali ke BPJS** (batasan, lihat README):
  nomor SEP lokal dibuat dulu, lalu worker mengirim ulang insert yang
  sama karena adapter tidak punya lookup idempotensi. Perbaiki sebelum
  live (cek "SEP sudah ada?" di adapter VClaim).
- **Tanda tangan `mark_outbox_sent` (agen A):**
  `(id, status, external_id, attempts, next_retry_at, last_error)` —
  bukan `(id, outcome: &SendOutcome, …)` seperti draf kontrak, karena
  `waras-db` tidak boleh bergantung ke `waras-bridge`. Worker memetakan
  `SendOutcome` ke dua parameter itu.
  `invoices.sep_id` lewat encounter, `GET /api/seps/{id}` dan
  `POST /api/claims` memakai tautan langsung.
- **SEP bisa terkirim dua kali ke BPJS** (batasan, lihat README):
  nomor SEP lokal dibuat dulu, lalu worker mengirim ulang insert yang
  sama karena adapter tidak punya lookup idempotensi. Perbaiki sebelum
  live (cek "SEP sudah ada?" di adapter VClaim).
- **Tanda tangan `mark_outbox_sent` (agen A):**
  `(id, status, external_id, attempts, next_retry_at, last_error)` —
  bukan `(id, outcome: &SendOutcome, …)` seperti draf kontrak, karena
  `waras-db` tidak boleh bergantung ke `waras-bridge`. Worker memetakan
  `SendOutcome` ke dua parameter itu.
- **`GET /api/bridges` mengeja tujuan dua cara:** serialisasi serde
  snake_case (`v_claim`, `p_care`, `e_klaim`) vs bentuk DB (`vclaim`,
  `pcare`, `eklaim`). `dest_from_str` menerima keduanya; `list_outbox`
  menormalisasi query ke bentuk DB. Ada test alias untuk ini.

## 10. Verifikasi terhadap dokumen/sandbox resmi (20 Nov 2026)

Verifikasi dilakukan tanpa kredensial live (hanya kredensial PALSU
untuk probe). Portal developer resmi SATUSEHAT
(`developer.satusehat.kemkes.go.id`) dan BPJS
(`dvlp.bpjs-kesehatan.go.id`) ada di balik login/registrasi, jadi
rujukan di bawah memakai dokumen publik + SDK/mirror yang sudah
diuji terhadap sandbox resmi (disebut per baris). Semua perbaikan
di bawah masuk commit verifikasi di branch `fase-2-jembatan`.

### 10.1 Tabel konformansi

| Area | Hasil | Perbaikan |
|---|---|---|
| SATUSEHAT staging auth `…/oauth2/v1/accesstoken`, grant `client_credentials`, Basic auth | COCOK | — |
| SATUSEHAT FHIR base `…/fhir-r4/v1`, POST `/{ResourceType}`, Bearer, token cache + refresh sekali setelah 401 | COCOK | — |
| Patient: identifier NIK `https://fhir.kemkes.go.id/id/nik` | COCOK | — |
| Encounter `class`: tanpa `system` | MISMATCH → DIPERBAIKI | `class.system` = `http://terminology.hl7.org/CodeSystem/v3-ActCode` |
| Encounter `identifier` `http://sys-ids.kemkes.go.id/encounter/{org_id}` | MISMATCH → DIPERBAIKI | `encounter_resource_with_org()`; `org_id` dari `SATUSEHAT_ORG_ID`, dipakai saat encounter ditutup |
| Encounter `participant` (ATND Practitioner) + `serviceProvider` (Organization) | BELUM BISA — tidak ada Practitioner/Organization terdaftar di SATUSEHAT; jangan fabrikasi ID | — |
| Observation: tanpa LOINC, tanpa `category`, `valueString` saja | MISMATCH → DIPERBAIKI | `category` vital-signs, kode LOINC per `VitalCode::loinc_code()` (BP panel 85354-9 + komponen 8480-6/8462-4, nadi 8867-4, suhu 8310-5, SpO2 59408-5, gula darah 2339-0), `valueQuantity` UCUM, fallback `valueString` bila nilai tak terparse |
| Condition `clinicalStatus`: teks "Aktif" | MISMATCH → DIPERBAIKI | coding `condition-clinical` (`active`/`resolved`; Kronis → `active`) + `verificationStatus` confirmed + `category` encounter-diagnosis |
| MedicationRequest: tanpa `intent` (FHIR R4 wajib) | MISMATCH → DIPERBAIKI | `"intent": "order"` |
| `effectiveDateTime` / `recordedDate`: `NaiveDateTime::to_string()` (spasi, bukan dateTime FHIR) | MISMATCH → DIPERBAIKI | `fhir::fhir_datetime()` → `%Y-%m-%dT%H:%M:%S` |
| MedicationRequest `medicationCodeableConcept`: teks saja, tanpa kode KFA | GAP — butuh data KFA | — |
| Patient `meta.profile` | BELUM TERKONFIRMASI wajib/tidak | — |
| Bundle `encounter_bundle`: type `collection` di-POST ke root | GAP laten — bukan interaksi batch/transaction FHIR; worker tidak pernah mengirim Bundle (hanya Encounter tunggal) | — |
| BPJS signature: HMAC-SHA256(`cons_id&timestamp`), base64; header `X-cons-id`/`X-timestamp`/`X-signature`/`user_key`; timestamp detik Unix UTC | COCOK | — |
| BPJS dekripsi: AES-256-CBC, kunci SHA-256(`cons_id+secret+timestamp`), IV 16 byte pertama kunci | COCOK (derivasi) | — |
| BPJS dekripsi: plaintext masih LZString | MISMATCH → DIPERBAIKI | `waras-bridge/src/lzstring.rs` (port `lz-string` 1.5.0, `decompressFromEncodedURIComponent`); `decrypt_response` coba JSON mentah dulu, lalu LZString; vektor uji dari paket `lz-string` asli |
| VClaim SEP insert: body datar, tanpa envelope | MISMATCH → DIPERBAIKI | adapter membungkus `{"request": {"t_sep": <payload>}}`; sama untuk SEP Batal (`t_sep`) dan Rujukan (`t_rujukan`); payload internal tetap datar |
| VClaim SEP insert path `SEP/2.0/insert` | COCOK | — |
| VClaim SEP: banyak field wajib 2.0 belum dikirim (ppkPelayanan, klsRawat, rujukan, diagAwal, poli sebagai objek, tujuanKunj, dpjpLayan, user, noTelp) | GAP — butuh config faskes + field form | — |
| VClaim SEP delete path `SEP/delete` | BELUM TERKONFIRMASI | — |
| VClaim Rujukan insert path `Rujukan/insert` | BELUM TERKONFIRMASI | — |
| PCare: body datar JSON (tanpa envelope `request`) | COCOK (pola umum) | — |
| PCare `kunjungan`: path + nama field | BELUM TERKONFIRMASI (dokumen tidak didapat) | — |
| Antrean `antrean/add` + `antrean/batal` | BELUM TERKONFIRMASI (dokumen resmi minta ~25 field; WARAS kirim 5) | — |
| Aplicares base `https://apijkn.bpjs-kesehatan.go.id/aplicaresws/` + `GET bed/get` | MISMATCH → DIPERBAIKI | base default → `https://new-api.bpjs-kesehatan.go.id/aplicaresws/` (dev `https://dvlp.bpjs-kesehatan.go.id:8888/`); path → `GET rest/bed/read/{kodeppk}/1/100`; env baru `BPJS_KODE_PPK`; `refresh_beds` terima `kodekelas` (huruf kecil) + `kapasitas` sebagai total; kode kelas dipetakan ke nama UI ("1" → "Kelas 1") |
| E-Klaim `POST klaim/submit` ke `https://apijkn.bpjs-kesehatan.go.id/eklaim/` | MISMATCH — endpoint BPJS-hosted itu TIDAK ADA | TIDAK DIPERBAIKI (butuh redesign, bukan tambal URL): E-Klaim asli adalah aplikasi lokal `ws.php` (kunci enkripsi simetris sendiri) atau alur FHIR Claim via SATUSEHAT. Adapter E-Klaim **belum siap live**; jangan aktifkan. |
| BPJS `health()` = Offline saat dikonfigurasi | MASIH — tidak ada endpoint probe tanpa efek samping yang dijamin ada di kelima layanan; status live dinilai dari hasil outbox | — |

### 10.2 Live probe (kredensial PALSU, tanpa token/kunci asli)

- SATUSEHAT staging FHIR `GET https://api-satusehat-stg.dto.kemkes.go.id/fhir-r4/v1/Patient`
  (tanpa auth) → **401** `OperationOutcome` (`code: invalid-access-token`,
  `details: "Invalid Access Token."`). Host staging FHIR **live**, bentuk
  error 401 sesuai yang diharapkan adapter.
- SATUSEHAT staging token `POST …/oauth2/v1/accesstoken` (Basic
  `FAKE-CLIENT-ID:FAKE-CLIENT-SECRET`, `grant_type=client_credentials`)
  → **429** `OperationOutcome` (`code: throttled`, "Authentication
  temporarily cannot be performed due to the rate limit policy. Please
  retry later."). Endpoint **live** (kebijakan rate limit 1 req/menit
  per client terdokumentasi; throttle terjadi sebelum validasi
  kredensial, jadi `invalid_client` belum teramati — butuh client id
  asli atau IP yang belum ter-throttle). Probe pertama sempat 000
  (timeout koneksi), ulangan konsisten 429.
- BPJS (`apijkn.bpjs-kesehatan.go.id`, `apijkn-dev.bpjs-kesehatan.go.id`,
  `new-api.bpjs-kesehatan.go.id`): DNS resolve, TLS handshake sukses,
  tapi **tidak ada respons HTTP dalam 20–30 detik** (curl 000/timeout)
  dari jaringan sandbox ini, termasuk `GET /` polos. Bukan putusan
  tentang URL-nya — probe BPJS **terblokir jaringan sandbox**, jadi
  konformansi BPJS di atas bersandar pada dokumen, bukan probe live.
  Ulangi probe BPJS dari VPS / jaringan lain sebelum live.

### 10.3 Dokumen yang dipakai

- SATUSEHAT: DeepWiki @ssecd/ihs (`deepwiki.com/ssecd/ihs/6.1-satusehat-platform`,
  `…/9-api-reference`), SDK @harizinside/satusehat (npm/jsr), OpenFn SATUSEHAT
  adaptor docs (`docs.openfn.org/adaptors/satusehat`), koleksi Postman SATUSEHAT
  (contoh Encounter + Observation vital-signs), playbook Klaim BPJS SATUSEHAT
  (`satusehat.kemkes.go.id/platform/docs/id/interoperability/klaim-bpjs/`).
- BPJS: ssecd/jkn DeepWiki (`deepwiki.com/ssecd/jkn/3.1-authentication-and-security`,
  `…/4-vclaim-service`), `dedenbangkit/bpjs-vclaim` (`bpjs.py`),
  `morizbebenk/flask-bpjs` (README, envelope `t_sep`), panduan VClaim
  nugrohosetiawan.com + matthewswong.com, gist bridging Aplicares RSUD Al Ihsan
  (`gist.github.com/denihida1216/4049a242ccd9c9ceee23f162ef0ee4a4`),
  Manual Web Service E-Klaim 5.10.x (`inacbg.kemkes.go.id`), materi bridging
  VClaim 2.0 (slideshare), `lz-string` 1.5.0 (npm, vektor uji).
- FHIR R4: `build.fhir.org/observation-vitalsigns.html` (profil vital signs:
  `category` vital-signs + LOINC + `valueQuantity` UCUM), contoh Observation
  SATUSEHAT di Postman (nadi 8867-4), deck bridging Satu Sehat (LOINC TTV:
  sistolik 8480-6, diastolik 8462-4, nadi 8867-4, suhu 8310-5).

### 10.4 Yang masih butuh kredensial Arya (uji sandbox live)

- SATUSEHAT: `client id` + `client secret` + `org id` (dari portal
  SATUSEHAT / Kemenkes). Dengan itu: token staging asli → POST
  Patient/Encounter/Observation sungguhan ke staging → validasi profil
  SATUSEHAT → `participant`/`serviceProvider` + `meta.profile` +
  kode KFA bisa dikonfirmasi.
- BPJS: `cons id` + `secret key` + `user key` (dari bridging BPJS).
  Dengan itu: VClaim dev (eligibilitas, SEP insert/batal, rujukan),
  PCare dev (kunjungan + field), Antrean dev (add/batal + field
  lengkap), Aplicares (`bed/read` + `ref/kelas`), dan dekripsi
  LZString terhadap respons enkripsi asli.
- **SEP double-submit tetap terbuka** (lihat §9 + README): perbaikan
  terpisah, masih wajib sebelum live.
