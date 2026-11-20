// IGD — Instalasi Gawat Darurat: GET /api/igd?date=,
// POST /api/igd/triage, GET /api/igd/{id},
// POST /api/igd/{id}/status, POST /api/igd/{id}/observations.
// Semua angka dari API (FASE3.md §5). Hitungan triase dihitung dari
// data nyata (FASE3.md §8.2: 1/3/4, bukan agregat ilustrasi desain).
// Endpoint Fase 3 belum ada saat halaman ini dibuat: loader menangkap
// 404/503 dan menampilkan "belum tersedia" — halaman tidak blank/crash.
(function () {
  'use strict';

  var TRIAGE_KIND = { merah: 'red', kuning: 'amber', hijau: 'green' };
  var TRIAGE_LABEL = { merah: 'Merah', kuning: 'Kuning', hijau: 'Hijau' };
  var STATUS_KIND = {
    menunggu: 'amber', observasi: 'teal', tindakan: 'teal',
    resusitasi: 'red', selesai: 'green', dirujuk: 'neutral'
  };
  var STATUS_LABEL = {
    menunggu: 'Menunggu', observasi: 'Observasi', tindakan: 'Tindakan',
    resusitasi: 'Resusitasi', selesai: 'Selesai', dirujuk: 'Dirujuk'
  };

  var selectedId = null;
  var selectedVisit = null;
  var patientsCache = null;

  function triagePill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(TRIAGE_LABEL[k] || String(v), TRIAGE_KIND[k] || 'neutral');
  }
  function statusPill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(STATUS_LABEL[k] || String(v), STATUS_KIND[k] || 'neutral');
  }
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

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }

  function confirmLine(map, patientId) {
    var p = map && map[patientId];
    return 'Pasien: ' + (p ? p.name + ' (' + p.id + ')' : (patientId || '—'));
  }

  async function fillPatientSelect() {
    var sel = document.getElementById('triage-patient');
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncTriageConfirm();
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncTriageConfirm() {
    var sel = document.getElementById('triage-patient');
    var opt = sel.options[sel.selectedIndex];
    setText('triage-confirm', opt && opt.value ?
      'Pasien: ' + opt.textContent.replace(' · ', ' (') + ')' : 'Pasien: —');
  }

  async function loadIgd() {
    var tbody = document.getElementById('igd-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/igd?date=' + Waras.TODAY);
      var counts = data.counts || {};
      setText('count-merah', Waras.fmtNum(counts.merah || 0) + ' pasien');
      setText('count-kuning', Waras.fmtNum(counts.kuning || 0) + ' pasien');
      setText('count-hijau', Waras.fmtNum(counts.hijau || 0) + ' pasien');
      var visits = Waras.asList(data.visits !== undefined ? data.visits : data);
      if (!visits.length) {
        tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Belum ada pasien IGD hari ini.</td></tr>';
        return;
      }
      tbody.innerHTML = visits.map(function (v) {
        return '<tr data-id="' + Waras.esc(v.id) + '" style="cursor:pointer"><td>' +
          Waras.esc(v.arrived_at || '') + '</td><td>' +
          Waras.esc(Waras.patientName(map, v.patient_id)) + '</td><td>' +
          Waras.esc(v.complaint || '') + '</td><td>' + triagePill(v.triage) + '</td><td>' +
          Waras.esc(v.doctor || '') + '</td><td>' + statusPill(v.status) + '</td></tr>';
      }).join('');
      tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
        tr.addEventListener('click', function () { selectVisit(tr.getAttribute('data-id')); });
      });
    } catch (e) {
      setText('count-merah', '—'); setText('count-kuning', '—'); setText('count-hijau', '—');
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Daftar IGD belum tersedia (endpoint /api/igd belum aktif).</td></tr>';
    }
  }

  async function selectVisit(id) {
    selectedId = id;
    var box = document.getElementById('obs-list');
    var actions = document.getElementById('obs-actions');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/igd/' + encodeURIComponent(id));
      var visit = data.visit || data.igd || data;
      selectedVisit = visit;
      var obs = Waras.asList(data.observations);
      var name = Waras.patientName(map, visit.patient_id);
      setText('obs-visit-label', name + ' · ' + (visit.id || id));
      setText('obs-confirm', confirmLine(map, visit.patient_id));
      var sel = document.getElementById('obs-status');
      if (sel) sel.value = visit.status || 'Menunggu';
      box.innerHTML = obs.length ? obs.map(function (o) {
        return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
          '<strong>' + Waras.esc(Waras.patientName(map, visit.patient_id)) + '</strong> · ' +
          Waras.esc(o.note || '') + '<br><span class="muted">' +
          Waras.esc(o.recorded_by || '') + ' · ' + Waras.esc(o.recorded_at || '') + '</span></div>';
      }).join('') : '<p class="empty-note">Belum ada catatan observasi untuk kunjungan ini.</p>';
      actions.style.display = 'block';
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Observasi belum tersedia (' + Waras.esc(e.message || 'endpoint belum aktif') + ').</p>';
      actions.style.display = 'none';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadIgd();

    var newBtn = document.getElementById('btn-new-triage');
    var card = document.getElementById('triage-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect();
      document.getElementById('triage-patient').focus();
    });
    document.getElementById('triage-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('triage-error', 'triage-ok');
    });
    document.getElementById('triage-patient').addEventListener('change', syncTriageConfirm);

    document.getElementById('triage-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('triage-submit');
      btn.disabled = true;
      clearMsg('triage-error', 'triage-ok');
      try {
        var encounter = document.getElementById('triage-encounter').value.trim();
        var visit = await Waras.api('/api/igd/triage', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('triage-patient').value,
            complaint: document.getElementById('triage-complaint').value.trim(),
            triage: document.getElementById('triage-level').value,
            doctor: document.getElementById('triage-doctor').value.trim(),
            encounter_id: encounter || undefined
          }
        });
        showOk('triage-ok', 'Triase ' + (visit.id || '') + ' tersimpan.');
        Waras.toast('Triase tersimpan.');
        await loadIgd();
      } catch (e) {
        showErr('triage-error', 'Simpan triase gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('obs-status-btn').addEventListener('click', async function () {
      if (!selectedId) return;
      clearMsg('obs-error', 'obs-ok');
      try {
        await Waras.api('/api/igd/' + encodeURIComponent(selectedId) + '/status', {
          method: 'POST',
          body: { status: document.getElementById('obs-status').value }
        });
        showOk('obs-ok', 'Status diubah.');
        Waras.toast('Status IGD diubah.');
        await loadIgd();
      } catch (e) {
        showErr('obs-error', 'Ubah status gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });

    document.getElementById('obs-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      if (!selectedId) return;
      var btn = document.getElementById('obs-submit');
      btn.disabled = true;
      clearMsg('obs-error', 'obs-ok');
      try {
        await Waras.api('/api/igd/' + encodeURIComponent(selectedId) + '/observations', {
          method: 'POST',
          body: { note: document.getElementById('obs-note').value.trim() }
        });
        showOk('obs-ok', 'Observasi ditambahkan.');
        document.getElementById('obs-note').value = '';
        await selectVisit(selectedId);
      } catch (e) {
        showErr('obs-error', 'Tambah observasi gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
