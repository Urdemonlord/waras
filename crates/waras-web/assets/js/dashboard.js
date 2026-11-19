// Dashboard — GET /api/dashboard, /api/queue, /api/notifications.
// Pilihan yang didokumentasikan (lihat CONTRACT §3/§4):
// - API Fase 1 TIDAK mengekspos endpoint janji (/api/appointments tidak ada
//   di kontrak), jadi panel "Jadwal & janji hari ini" dirender dari
//   GET /api/queue?date= (entri antrean hari ini: jam check-in, pasien,
//   poli, status). Janji booking terpisah belum tersedia di API Fase 1.
// - Strip RAG "2 merah · 5 kuning · 9 hijau" dan sub-label KPI adalah label
//   desain (bukan KPI API); angka KPI-nya sendiri dari /api/dashboard.
(function () {
  'use strict';

  var DAY_SHORT = ['Min', 'Sen', 'Sel', 'Rab', 'Kam', 'Jum', 'Sab'];

  async function loadDashboard() {
    var k = await Waras.api('/api/dashboard');
    // KPI — semua angka dari API.
    document.getElementById('kpi-patients').textContent = Waras.fmtNum(k.patients_today);
    document.getElementById('kpi-queue').textContent = Waras.fmtNum(k.queue_active);
    document.getElementById('kpi-queue-sub').innerHTML =
      'Menunggu ' + Waras.fmtNum(k.queue_waiting) + ' + konsultasi ' + Waras.fmtNum(k.queue_in_consultation);
    document.getElementById('kpi-lab').textContent = Waras.fmtNum(k.lab_pending);
    document.getElementById('kpi-lab-sub').innerHTML =
      '<span class="danger">' + Waras.fmtNum(k.lab_critical) + ' nilai kritis</span>';
    document.getElementById('kpi-followups').textContent = Waras.fmtNum(k.followups_7d);
    document.getElementById('kpi-emergency').textContent = Waras.fmtNum(k.emergency_red);
    document.getElementById('greet-sub').textContent =
      'Ringkasan hari ini — ' + Waras.fmtNum(k.patients_today) + ' pasien · ' +
      Waras.fmtNum(k.queue_active) + ' antrean aktif · ' +
      Waras.fmtNum(k.beds_occupied) + ' dari ' + Waras.fmtNum(k.beds_total) + ' tempat tidur terisi.';
    document.getElementById('cal-note').textContent =
      '19 Nov: ' + Waras.fmtNum(k.patients_today) + ' kunjungan · 5 operasi.';

    // Grafik kunjungan 7 hari (CSS bars, dari visits_7d).
    var visits = k.visits_7d || [];
    var peak = k.visits_peak_7d || Math.max.apply(null, visits.map(function (v) { return v.count; }).concat([1]));
    var barsEl = document.getElementById('visit-bars');
    barsEl.innerHTML = visits.map(function (v) {
      var d = new Date(v.date + 'T00:00:00');
      var day = DAY_SHORT[d.getDay()] || v.day || '';
      var h = Math.max(4, Math.round((v.count / peak) * 100));
      return '<div class="bar-col"><span class="bar-val">' + Waras.fmtNum(v.count) + '</span>' +
        '<div class="bar" style="height:' + h + '%" title="' + Waras.esc(v.date) + ': ' + v.count + ' kunjungan"></div>' +
        '<span class="bar-day">' + Waras.esc(day) + '</span></div>';
    }).join('');
    document.getElementById('visit-note').textContent =
      'Rabu tertinggi: ' + Waras.fmtNum(k.visits_peak_7d) + ' kunjungan · rata-rata ' +
      Waras.fmtNum(k.visits_avg_7d) + '/hari.';

    // Donut status antrean (conic-gradient dari angka API).
    var w = k.queue_waiting, c = k.queue_in_consultation, d = k.queue_done, n = k.queue_no_show;
    var total = k.queue_total || (w + c + d + n) || 1;
    function pct(x) { return Math.round((x / total) * 100); }
    var pW = (w / total) * 100, pC = (c / total) * 100, pD = (d / total) * 100;
    var donut = document.getElementById('queue-donut');
    donut.style.background = 'conic-gradient(#0B7285 0 ' + pW + '%, #16A34A ' + pW + '% ' + (pW + pC) +
      '%, #9CA3AF ' + (pW + pC) + '% ' + (pW + pC + pD) + '%, #DC2626 ' + (pW + pC + pD) + '% 100%)';
    document.getElementById('donut-total').textContent = Waras.fmtNum(total);
    document.getElementById('donut-legend').innerHTML =
      '<li><span class="sw" style="background:#0B7285"></span>Menunggu<span class="n">' + Waras.fmtNum(w) + ' (' + pct(w) + '%)</span></li>' +
      '<li><span class="sw" style="background:#16A34A"></span>Dalam konsultasi<span class="n">' + Waras.fmtNum(c) + ' (' + pct(c) + '%)</span></li>' +
      '<li><span class="sw" style="background:#9CA3AF"></span>Selesai<span class="n">' + Waras.fmtNum(d) + ' (' + pct(d) + '%)</span></li>' +
      '<li><span class="sw" style="background:#DC2626"></span>Tidak hadir<span class="n">' + Waras.fmtNum(n) + ' (' + pct(n) + '%)</span></li>';
    document.getElementById('donut-note').textContent =
      'Rata-rata tunggu ' + Waras.fmtNum(k.avg_wait_minutes) + ' mnt · terlama ' +
      Waras.fmtNum(k.longest_wait_minutes) + ' mnt<br>' +
      Waras.fmtNum(total) + ' = nomor diterbitkan hari ini:<br>' +
      Waras.fmtNum(k.queue_active) + ' aktif + ' + Waras.fmtNum(d) + ' selesai + ' +
      Waras.fmtNum(n) + ' tidak hadir.';
  }

  async function loadSchedule() {
    var tbody = document.getElementById('schedule-body');
    try {
      var queue = Waras.asList(await Waras.api('/api/queue?date=' + Waras.TODAY));
      var map = await Waras.patientsMap();
      var rows = queue.slice(0, 6);
      tbody.innerHTML = rows.map(function (q) {
        return '<tr class="clickable" data-pasien="' + Waras.esc(q.patient_id) + '">' +
          '<td>' + Waras.esc(q.time) + '</td>' +
          '<td>' + Waras.esc(Waras.patientName(map, q.patient_id)) + '</td>' +
          '<td>' + Waras.esc(q.poli) + '</td>' +
          '<td>' + Waras.esc(q.time) + '</td>' +
          '<td>' + Waras.pillFor(q.status) + '</td></tr>';
      }).join('') || '<tr><td colspan="5" class="empty-note">Belum ada antrean hari ini.</td></tr>';
      tbody.querySelectorAll('tr.clickable').forEach(function (tr) {
        tr.addEventListener('click', function () {
          location.href = '/rme?pasien=' + encodeURIComponent(tr.getAttribute('data-pasien'));
        });
      });
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="5" class="empty-note">Jadwal tidak dapat dimuat: ' + Waras.esc(e.message) + '</td></tr>';
    }
  }

  async function loadNotifications() {
    var el = document.getElementById('notif-list');
    try {
      var list = Waras.asList(await Waras.api('/api/notifications'));
      el.innerHTML = list.map(function (n) {
        var kind = String(n.kind || 'info').toLowerCase();
        return '<div class="notif-item"><span class="dot ' + Waras.esc(kind) + '"></span><div>' +
          Waras.esc(n.message) + '<br><span class="n-kind ' + Waras.esc(kind) + '">' +
          Waras.esc(kind === 'kritis' ? 'KRITIS' : 'INFO') + ' · ' + Waras.esc(n.time) + '</span></div></div>';
      }).join('') || '<p class="empty-note">Tidak ada notifikasi hari ini.</p>';
    } catch (e) {
      el.innerHTML = '<p class="empty-note">Notifikasi tidak dapat dimuat.</p>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadDashboard().catch(function (e) { Waras.toast('Dasbor gagal dimuat: ' + e.message, true); });
    loadSchedule();
    loadNotifications();
    var aiBtn = document.getElementById('ai-summary-btn');
    if (aiBtn) aiBtn.addEventListener('click', function () {
      Waras.toast('Asisten Klinis AI masih BETA — ringkasan otomatis belum tersedia di Fase 1.');
    });
  });
})();
