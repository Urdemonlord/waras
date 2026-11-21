# Fase A — Follow-up (21 Nov 2026)

Branch `fase-a-followup` dari `main` @ `567d738` (Fase A live, 241 tes).
Dokumen ini mencatat tiga item terbuka dari `docs/FASE-A.md` § "Yang
masih terbuka" / `docs/FASE-A-A4.md` §8 yang dikerjakan setelah Fase A
selesai. Jangan merge, push, atau deploy dari branch ini tanpa
persetujuan Arya (sama seperti aturan Fase A).

## Item 1 — Endpoint set/ganti kata sandi portal — SELESAI

Celah yang ditutup: pasien baru dari registrasi (`POST /api/patients`)
lahir dengan `portal_password_hash` NULL dan tidak bisa login portal
(`Db::set_portal_password` sudah tersedia sejak A4, tapi tidak ada
endpoint yang memanggilnya).

### Desain yang dipilih

Dua endpoint, mengikuti persis usulan `docs/FASE-A-A4.md` §1:

| Endpoint | Auth | Kegunaan |
|---|---|---|
| `POST /api/patients/{id}/portal-password` | Staf, Admin saja | Set awal SEKALIGUS reset |
| `POST /api/portal/change-password` | Token portal pasien | Ganti sendiri |

- **Set staf = set awal + reset dalam satu endpoint.** Set tidak
  membutuhkan kata sandi lama karena pasien baru memang belum punya
  kata sandi — tidak ada yang bisa diverifikasi. Endpoint ini menimpa
  hash yang ada (atau NULL) dengan hash Argon2 baru via
  `Db::set_portal_password` yang sudah ada; tidak ada logika hash baru.
- **RBAC set staf: `Action::ManageUsers` (Admin saja).** Sengaja
  memakai aksi yang sudah ada, bukan aksi baru di `waras-core`:
  kontrak menyatakan core "selesai kecuali bug", dan matriks
  `waras_core::rbac` sudah membatasi `ManageUsers` hanya untuk Admin.
  Konsekuensinya Dokter/Perawat di meja registrasi TIDAK bisa set kata
  sandi pasien — bila Arya mau mereka bisa, itu butuh aksi RBAC baru
  (mis. `SetPortalPassword`) + perubahan matriks core, bukan sekadar
  mengubah endpoint ini.
- **Ganti pasien wajib kata sandi lama** (verifikasi Argon2 terhadap
  hash tersimpan). Pasien yang hash-nya masih NULL selalu 401 di sini
  — jalurnya adalah set staf, bukan ganti.
- **Brute-force (DB-backed, pola sama seperti login Fase A):**
  ganti pasien memakai kunci `portal-change:<ip>:<pasien-id>` di tabel
  `login_attempts` yang sama — 5 kata sandi lama salah dalam 15 menit
  -> 429 + header `Retry-After`, sukses menghapus hitungan (juga
  hitungan login portal `patient:<ip>:<nik>` yang sama, karena pasien
  baru saja membuktikan ia tahu kata sandinya). Kunci ini terpisah dari
  kunci login supaya percobaan ganti tidak mengunci login dan
  sebaliknya. Set staf TIDAK memakai lockout: tidak ada rahasia yang
  ditebak di sana — perlindungannya adalah auth staf + RBAC Admin,
  dan token yang tidak sah ditolak extractor `AuthUser` (401) sebelum
  DB disentuh.
- **Jalur yang DITOLAK: self-service first-set dengan NIK + tanggal
  lahir.** A4 menutup login NIK + tanggal lahir justru karena
  keduanya adalah data yang bisa diketahui orang lain; memakainya
  lagi untuk first-set membuka kembali lubang yang sama dalam bentuk
  lain — siapa pun yang tahu NIK + tanggal lahir pasien bisa mengklaim
  akunnya lebih dulu dan mengunci pasien aslinya (pemulihannya hanya
  reset staf). Tanggal lahir tetap TIDAK bisa dipakai untuk apa pun
  terkait kata sandi portal. Bila nanti dibutuhkan first-set
  self-service, jalurnya harus bukti yang tidak bisa ditebak orang
  lain (mis. kode klaim satu kali yang diberikan saat registrasi),
  bukan tanggal lahir.
- **Aturan kata sandi baru: minimal 8 karakter** (kedua endpoint).
  Tidak ada aturan panjang di codebase sebelumnya; 8 dipilih karena
  sama dengan kata sandi seed `waras123`. Validasi jalan sebelum
  verifikasi kata sandi lama, supaya kata sandi baru yang pendek tidak
  menghabiskan hitungan lockout.
- **Audit:** set staf dicatat `portal.password_set` (aktor nama staf),
  ganti pasien dicatat `portal.password_changed` (aktor nama pasien).
  Detail audit tidak pernah berisi kata sandi.
- **Yang SENGAJA tidak diubah:** perilaku login tidak berubah
  (login portal tetap NIK + kata sandi, jalur lama tetap 400/401);
  sesi yang sudah ada TIDAK dicabut saat kata sandi diganti/di-reset
  (konsisten dengan A4 §5.2: tidak ada rotasi token; pencabutan sesi
  saat ganti kata sandi adalah tindak lanjut terpisah bila dibutuhkan
  — perlu endpoint/DB baru untuk menghapus semua sesi satu pasien
  kecuali sesi saat ini). Lockout login portal yang sedang berjalan
  juga tidak langsung dibuka oleh reset staf (kunci `patient:<ip>:<nik>`
  memakai IP penyerang yang tidak diketahui staf); lockout tetap
  kedaluwarsa sendiri maksimal 15 menit.

### Perubahan file

- `crates/waras-api/src/portal_handlers.rs`: handler
  `set_portal_password_staff` + `change_portal_password`, konstanta
  `MIN_PORTAL_PASSWORD_LEN = 8`, 6 tes API baru.
- `crates/waras-api/src/lib.rs`: route kedua endpoint.
- `crates/waras-web/assets/portal.html` + `assets/js/portal.js`:
  form "Ganti kata sandi portal" (mode pasien, sesudah login) dan
  form "Set kata sandi portal pasien" (mode staf
  `/portal?pasien=RM-...`, hanya Admin — peran lain mendapat 403
  dari server). Tidak ada redesign halaman.
- `docs/FASE-A.md`: satu baris penunjuk di daftar terbuka.
- `docs/FASE-A-FOLLOWUP.md`: dokumen ini.

### Verifikasi (22 Nov 2026, sandbox)

- `cargo test --offline` SQLite: **247 lolos, 0 gagal** (api 95,
  bridge 35, core 32, db 82, web 3; baseline Fase A 241 + 6 tes baru
  item ini). Suite Postgres tidak dijalankan: tidak ada server PG di
  sandbox ini (tidak ada `pg_lsclusters`/`psql`); migrasi tidak
  berubah di item ini (tidak ada migrasi baru), jadi tidak ada yang
  perlu dibuktikan di PG untuk item ini.
- `cargo build --release --offline`: OK (1m 32s, binary 15.847.016 byte).
- Boot + curl (skrip
  `~/workspace/waras/.fase-a-scratch/followup-item1/verify-item1.sh`,
  log `verify-item1.log`): **31 PASS, 0 FAIL** — health mode sqlite,
  login staf admin + dokter, registrasi pasien baru RM-000826 ->
  login portal 401 -> set staf 200 -> login portal 200 + `/portal/me`
  200, set tanpa token 401 / dokter 403 / RM tak ada 404 / pendek 400,
  ganti tanpa token 401 / lama salah 401 / benar 200 (lama mati, baru
  jalan), ganti 4x salah 401 lalu ke-5 429 + `Retry-After: 900` dan
  kata sandi benar saat terkunci tetap 429, login Budi tetap 200 saat
  kunci ganti aktif (kunci terpisah dari kunci login), jalur lama
  NIK+tanggal lahir tetap 400 dan tanggal lahir sebagai kata sandi
  tetap 401, header `nosniff` ada, KPI dasbor sama seperti seed
  (lab_pending 3, beds_occupied 86, queue_total 44, patients_today 248).
- Playwright Chromium (skrip `playwright-item1.py`): **SEMUA PASS** —
  login pasien + form ganti terlihat, ganti lama salah -> error,
  ganti benar -> sukses + login kata sandi baru jalan, mode staf
  `/portal?pasien=RM-000813` set kata sandi -> sukses + login portal
  dengan hasil reset jalan, **0 console error, 0 page error,
  0 request gagal** (satu 401 yang disengaja di langkah ganti-salah
  dikecualikan dari kolektor error dengan alasan tertulis di skrip).

## Item 2 — (diisi agen berikutnya)

## Item 3 — (diisi agen berikutnya)
