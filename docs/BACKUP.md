# BACKUP.md — Runbook Backup & Restore WARAS (Fase A, item A3)

> Berlaku untuk **mode SQLite** (mode klinik, default). Mode PostgreSQL
> (mode rumah sakit, `WARAS_DATABASE_URL=postgres://...`) TIDAK dicakup
> skrip ini — lihat [§ PostgreSQL](#postgresql-mode-rumah-sakit) di bawah.

## Ringkas

| | |
|---|---|
| Backup | `scripts/waras-backup.sh` — harian, otomatis, di luar direktori data |
| Restore | `scripts/waras-restore.sh <file-backup>` — satu perintah |
| Jadwal | systemd timer `waras-backup.timer` (02:30, `Persistent=true`), atau cron |
| Retensi | 7 backup terakhir (default, `WARAS_BACKUP_KEEP`) |
| Metode | `sqlite3 .backup` (API backup online SQLite) — aman walau server berjalan |
| Verifikasi | tiap backup dicek `PRAGMA integrity_check` (= `ok`) + sha256 |

## Kenapa bukan `cp` mentah

WARAS membuka SQLite tanpa WAL (`crates/waras-db/src/lib.rs` memakai
`sqlite:{path}?mode=rwc`, tanpa `PRAGMA journal_mode=WAL` — journal
default `delete`). `cp` atas file DB yang sedang ditulis server bisa
menyalin file di tengah transaksi (hasilnya DB korup atau setengah
tulis). `sqlite3 .backup` memakai API backup online SQLite: ia membaca
lewat transaksi baca yang konsisten dan ikut menyalin halaman yang
masih di journal, jadi hasilnya selalu snapshot yang konsisten walau
server sedang menulis. Itu yang dipakai `waras-backup.sh`; bila CLI
`sqlite3` tidak ada, skrip memakai API yang sama lewat modul sqlite3
Python (`Connection.backup()`).

## Instalasi

Di server produksi (contoh path instalasi `/opt/waras`, user `waras`,
backup di `/var/backups/waras` — sesuaikan dengan instalasi Anda):

```sh
# 1. Pastikan skrip ada di instalasi (ikut repo, di scripts/).
ls /opt/waras/scripts/waras-backup.sh /opt/waras/scripts/waras-restore.sh

# 2. Direktori backup: di LUAR direktori data, di disk yang sama
#    (atau lebih baik: mount/disk terpisah). User waras harus bisa tulis.
sudo mkdir -p /var/backups/waras
sudo chown waras:waras /var/backups/waras

# 3. Uji sekali jalan manual (server boleh tetap berjalan):
sudo -u waras WARAS_DB=/opt/waras/waras.db \
    WARAS_BACKUP_DIR=/var/backups/waras \
    /opt/waras/scripts/waras-backup.sh
ls -l /var/backups/waras/
```

Nilai default bila env tidak diisi:

| Variabel | Default |
|---|---|
| `WARAS_DB` | `./waras.db` (relatif terhadap working directory) |
| `WARAS_BACKUP_DIR` | `<dir-db>/../waras-backups` (selalu di luar direktori data) |
| `WARAS_BACKUP_KEEP` | `7` |

Skrip menolak jalan bila direktori backup sama dengan direktori data,
bila DB tidak ditemukan, dan bila `WARAS_DATABASE_URL` menunjuk
PostgreSQL. Keluar 0 hanya bila backup baru terverifikasi dan tersimpan;
bukan 0 bila ada yang gagal — jadi aman dipakai di cron/systemd dan
kegagalannya terlihat.

## Penjadwalan

### Pilihan utama: systemd timer (disarankan)

```sh
sudo cp scripts/waras-backup.service scripts/waras-backup.timer /etc/systemd/system/
# Sesuaikan dulu User/WorkingDirectory/WARAS_DB/WARAS_BACKUP_DIR di
# waras-backup.service dengan instalasi Anda.
sudo systemctl daemon-reload
sudo systemctl enable --now waras-backup.timer
sudo systemctl list-timers waras-backup.timer   # cek jadwal berikut
sudo systemctl start waras-backup.service       # uji sekali jalan manual
sudo journalctl -u waras-backup.service -n 50   # lihat log jalan itu
```

Timer jalan tiap hari 02:30 waktu lokal (`OnCalendar=*-*-* 02:30:00`,
jeda acak sampai 5 menit). `Persistent=true` berarti bila server mati
tepat pada jam jadwal, backup yang tertinggal dijalankan saat boot
berikut — cron tidak punya ini.

### Alternatif: cron

```cron
# Backup harian WARAS, 02:30. Log ke file agar kegagalan terlihat.
30 2 * * * waras WARAS_DB=/opt/waras/waras.db WARAS_BACKUP_DIR=/var/backups/waras /opt/waras/scripts/waras-backup.sh >> /var/log/waras-backup.log 2>&1
```

## Restore (satu perintah)

**Hentikan dulu service WARAS.** Mengganti file DB di bawah server yang
berjalan tidak aman: server memegang file descriptor ke inode lama.

```sh
# 1. Hentikan service.
sudo systemctl stop waras

# 2. Pilih backup (yang terbaru, atau tanggal tertentu).
ls -lt /var/backups/waras/waras-*.db

# 3. Restore satu perintah. Skrip akan:
#    - menolak jalan bila server masih terdeteksi berjalan,
#    - memverifikasi backup (header SQLite + integrity_check + sha256),
#    - menyimpan salinan pra-restore DB aktif
#      (<db>.pre-restore-YYYYMMDD-HHMMSS),
#    - mengganti DB lewat rename atomik, lalu menghapus -wal/-shm sisa.
sudo -u waras WARAS_DB=/opt/waras/waras.db \
    /opt/waras/scripts/waras-restore.sh /var/backups/waras/waras-20261121-023000.db

# 4. Nyalakan lagi dan verifikasi.
sudo systemctl start waras
curl -fsS http://127.0.0.1:3000/api/health
# Login dengan akun seed/staf, buka dashboard, cek KPI + satu pasien
# dan satu antrean yang Anda kenal datanya.
```

Bila backup rusak (verifikasi gagal), skrip keluar bukan 0 dan **DB
aktif tidak disentuh** — pilih backup satu hari sebelumnya dan ulangi.
Bila restore sudah terverifikasi lewat login + KPI, salinan
`.pre-restore-*` boleh dihapus manual.

### Bila DB hilang total (tidak ada file DB aktif)

`waras-restore.sh` tetap jalan walau file DB aktif sudah hilang —
dalam kasus itu tidak ada salinan pra-restore (tidak ada yang
disalin), dan skrip mencatat peringatan itu dengan jelas. Backup yang
terverifikasi adalah satu-satunya sumber data, jadi verifikasi
(`integrity_check` + sha256) tetap jalan penuh sebelum DB ditulis.

## Retensi & penyimpanan

- Default 7 backup terakhir (satu per hari = seminggu ke belakang).
  Ubah lewat `WARAS_BACKUP_KEEP` (di service: `Environment=`).
- Tiap backup adalah satu file `waras-YYYYMMDD-HHMMSS.db` + file
  `.sha256` pendamping (hash ditulis saat backup dibuat, dicek saat
  restore).
- Backup lama di-prune otomatis oleh `waras-backup.sh` setiap kali
  backup baru berhasil. Prune yang gagal tidak menggagalkan backup
  baru, tapi dicatat di log — periksa log bila jumlah file terus
  bertambah.
- **Backup di disk yang sama bukan pemulihan bencana.** Untuk bencana
  (disk rusak, server hilang), salin `/var/backups/waras` ke luar
  server secara berkala (rsync ke server lain / object storage).
  Itu di luar skrip ini dan menjadi bagian Fase C (monitoring +
  runbook bencana).

## Bila backup gagal

1. **Baca log dulu.** Timer: `journalctl -u waras-backup.service -n 100`.
   Cron: `/var/log/waras-backup.log`. Pesan `ERROR:` menyebut
   penyebabnya.
2. **DB tidak ditemukan** — cek `WARAS_DB`: path salah, working
   directory service berubah, atau file DB terhapus. Jangan restore
   dari backup sebelum memastikan DB mana yang hilang (lihat
   [§ Bila DB hilang total](#bila-db-hilang-total-tidak-ada-file-db-aktif)).
3. **`integrity_check` gagal** — DB aktif kemungkinan sudah korup.
   Jangan tunggu: hentikan service, periksa disk (`df -h`, `dmesg`),
   restore dari backup terakhir yang lolos verifikasi, lalu selidiki
   penyebab (disk penuh adalah penyebab paling umum).
4. **Direktori backup tidak bisa ditulis** — cek pemilik & izin
   (`ls -ld /var/backups/waras`), dan sisa ruang disk (`df -h`).
   Backup gagal karena disk penuh harus ditindak hari itu juga:
   tanpa backup baru, retensi 7 hari Anda menipis satu hari setiap hari.
5. **Mode PostgreSQL aktif** — skrip menolak dengan pesan yang jelas.
   Itu bukan error: pakai `pg_dump` (lihat bawah), jangan paksa skrip
   SQLite.
6. **Tidak ada `sqlite3` maupun `python3`** — instal salah satunya
   (`apt install sqlite3`). Tanpa keduanya skrip tidak bisa backup
   dengan aman dan sengaja menolak jalan.

Aturan umum: **backup yang gagal itu darurat ringan, bukan darurat
berat** — data masih ada di DB aktif. Yang darurat berat adalah backup
gagal berhari-hari tanpa ada yang sadar. Karena itu periksa hasil
backup secara berkala (lihat [§ Pemeriksaan berkala](#pemeriksaan-berkala)).

## Pemeriksaan berkala

- **Mingguan:** `ls -lt /var/backups/waras/ | head` — pastikan ada
  file baru tiap hari, dan `systemctl list-timers waras-backup.timer`
  menunjukkan `NEXT` di masa depan.
- **Bulanan (wajib): latihan restore.** Ikuti
  [§ Latihan restore (drill)](#latihan-restore-drill) di bawah pada
  salinan/scratch, bukan di DB produksi. Backup yang belum pernah
  di-restore adalah backup yang belum terbukti.

## Latihan restore (drill)

Prosedur yang dipakai untuk membuktikan A3 (dan yang harus diulang
tiap bulan):

1. Boot server di DB scratch kosong (seed jalan), tulis data baru
   (1 pasien + 1 antrean + 1 resep), catat snapshot SEBELUM (jumlah
   pasien/antrean/resep + satu baris resep + KPI dashboard).
2. Backup **dengan server masih berjalan** (membuktikan backup aman
   terhadap server live).
3. Matikan server, **hapus DB**, restore dengan satu perintah.
4. Boot lagi, login, bandingkan snapshot SESUDAH dengan SEBELUM —
   harus identik.

Skrip drill (tidak ikut rilis, hanya untuk latihan):
`~/workspace/waras/.fase-a-scratch/a3/drill-a3.sh` di worktree
pengembangan — jalankan dari worktree `a3-backup` setelah
`cargo build --release`.

### Hasil drill 21 Nov 2026 (sandbox, pertama kali)

Drill dijalankan 21 Nov 2026 ~04:08 WIB (20 Nov 21:08 UTC) di sandbox,
worktree `a3-backup`, binary release dari commit yang sama dengan
skrip (tidak ada perubahan kode aplikasi). Skrip drill:
`~/workspace/waras/.fase-a-scratch/a3/drill-a3.sh` (tidak ikut rilis).
Log lengkap: `~/workspace/waras/.fase-a-scratch/a3/drill-a3.log`.

- Boot 1 (DB scratch kosong, seed): `/api/health` OK setelah 94 dtk
  (seed lambat di sandbox 1,5 CPU karena hash Argon2 — di VPS akan
  jauh lebih cepat).
- Data drill: pasien baru `RM-000826` (Drill Pasien A3), antrean baru
  `A-045`, resep `RX-1048` Azithromycin 500 mg DRILL-A3 (encounter
  seed `ENC-2026-11-19-014`, lolos safety). Login seed
  `ahmad`/`waras123` — tidak diubah.
- Snapshot SEBELUM: `patients=15 queue=45 rx=9 audit=10`,
  `RX-1048|Azithromycin 500 mg DRILL-A3|aktif`,
  dashboard `queue_total=45 patients_today=248`.
- Backup diambil **dengan server masih berjalan** (1 detik, 508K,
  `integrity_check` ok, `pasien/antrean/resep = 15/45/9`).
- Server dimatikan, DB dihapus, restore satu perintah:
  `waras-restore.sh` memverifikasi backup (`integrity_check` ok +
  sha256 cocok), mencatat tidak ada salinan pra-restore (DB sudah
  hilang — perilaku yang diharapkan), rename atomik, OK dalam 1 detik.
- Boot 2: health OK setelah 2 dtk (tidak perlu seed ulang), login
  seed OK.
- Snapshot SESUDAH: **identik** — `patients=15 queue=45 rx=9
  audit=10`, `RX-1048|Azithromycin 500 mg DRILL-A3|aktif`,
  `queue_total=45 patients_today=248`. **DRILL LULUS, tanpa kehilangan
  data.**

Catatan dari tiga kali percobaan drill (dua gagal, satu lulus) — semua
kegagalan ada di skrip/drill, bukan di data:
1. Nama tabel resep adalah `medication_requests`, bukan
   `prescriptions` — snapshot sempat kosong. Diperbaiki di kedua skrip
   + drill.
2. `waras-restore.sh` awalnya menolak bila DB aktif tidak ada — padahal
   drill (dan bencana nyata) justru menghapus DB. Sekarang restore
   tetap jalan dengan peringatan bila DB hilang.
3. Deteksi server `pgrep -f waras` terlalu luas (cocok dengan `cargo
   test` di worktree lain + skrip drill sendiri) dan menolak restore
   yang aman. Sekarang hanya `pgrep -x waras` + `$WARAS_PID_FILE`.
4. Hitungan `audit_log` SESUDAH harus diambil sebelum login verifikasi
   (login menulis 1 entri `auth.login` ke audit yang append-only).

## PostgreSQL (mode rumah sakit)

Skrip di sini khusus SQLite. Untuk Postgres, backup yang benar adalah
`pg_dump` (format custom, konsisten walau server berjalan):

```sh
# Harian, lewat cron/timer yang sama polanya:
pg_dump -Fc -d "$WARAS_DATABASE_URL" -f /var/backups/waras/waras-pg-$(date +%Y%m%d-%H%M%S).dump
# Restore (DB kosong/dihapus dulu):
pg_restore -d "$WARAS_DATABASE_URL" /var/backups/waras/waras-pg-<tanggal>.dump
```

Skrip `pg_dump` terjadwal menjadi bagian A2 (mode PostgreSQL
dibuktikan) — jangan campur backup SQLite dan Postgres dalam satu
direktori tanpa penanda nama yang jelas.

## Batasan yang diketahui

- Backup hanya menyalin **file DB SQLite**. File lain (log, konfigurasi
  `waras.env`, kunci) tidak ikut — cadangkan terpisah bila perlu.
- Retensi dihitung dari file `waras-*.db` di direktori backup; file
  backup yang dipindah manual ke nama lain tidak ikut prune.
- `waras-restore.sh` mendeteksi server berjalan secara best-effort
  (proses bernama persis `waras`, atau PID di `$WARAS_PID_FILE` bila
  diisi); operator tetap bertanggung jawab memastikan service
  berhenti sebelum restore.
- Seed WARAS lambat di mesin kecil (hash Argon2; di sandbox 1,5 CPU
  butuh ~65 detik sampai `/api/health` OK). Itu waktu boot, bukan
  waktu backup/restore — backup/restore sendiri hitungan detik.
