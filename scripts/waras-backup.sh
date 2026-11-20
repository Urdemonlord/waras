#!/usr/bin/env bash
# waras-backup.sh — backup SQLite WARAS yang aman terhadap server yang
# sedang berjalan (online backup API, bukan `cp` mentah).
#
# Penggunaan:
#   scripts/waras-backup.sh [DB_PATH] [BACKUP_DIR]
#   WARAS_DB=/opt/waras/waras.db WARAS_BACKUP_DIR=/var/backups/waras \
#       scripts/waras-backup.sh
#
# Env:
#   WARAS_DB          path basis data SQLite (default: ./waras.db)
#   WARAS_BACKUP_DIR  direktori backup (default: <dir-db>/../waras-backups,
#                     SELALU di luar direktori data)
#   WARAS_BACKUP_KEEP jumlah backup terakhir yang disimpan (default: 7)
#   WARAS_BACKUP_LOG  file log tambahan (opsional; selain stdout)
#
# Metode: `sqlite3 <db> ".backup <tmp>"` — API backup online SQLite:
# membaca lewat transaksi baca yang konsisten walau server sedang
# menulis, dan ikut menyalin halaman WAL (bila ada). Bila CLI sqlite3
# tidak ada, fallback ke API backup modul sqlite3 Python (cara yang
# sama, `Connection.backup()`). Backup lalu diverifikasi
# (`PRAGMA integrity_check` harus `ok`) SEBELUM dipindah ke nama
# akhirnya, dan dicatat sha256-nya di file `.sha256` pendamping.
# Backup lama di-prune sampai tinggal WARAS_BACKUP_KEEP terakhir.
#
# Keluar 0 bila backup baru terverifikasi dan tersimpan; bukan 0 bila
# ada yang gagal (DB hilang, backup gagal, verifikasi gagal, prune
# gagal tidak menggagalkan backup baru tapi dicatat di log).
#
# Mode PostgreSQL (WARAS_DATABASE_URL=postgres://...) TIDAK didukung
# skrip ini — pakai pg_dump (lihat docs/BACKUP.md).

set -u
# set -e sengaja TIDAK dipakai: tiap langkah dicek eksplisit supaya
# pesan kesalahannya jelas dan file setengah jadi tidak tertinggal.

PROG="$(basename "$0")"

log() { printf '%s [%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S %Z')" "$PROG" "$*" ; }
die() { log "ERROR: $*" >&2; exit 1; }

DB="${1:-${WARAS_DB:-waras.db}}"
DB_DIR="$(dirname "$DB")"
[ "$DB_DIR" = "." ] && DB_DIR="$PWD"
BACKUP_DIR="${2:-${WARAS_BACKUP_DIR:-$DB_DIR/../waras-backups}}"
KEEP="${WARAS_BACKUP_KEEP:-7}"

# --- validasi ---------------------------------------------------------------
case "$KEEP" in
    ''|*[!0-9]*|0) die "WARAS_BACKUP_KEEP harus bilangan bulat >= 1 (dapat: '$KEEP')" ;;
esac

[ -f "$DB" ] || die "basis data tidak ditemukan: $DB"
case "${WARAS_DATABASE_URL:-}" in
    postgres://*|postgresql://*)
        die "mode PostgreSQL aktif (WARAS_DATABASE_URL); skrip ini hanya untuk SQLite — pakai pg_dump, lihat docs/BACKUP.md" ;;
esac

# Normalisasi path (tanpa realpath -m yang tidak ada di semua sistem).
DB_ABS="$(cd "$DB_DIR" && pwd)/$(basename "$DB")"
mkdir -p "$BACKUP_DIR" || die "tidak bisa membuat direktori backup: $BACKUP_DIR"
BACKUP_DIR_ABS="$(cd "$BACKUP_DIR" && pwd)"
[ "$BACKUP_DIR_ABS" != "$(dirname "$DB_ABS")" ] \
    || die "direktori backup ($BACKUP_DIR_ABS) sama dengan direktori data — backup harus di luar direktori data"

# --- backup -----------------------------------------------------------------
STAMP="$(date '+%Y%m%d-%H%M%S')"
NAME="waras-$STAMP.db"
DEST="$BACKUP_DIR_ABS/$NAME"
TMP="$BACKUP_DIR_ABS/.waras-$STAMP.db.tmp.$$"

log "memulai backup: $DB_ABS -> $DEST (simpan $KEEP terakhir)"

if command -v sqlite3 >/dev/null 2>&1; then
    # `.backup` = sqlite3_backup API: aman terhadap server yang berjalan.
    if ! sqlite3 "$DB_ABS" ".backup '$TMP'"; then
        rm -f "$TMP"
        die "sqlite3 .backup gagal"
    fi
elif command -v python3 >/dev/null 2>&1; then
    log "sqlite3 CLI tidak ada; fallback ke API backup sqlite3 Python"
    if ! python3 - "$DB_ABS" "$TMP" <<'PYEOF'
import sqlite3, sys
src = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
dst = sqlite3.connect(sys.argv[2])
try:
    src.backup(dst)
finally:
    dst.close(); src.close()
PYEOF
    then
        rm -f "$TMP"
        die "backup lewat Python sqlite3 gagal"
    fi
else
    die "tidak ada sqlite3 CLI maupun python3 — tidak bisa backup dengan aman"
fi

# --- verifikasi SEBELUM dipindah ke nama akhir ------------------------------
verify() {
    # $1 = file yang dicek; harus mengembalikan "ok" dan hanya itu.
    local f="$1" out
    if command -v sqlite3 >/dev/null 2>&1; then
        out="$(sqlite3 "$f" "PRAGMA integrity_check;" 2>&1)" \
            || { log "ERROR: integrity_check tidak bisa jalan: $out"; return 1; }
    else
        out="$(python3 -c 'import sqlite3,sys; print(sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True).execute("PRAGMA integrity_check").fetchone()[0])' "$f" 2>&1)" \
            || { log "ERROR: integrity_check tidak bisa jalan: $out"; return 1; }
    fi
    [ "$out" = "ok" ] || { log "ERROR: integrity_check = '$out' (bukan ok)"; return 1; }
    return 0
}

if ! verify "$TMP"; then
    rm -f "$TMP"
    die "backup gagal verifikasi integrity_check — file sementara dihapus, backup lama tidak disentuh"
fi

# Ukuran + jumlah baris kunci sebagai bukti isi (bukan sekadar file ada).
ROWS="$(sqlite3 "$TMP" "SELECT (SELECT COUNT(*) FROM patients) || '/' || (SELECT COUNT(*) FROM queue_entries) || '/' || (SELECT COUNT(*) FROM prescriptions);" 2>/dev/null || echo '?/?/?')"
mv "$TMP" "$DEST" || die "tidak bisa memindah backup ke $DEST"
sha256sum "$DEST" > "$DEST.sha256" || die "tidak bisa menulis $DEST.sha256"
SIZE="$(du -h "$DEST" | cut -f1)"
log "backup OK: $DEST ($SIZE, pasien/antrean/resep = $ROWS)"

# --- prune: simpan KEEP terakhir --------------------------------------------
# Urutkan berdasarkan nama (timestamp di nama = urutan kronologis).
# shellcheck disable=SC2012
COUNT="$(ls -1 "$BACKUP_DIR_ABS"/waras-*.db 2>/dev/null | wc -l)"
if [ "$COUNT" -gt "$KEEP" ]; then
    # shellcheck disable=SC2012
    ls -1 "$BACKUP_DIR_ABS"/waras-*.db | head -n "$((COUNT - KEEP))" | while read -r old; do
        rm -f "$old" "$old.sha256" && log "prune: hapus backup lama $old" \
            || log "WARNING: gagal menghapus backup lama $old"
    done
fi
log "selesai: $(ls -1 "$BACKUP_DIR_ABS"/waras-*.db 2>/dev/null | wc -l) backup tersimpan di $BACKUP_DIR_ABS"
exit 0
