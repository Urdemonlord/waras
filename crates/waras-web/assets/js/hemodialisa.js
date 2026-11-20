// Hemodialisa: GET /api/hemodialisa?date= -> {machines:[], sessions:[]},
// POST /api/hemodialisa/sessions, POST /api/hemodialisa/sessions/{id}/status.
// Data mesin/sesi dari API (FASE4.md §5.1); angka layar desain (8 mesin,
// pasien HD-01 Bambang dkk.) TIDAK dipakai — seed kontrak hanya 4 mesin
// (HD-1..HD-4) dan 3 sesi 19 Nov 2026.
(function () {
  'use strict';

  var MACHINE_KIND = { siap: 'green', terpakai: 'teal', perawatan: 'amber' };
  var SESSION_KIND = { terjadwal: 'amber', berjalan: 'teal', selesai: 'green', batal: 'neutral' };

  var machines = [];
  var sessions = [];
  var patientsCache = null;

  function setText(id, text) {
    var el = document.getElementById(id);
    if (el) el.textContent = text;
  }
  function showErr(id, msg) {
    var el = document.getElementById(id);
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }
  function showOk(id, msg) {
    var el = document.getElementById(id);
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }
  function clearMsg(errId, okId) {
    var e = document.getElementById(errId), o = document.getElementById(okId);
    if (e) e.classList.remove('on');
    if (o) o.classList.remove('on');
  }
  function statusPill(v, kindMap) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(String(v), (kindMap && kindMap[k]) || 'neutral');
  }
  function timeOf(scheduledAt) {
    var s = String(scheduledAt || '');
    return s.length >= 16 ? s.slice(11, 16) : s;
  }

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }

  function fillPatientSelects() {
    getPatients().then(function (map) {
      var sel = document.getElementById('hd-patient');
      var ids = Object.keys(map).sort();
      sel.innerHTML = ids.map(function (id) {
        return '<option value="' + Waras.esc(id) + '">' + Waras.esc(map[id].name) + ' (' + Waras.esc(id) + ')</option>';
      }).join('');
    }).catch(function () {
      document.getElementById('hd-patient').innerHTML = '<option value="">Gagal memuat pasien</option>';
    });
  }

  function fillMachineSelect() {
    var sel = document.getElementById('hd-machine');
    var ready = machines.filter(function (m) { return String(m.status).toLowerCase() === 'siap'; });
    var opts = (ready.length ? ready : machines).map(function (m) {
      return '<option value="' + Waras.esc(m.id) + '">' + Waras.esc(m.name || m.id) + ' — ' + Waras.esc(m.status) + '</option>';
    }).join('');
    sel.innerHTML = opts || '<option value="">Belum ada mesin</option>';
  }

  function renderMachines() {
    var grid = document.getElementById('machine-grid');
    if (!machines.length) {
      grid.innerHTML = '<div class="kpi"><div class="kpi-label">Mesin</div><div class="kpi-value">&mdash;</div><div class="kpi-sub">Belum ada mesin hemodialisa.</div></div>';
      return;
    }
    grid.innerHTML = machines.map(function (m) {
      return '<div class="kpi"><div class="kpi-label">' + Waras.esc(m.name || m.id) + '</div>' +
        '<div class="kpi-value" style="font-size:20px">' + Waras.esc(m.status) + '</div>' +
        '<div class="kpi-sub">' + Waras.esc(m.id) + '</div></div>';
    }).join('');
  }

  async function loadHd() {
    var date = document.getElementById('hd-date').value || Waras.TODAY;
    var tbody = document.getElementById('hd-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/hemodialisa?date=' + encodeURIComponent(date));
      machines = Waras.asList(data.machines || []);
      sessions = Waras.asList(data.sessions || []);
      renderMachines();
      fillMachineSelect();
      if (!sessions.length) {
        tbody.innerHTML = '<tr><td colspan="10" class="empty-note">Belum ada sesi hemodialisa pada ' + Waras.esc(date) + '.</td></tr>';
        return;
      }
      tbody.innerHTML = sessions.map(function (s) {
        var st = String(s.status || '').toLowerCase();
        var next = st === 'terjadwal' ? 'Berjalan' : (st === 'berjalan' ? 'Selesai' : null);
        return '<tr><td>' + Waras.esc(s.id) + '</td><td>' + Waras.esc(Waras.patientName(map, s.patient_id)) + '</td><td>' +
          Waras.esc(s.machine_id) + '</td><td>' + Waras.esc(s.dialyzer || '') + '</td><td>' +
          Waras.esc(timeOf(s.scheduled_at)) + '</td><td>' + Waras.fmtNum(s.duration_minutes || 0) + ' mnt</td><td>' +
          Waras.fmtNum(s.blood_flow || 0) + ' mL/mnt</td><td>' + Waras.fmtNum(s.uf_goal_ml || 0) + ' mL</td><td>' +
          statusPill(s.status, SESSION_KIND) + '</td><td>' +
          (next ? '<button type="button" class="btn sm" data-status="' + Waras.esc(s.id) + '" data-next="' + next + '">&rarr; ' + next + '</button>' : '') +
          '</td></tr>';
      }).join('');
      tbody.querySelectorAll('button[data-status]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
          btn.disabled = true;
          try {
            await Waras.api('/api/hemodialisa/sessions/' + encodeURIComponent(btn.getAttribute('data-status')) + '/status', {
              method: 'POST', body: { status: btn.getAttribute('data-next') }
            });
            Waras.toast('Status sesi diubah menjadi ' + btn.getAttribute('data-next') + '.');
            await loadHd();
          } catch (e) {
            Waras.toast('Ubah status gagal: ' + (e && e.message ? e.message : ''), true);
            btn.disabled = false;
          }
        });
      });
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="10" class="empty-note">Sesi hemodialisa belum tersedia (endpoint /api/hemodialisa belum aktif).</td></tr>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    fillPatientSelects();
    loadHd();
    document.getElementById('hd-date').addEventListener('change', loadHd);
    document.getElementById('hd-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('hd-error', 'hd-ok');
      try {
        var s = await Waras.api('/api/hemodialisa/sessions', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('hd-patient').value,
            machine_id: document.getElementById('hd-machine').value,
            dialyzer: document.getElementById('hd-dialyzer').value.trim(),
            duration_minutes: Number(document.getElementById('hd-duration').value),
            blood_flow: Number(document.getElementById('hd-blood-flow').value),
            dialysate_flow: Number(document.getElementById('hd-dialysate-flow').value),
            uf_goal_ml: Number(document.getElementById('hd-uf').value),
            scheduled_at: document.getElementById('hd-scheduled').value.replace('T', ' '),
            notes: document.getElementById('hd-notes').value.trim()
          }
        });
        showOk('hd-ok', 'Sesi ' + (s.id || '') + ' terjadwal.');
        Waras.toast('Sesi hemodialisa terjadwal.');
        await loadHd();
      } catch (e) {
        showErr('hd-error', 'Jadwal gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
