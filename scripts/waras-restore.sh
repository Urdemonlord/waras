#!/usr/bin/env bash
# waras-restore.sh — restore satu perintah dari backup SQLite WARAS.
#
# Penggunaan:
#   scripts/waras-restore.sh <BACKUP_FILE> [DB_PATH]
#   WARAS_DB=/opt/waras/waras.db scripts/waras-restore.sh /var/backups/waras/waras-20261121-023000.db
#
# PENTING: hentikan dulu service WARAS sebelum restore (skrip ini
# menolak jalan bila mendeteksi server WARAS masih berjalan, kecuali
# diberi --force — jangan pakai --force di produksi). Mengganti file DB
# di bawah server yang berjalan TIDAK aman: server memegang file
# descriptor ke inode lama dan bisa menulis ke DB yang sudah diganti.
#
# Langkah: (1) verifikasi backup (header SQLite + `PRAGMA
# integrity_check` harus `ok` + sha256 bila ada file `.sha256`);
# (2) simpan salinan pra-restore DB yang sedang aktif
# (`<db>.pre-restore-YYYYMMDD-HHMMSS`); (3) ganti DB lewat rename
# atomik di direktori yang sama; (4) hapus file -wal/-shm sisa.
# Bila verifikasi gagal, DB aktif TIDAK disentuh dan skrip keluar
# bukan 0. Setelah restore, jalankan lagi server dan cek
# `/api/health` + login (lihat docs/BACKUP.md).
#
# Env: WARAS_DB (default: ./waras.db). Argumen DB_PATH mengalahkan env.

set -u

PROG="$(basename "$0")"

log() { printf '%s [%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S %Z')" "$PROG" "$*" ; }
die() { log "ERROR: $*" >&2; exit 1; }

FORCE=0
if [ "${1:-}" = "--force" ]; then FORCE=1; shift; fi
BACKUP="${1:-}"
DB="${2:-${WARAS_DB:-waras.db}}"

[ -n "$BACKUP" ] || die "penggunaan: $PROG [--force] <BACKUP_FILE> [DB_PATH]"
[ -f "$BACKUP" ] || die "file backup tidak ditemukan: $BACKUP"
[ -f "$DB" ] || die "basis data aktif tidak ditemukan: $DB (restore butuh DB lama untuk salinan pra-restore; bila DB hilang total, salin saja backup ke $DB lalu verifikasi)"

DB_DIR="$(dirname "$DB")"
[ "$DB_DIR" = "." ] && DB_DIR="$PWD"
DB_ABS="$(cd "$DB_DIR" && pwd)/$(basename "$DB")"
BACKUP_ABS="$(cd "$(dirname "$BACKUP")" && pwd)/$(basename "$BACKUP")"

# --- 0. server harus berhenti dulu ------------------------------------------
# Deteksi best-effort: proses yang command line-nya memuat path DB ini,
# atau biner waras apa pun. Bukan jaminan mutlak — operator tetap yang
# bertanggung jawab menghentikan service (lihat docs/BACKUP.md).
if command -v pgrep >/dev/null 2>&1; then
    RUNNING="$(pgrep -af "waras" 2>/dev/null | grep -v "$PROG" | grep -v "pgrep" || true)"
    if [ -n "$RUNNING" ]; then
        if [ "$FORCE" = 1 ]; then
            log "WARNING: --force: proses WARAS masih berjalan, lanjut atas risiko sendiri:"
            log "WARNING: $RUNNING"
        else
            log "ERROR: server WARAS tampaknya masih berjalan — hentikan dulu (systemctl stop waras), baru restore:"
            log "ERROR: $RUNNING"
            die "restore dibatalkan; pakai --force hanya bila Anda yakin server sudah mati"
        fi
    fi
fi

# --- 1. verifikasi backup ----------------------------------------------------
log "memverifikasi backup: $BACKUP_ABS"
# Header file SQLite: "SQLite format 3\0".
if ! head -c 16 "$BACKUP_ABS" | grep -q "SQLite format 3"; then
    die "bukan file SQLite (header tidak cocok) — DB aktif tidak disentuh"
fi

verify() {
    local f="$1" out
    if command -v sqlite3 >/dev/null 2>&1; then
        out="$(sqlite3 "$f" "PRAGMA integrity_check;" 2>&1)" \
            || { log "ERROR: integrity_check tidak bisa jalan: $out"; return 1; }
    elif command -v python3 >/dev/null 2>&1; then
        out="$(python3 -c 'import sqlite3,sys; print(sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True).execute("PRAGMA integrity_check").fetchone()[0])' "$f" 2>&1)" \
            || { log "ERROR: integrity_check tidak bisa jalan: $out"; return 1; }
    else
        die "tidak ada sqlite3 CLI maupun python3 untuk verifikasi"
    fi
    [ "$out" = "ok" ] || { log "ERROR: integrity_check = '$out' (bukan ok)"; return 1; }
    return 0
}

verify "$BACKUP_ABS" || die "backup RUSAK (integrity_check gagal) — DB aktif tidak disentuh"

if [ -f "$BACKUP_ABS.sha256" ]; then
    if (cd "$(dirname "$BACKUP_ABS")" && sha256sum -c "$(basename "$BACKUP_ABS").sha256" >/dev/null 2>&1); then
        log "sha256 cocok dengan $BACKUP_ABS.sha256"
    else
        die "sha256 TIDAK cocok dengan $BACKUP_ABS.sha256 — backup mungkin korup/diubah; DB aktif tidak disentuh"
    fi
else
    log "WARNING: tidak ada file .sha256 pendamping — lanjut hanya dengan integrity_check"
fi
ROWS="$(sqlite3 "$BACKUP_ABS" "SELECT (SELECT COUNT(*) FROM patients) || '/' || (SELECT COUNT(*) FROM queue_entries) || '/' || (SELECT COUNT(*) FROM prescriptions);" 2>/dev/null || echo '?/?/?')"
log "backup terverifikasi OK (pasien/antrean/resep = $ROWS)"

# --- 2. salinan pra-restore DB aktif -----------------------------------------
STAMP="$(date '+%Y%m%d-%H%M%S')"
PRESTORE="$DB_ABS.pre-restore-$STAMP"
cp "$DB_ABS" "$PRESTORE" || die "tidak bisa membuat salinan pra-restore $PRESTORE"
# Ikut salin -wal/-shm bila ada (mode journal default WARAS adalah
# delete, jadi normalnya tidak ada — tapi jangan buang data diam-diam).
[ -f "$DB_ABS-wal" ] && cp "$DB_ABS-wal" "$PRESTORE-wal"
[ -f "$DB_ABS-shm" ] && cp "$DB_ABS-shm" "$PRESTORE-shm"
log "salinan pra-restore disimpan: $PRESTORE"

# --- 3. ganti DB lewat rename atomik -----------------------------------------
TMP_RESTORE="$DB_ABS.restore-tmp.$$"
cp "$BACKUP_ABS" "$TMP_RESTORE" || { rm -f "$TMP_RESTORE"; die "tidak bisa menyalin backup ke direktori data"; }
# Verifikasi ulang salinan yang akan jadi DB aktif (paranoia murah).
verify "$TMP_RESTORE" || { rm -f "$TMP_RESTORE"; die "salinan restore gagal verifikasi — DB aktif tidak disentuh"; }
mv "$TMP_RESTORE" "$DB_ABS" || { rm -f "$TMP_RESTORE"; die "rename ke $DB_ABS gagal"; }
# File -wal/-shm lama milik DB sebelum restore tidak boleh tertinggal:
# isinya transaksi DB lama dan SQLite akan menganggapnya bagian dari
# DB yang baru.
rm -f "$DB_ABS-wal" "$DB_ABS-shm"
# Pastikan rename + hapusan tahan crash (fsync direktori).
python3 -c 'import os,sys; os.fsync(os.open(sys.argv[1], os.O_RDONLY))' "$DB_DIR" 2>/dev/null || true

log "restore OK: $DB_ABS <- $BACKUP_ABS (pasien/antrean/resep = $ROWS)"
log "salinan DB sebelum restore: $PRESTORE (hapus manual bila restore sudah terverifikasi)"
log "langkah berikut: nyalakan service (systemctl start waras), cek /api/health, lalu login dan cek KPI di dashboard"
exit 0
