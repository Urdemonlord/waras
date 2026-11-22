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

## Item 2 — Timer backup otomatis aktif di VPS — SELESAI

Tindak lanjut dari A3 (`docs/BACKUP.md`): timer systemd yang di A3 masih
placeholder akhirnya dipasang dan dibuktikan jalan di VPS live
(22 Nov 2026, dikerjakan agen Item 2 di VPS; agen Item 3 hanya
mendokumentasikan hasilnya di sini dan menyelaraskan file unit di repo).

### Yang terpasang di VPS

| Bagian | Lokasi di VPS |
|---|---|
| Skrip backup | `/home/meowlabs/waras-scripts/waras-backup.sh` |
| Skrip restore | `/home/meowlabs/waras-scripts/waras-restore.sh` |
| Unit service | `/etc/systemd/system/waras-backup.service` |
| Unit timer | `/etc/systemd/system/waras-backup.timer` |
| Direktori backup | `/home/meowlabs/waras-backups` |
| DB live | `/home/meowlabs/waras-data/waras.db` |

- Skrip disalin verbatim dari repo dan sha256 terverifikasi identik:
  backup `88818b3f…`, restore `59225437…`.
- Skrip tinggal di `/home/meowlabs/waras-scripts/`, terpisah dari
  direktori rilis, karena rilis hanya berisi binary (tanpa `scripts/`):
  menunjuk unit ke path rilis akan rusak saat deploy berikutnya.
- `daemon-reload` sudah dijalankan, timer enabled.

### Verifikasi (22 Nov 2026, VPS)

- Pemicu manual: backup
  `/home/meowlabs/waras-backups/waras-20261122-064215.db`
  (548.864 byte, sama persis dengan ukuran DB live), sidecar `.sha256`
  dihitung ulang dan cocok, `PRAGMA integrity_check` ok, spot-check
  baris backup vs live identik (patients/antrean/resep = 12/44/12).
- `waras.service` tetap aktif selama backup; tidak ada backup yang
  dihapus atau dipindah.
- Timer enabled, jalan berikutnya Sen 2026-11-23 02:31:50 waktu server.

### Penyimpangan dari unit placeholder di repo

- User/Group `meowlabs` (bukan `waras`).
- `WARAS_DB=/home/meowlabs/waras-data/waras.db` (bukan `/opt/waras/waras.db`).
- `WARAS_BACKUP_DIR=/home/meowlabs/waras-backups` (bukan `/var/backups/waras`).
- `ExecStart=/home/meowlabs/waras-scripts/waras-backup.sh` (bukan
  `/opt/waras/scripts/waras-backup.sh`).
- `WorkingDirectory=/home/meowlabs` (bukan `/opt/waras`).
- Baris `Wants=waras-backup.timer` di repo dihapus (tidak diperlukan:
  timer yang mengaktifkan service, bukan sebaliknya).
- File unit di repo (`scripts/waras-backup.service`,
  `scripts/waras-backup.timer`) sudah diperbarui agar sama dengan unit
  yang terpasang di VPS, lengkap dengan komentar layout VPS di atas.

### Perhatian zona waktu (perlu keputusan Arya)

Jam server VPS adalah CST, jadi "02:30 lokal" di timer berarti 02:30
CST = 01:30 WIB. Bila Arya maunya backup jalan 02:30 WIB, pilih satu:
OnCalendar timer diubah jadi 01:30, atau zona waktu server diubah.
Sampai diputuskan, backup harian jalan 01:30 WIB.

### Celah proses deploy (dicatat, belum diperbaiki)

Rilis hanya mengirim binary, jadi skrip backup/restore hidup di luar
rotasi rilis. Bila skrip berubah di repo, skrip itu HARUS disalin ulang
ke `/home/meowlabs/waras-scripts/` secara manual, atau pipeline deploy
diajari ikut mengirim `scripts/`. Bila langkah ini lupa, timer akan
menjalankan skrip versi lama tanpa ada yang memberi tahu.

## Item 3: Race alokasi ID di jalur tersisa (SELESAI)

(22 Nov 2026, branch `fase-a-followup`.)

### Pola yang dipakai

Sama seperti A2: tiap fungsi alokasi publik dibungkus retry maks 5x
(hanya untuk pelanggaran unique: Postgres "duplicate key value violates
unique constraint", SQLite "UNIQUE constraint failed"; error lain
langsung dikembalikan), memanggil `*_once` yang memegang
`Db::alloc_lock` selama hitung-max + INSERT. Baca, validasi, dan kerja
CPU berat (mis. hash Argon2 di `create_user`) tetap di luar kunci
supaya seksi kritis pendek. `log_audit` tidak pernah mengambil kunci,
jadi tidak ada nesting kunci.

### Situs yang ditemukan dan diperbaiki

Inventaris dari grep `next_seq_from_ids` / `SELECT id FROM` / `MAX(`
di `crates/waras-db/src` (bukan tebakan): 29 fungsi publik + 2 helper.
Sudah dikunci di A2: `queue::take_queue`, `prescriptions::prescribe_as`,
`outbox::enqueue_outbox`.

| Fungsi | Pola ID |
|---|---|
| `patients::create_patient` | RM-###### |
| `encounters::create_encounter` | ENC-`<tanggal>`-`<seq>` |
| `invoices::create_invoice` | INV-`<tahun>`-`<seq>` |
| `claims::create_claim` | CLM-`<yyyymmdd>`-### (bila id kosong) + helper baru `pub(crate) insert_claim` |
| `casemix::group_claim_for_encounter` | CLM-`<seq>` (mulai 5518) |
| `ranap::create_admission` | ADM-`<yyyymmdd>`-### |
| `ranap::add_care_note` | CRN-###### |
| `lab::create_lab_order` | LAB-`<seq>` (mulai 2210) |
| `lab::add_lab_results` | LBR-###### (batch) + `insert_notification_kritis` NOTIF-### (caller-holds-lock) |
| `radiologi::create_rad_order` | RAD-#### (mulai 911) |
| `radiologi::save_rad_report` | RDR-###### (upsert) |
| `igd::create_igd_triage` | IGD-`<yyyymmdd>`-### |
| `igd::add_igd_observation` | IGO-###### |
| `operasi::create_surgery` | OK-`<yyyymmdd>`-### |
| `operasi::create_delivery` | VK-`<yyyymmdd>`-### |
| `hemodialisa::create_hd_session` | HD-`<yyyymmdd>`-### |
| `transfusi::create_blood_request` | TR-`<yyyymmdd>`-### |
| `transfusi::add_crossmatch` | XM-###### |
| `gigi::create_dental_record` | GG-###### |
| `gizi::create_nutrition_assessment` | GZ-###### |
| `gizi::create_nutrition_diet` | DT-###### |
| `inventori::create_inventory_item` | INV-ITM-### |
| `inventori::add_inventory_move` | INM-###### |
| `kepegawaian::create_staff` | STF-### |
| `kepegawaian::create_shift` | SHF-`<yyyymmdd>`-### |
| `users::create_user` | U-### |
| `api_keys::create_api_key` | AK-###### |
| `seps::create_sep` | SEP-`<yyyymmdd>`-### |
| `seps::create_referral` | RUJ-`<yyyymmdd>`-### |
| `seps::create_pcare_visit` | PCV-`<yyyymmdd>`-### |

Untuk SEP/rujukan/PCare, `seps::next_dated_id` kini menerima bukti
kunci (`&MutexGuard`) sebagai parameter, jadi hitung-max di helper dan
INSERT pemanggil tetap atomik. Untuk casemix, grouping memakai helper
`insert_claim` (bukan `create_claim`) supaya tidak mengunci dua kali
saat sudah memegang kunci. Untuk fungsi berbasis transaksi
(`create_admission`, `create_delivery`, `create_hd_session`,
`add_inventory_move`), kunci diambil SEBELUM `pool.begin()` dan
dipegang sampai commit.

Efek samping yang didokumentasikan di kode: kunci juga menyeragamkan
cek-lalu-bertindak dalam satu proses, jadi dua pemanggil bersamaan
tidak bisa lolos bersama untuk admisi ranjang yang sama, shift ganda
staf + tanggal yang sama, persalinan ke ruang yang sama, sesi HD ke
mesin yang sama, mutasi Keluar yang menghabiskan stok yang sama, upsert
laporan radiologi untuk order yang sama, dan cek draft klaim di
grouping.

Dikecualikan setelah dicek: ID UUID (vitals OBS-, conditions COND-,
allergies ALG-, payments PAY-, versi SOAP SV-, sesi) aman tabrakan;
`audit_log` INTEGER PRIMARY KEY AUTOINCREMENT (rantai prev_hash-nya
race terpisah, di luar cakupan, lihat di bawah);
appointments/online_queue read-only (tanpa fungsi create di luar seed);
obat, bangsal, ranjang, ruang VK, mesin HD, dan stok darah adalah data
referensi seed-only.

### Uji konkurensi baru

File `crates/waras-db/src/alloc_concurrency.rs`, 5 tes (gaya uji beban
A2, 20 thread x 10 tulis):

- `concurrent_allocations_are_unique`: 20 task x 10 ronde; tiap ronde
  19 jalur baru di pasien + encounter segar. Assert semua Ok, pasien
  14 seed + 200 = 214, dan COUNT(*) == COUNT(DISTINCT id) untuk
  24 tabel.
- `concurrent_scarce_resource_allocations`: 10 task x 2 admisi di
  ranjang Kosong berbeda + care note tiap admisi; 4 persalinan + 4 sesi
  HD setelah setup SQL membuka semua ruang/mesin (seed hanya punya
  2 ruang VK Kosong dan 1 mesin Siap). Assert admisi 7 seed + 20 = 27
  dan id unik untuk admisi, care note, persalinan, sesi HD.
- `concurrent_user_creation_is_unique`: 10 task x 2 ronde (Argon2,
  bukan 20x10).
- `concurrent_same_bed_admission_only_one_wins` dan
  `concurrent_duplicate_shift_only_one_wins`: tepat satu Ok, satu lagi
  422 Unprocessable.
- Berdiri sendiri (SQLite): 5/5 lolos dalam 36,54 dtk.

### Bug laten yang ketemu dan diperbaiki

Tes baru menemukan bug lama di `casemix::group_claim_for_encounter`:
`SELECT sep_id FROM invoices ... fetch_optional()` dengan tipe
`Option<String>` gagal decode saat `sep_id` NULL ("expected TEXT, got
Null(Text)"). Seed invoice di tes lama semuanya punya `sep_id`, jadi
bug ini tidak pernah kena. Perbaikan: `fetch_one` (baris pasti ada;
scalar `Option<String>` men-decode NULL jadi None). Satu baris +
komentar penjelas. Bukan bagian dari race, tapi ditemukan dan dibuktikan
oleh tes item 3 ini.

### Verifikasi

- SQLite: EXIT=0, 252 lolos, 0 gagal (api 95 dalam 388,33 dtk, bridge
  35, core 32, db 87 dalam 328,55 dtk, web 3). Baseline item 1: 247,
  +5 tes baru. Log:
  `.fase-a-scratch/followup-item3/test-sqlite.log`.
- Postgres 16 (sandbox, diinstal ulang dari .deb cache karena VM
  sandbox diganti dan instalasi PG lama hilang): EXIT=0, 252 lolos,
  0 gagal (api 95 dalam 443,70 dtk, bridge 35, core 32, db 87 dalam
  390,62 dtk, web 3). Log:
  `.fase-a-scratch/followup-item3/test-pg.log`.
- Release build: `cargo build --release --offline` sukses, 1m39d,
  binary 15.938.712 byte, sha256 `dce1107f…`.
- Boot + curl (`followup-item3/verify-item3.sh`, port 3103, DB temp
  SQLite): health 200, login staf admin + dokter, KPI seed
  patients_today 248 dan queue_total 44, tulis perwakilan tiap jalur
  201 (pasien RM-000826, encounter, tagihan, order lab, order
  radiologi, klaim, triase IGD). SEP tidak diuji HTTP: POST /api/seps
  503 tanpa konfigurasi BPJS (perilaku benar, tercakup suite API + tes
  Rust).
- Uji beban HTTP: 20 thread x 10 ronde x 5 tulis = 1000/1000 lolos
  dalam 232,4 dtk (4,3 tulis/dtk). Per jenis: pasien p50 6473ms p95
  8968ms maks 16632ms; encounter p50 3774ms p95 10250ms maks 22437ms;
  tagihan p50 3366ms p95 6607ms maks 19666ms; lab p50 3339ms p95 5696ms
  maks 12509ms; rad p50 3318ms p95 7772ms maks 17966ms. Hitung baris
  akhir +200 tiap tabel (patients 215, encounters 213, invoices 209,
  lab_orders 209, rad_orders 208), id pasien dan encounter unik.
  Total skrip verifikasi: 22 PASS, 0 FAIL.
- Angka latensi adalah angka kontensi di disk sandbox yang lambat
  (fsync ~22ms; satu tulis pasien ~260ms karena INSERT + audit),
  bukan kapasitas bersih, sama seperti catatan A2. Di SSD VPS angkanya
  akan jauh lebih kecil.

### Temuan selama uji beban: busy_timeout SQLite (perubahan tambahan)

Jalankan pertama uji beban gagal 29/940: pasien, encounter, lab, dan
rad 500 `database is locked` (SQLITE_BUSY, kode 5), tepat di ~5007ms.
Penyebab: SQLite mengizinkan satu penulis dalam satu waktu, pool
5 koneksi, busy_timeout default sqlx 5 dtk; antrean 20 penulis di disk
lambat melewati 5 dtk. Ini BUKAN disebabkan perubahan item 3 (perubahan
ini tidak menambah tulis apa pun); tes Rust lolos karena memakai
`:memory:` 1 koneksi tanpa fsync.

Perbaikan (`lib.rs`, `DbPool::connect_sqlite`): `PRAGMA busy_timeout =
30000` lewat `after_connect` di tiap koneksi baru. URL sqlite sqlx
0.8.6 tidak menerima parameter busy_timeout, jadi lewat hook ini.
Tidak mengubah durabilitas atau format file; Postgres tidak memakai
jalur ini. Setelah perbaikan: 1000/1000, maks 22,4 dtk, di bawah 30
dtk. Efek samping: error "database is locked" worker outbox berkurang
dari 5 jadi 3 per jalankan beban (tetap ada sesekali di beban ekstrem;
job tetap Pending dan dicoba lagi siklus berikut, tanpa kehilangan
data; retry worker sendiri di luar cakupan item 3).

Caveat retry batch lab: `add_lab_results` mengunci seluruh batch dan
retry mengulang seluruh batch, jadi tabrakan antar proses di tengah
batch bisa menduplikasi baris. Praktis tidak terjangkau karena kunci
dalam proses + deploy satu proses; dicatat di sini.

### Yang masih terbuka / di luar cakupan

- Rantai prev_hash `audit_log`: race baca-lalu-tulis terpisah, bukan
  alokasi ID, belum diperbaiki.
- `create_claim` dengan id dari pemanggil: retry tidak berguna tapi
  tidak berbahaya (gagal setelah 5x dengan pelanggaran yang sama).
- Kapasitas bersih di bawah konkurensi adalah kerja Fase C/C2, sama
  seperti catatan A2.
