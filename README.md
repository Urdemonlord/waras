# WARAS — Sistem Informasi Rumah Sakit & Klinik (open source)

WARAS (*Web-based Architecture for Records and Administration of
Healthcare Services*, bahasa Jawa: sehat/sembuh) adalah SIMRS open source
yang modern dan ringan: **modular monolith, satu binary, full Rust**,
model data mengikuti resource **HL7 FHIR R4** sejak skema pertama.

Repo ini adalah implementasi **Fase 1 — Inti Klinis, Fase 2 —
Jembatan Nasional, Fase 3 — Rumah Sakit, dan Fase 4 — Ekosistem**
dari paper desain
(`~/workspace/your_files/waras-paper-simrs-modern/waras-paper-simrs-modern.pdf`,
v4) dan desain UI FigCraft v16 (`~/workspace/waras/figcraft/`, file
`waras-full-v16-lucide.figcraft`). Semua fase sudah merge ke `main`
(Fase 4: commit `c23844c`); lihat `docs/FASE2.md`, `docs/FASE3.md`,
`docs/FASE4.md`. **Versi: v0.1.0** (lihat `CHANGELOG.md`).

## Demo live

https://waras.meowlabs.id — deploy 20 November 2026, `main` @ `c23844c`,
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
| `WARAS_DATABASE_URL` | _(kosong)_ | Bila diisi `postgres://...` → mode PostgreSQL (rumah sakit); bila kosong → SQLite |
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

Login portal pasien (`/portal`): NIK pasien seed + kata sandi portal
`waras123` (sama untuk semua pasien seed, hash Argon2). NIK + tanggal
lahir saja tidak bisa masuk. Pasien baru dari registrasi belum punya
kata sandi portal (lihat `docs/FASE-A-A4.md` — belum ada endpoint
set/ganti kata sandi portal).

## Operasional (backup & restore)

Mode SQLite (default) dicadangkan harian secara otomatis — runbook
lengkapnya di [`docs/BACKUP.md`](docs/BACKUP.md):

```sh
# Backup sekali jalan (aman walau server berjalan):
WARAS_DB=/opt/waras/waras.db WARAS_BACKUP_DIR=/var/backups/waras \
    scripts/waras-backup.sh
# Restore satu perintah (hentikan dulu service WARAS):
sudo systemctl stop waras
WARAS_DB=/opt/waras/waras.db \
    scripts/waras-restore.sh /var/backups/waras/waras-20261121-023000.db
sudo systemctl start waras
```

Penjadwalan utama: systemd timer `scripts/waras-backup.timer`
(02:30 tiap hari, retensi 7 backup terakhir); alternatif cron ada di
runbook. Latihan restore sudah dilakukan 21 Nov 2026 (lihat
`docs/BACKUP.md` § Latihan restore) — ulangi tiap bulan.

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
# WARAS_HTTP_PROXY=http://10.200.0.2:18080  # OPSIONAL, kosong = langsung.
# Isi bila IP server diblokir edge SATUSEHAT/BPJS (VPS 129.226.193.34
# diblokir 403 bahkan tanpa auth; lewat proxy Proton VPS tembus).
# Berlaku untuk SEMUA adapter (SATUSEHAT + BPJS); tanpa ini reqwest
# memakai proxy env sistem (HTTPS_PROXY/HTTP_PROXY) atau langsung.
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
MedicationRequest `intent: order`, dateTime format `T`). **Bentuk auth
SATUSEHAT DIPERBAIKI setelah terbukti salah secara live:** adapter lama
mengirim HTTP Basic auth + form `grant_type`, staging menolaknya 401
"client_id or client_secret combination is unacceptable"; bentuk resmi
adalah `POST …/accesstoken?grant_type=client_credentials` dengan
`client_id` + `client_secret` di body form (tanpa Basic). **Verifikasi
live dengan kredensial sandbox asli (20 Nov 2026, via proxy VPS):**
token 200 (`expires_in` 14399), `GET /metadata` 200 (FHIR 4.0.1),
`GET Patient` NIK dummy 200 (total 1, pasien uji staging) — semuanya lewat adapter yang
sudah diperbaiki. Egress: IP VPS langsung (129.226.193.34) diblokir 403
oleh edge SATUSEHAT (staging + production, bahkan tanpa auth), jadi
deploy di VPS WAJIB mengisi `WARAS_HTTP_PROXY` (proxy Proton VPS);
probe BPJS dari VPS masih harus diulang sebelum live.

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
- **SEP double-submit — DIPERBAIKI (20 Nov 2026).** Dulu nomor SEP lokal
  dibuat dulu lalu worker mengirim ulang insert yang sama (teramati dua
  POST insert identik ke stub). Sekarang: handler single-send (pre-check
  lokal + insert live sekali + nomor BPJS ditulis ke `seps.bpjs_sep_number`
  + job outbox langsung Acked), guard adapter `GET SEP/{noSep}` sebelum
  insert, dan kunci idempotensi outbox. Nomor otoritatif yang dikenal
  BPJS adalah `seps.bpjs_sep_number` (= `outbox.external_id`); nomor
  lokal hanya kunci internal. Catatan: endpoint lookup `GET SEP/{noSep}`
  dikonfirmasi via DeepWiki SDK `ssecd/jkn` (sumber sekunder, SDK sudah
  diuji terhadap API VClaim asli), belum lewat portal resmi BPJS —
  konfirmasi saat uji sandbox live dengan kredensial asli.
- **Encounter ditutup mengirim satu resource Encounter, bukan Bundle.**
  Adapter SATUSEHAT menolak Bundle (tidak ada `subject` tingkat atas)
  dan sync-back `satusehat_id` hanya memetakan resource `Encounter`;
  bundle lengkap tetap tersedia di `GET /api/encounters/{id}/fhir`.
- **Idempotensi dijamin outbox, bukan adapter — kecuali SEP:** adapter
  mengirim ulang job yang sama sebagai HTTP baru; outbox yang mencegah
  duplikat lewat `idempotency_key`. Pengecualian: SEP VClaim punya guard
  adapter sendiri (`GET SEP/{noSep}` sebelum insert) di samping
  single-send handler + kunci idempotensi, lihat butir di atas.
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

## Modul rumah sakit (Fase 3) — MERGED + DEPLOYED LIVE (main `eb588d2`)

Fase 3 membangun modul rumah sakit dari layar FigCraft v16 (lihat
`docs/FASE3.md` untuk kontrak lengkap + penyimpangan yang
didokumentasikan):

- **IGD**: triase Merah/Kuning/Hijau, status Resusitasi/Observasi/
  Tindakan/Menunggu, encounter fast-track, catatan observasi.
- **Rawat Inap**: 5 ward (Melati 32, Mawar 28, Anggrek 24, Cempaka 24,
  ICU 12 = 120 bed, 86 terisi di seed), peta bed, admisi/mutasi/pulang,
  catatan perawatan, badge Risiko Jatuh.
- **Kamar Operasi & Bersalin**: jadwal operasi (OK 1–3, kode ICD-9-CM),
  kamar bersalin VK 1–4, catatan persalinan.
- **Laboratorium**: order + hasil terstruktur LOINC (Darah lengkap
  58410-2, Troponin I 10839-9, HbA1c 4548-4, Urinalisis 24357-6,
  Golongan darah 883-9, Elektrolit 2951-2, Gula darah puasa 1558-6),
  flag Normal/Tinggi/Rendah/Kritis dihitung server; nilai Kritis
  membuat notifikasi KRITIS + entri audit.
- **Radiologi**: order (X-Ray/USG/CT Scan/MRI) + laporan hasil
  (teks; tidak ada PACS/DICOM — penampil citra menampilkan
  modalitas/area sebagai teks, bukan citra diagnostik).
- **Casemix / INA-CBG workbench**: grouping LOKAL dari diagnosis
  utama encounter ke kode + tarif contoh layar desain (mis. I20.9 →
  I-4-13-I Rp 4.210.000). **Grouping lokal = perkiraan, bukan grouper
  resmi INA-CBG; tarif adalah contoh, bukan tarif Kemenkes.**
  `GET /api/casemix/{id}/export` mengembalikan JSON berkas untuk
  disalin ke aplikasi E-Klaim — **WARAS tidak mengirim ke E-Klaim
  langsung** (tidak ada endpoint BPJS-hosted untuk itu; E-Klaim asli
  adalah aplikasi lokal `ws.php` atau alur FHIR Claim).
- **Mode PostgreSQL**: `WARAS_DATABASE_URL=postgres://...` memilih
  PostgreSQL (skema sama, DDL terpisah di `migrations-pg/`);
  `/api/health` mengembalikan `mode: "postgres"`, footer UI
  "Mode Rumah Sakit · PostgreSQL". SQLite tetap default.
  Dibuktikan di PG 16 nyata (Fase A, A2 — lihat `docs/FASE-A-A2.md`).
- **Validasi pasien salah**: semua tulis klinis yang membawa
  `patient_id` + `encounter_id` dicocokkan di server — tidak cocok →
  422 `WRONG_PATIENT`. Form tulis menampilkan konfirmasi
  "Pasien: {nama} ({rm})" sebelum kirim.
- **RBAC per modul** (`waras-core::rbac`): Dokter semua aksi Fase 3;
  Perawat triase/admisi/order lab & radiologi; Kasir grouping Casemix;
  Apoteker hanya lihat; Admin semua.
- **KPI dasbor kini nyata** untuk lab/IGD/bed (dihitung dari tabel
  domain, bukan `daily_stats`): lab menunggu 3 (dulu 26), lab kritis 1
  (dulu 2), IGD merah 1 (dulu 3), bed 86/120 (tetap).

## Ekosistem (Fase 4) — branch `fase-4-ekosistem`, belum deploy

Fase 4 menuntaskan semua modul yang masih `segera` di sidebar
(lihat `docs/FASE4.md` untuk kontrak lengkap). Setelah Fase 4,
**tidak ada lagi item sidebar bertanda "segera"**:

- **Hemodialisa**: 4 mesin (HD-1..HD-4) + sesi dialisis (jadwal,
  Berjalan/Selesai, mesin otomatis Sibuk → Siap kembali).
- **Transfusi Darah**: stok kantong (8 golongan/komponen, status
  Tersedia/Segera/Habis), permintaan + crossmatch (Cocok/Tidak cocok).
- **Gizi**: asesmen (IMT dihitung server, kategori Normal/Kurus/
  Gemuk/Obesitas) + diet & menu.
- **Rekam Medis Gigi**: odontogram grid FDI 11–48 + perawatan gigi.
- **Portal Pasien** (`/portal`, mobile 390px): pasien login dengan
  **NIK + kata sandi portal** (`waras123` untuk semua pasien seed,
  hash Argon2 — lihat `docs/FASE-A-A4.md`) dan hanya melihat data
  sendiri (e-resep, janji, hasil lab, antrean; NIK dimaskir
  `337402******0001`). Login dibatasi brute-force (5 gagal/15 menit
  -> 429). NIK + tanggal lahir saja tidak bisa masuk.
  Peran baru `pasien`: hanya `ViewPortal`.
- **API Publik** (`/public/v1/*`, `/api-publik`): kunci API dibuat
  Admin (`wrs_` + 32 heksadesimal acak, disimpan sebagai hash
  SHA-256, kunci penuh HANYA ditampilkan sekali saat dibuat, tidak
  pernah di-seed), cakupan `patients:read`, `queue:read`,
  `encounters:read`, `fhir:read`, rate limit 100/menit per kunci (persisten di DB, bertahan setelah restart),
  NIK dimaskir di semua respons. Hanya baca — tidak ada tulis.
- **Inventori**: stok obat/alkes (status Kritis ≤12 / Segera ≤30 /
  Aman — logika sama dengan Farmasi), kartu masuk/keluar.
- **Kepegawaian**: daftar staf + jadwal jaga (satu staf satu shift
  per hari; duplikat → 422).
- **Laporan & SIRS**: kunjungan per hari (dari `daily_stats.visits`,
  sama seperti dasbor; antrean dipakai sebagai pelengkap bila
  tanggal belum ada di `daily_stats`), 10 besar penyakit, agregat
  gaya SIRS (bed, kunjungan hari ini, 10 besar penyakit).
- **Pengaturan & Audit**: 4 kunci pengaturan aplikasi (validasi
  server; retensi audit hanya Admin) + penampil audit trail
  (append-only, hash chain SHA-256, read-only).
- **Rawat Jalan**: daftar kunjungan rawat jalan hari ini dari
  antrean/encounter.
- **Modal bersama** (`Waras.modal()`): dipakai di API Publik
  (tampil kunci sekali + cabut kunci), Kepegawaian
  (aktifkan/nonaktifkan staf), dan tempat lain yang butuh dialog.

## Yang SENGAJA belum dibangun

- **Aksesibilitas keyboard + layout responsif**: tetap nice-to-have,
  belum dibangun (dicatat di `docs/FASE4.md` §8).
- **Mode PostgreSQL dibuktikan (Fase A, A2, 21 Nov 2026):** migrasi
  0001–0006, seed, boot HTTP (`mode: "postgres"`), seluruh test
  suite, dan uji beban 20 pengguna (200/200 tulis) di PostgreSQL
  16 nyata — lihat `docs/FASE-A-A2.md`. Tiga bug ditemukan dan
  diperbaiki: `SUM(amount)` → Numeric (dasbor 500), pencarian pasien
  case-sensitive (`LIKE` → `LOWER()`), dan tabrakan PK saat tulis
  bersamaan (alokasi id kini diserialisasi `Db::alloc_lock`).
- **E-Klaim**: tidak ada endpoint pengiriman (lihat Fase 3 di atas).
- **BPJS live**: adapter dibangun + terverifikasi stub; butuh
  kredensial Dev dari faskes mitra JKN (lihat Fase 2 di atas).

## Penyimpangan dari desain (dicatat terbuka)

1. **Antrean 44, bukan 42.** Desain menulis total donut 42 dan "nomor
   berikutnya A-045", tapi baris antrean yang terlihat sudah sampai
   A-044 — keduanya tidak bisa benar sekaligus. Seed memakai 44 entri
   supaya baris A-041..A-044 persis seperti desain; KPI dihitung dari
   data nyata sehingga total 44 dan antrean aktif 32 (bukan 42/30).
2. **Footer mode mengikuti backend**: SQLite → "Mode Klinik · SQLite",
   PostgreSQL (`WARAS_DATABASE_URL`) → "Mode Rumah Sakit · PostgreSQL".
   `/api/health` mengembalikan `mode` yang sama.
3. **Agregat hari ini dari `daily_stats`**: pendaftaran hari ini (248),
   kunjungan 7 hari (rata-rata 178, puncak 210), pendapatan (Rp 18,4 jt,
   142 transaksi), piutang BPJS (Rp 212 jt) dan deposit rawat inap
   (Rp 46 jt) adalah agregat seed. Sejak Fase 3, lab menunggu (3),
   lab kritis (1), IGD merah (1), dan tempat tidur (86/120) dihitung
   dari tabel domain, bukan dari `daily_stats`.
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

Apache-2.0 — lihat file `LICENSE`.
