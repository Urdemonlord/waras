# Fase A — A4: Keamanan Produksi

Branch `a4-security` (dari main `c23844c`, Fase 1–4). Kriteria keluar
roadmap: portal tidak lagi bisa masuk hanya dengan NIK + tanggal
lahir, login tahan brute-force, rate limit bertahan setelah restart.
Ketiganya terpenuhi + tinjauan keamanan (§5).

## 1. Login portal pasien: kata sandi, bukan NIK + tanggal lahir

- `POST /api/auth/patient-login` kini menerima `{nik, password}`.
  Verifikasi Argon2 terhadap `patients.portal_password_hash`
  (migrasi 0007, kolom baru nullable; pola hash yang sama dengan
  staf — `Db::hash_password` / `Db::verify_password`).
- Payload lama `{nik, birth_date}` DITOLAK: field `password` wajib,
  jadi request lama gagal validasi (400, bukan login). Tanggal lahir
  yang dipakai sebagai kata sandi gagal verifikasi (401). Tes:
  `patient_login_jalur_lama_nik_tanggal_lahir_ditolak`.
- Kata sandi demo semua pasien seed: **`waras123`** — sama seperti
  kata sandi seed staf, didokumentasikan di README persis seperti
  login seed staf (bukan kata sandi tersembunyi). Satu hash dipakai
  semua pasien seed (kata sandi demo, bukan rahasia per pasien).
- DB yang sudah ter-seed sebelum migrasi 0007 (kolom baru NULL)
  di-backfill saat boot (`Db::backfill_portal_passwords`, dipanggil
  `main.rs` setelah seed Fase 4; idempotent, hanya menyentuh NULL).
  Tes backfill ada di `waras-db/src/security.rs`.
- `portal.html` + `portal.js`: field tanggal lahir diganti field
  kata sandi (`type=password`); pesan error membedakan 429
  (terlalu banyak percobaan) dari 401.
- **Belum ada endpoint set/ganti kata sandi portal.** Pasien baru
  dari registrasi (`POST /api/patients`) lahir dengan hash NULL =
  tidak bisa login portal sampai ada endpointnya. Ini celah fungsi
  yang disengaja untuk A4 (tidak ada alur "buat kata sandi saat
  registrasi" di kontrak); tindak lanjut: endpoint staf
  `POST /api/patients/{id}/portal-password` (set awal + reset) dan
  `POST /api/portal/change-password` (pasien ganti sendiri, butuh
  kata sandi lama). Sampai itu ada, kata sandi portal hanya bisa
  diubah langsung di DB (`Db::set_portal_password` sudah tersedia).
- **Mengapa kata sandi, bukan OTP** (keputusan A4): tidak ada kanal
  pengirim SMS/WA di WARAS maupun di VPS — OTP butuh penyedia SMS
  (biaya + kredensial + nomor pasien yang tervalidasi). Kata sandi
  menutup lubang yang sama (NIK + tanggal lahir adalah data yang
  bisa diketahui orang lain) tanpa dependensi eksternal. OTP tetap
  bisa ditambahkan nanti di atas fondasi ini (kolom hash tetap
  berguna sebagai faktor kedua / fallback).

## 2. Brute-force: `/api/auth/login` + `/api/auth/patient-login`

- Tabel `login_attempts` (migrasi 0007, kedua dialek): kunci
  `staff:<ip>:<username-atau-NIP>` / `patient:<ip>:<nik>`,
  `failed_count`, `window_start`, `locked_until`.
- Aturan: 5 kegagalan dalam jendela 15 menit -> kunci 15 menit dari
  kegagalan ke-5, dibalas **429 + header `Retry-After`** (kode error
  `RATE_LIMITED`, bentuk JSON sama seperti error lain). Login sukses
  menghapus baris hitungan. Jendela kedaluwarsa me-reset hitungan.
  Percobaan saat terkunci TIDAK memperpanjang kunci (penyerang
  tidak bisa mengunci akun orang lain selamanya dengan menembak
  terus).
- IP = peer TCP (`ConnectInfo<SocketAddr>`, `main.rs` memakai
  `into_make_service_with_connect_info`; extractor `ClientIp` di
  `auth.rs` yang tidak pernah menolak — tanpa ConnectInfo IP-nya
  `"unknown"`, jadi tes oneshot tetap jalan). SENGAJA tidak memakai
  `X-Forwarded-For`: header itu bisa dipalsukan pemanggil dan
  pemalsuan membuat tiap percobaan memakai kunci baru (lockout
  tidak pernah tercapai). Konsekuensi di balik nginx: peer-nya
  adalah nginx (127.0.0.1) untuk semua, jadi komponen IP-nya
  konstan — kunci tetap membedakan pengguna, lockout per pengguna
  tetap jalan. Bila nanti nginx meneruskan IP asli dengan cara
  tepercaya (mis. `X-Real-IP` dari proxy yang dikonfigurasi),
  `ClientIp` bisa diperluas di satu tempat.
- Batas 5/15 menit berlaku per pasangan IP + pengguna: penyerang
  dari banyak IP terhadap satu pengguna tetap dibatasi 5 percobaan
  per IP (bukan batas global per pengguna). Ini trade-off yang
  disengaja: batas global per pengguna memungkinkan penyerang
  mengunci akun staf sah dari jauh (denial of service). Mitigasi
  lanjutan (bukan A4): CAPTCHA atau penundaan progresif.
- Tes: unit `waras-db` (lockout, tidak diperpanjang, reset jendela,
  kunci kedaluwarsa) + router `waras-api` (5 gagal -> 429, kata
  sandi benar ditolak saat terkunci, sukses membersihkan hitungan)
  + portal (5 gagal -> 429).

## 3. Rate limit API publik: persisten di DB

- Tabel `api_rate_limits` (migrasi 0007): per `key_id`, jendela
  60 detik, 100 request. `Db::check_api_rate_limit` menggantikan
  `HashMap` in-memory + `RateLimiter`/`rate_limit_allowed` di
  `public_api.rs` (dihapus). Perilaku sama: 100 lolos, ke-101 ->
  429 + `Retry-After`, isolasi per kunci, reset setelah 60 detik.
- Bukti persistensi: tes `public_rate_limit_bertahan_setelah_restart`
  (DB file: 100 request, tutup pool + router, buka lagi file yang
  sama, request ke-101 tetap 429) + verifikasi curl saat boot
  (§6). Dengan HashMap lama, hitungan mulai dari nol.
- Catatan konkurensi: cek + hitung dilakukan sebagai baca-lalu-tulis
  tanpa transaksi; dua request bersamaan di batas bisa sama-sama
  lolos (kelebihan 1–2 request di tepi jendela). Diterima untuk
  A4 — ini pembatas penyalahgunaan, bukan kuota penagihan. Bila
  nanti butuh ketepatan, bungkus `check_api_rate_limit` dalam
  transaksi / `UPDATE ... WHERE count < 100`.
- `Retry-After` dihitung dari sisa jendela (detik), bukan nilai
  tetap 60.

## 4. Endpoint logout (temuan saat tinjauan)

- `POST /api/auth/logout` (baru): menghapus sesi pemilik token
  Bearer — staf maupun portal (`delete_session` berdasarkan token).
  Selalu 200, juga tanpa token (jangan bocorkan apakah token ada).
- Sebelumnya: tombol Keluar portal hanya menghapus token dari
  `localStorage`; sesi server tetap berlaku sampai kedaluwarsa
  12 jam — siapa pun yang menyalin token tetap bisa memakai portal
  setelah pemiliknya "keluar". `portal.js` kini memanggil endpoint
  ini (best-effort) saat logout. UI staf belum punya tombol logout
  (tidak ada di `app.js`/`login.js`), tapi endpointnya bisa dipakai
  staf juga.

## 5. Tinjauan keamanan (yang dicek, yang diperbaiki, yang dibiarkan)

### 5.1 Header HTTP — DIPERBAIKI

Sebelum A4, server tidak mengirim satu pun header keamanan (dicek
lewat curl sebelum perubahan — lihat §6). Kini, lewat
`tower-http` `SetResponseHeaderLayer` (fitur `set-header`, tanpa
dep tambahan; `SetHeaderLayer` lama tidak ada di tower-http 0.6 —
namanya `SetResponseHeaderLayer`) di semua respons (API + aset +
fallback):

- `X-Content-Type-Options: nosniff` — cegah browser menebak MIME
  (mis. menyajikan JSON sebagai script).
- `X-Frame-Options: DENY` — UI tidak pernah di-iframe (cegah
  clickjacking).
- `Referrer-Policy: strict-origin-when-cross-origin` — jangan
  bocorkan path lengkap (yang bisa berisi RM pasien) ke situs lain.

Tidak ditambah: `Content-Security-Policy` — UI memakai inline
`style` dan `onclick`-free JS eksternal, tapi CSP yang salah akan
mematikan halaman; CSP butuh audit terpisah per halaman (tindak
lanjut Fase C). `Strict-Transport-Security` — WARAS di-serve
langsung via HTTP di 127.0.0.1:3000 dan TLS ditangani nginx di
depan; HSTS harus dipasang di nginx, bukan di aplikasi (bila
dipasang di aplikasi, headernya tidak akan sampai bila nginx
men-strip-nya — dan HSTS di HTTP lokal tidak ada gunanya).

### 5.2 Token sesi — DIBIARKAN (dengan alasan)

- Token: 32 byte acak -> 64 hex, disimpan mentah di `sessions`,
  kedaluwarsa +12 jam, dicek di tiap lookup (`expires_at > now`).
  Penyimpanan mentah DIBIARKAN: token acak 256-bit yang disimpan
  di DB adalah pola sesi standar (bila DB sudah bisa dibaca
  penyerang, seluruh rekam medis — NIK, diagnosis, resep — sudah
  bocor; token tidak menambah permukaan). Meng-hash token akan
  menginvalidasi semua sesi saat upgrade dan menambah kerumitan
  tanpa mengubah ancaman utama. Yang DIPERBAIKI adalah logout
  (§4): sesi kini bisa dicabut sebelum kedaluwarsa.
- Token tidak dirotasi tiap request (sesi 12 jam tetap) — diterima
  untuk aplikasi klinik intranet; rotasi + refresh token adalah
  tindak lanjut bila ada kebutuhan.
- Tidak ada pembatasan sesi bersamaan (satu pengguna boleh login
  dari banyak perangkat) — diterima, didokumentasikan.

### 5.3 Cookie — TIDAK ADA (dibiaran, didokumentasikan)

Aplikasi tidak memakai cookie sama sekali (dicek: tidak ada
`Set-Cookie` di respons mana pun). Token disimpan di
`localStorage` (`waras_token`, `waras-portal-token`). Implikasi
XSS: script jahat yang lolos ke halaman bisa membaca token dan
memakainya — ini risiko yang melekat pada SPA + localStorage.
Tidak diubah di A4 karena: (a) tidak ada mekanisme cookie
httpOnly + CSRF di aplikasi ini (butuh endpoint CSRF + SameSite
+ perubahan semua fetch); (b) permukaan XSS saat ini kecil —
semua render memakai `esc()` / `textContent`, tidak ada
`innerHTML` dari data pengguna kecuali potongan yang sudah
di-escape di `portal.js`/`app.js` (dicek sekilas, bukan audit
XSS penuh). Tindak lanjut: audit XSS menyeluruh + pertimbangkan
cookie httpOnly + CSRF bila aplikasi dibuka ke internet tanpa
nginx WAF.

### 5.4 Aset statis / path traversal — AMAN (guard ditambah)

- `waras-web::get` HANYA lookup kunci persis di peta rust-embed
  (tidak ada akses filesystem): `/assets/../../etc/passwd` tidak
  akan pernah menemukan file (dicek lewat curl — 404). Meski
  begitu, `serve_asset` kini menolak eksplisit segmen `..`/`.`,
  backslash, dan `%2e` (defense in depth bila routing berubah).
- `mime_for`: ekstensi tidak dikenal -> `application/octet-stream`
  + `nosniff` (§5.1) mencegah eksekusi sebagai script.

### 5.5 `/api/health` — DIBIARKAN

Mengembalikan `{status, version: "0.1.0", mode, today}` tanpa
auth. Pengungkapan versi DIBIARKAN: ini versi demo publik
(`0.1.0`), bukan rahasia; endpoint ini dipakai monitoring dan
tidak membocorkan data pasien, kredensial, atau konfigurasi.
Bila versi nanti mengandung info kerentanan spesifik, tinjau
ulang.

### 5.6 Lainnya (dicek, tidak ada masalah)

- Pesan error login generik (401 tanpa membedakan NIK/username
  salah vs kata sandi salah) — dipertahankan di kedua login.
- NIK tetap disensor tengah di semua respons portal + publik
  (tidak diubah A4; tes sensor tetap hijau).
- `WarasError` tidak punya varian 429 — 429 dibangun langsung
  sebagai `Response` (`ApiError::rate_limited_response`) supaya
  bisa membawa header `Retry-After`. `waras-core` SENGAJA tidak
  diubah (kontrak: selesai kecuali bug).
- Kata sandi seed staf TIDAK diubah (`waras123`, kontrak §5.1).
- Data seed TIDAK diubah (Siti RM-000812 + alergi penisilin,
  5 ward / 120 bed / 86 terisi, 8 order lab, 7 order radiologi,
  4 mesin HD). KPI dasbor tidak berubah karena A4 (tidak ada
  perubahan data; hanya 3 tabel baru + 1 kolom baru).

## 6. Verifikasi

<!-- TODO: isi setelah boot + curl -->

- `cargo test --offline`: TODO tes, semua hijau.
- `cargo build --release --offline`: TODO.
- Boot di `~/workspace/waras/.fase-a-scratch/` (DB temp, BUKAN
  `/tmp`, bukan live):
  - portal login `{nik, password: waras123}` -> 200 + token;
    `{nik, birth_date}` -> 400; tanggal lahir sebagai kata
    sandi -> 401;
  - login staf 5x salah -> 429 + `Retry-After`; kata sandi benar
    saat terkunci -> 429; (jendela 15 menit tidak ditunggu penuh
    saat verifikasi curl — kedaluwarsa jendela dibuktikan tes unit
    dengan memajukan `window_start` di DB);
  - rate limit publik: 100 OK, ke-101 429, restart server, 429
    tetap (bukti persistensi);
  - header keamanan ada di `/api/health`, `/`, dan `/portal`;
  - traversal: TODO.
- Migrasi 0007 dialek Postgres (`migrations-pg/`) BELUM TERBUKTI
  — ditulis mengikuti pola 0001–0006 (BIGINT, ALTER biasa),
  tapi TIDAK PERNAH dijalankan di Postgres nyata. Koordinator
  WAJIB menjalankan migrasi 0007 di Postgres (A2) dan melaporkan
  bila gagal; sampai saat itu statusnya UNPROVEN. Perhatian:
  `ALTER TABLE patients ADD COLUMN portal_password_hash TEXT`
  di Postgres terhadap tabel berisi data = NULL untuk semua baris
  lama; backfill di `main.rs` mengisi `waras123` (sama seperti
  SQLite) — pastikan itu memang yang diinginkan di instalasi
  Postgres (bila tidak, isi manual sebelum boot pertama).

## 7. Perubahan file

- `crates/waras-db/migrations/0007_fase_a.sql` +
  `crates/waras-db/migrations-pg/0007_fase_a.sql` (baru)
- `crates/waras-db/src/security.rs` (baru): hash portal,
  backfill, lockout, rate limit + 5 tes unit
- `crates/waras-db/src/lib.rs`, `src/seed.rs`: registrasi modul
  + ekspor; seed 14 pasien (+2 Fase 4) dapat hash portal
- `crates/waras-api/src/handlers.rs`: lockout login staf +
  `logout`
- `crates/waras-api/src/portal_handlers.rs`: login kata sandi +
  lockout + tes jalur lama
- `crates/waras-api/src/public_api.rs`: rate limit DB +
  `Retry-After` + tes restart
- `crates/waras-api/src/lib.rs`: layer header + route logout
- `crates/waras-api/src/auth.rs`: extractor `ClientIp`
- `crates/waras-api/src/error.rs`: `rate_limited_response`
- `crates/waras-api/src/web.rs`: guard traversal
- `crates/waras-api/src/tests.rs`: tes lockout staf, header,
  logout
- `crates/waras-web/assets/portal.html`, `assets/js/portal.js`:
  field kata sandi + logout server
- `crates/waras/src/main.rs`: backfill + ConnectInfo
- `Cargo.toml`, `crates/waras-api/Cargo.toml`: `tower-http`
  0.6.11 fitur `set-header` (sudah di cache registry)
- `README.md`: login portal demo + rate limit persisten
- `docs/FASE-A-A4.md`: dokumen ini

## 8. Tindak lanjut (bukan A4)

1. Endpoint set/reset + ganti kata sandi portal (§1).
2. CSP per halaman + audit XSS (§5.1/§5.3).
3. HSTS di nginx (bukan aplikasi).
4. Buktikan migrasi 0007 di Postgres nyata (A2).
5. Batas global per pengguna / CAPTCHA bila brute-force
   terdistribusi jadi masalah (§2).
6. Ketepatan rate limit di bawah konkurensi (transaksi, §3).
7. Rotasi token / pembatasan sesi bersamaan bila dibutuhkan.
8. Tombol logout staf di UI (endpoint sudah ada).
