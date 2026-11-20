// Laporan & SIRS: GET /api/laporan/kunjungan?from=&to=,
// GET /api/laporan/penyakit?limit=, GET /api/laporan/sirs (FASE4.md §5.1).
// Grafik kunjungan = bar dari div (.bars/.bar-col di app.css, dipakai juga
// dasbor) — BUKAN library chart. Angka dari API, bukan dari layar desain.
(function () {
  'use strict';

  function showErr(id, msg) {
    var el = document.getElementById(id);
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }

  function dayShort(iso) {
    var d = String(iso || '').slice(0, 10).split('-');
    if (d.length !== 3) return String(iso || '');
    return Number(d[2]) + '/' + Number(d[1]);
  }

  async function loadVisits() {
    var from = document.getElementById('range-from').value;
    var to = document.getElementById('range-to').value;
    var box = document.getElementById('visit-bars');
    try {
      var list = Waras.asList(await Waras.api('/api/laporan/kunjungan?from=' + encodeURIComponent(from) + '&to=' + encodeURIComponent(to)));
      if (!list.length) {
        box.innerHTML = '<p class="empty-note">Tidak ada kunjungan pada rentang ' + Waras.esc(from) + ' s.d. ' + Waras.esc(to) + '.</p>';
        return;
      }
      var max = 0;
      list.forEach(function (row) {
        var v = Number(row.visits != null ? row.visits : row.count) || 0;
        if (v > max) max = v;
      });
      if (max === 0) max = 1;
      box.innerHTML = list.map(function (row) {
        var v = Number(row.visits != null ? row.visits : row.count) || 0;
        var h = Math.max(4, Math.round((v / max) * 140));
        return '<div class="bar-col"><div class="bar-val">' + Waras.fmtNum(v) + '</div>' +
          '<div class="bar" style="height:' + h + 'px"></div>' +
          '<div class="bar-day">' + dayShort(row.date) + '</div></div>';
      }).join('');
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Kunjungan belum tersedia (endpoint /api/laporan/kunjungan belum aktif).</p>';
    }
  }

  async function loadDiseases() {
    var tbody = document.getElementById('disease-body');
    try {
      var list = Waras.asList(await Waras.api('/api/laporan/penyakit?limit=10'));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="3" class="empty-note">Belum ada data penyakit.</td></tr>';
        return;
      }
      tbody.innerHTML = list.map(function (row) {
        return '<tr><td>' + Waras.esc(row.code || '') + '</td><td>' + Waras.esc(row.display || '') +
          '</td><td class="num">' + Waras.fmtNum(row.count || 0) + '</td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="3" class="empty-note">10 besar penyakit belum tersedia (endpoint /api/laporan/penyakit belum aktif).</td></tr>';
    }
  }

  async function loadSirs() {
    var box = document.getElementById('sirs-cards');
    try {
      var s = await Waras.api('/api/laporan/sirs');
      var sirs = s.sirs || s;
      var bor = sirs.beds_total ? ((Number(sirs.beds_occupied || 0) * 100 / Number(sirs.beds_total)).toLocaleString('id-ID', { maximumFractionDigits: 1 }) + '%') : '—';
      var rows = [
        ['RL1 — Data dasar', 'Kunjungan hari ini: ' + Waras.fmtNum(sirs.visits_today || 0) + ' (rawat jalan ' + Waras.fmtNum(sirs.outpatient_today || 0) + ', rawat inap ' + Waras.fmtNum(sirs.inpatient_today || 0) + ')'],
        ['RL2 — 10 besar penyakit', (function () {
          var top = sirs.top_diseases || [];
          if (Array.isArray(top) && top.length && Array.isArray(top[0])) {
            return top.slice(0, 3).map(function (t) { return (t[1] || t[0]) + ' (' + Waras.fmtNum(t[2] || 0) + ')'; }).join(', ');
          }
          return 'Lihat tabel 10 besar penyakit.';
        })()],
        ['RL3 — Kegiatan pelayanan', 'BOR ' + bor + ' (' + Waras.fmtNum(sirs.beds_occupied || 0) + '/' + Waras.fmtNum(sirs.beds_total || 0) + ' bed) · Lab menunggu ' + Waras.fmtNum(sirs.lab_pending || 0)]
      ];
      box.innerHTML = rows.map(function (r) {
        return '<div style="padding:10px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' + Waras.esc(r[0]) + '</strong><br>' +
          '<span class="muted">' + Waras.esc(r[1]) + '</span> <span style="float:right">' + Waras.statusPill('Siap', 'green') + '</span></div>';
      }).join('') + '<p class="muted small" style="margin:10px 0 0">RL1/RL2/RL3 dihitung dari tabel nyata (kunjungan, kondisi, bed, lab).</p>';
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Ringkasan SIRS belum tersedia (endpoint /api/laporan/sirs belum aktif).</p>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadVisits();
    loadDiseases();
    loadSirs();
    document.getElementById('range-form').addEventListener('submit', function (ev) {
      ev.preventDefault();
      var from = document.getElementById('range-from').value;
      var to = document.getElementById('range-to').value;
      if (from && to && from > to) {
        showErr('range-error', 'Tanggal awal tidak boleh setelah tanggal akhir.');
        return;
      }
      document.getElementById('range-error').classList.remove('on');
      loadVisits();
    });
  });
})();
