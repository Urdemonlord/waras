// Rawat Jalan: GET /api/rawat-jalan?date= -> {visits:[OutpatientVisit...]}
// (FASE4.md §5.1). Satu baris per pasien+poli; sumber = Antrean /
// Encounter / Janji. Kartu poli dihitung dari kunjungan hari itu
// (jumlah pasien per poli) — layar desain menampilkan jadwal dokter +
// diagnosis, yang TIDAK ada di OutpatientVisit kontrak, jadi tidak
// ditampilkan (kontrak menang).
(function () {
  'use strict';

  var SOURCE_KIND = { antrean: 'teal', encounter: 'blue', janji: 'amber' };

  async function loadVisits() {
    var date = document.getElementById('rj-date').value || Waras.TODAY;
    var tbody = document.getElementById('rj-body');
    var grid = document.getElementById('poli-grid');
    try {
      var data = await Waras.api('/api/rawat-jalan?date=' + encodeURIComponent(date));
      var visits = Waras.asList(data.visits !== undefined ? data.visits : data);
      if (!visits.length) {
        tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Belum ada kunjungan rawat jalan pada ' + Waras.esc(date) + '.</td></tr>';
        grid.innerHTML = '<div class="kpi"><div class="kpi-label">Poli</div><div class="kpi-value">&mdash;</div><div class="kpi-sub">Belum ada kunjungan.</div></div>';
        return;
      }
      // Ringkasan per poli (hitung dari kunjungan nyata).
      var perPoli = {};
      visits.forEach(function (v) {
        var p = v.poli || '—';
        perPoli[p] = (perPoli[p] || 0) + 1;
      });
      var polis = Object.keys(perPoli).sort();
      grid.innerHTML = polis.slice(0, 4).map(function (p) {
        return '<div class="kpi"><div class="kpi-label">' + Waras.esc(p) + '</div>' +
          '<div class="kpi-value">' + Waras.fmtNum(perPoli[p]) + ' <span style="font-size:14px;font-weight:600">pasien</span></div>' +
          '<div class="kpi-sub">Kunjungan ' + Waras.esc(date) + '</div></div>';
      }).join('');
      tbody.innerHTML = visits.map(function (v) {
        var k = String(v.source || '').toLowerCase();
        return '<tr><td>' + Waras.esc(v.time || '') + '</td><td>' + Waras.esc(v.patient_name || v.patient_id) +
          (v.rm ? ' <span class="muted">(' + Waras.esc(v.rm) + ')</span>' : '') + '</td><td>' +
          Waras.esc(v.poli || '') + '</td><td>' + Waras.esc(v.doctor || '') + '</td><td>' +
          Waras.statusPill(String(v.source || ''), SOURCE_KIND[k] || 'neutral') + '</td><td>' +
          Waras.esc(v.status || '') + '</td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Kunjungan rawat jalan belum tersedia (endpoint /api/rawat-jalan belum aktif).</td></tr>';
      grid.innerHTML = '<div class="kpi"><div class="kpi-label">Poli</div><div class="kpi-value">&mdash;</div><div class="kpi-sub">Kunjungan belum tersedia.</div></div>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadVisits();
    document.getElementById('rj-date').addEventListener('change', loadVisits);
  });
})();
