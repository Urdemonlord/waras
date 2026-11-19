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

52 tes: `waras-core` 9 (termasuk blokir penisilin + azithromycin lolos),
`waras-db` 15 (seed, KPI, rantai audit, versi SOAP, dispensing),
`waras-api` 25 (login, RBAC, alur penuh pasien→antrean→encounter→SOAP,
resep diblokir 422 / diizinkan, bayar tagihan, bundle FHIR),
`waras-web` 3 (aset tertanam).

## Jembatan nasional (Fase 2, adapter saja)

`waras-bridge` mengimplementasikan pengiriman SATUSEHAT (OAuth2 client
credentials + POST FHIR, token di-cache, refresh sekali setelah 401,
validasi profil sebelum kirim) dan BPJS (VClaim, PCare, Antrean Online,
Aplicares, E-Klaim; header tanda tangan HMAC-SHA256, envelope
`metaData`, dekripsi respons AES-256-CBC bila `response` terenkripsi).
Semua pengujian memakai stub server lokal (axum, port ephemeral);
tidak ada panggilan live.

**Pola bridging, bukan fakta terverifikasi:** path endpoint BPJS
(mis. VClaim `SEP/2.0/insert`, `Peserta/nik/{nik}/tglSEP/{date}`;
PCare `kunjungan`; Antrean `antrean/add`; Aplicares `bed/get`; E-Klaim
`klaim/submit`) dan skema dekripsi AES-256-CBC (kunci = SHA-256 mentah
dari `cons_id + secret_key + timestamp`, IV = 16 byte pertama kunci)
adalah pola bridging yang umum. Semuanya harus diverifikasi ulang ke
dokumen bridging resmi BPJS saat kredensial live tersedia, sebelum
diaktifkan di produksi. Jangan klaim path ini sudah terverifikasi.

**Dekripsi diimplementasikan** (`aes 0.9` + `cbc 0.2`, keduanya resolve
offline dari cache; kontrak menyebut `aes 0.8`, yang tidak ada di
cache). Round-trip enkripsi/dekripsi diuji, termasuk end-to-end lewat
stub yang mengenkripsi respons dengan timestamp dari header request.

**Idempotensi dijamin outbox, bukan adapter:** adapter mengirim ulang
job yang sama sebagai HTTP baru; worker/outbox (agen A/C) yang
mencegah duplikat lewat `idempotency_key`.

Contoh nilai kredensial (PALSU) ada di `docs/FASE2.md` §7.

## Yang SENGAJA belum dibangun (Fase 2–4)

Jembatan nasional (SATUSEHAT/FHIR sinkronisasi, BPJS VClaim/PCare,
Aplicares, Antrean Online, E-Klaim), IGD, Rawat Inap, Kamar Operasi &
Bersalin, Laboratorium, Radiologi, Casemix, Inventori, Kepegawaian,
Laporan & SIRS, Hemodialisa, Transfusi Darah, Gizi, Rekam Medis Gigi,
Portal Pasien, API Publik. Seam yang sudah disiapkan: tipe FHIR di
`waras-core`, helper `fhir::`, kolom `ina_cbg_code` di tagihan, dan
struktur crate per modul. Lihat paper Bagian 3.4 (kriteria keluar fase)
dan Bagian 10 (peta jalan).

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
