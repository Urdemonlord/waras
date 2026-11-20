# WARAS — Fase A, A2: Mode PostgreSQL dibuktikan (PG 16 nyata)

Dokumen ini mencatat pembuktian mode PostgreSQL WARAS di atas server
PostgreSQL 16 nyata di sandbox (branch `a2-postgres`, dari main @
`c23844c`). Menutup `docs/FASE3.md` §10.4: semua yang di sana masih
"TIDAK terverifikasi" kini dieksekusi di PG nyata, atau dicatat jujur
bila belum.

Server: lihat `~/workspace/waras/.fase-a-scratch/pg-server.txt`
(install, data dir, port, superuser, cara restart). Ringkasnya:
PostgreSQL 16.15 (Ubuntu noble), data dir
`/var/lib/postgresql/16/main`, port 5432,
role `waras` / `waras_pg` + `waras_pg_test`.
PERINGATAN: data dir sistem EPHEMERAL — cluster pertama (20 Nov)
HILANG saat VM sandbox diganti 21 Nov ~21:45 WIB beserta seluruh
datanya, dan paket apt-nya ikut hilang. Data dir di workspace
dicoba tapi tidak bisa (chown ke user postgres ditolak filesystem
home, EPERM). Yang persisten: `.deb` di
`.fase-a-scratch/debs/` + skrip `reinstall-pg.sh` di scratch dir
(install ulang + buat role/DB dalam ~1 menit); database lalu
di-seed ulang otomatis saat server WARAS boot. Semua angka di
dokumen ini diukur ULANG pada cluster kedua (install ulang
21 Nov ~22:00 WIB) kecuali dinyatakan lain.

## 1. Yang dibuktikan

- Migrasi 0001–0006 (kedua direktori) di PG nyata: `_sqlx_migrations`
  6 baris, semua sukses. Termasuk 0005 (`claims.patient_id`) dan 0006
  (`sessions` rebuild → di PG `ALTER COLUMN user_id DROP NOT NULL` +
  `ADD COLUMN patient_id`) yang sebelumnya belum pernah dieksekusi di
  PG nyata.
- Seed Fase 1–4 di PG nyata: 14 pasien, 6 pengguna, 44 antrean,
  8 resep, 9 obat, 8 tagihan, 8 hasil lab, 7 order radiologi,
  9 klaim — sama dengan SQLite.
- Boot HTTP mode Postgres: `GET /api/health` →
  `{"mode":"postgres","status":"ok","today":"2026-11-19","version":"0.1.0"}`.
  Login dokter (`ahmad` / `waras123`) 200, token 64 hex,
  `GET /api/auth/me` → dr. Ahmad Hidayat (U-001).
- Dasbor di PG: pasien 248, antrean 44 (aktif 32), lab menunggu 3,
  lab kritis 1, IGD merah 1, bed 86/120, revenue Rp 18.400.000 / 142
  transaksi, piutang Rp 5.875.000 / 4 — sama dengan seed SQLite.
- Alur tulis Fase 3 di PG (skrip
  `~/workspace/waras/.fase-a-scratch/a2-pg-flow.sh`, 15/15 PASS):
  triase IGD + observasi + ubah status; admisi + catatan + transfer
  bed + pulang (bed 3A-07 Terisi → 3A-10 → Dibersihkan); order lab
  (LOINC autofill 1558-6) + hasil KRITIS (gula darah 385, ambang
  kritis 300 → status order `kritis`); order radiologi + laporan;
  grouping casemix ENC-2026-11-19-016 (J18.9 → J-4-12-I
  Rp 3.150.000, klaim CLM-5518); regresi Amoxicillin untuk Siti →
  422 (alergi Penisilin); antrean baru A-045; resep Paracetamol 201.
- Cabang `ON CONFLICT (idempotency_key) DO NOTHING` di
  `enqueue_outbox`: dieksekusi di PG nyata lewat test
  `outbox::tests::enqueue_is_idempotent_on_key` (enqueue 2x kunci
  sama → `true` lalu `false`, 1 baris) — bagian dari suite PG.
- `persistent=false` di mode Postgres (lihat `waras-db/src/lib.rs`):
  tetap berlaku. Biayanya satu round-trip Parse per query; pada uji
  beban §3 latensi median tulis ~0,6–0,7 dtk di sandbox yang
  CPU-nya berbagi dengan agen lain (bukan angka batas yang bersih),
  jadi tidak ada pengukuran terisolasi yang membuktikan ini masalah
  atau bukan. Tidak diubah: alasannya bug sqlx 0.8.6 (NULL float
  meracuni tipe parameter statement yang di-cache) masih berlaku.

## 2. Yang diperbaiki (3 bug nyata, semua ditemukan lewat PG)

### 2.1 `SUM(amount)` → PG Numeric, dasbor 500
`SUM` atas kolom BIGINT di Postgres mengembalikan `numeric`, bukan
`bigint`; sqlx Any tidak bisa decode Numeric ke `i64`, jadi
`GET /api/dashboard` 500 di mode Postgres (pesan: `error occurred
while decoding column coalesce: ... Any driver does not support the
Postgres type PgTypeInfo(Numeric)`). Tiga tempat kena pola yang sama:
`dashboard.rs` (`SUM(amount)` tagihan belum lunas),
`invoices.rs` (`SUM(amount)` pembayaran, dipakai `pay_invoice`),
`casemix.rs` (`SUM(CASE ... THEN amount ...)` klaim dibayar).
Perbaiki dengan `CAST(COALESCE(SUM(amount), 0) AS BIGINT)` — CAST
standar, jalan di SQLite maupun PG. `SUM(CASE ... THEN 1 ELSE 0
END)` tidak kena (literal int → `bigint`, aman).

### 2.2 Pencarian pasien case-sensitive di PG
SQLite `LIKE` case-insensitive (ASCII), PostgreSQL `LIKE`
case-sensitive. Terbukti di psql: `name LIKE '%siti%'` → 0 baris,
`ILIKE` → 1 baris (Siti Rahmawati). `list_patients` kini memakai
`LOWER(name) LIKE LOWER(?) OR LOWER(nik) ... OR LOWER(id) ...`
(jalan di kedua backend). Tambah test
`patients::tests::search_is_case_insensitive` (cari `siti`/`SITI`/
`Siti`/`sItI rAhMaWaTi`, NIK, `rm-000812`).

### 2.3 Tabrakan PK saat tulis bersamaan (beban 20 pengguna)
`take_queue`, `prescribe_as`, dan `enqueue_outbox` menghitung id
berikutnya dari MAX lalu INSERT tanpa transaksi. Di SQLite satu
writer hal ini jarang terlihat; di PG (pool 5 koneksi) 20 penulis
bersamaan menghitung id yang sama. Uji beban pertama: 59/100 ambil
antrean + 61/100 resep gagal 500 `duplicate key value violates
unique constraint "queue_entries_pkey"` (terbukti verbatim dari
bodi respons).
Perbaikan bertahap:
(a) retry maks 5x pada pelanggaran unique saja (helper
`is_unique_violation_msg` di `lib.rs`: pesan PG `duplicate key
value violates unique constraint` / SQLite `UNIQUE constraint
failed`); error lain diteruskan. Hasil: 91/100 + 91/100 — membaik
tapi belum tuntas, karena 20 thread tersinkron: tiap ronde retry
hanya 1 yang lolos, jadi butuh ~20 ronde, bukan 5.
(b) Perbaiki di akar: `Db` kini membawa `alloc_lock`
(`Arc<tokio::sync::Mutex<()>>`) yang dipegang `take_queue_once` /
`prescribe_as_once` / `enqueue_outbox_once` selama hitung-max +
INSERT (+ audit resep), sehingga alokasi id atomik antar task dalam
satu proses. Retry 5x tetap ada sebagai jaring pengaman untuk
tabrakan antar proses. Tidak pernah bersarang (`log_audit` tidak
mengambil kunci ini); worker outbox + API berbagi `Db` yang sama
(`Arc`) jadi berbagi kunci yang sama. Hasil: 200/200 (§3).

## 3. Uji beban (20 pengguna bersamaan, server :3102, DB waras_pg)

Skrip `~/workspace/waras/.fase-a-scratch/a2-pg-load.py`: 20 thread ×
(5 antrean + 5 resep Paracetamol) = 200 tulis.

- SEBELUM perbaikan §2.3: queue 41/100 ok (59× HTTP 500),
  resep 39/100 ok (61× HTTP 500), 200 tulis / 9,0 dtk
  (22,2 tulis/detik); p50 queue 589 ms, p95 2187 ms, max 2509 ms;
  p50 resep 702 ms, p95 1393 ms, max 1422 ms. Latensi terukur saat
  CPU sandbox berbagi dengan agen lain (bukan angka batas bersih).
- Setelah retry 5x saja: queue 91/100, resep 91/100 (9× HTTP 500
  tiap jenis, masih pelanggaran PK), 200 tulis / 13,4 dtk.
- SESUDAH `alloc_lock` (§2.3b): **queue 100/100, resep 100/100,
  200 tulis / 5,0 dtk (40,0 tulis/detik)**; p50 queue 281 ms,
  p95 1462 ms, max 1603 ms; p50 resep 506 ms, p95 607 ms,
  max 677 ms. Latensi tetap terukur saat CPU sandbox berbagi
  dengan agen lain (bukan angka batas bersih).

Catatan: angka di atas BUKAN batas kapasitas (Fase C/C2 yang
mengukur itu). Yang dibuktikan di sini: tidak ada lagi 500 akibat
tabrakan id pada beban 20 pengguna.

## 4. Test suite: SQLite vs Postgres (hitungan TERPISAH)

Satu suite test, dua backend — semua test DB kini memakai
`Db::connect_for_tests()` (`waras-db/src/lib.rs`): bila env
`WARAS_TEST_PG_URL` diisi, tiap test mendapat database Postgres
fresh sendiri (`waras_t<pid>_<n>`, dibuat lewat koneksi admin ke
database `postgres`); bila kosong, SQLite in-memory seperti biasa.
Dua test dikecualikan sengaja: fallback di `connect_for_tests`
sendiri, dan `db_mode_is_sqlite_by_default` (membuktikan default
tetap SQLite). `postgres_smoke_via_env` (lama) tetap ada.

Perintah:
```
cargo test --offline                                        # SQLite
WARAS_TEST_PG_URL=postgres://waras:waras@127.0.0.1:5432/waras_pg_test cargo test --offline   # Postgres
```

- SQLite (21 Nov, kode pra-perbaikan §2.1–§2.2, 228 test):
  bin 0, api 82, bridge 35, core 32, db 76, web 3 — **228 lolos,
  0 gagal** (EXIT=0). Durasi membengkak karena CPU sandbox berbagi
  (api 731 dtk, db 648 dtk); bukan indikasi lambatnya aplikasi.
  Run ulang SQLite pada kode final (229 test, termasuk
  `search_is_case_insensitive`): bin 0, api 82, bridge 35, core 32,
  db 77, web 3 — **229 lolos, 0 gagal** (EXIT=0; api 282 dtk,
  db 201 dtk). Jadi total yang sama di kedua backend.
- Postgres (21 Nov, kode final, 229 test): bin 0, api 82, bridge 35,
  core 32, db 77, web 3 — **229 lolos, 0 gagal** (EXIT=0).
  Termasuk `search_is_case_insensitive`,
  `enqueue_is_idempotent_on_key` (cabang ON CONFLICT di PG nyata),
  dan `postgres_smoke_via_env`. Run pertama sempat 81/82 karena
  `health_is_public` assert mode `"sqlite"` harfiah — bukan bug
  aplikasi (server benar mengembalikan `"postgres"`); test dibuat
  backend-aware (`mode == db.mode()`), lalu run kedua hijau penuh.
  Durasi: api 335 dtk, db 265 dtk (CPU sandbox berbagi).

Database test `waras_t*` tidak di-drop otomatis; bersihkan berkala:
```sql
SELECT 'DROP DATABASE "' || datname || '";' FROM pg_database
 WHERE datname LIKE 'waras_t%';
```
lalu jalankan hasilnya sebagai superuser.

## 5. Perubahan KPI / data seed

Tidak ada perubahan semantik seed. Data yang BERUBAH di `waras_pg`
(cluster kedua, setelah install ulang) hanya akibat alur tulis
verifikasi §1 + uji beban §3 (bukan akibat perbaikan kode):
antrean 44 → 250 (A-045 flow, +91 beban retry, +14 probe, +100 beban
final), resep 8 → 201 (RX-1048 Diblokir Amoxicillin regresi,
RX-1049 Paracetamol, +91 +100 beban), lab order 8 → 9 (LAB-2218
KRITIS — `lab_critical` dasbor 1 → 2 di DB ini), klaim 9 → 10
(CLM-5518), IGD 8 → 9, admisi 7 → 8 (ADM-...-008, sudah
dipulangkan), radiologi 7 → 8 (RAD-0918), outbox 0 → 185 (worker
men-queue event resep yang dibuat saat beban — bukan seed).

## 6. Yang masih terbuka / batasan PG

- Pola alokasi id baca-max-lalu-insert yang SAMA masih ada di jalur
  lain (pasien RM-######, encounter ENC-..., tagihan INV-...,
  klaim CLM-..., admisi, order lab/radiologi, operasi, persalinan,
  HD, transfusi, staf, shift, kunci API): tidak di-retry, karena
  tidak diuji bersamaan di kriteria A2. Bila pilot rumah sakit
  menulis jalur itu bersamaan, terapkan pola retry yang sama
  (atau kunci advisory / sequence) — JANGAN anggap sudah aman.
- `persistent=false` (§1): satu Parse per query di PG; belum ada
  pengukuran terisolasi apakah ini berarti di beban nyata.
  Perbaikan sesungguhnya (ganti sqlx / tunggu upstream) di luar A2.
- Rate limit API publik + worker outbox tetap in-memory (catatan
  lama, bukan spesifik PG).
- Uji beban di sini 20 pengguna / 200 tulis di sandbox berbagi CPU;
  batas kapasitas per mode (SQLite vs Postgres) adalah Fase C/C2.
- Release build OK (17 mnt 38 dtk, biner 15,7 MB) dan biner release
  di-boot di :3103 terhadap `waras_pg` (health `mode: "postgres"`,
  login dokter OK) lalu dimatikan lagi; server :3102 yang dibiarkan
  jalan memakai biner DEBUG.

## 7. Verifikasi akhir (koordinator — ulangi sebelum integrasi)

```
# Bila VM sandbox habis diganti (PG hilang): bash ~/workspace/waras/.fase-a-scratch/reinstall-pg.sh
sudo pg_ctlcluster 16 main start   # lalu cek: pg_lsclusters
WARAS_TEST_PG_URL=postgres://waras:waras@127.0.0.1:5432/waras_pg_test cargo test --offline   # Postgres
cargo test --offline               # SQLite
cargo build --release --offline
WARAS_PORT=3102 WARAS_DATABASE_URL=postgres://waras:waras@127.0.0.1:5432/waras_pg ./target/release/waras
curl http://127.0.0.1:3102/api/health   # mode postgres
bash ~/workspace/waras/.fase-a-scratch/a2-pg-flow.sh    # 15/15
python3 ~/workspace/waras/.fase-a-scratch/a2-pg-load.py # 200/200
```
