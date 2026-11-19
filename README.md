# WARAS — Sistem Informasi Rumah Sakit & Klinik (open source)

WARAS (*Web-based Architecture for Records and Administration of
Healthcare Services*, bahasa Jawa: sehat/sembuh) adalah SIMRS open source
yang modern dan ringan: **modular monolith, satu binary, full Rust**,
model data mengikuti resource **HL7 FHIR R4** sejak skema pertama.

Repo ini adalah implementasi **Fase 1 — Inti Klinis** dari paper desain
(`~/workspace/your_files/waras-paper-simrs-modern/waras-paper-simrs-modern.pdf`,
v4) dan desain UI FigCraft v16 (`~/workspace/waras/figcraft/`, file
`waras-full-v16-lucide.figcraft`).

## Demo live

https://waras.meowlabs.id — deploy 19 November 2026, commit `6ee6835`,
`waras.service` di VPS Meow Labs (port internal 3200, nginx +
Let's Encrypt, DNS A record via Cloudflare). Login memakai akun seed
di bawah.

## Menjalankan (satu perintah)

```sh
cargo run --release
# atau, setelah build:
./target/release/waras
```

Lalu buka http://127.0.0.1:3000 — halaman Login akan muncul.

Konfigurasi (environment, semua opsional):

| Variabel | Default | Arti |
|---|---|---|
| `WARAS_HOST` | `127.0.0.1` | Alamat dengar |
| `WARAS_PORT` | `3000` | Port |
| `WARAS_DB` | `waras.db` | Berkas SQLite (dibuat otomatis) |
| `WARAS_TODAY` | `2026-11-19` | Tanggal klinik ("hari ini") |
| `RUST_LOG` | `waras=info` | Level log |

Saat boot pertama: migrasi dijalankan, lalu data seed Fase 1 dimuat
(idempotent — bila basis data sudah berisi pengguna, seed dilewati).
Satu-satunya berkas yang ditulis server adalah `waras.db`
(+ `-shm`/`-wal` saat berjalan).

### Login seed (data demo)

| Pengguna | NIP / username | Kata sandi | Peran |
|---|---|---|---|
| dr. Ahmad Hidayat | `198505122010011001` / `ahmad` | `waras123` | Dokter Umum |
| Ns. Sari Wijaya | `sari` | `waras123` | Perawat |
| dr. Rina, Sp.PD | `rina` | `waras123` | Dokter Umum |
| apt. Dina Puspita | `dina` | `waras123` | Apoteker |
| Sinta Maharani | `sinta` | `waras123` | Kasir |
| Admin Sistem | `admin` | `waras123` | Admin |

Ganti kata sandi ini sebelum dipakai di luar demo. Login memakai NIP
atau username.

## Yang sudah dibangun (Fase 1)

- **Satu Cargo workspace, satu binary** (`waras`): Tokio + Axum + SQLx +
  Serde, rustls untuk TLS bila dibutuhkan, aset web tertanam di binary
  (rust-embed). Tiap modul adalah crate di dalam workspace.
- **Mode Klinik (SQLite tertanam)**. Skema dirancang mengikuti FHIR R4
  supaya mode PostgreSQL (rumah sakit) bisa menyusul tanpa tulis ulang.
- **Domain FHIR R4**: Patient, Encounter, Observation (tanda vital),
  Condition, MedicationRequest, AllergyIntolerance, Appointment/Queue,
  User + RBAC, audit trail append-only dengan rantai hash SHA-256.
- **Modul**: Dashboard (KPI dihitung dari data nyata), Registrasi &
  Antrean (daftar pasien, ambil nomor, panggil berikutnya), RME Clinical
  Workspace (SOAP 4 seksi dengan draft autosave + riwayat versi, tabel
  tanda vital, diagnosis per kelompok encounter, timeline, panel relasi
  FHIR), Rawat Jalan (jadwal & janji), Farmasi (e-resep, dispensing, stok
  KFA), Billing & Kasir (tagihan + pembayaran, kolom kode INA-CBG).
- **Aturan keamanan obat (diuji)**: resep Amoxicillin — atau obat
  penisilin apa pun — untuk pasien dengan alergi penisilin **DIBLOKIR**
  otomatis (API mengembalikan 422 + alert merah di UI). Kasus kanonik:
  Siti Rahmawati (Bronkitis akut J20.9 + DM tipe 2, ALERGI PENISILIN):
  Amoxicillin 500 mg diblokir, diganti Azithromycin 500 mg 1x1 3 hari.
- **Data seed** dari cerita desain: dr. Ahmad Hidayat, Siti Rahmawati,
  Budi Santoso, Dewi Lestari, Agus Prasetyo, Rina Marlina, Joko Susilo,
  Ahmad Fauzi, Putri Ayu, Bambang Riyanto, Sri Wahyuni; antrean, janji,
  resep, stok, tagihan, statistik 7 hari.
- **UI**: 6 halaman (Login, Dashboard, Registrasi, RME, Farmasi,
  Billing) mengikuti layar FigCraft v16 — sidebar gelap, topbar dengan
  search ⌘K, status pill, aksen teal `#0E7490`, ikon Lucide, bahasa
  Indonesia. Semua data halaman diambil dari API, bukan hardcode.

### Struktur crate

```text
crates/
  waras-core/   tipe domain, aturan keamanan obat, RBAC, error, helper FHIR
  waras-db/     SQLx SQLite, migrasi, repository, seed, audit trail
  waras-api/    Axum: router, auth token sesi, guard RBAC, handler, UI statis
  waras-web/    aset statis (HTML/CSS/JS + ikon Lucide + logo) via rust-embed
  waras/        binary: config, migrasi, seed, serve
docs/CONTRACT.md  kontrak antar-crate + spesifikasi seed (sumber kebenaran)
```

### Tes

```sh
cargo test --offline
```

117 tes: `waras-core` 16 (termasuk blokir penisilin + azithromycin
lolos, FHIR AllergyIntolerance), `waras-db` 33 (seed, KPI, rantai
audit, versi SOAP, dispensing, outbox idempotensi + klaim + retry
otomatis), `waras-api` 45 (login, RBAC, alur penuh pasien→antrean→
encounter→SOAP, resep diblokir 422 / diizinkan, bayar tagihan, bundle
FHIR, worker + endpoint jembatan dengan stub), `waras-bridge` 20
(OAuth2 + token cache, tanda tangan BPJS, envelope, dekripsi AES),
`waras-web` 3 (aset tertanam).

## Jembatan nasional (Fase 2) — SELESAI, terverifikasi dengan stub

Fase 2 membangun pengiriman asinkron ke SATUSEHAT + BPJS lewat outbox
dan worker latar (paper Bagian 4.2: tidak ada pengiriman sinkron di
jalur transaksi klinis). Status per jembatan terlihat di UI
(`/satusehat`, `/bpjs`, `/antrean-online`) dan semua angka dihitung
dari tabel nyata.

**Yang dibangun:** tabel `outbox` (idempotency key UNIQUE, status,
backoff 2^attempts menit max 60, max 5 percobaan) + worker Tokio yang
mengurasnya; adapter SATUSEHAT (OAuth2 client credentials, token
di-cache, refresh sekali setelah 401, POST FHIR per resource, validasi
profil sebelum kirim) dan BPJS (VClaim, PCare, Antrean Online,
Aplicares, E-Klaim; header tanda tangan HMAC-SHA256, envelope
`metaData`, dekripsi respons AES-256-CBC bila `response` terenkripsi);
endpoint jembatan (`/api/bridges`, `/api/outbox`, `/api/seps`,
`/api/bpjs/peserta`, `/api/referrals`, `/api/pcare/visits`,
`/api/antrean-online`, `/api/beds`, `/api/claims`,
`/api/satusehat/resources`, `POST /api/encounters/{id}/close`);
enqueue otomatis (pasien baru, vital, diagnosis, resep, encounter
ditutup, SEP, rujukan, kunjungan PCare, klaim); kolom `satusehat_id` /
`satusehat_status` per baris; seed Fase 2 (8 SEP, 7 booking JKN,
5 kelas bed, 2 rujukan, 3 kunjungan PCare, 1 klaim draft).

**Tidak ada koneksi live.** Tanpa kredensial, semua jembatan berstatus
`Belum dikonfigurasi`, pekerjaan outbox tetap Pending (tanpa
menghabiskan percobaan), dan endpoint yang butuh panggilan live
menjawab 503 `BRIDGE_NOT_CONFIGURED`. Semua pengujian memakai stub
server lokal (axum, port ephemeral); tidak ada panggilan live.

**Untuk koneksi live, Arya harus menyediakan:** kredensial SATUSEHAT
(client id + client secret + org id, dari portal SATUSEHAT) dan
kredensial BPJS (cons id + secret key + user key, dari bridging BPJS),
lalu mengisi env di bawah dan me-restart. Verifikasi dulu terhadap
sandbox resmi SATUSEHAT dan BPJS sebelum mengaktifkan di produksi.

```sh
SATUSEHAT_ENABLED=true
SATUSEHAT_BASE_URL=https://api-satusehat.kemkes.go.id/fhir-r4/v1
SATUSEHAT_AUTH_URL=https://api-satusehat.kemkes.go.id/oauth2/v1
SATUSEHAT_CLIENT_ID=CONTOH-CLIENT-ID        # PALSU, ganti dengan yang asli
SATUSEHAT_CLIENT_SECRET=CONTOH-CLIENT-SECRET # PALSU
SATUSEHAT_ORG_ID=CONTOH-ORG-ID              # PALSU
BPJS_ENABLED=true
BPJS_CONS_ID=12345                           # PALSU
BPJS_SECRET_KEY=CONTOH-SECRET-KEY            # PALSU
BPJS_USER_KEY=CONTOH-USER-KEY                # PALSU
BPJS_VCLAIM_URL=https://apijkn.bpjs-kesehatan.go.id/vclaim-rest/
BPJS_PCARE_URL=https://apijkn.bpjs-kesehatan.go.id/pcare-rest-v3.0/
BPJS_ANTREAN_URL=https://apijkn.bpjs-kesehatan.go.id/antrean-rs/
BPJS_APLICARES_URL=https://new-api.bpjs-kesehatan.go.id/aplicaresws/
BPJS_KODE_PPK=1234567                          # PALSU, kode PPK faskes
BPJS_EKLAIM_URL=https://apijkn.bpjs-kesehatan.go.id/eklaim/
WARAS_WORKER_ENABLED=true
WARAS_WORKER_INTERVAL_SECS=10
WARAS_WORKER_BATCH=10
WARAS_OUTBOX_MAX_ATTEMPTS=5
```

**Terverifikasi terhadap dokumen/sandbox resmi (20 Nov 2026, lihat
`docs/FASE2.md` §10):** tanda tangan HMAC-SHA256 + header BPJS COCOK;
dekripsi AES-256-CBC COCOK (derivasi kunci/IV), plus langkah LZString
yang sebelumnya hilang — plaintext respons BPJS asli masih terkompresi
`compressToEncodedURIComponent`, kini didekompresi
(`crates/waras-bridge/src/lzstring.rs`, vektor uji dari paket
`lz-string` 1.5.0 asli); VClaim `SEP/2.0/insert` COCOK + envelope
`{"request": {"t_sep": …}}` (juga SEP Batal + Rujukan `t_rujukan`);
Aplicares DIPERBAIKI ke host `new-api.bpjs-kesehatan.go.id` +
`GET rest/bed/read/{kodeppk}/1/100` (sebelumnya host + path salah);
builder FHIR DIPERBAIKI ke profil SATUSEHAT (Encounter `class.system` +
`identifier` org, Observation LOINC + `category` vital-signs +
`valueQuantity` UCUM, Condition coding `active`/`resolved`,
MedicationRequest `intent: order`, dateTime format `T`). Probe live
(tanpa kredensial asli): SATUSEHAT staging FHIR 401 `invalid-access-token`
dan token endpoint 429 throttled (keduanya membuktikan host + bentuk
error); probe BPJS terblokir jaringan sandbox (TLS nyambung, HTTP
timeout 20–30 dtk) — ulangi dari VPS sebelum live.

**Belum terverifikasi / masih gap:** path `SEP/delete`,
`Rujukan/insert`, seluruh PCare `kunjungan` (path + field), Antrean
(field lengkap), `participant`/`serviceProvider` Encounter, kode KFA,
`meta.profile` Patient, tipe Bundle. **E-Klaim: endpoint
`klaim/submit` di host BPJS TIDAK ADA** — E-Klaim asli memakai `ws.php`
lokal atau alur FHIR Claim via SATUSEHAT; adapter E-Klaim belum siap
live, jangan aktifkan. Field wajib SEP 2.0 yang belum dikirim
(ppkPelayanan, klsRawat, diagAwal, dpjpLayan, dsb.) butuh config faskes
+ field form (tugas lanjutan).

**Dekripsi diimplementasikan** (`aes 0.9` + `cbc 0.2`, keduanya resolve
offline dari cache; kontrak menyebut `aes 0.8`, yang tidak ada di
cache). Round-trip enkripsi/dekripsi diuji, termasuk end-to-end lewat
stub yang mengenkripsi respons dengan timestamp dari header request.

**Batasan yang diketahui (jujur):**
- **SEP bisa terkirim dua kali ke BPJS.** Nomor SEP lokal dibuat
  dulu (`SEP-20261119-009`), lalu worker mengirim ulang insert SEP
  yang sama (adapter tidak punya lookup idempotensi ke BPJS). Terhadap
  stub teramati dua POST insert identik. Sebelum live, tambahkan cek
  "SEP sudah ada?" di adapter VClaim.
- **Encounter ditutup mengirim satu resource Encounter, bukan Bundle.**
  Adapter SATUSEHAT menolak Bundle (tidak ada `subject` tingkat atas)
  dan sync-back `satusehat_id` hanya memetakan resource `Encounter`;
  bundle lengkap tetap tersedia di `GET /api/encounters/{id}/fhir`.
- **Idempotensi dijamin outbox, bukan adapter:** adapter mengirim ulang
  job yang sama sebagai HTTP baru; outbox yang mencegah duplikat lewat
  `idempotency_key`.
- **KPI jembatan dihitung dari data nyata** (bukan angka desain 12.480 /
  96 / 182): SEP hari ini = 8 baris seed, kunjungan PCare = 3, resource
  terkirim = jumlah outbox Acked. Kecil, tapi jujur.
- **E-Klaim di Fase 2 hanya adapter + alur submit/status klaim — dan
  adapter itu BELUM SIAP LIVE** (endpoint `klaim/submit` di host BPJS
  tidak ada; integrasi asli memakai `ws.php` lokal atau alur FHIR
  Claim via SATUSEHAT — redesign, lihat `docs/FASE2.md` §10).
  Workbench Casemix penuh (grouping INA-CBG) tetap Fase 3 (paper
  Bagian 6). Kode INA-CBG di seed (`J-4-17-I`) adalah contoh, bukan
  hasil grouping sungguhan.
- **BPJS `health()` = Offline saat dikonfigurasi.** Tidak ada endpoint
  probe tanpa efek samping yang dijamin ada di kelima layanan BPJS;
  status live dinilai dari hasil kirim outbox, bukan dari pill ini.
  (SATUSEHAT `health()` asli — fetch token.)
- **Kode kelas bed Aplicares dipetakan ke nama UI** ("1" → "Kelas 1")
  supaya refresh tidak membuat baris ganda di samping seed.

## Yang SENGAJA belum dibangun (Fase 3–4)

IGD, Rawat Inap, Kamar Operasi & Bersalin, Laboratorium, Radiologi,
Casemix penuh (grouping INA-CBG), Inventori, Kepegawaian, Laporan &
SIRS, Hemodialisa, Transfusi Darah, Gizi, Rekam Medis Gigi, Portal
Pasien, API Publik. Jembatan nasional (SATUSEHAT + BPJS) sudah dibangun
di Fase 2 (lihat bagian di atas); mode PostgreSQL adalah Fase 3. Lihat
paper Bagian 3.4 (kriteria keluar fase) dan Bagian 10 (peta jalan).

## Penyimpangan dari desain (dicatat terbuka)

1. **Antrean 44, bukan 42.** Desain menulis total donut 42 dan "nomor
   berikutnya A-045", tapi baris antrean yang terlihat sudah sampai
   A-044 — keduanya tidak bisa benar sekaligus. Seed memakai 44 entri
   supaya baris A-041..A-044 persis seperti desain; KPI dihitung dari
   data nyata sehingga total 44 dan antrean aktif 32 (bukan 42/30).
2. **Footer "Mode Klinik · SQLite"**, bukan "Mode Rumah Sakit ·
   PostgreSQL" seperti di desain — itu mode Fase 3; yang berjalan
   sekarang SQLite.
3. **Agregat hari ini dari `daily_stats`**: pendaftaran hari ini (248),
   kunjungan 7 hari (rata-rata 178, puncak 210), pendapatan (Rp 18,4 jt,
   142 transaksi), lab menunggu (26, 2 kritis), IGD merah (3), tempat
   tidur (86/120), piutang BPJS (Rp 212 jt) dan deposit rawat inap
   (Rp 46 jt) adalah agregat seed — Lab/IGD/Rawat Inap belum modul
   Fase 1, jadi belum ada tabel domain untuk menghitungnya. Antrean,
   janji, resep, tagihan, dan KPI turunannya dihitung langsung dari
   tabel domain.
4. **Alamat dan tanggal lahir** pasien selain Siti Rahmawati adalah
   data sintetis seed (desain hanya memberi data lengkap untuk Siti).

## Keamanan

- Kata sandi di-hash Argon2; token sesi 32 byte acak, kedaluwarsa 12 jam.
- RBAC per aksi (`waras-core::rbac`): hanya Dokter yang boleh meresepkan,
  hanya Apoteker yang boleh dispensing, hanya Kasir yang boleh menerima
  pembayaran; audit trail hanya dibaca Admin dan Dokter.
- Audit trail append-only dengan rantai hash (entri pertama `GENESIS`).
- Retensi dan enkripsi berkas (UU PDP 27/2022, 25 tahun) adalah pekerjaan
  Fase 1.1 — belum ada di build ini.

## Lisensi

Apache-2.0.
