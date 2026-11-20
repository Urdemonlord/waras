// Kamar Operasi & Bersalin: GET /api/operasi?date=,
// POST /api/operasi, POST /api/operasi/{id}/status,
// POST /api/bersalin, GET /api/bersalin, POST /api/bersalin/{id}/status.
// Semua angka dari API (FASE3.md §5). "1 rujukan masuk (VK 2)" di layar
// desain tidak dimodelkan sebagai baris — hanya status ruang (FASE3 §8.8).
// Endpoint Fase 3 belum ada saat halaman ini dibuat: loader menangkap
// 404/503 dan menampilkan "belum tersedia" — halaman tidak blank/crash.
(function () {
  'use strict';

  var SURG_KIND = {
    terjadwal: 'amber', menunggu: 'amber', siap: 'teal',
    berjalan: 'teal', selesai: 'green', batal: 'neutral'
  };
  var SURG_LABEL = {
    terjadwal: 'Terjadwal', menunggu: 'Menunggu', siap: 'Siap',
    berjalan: 'Berjalan', selesai: 'Selesai', batal: 'Batal'
  };
  var DLV_KIND = { aktif: 'teal', selesai: 'green', dirujuk: 'neutral' };

  var selectedSurgery = null;
  var selectedDelivery = null;
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
  function hhmm(scheduledAt) {
    // "2026-11-19 09:30" / ISO -> "09.30".
    if (!scheduledAt) return '—';
    var m = String(scheduledAt).match(/(\d{2})[:.](\d{2})/);
    return m ? m[1] + '.' + m[2] : String(scheduledAt);
  }
  function surgPill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(SURG_LABEL[k] || String(v), SURG_KIND[k] || 'neutral');
  }

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }

  async function fillPatientSelect(selId, confirmId) {
    var sel = document.getElementById(selId);
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncConfirm(selId, confirmId);
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncConfirm(selId, confirmId) {
    var sel = document.getElementById(selId);
    var opt = sel.options[sel.selectedIndex];
    setText(confirmId, opt && opt.value ?
      'Pasien: ' + opt.textContent.replace(' · ', ' (') + ')' : 'Pasien: —');
  }

  function renderTeams(surgeries) {
    var box = document.getElementById('team-list');
    var byRoom = {};
    surgeries.forEach(function (s) {
      var room = s.room || '—';
      if (!byRoom[room]) byRoom[room] = s.team || '';
      else if (!byRoom[room] && s.team) byRoom[room] = s.team;
    });
    var rooms = Object.keys(byRoom).sort();
    if (!rooms.length) {
      box.innerHTML = '<p class="empty-note">Belum ada tim operasi hari ini.</p>';
      return;
    }
    box.innerHTML = rooms.map(function (room) {
      return '<div style="margin-bottom:14px"><strong>Tim — ' + Waras.esc(room) + '</strong><br>' +
        '<span class="muted">' + Waras.esc(byRoom[room] || 'Tim belum dicatat') + '</span></div>';
    }).join('');
  }

  async function loadOperasi() {
    var tbody = document.getElementById('surgery-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/operasi?date=' + Waras.TODAY);
      var surgeries = Waras.asList(data.surgeries !== undefined ? data.surgeries : data);
      if (!surgeries.length) {
        tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Belum ada jadwal operasi hari ini.</td></tr>';
      } else {
        tbody.innerHTML = surgeries.map(function (s) {
          var proc = Waras.esc(s.procedure || '') +
            (s.procedure_code ? ' (' + Waras.esc(s.procedure_code) + ')' : '');
          return '<tr data-id="' + Waras.esc(s.id) + '" style="cursor:pointer"><td>' +
            Waras.esc(hhmm(s.scheduled_at)) + '</td><td>' + Waras.esc(s.room || '') + '</td><td>' +
            Waras.esc(Waras.patientName(map, s.patient_id)) + '</td><td>' + proc + '</td><td>' +
            Waras.esc(s.operator || '') + '</td><td>' + surgPill(s.status) + '</td></tr>';
        }).join('');
        tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
          tr.addEventListener('click', function () { selectSurgery(tr.getAttribute('data-id'), surgeries); });
        });
      }
      renderTeams(surgeries);
      renderDeliveries(data.deliveries || null, data.delivery_rooms || null, map);
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Jadwal operasi belum tersedia (endpoint /api/operasi belum aktif).</td></tr>';
      document.getElementById('team-list').innerHTML = '<p class="empty-note">Tim operasi belum tersedia.</p>';
      // Persalinan dicoba lewat /api/bersalin sebagai fallback.
      loadBersalinOnly();
    }
  }

  function renderDeliveries(deliveries, rooms, map) {
    var box = document.getElementById('delivery-list');
    var list = deliveries || [];
    if (!list.length) {
      box.innerHTML = '<p class="empty-note">Belum ada persalinan aktif.</p>';
    } else {
      box.innerHTML = list.map(function (d) {
        var k = String(d.status == null ? '' : d.status).toLowerCase();
        return '<div data-id="' + Waras.esc(d.id) + '" style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px;cursor:pointer">' +
          '<strong>' + Waras.esc(Waras.patientName(map, d.patient_id)) + '</strong> · ' +
          Waras.esc(d.age_years) + ' thn · ' + Waras.esc(d.gravida_para || '') +
          ' · Pembukaan ' + Waras.esc(d.dilation_cm) + ' cm · DJJ ' + Waras.esc(d.fetal_heart_rate) + ' x/mnt.<br>' +
          '<span class="muted">Bidan: ' + Waras.esc(d.midwife || '') + ' · Dokter: ' + Waras.esc(d.doctor || '') +
          ' · Ruang ' + Waras.esc(d.room || '') + '</span><br>' +
          '<span class="muted">Rencana: ' + Waras.esc(d.plan || '') + '</span> ' +
          Waras.statusPill(Waras.labelFor(d.status), DLV_KIND[k] || 'neutral') + '</div>';
      }).join('');
      box.querySelectorAll('[data-id]').forEach(function (el) {
        el.addEventListener('click', function () { selectDelivery(el.getAttribute('data-id'), list, map); });
      });
    }
    if (rooms) {
      var terisi = rooms.filter(function (r) {
        return String(r.status || '').toLowerCase() === 'terisi';
      }).length;
      setText('delivery-rooms-summary',
        'Tempat bersalin: ' + Waras.fmtNum(terisi) + ' terisi · ' +
        Waras.fmtNum(rooms.length - terisi) + ' kosong.');
    }
  }

  async function loadBersalinOnly() {
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/bersalin');
      renderDeliveries(data.deliveries, data.rooms, map);
    } catch (e) {
      document.getElementById('delivery-list').innerHTML =
        '<p class="empty-note">Persalinan belum tersedia (endpoint /api/bersalin belum aktif).</p>';
    }
  }

  function selectSurgery(id, surgeries) {
    var s = null;
    (surgeries || []).forEach(function (x) { if (x.id === id) s = x; });
    if (!s) return;
    selectedSurgery = s;
    var box = document.getElementById('surgery-status-box');
    box.style.display = 'block';
    var map = patientsCache || {};
    setText('surgery-selected', 'Operasi terpilih: ' +
      (s.procedure || '') + ' · ' + Waras.patientName(map, s.patient_id) +
      ' · ' + (s.room || '') + ' ' + hhmm(s.scheduled_at));
    var sel = document.getElementById('surgery-status');
    if (sel) sel.value = s.status || 'Terjadwal';
  }

  function selectDelivery(id, deliveries, map) {
    var d = null;
    (deliveries || []).forEach(function (x) { if (x.id === id) d = x; });
    if (!d) return;
    selectedDelivery = d;
    var box = document.getElementById('delivery-status-box');
    box.style.display = 'block';
    setText('delivery-selected', 'Persalinan terpilih: ' +
      Waras.patientName(map, d.patient_id) + ' · Ruang ' + (d.room || ''));
    var sel = document.getElementById('delivery-status');
    if (sel) sel.value = d.status || 'Aktif';
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadOperasi();

    var newBtn = document.getElementById('btn-new-surgery');
    var card = document.getElementById('surgery-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect('srg-patient', 'srg-confirm');
      document.getElementById('srg-patient').focus();
    });
    document.getElementById('srg-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('srg-error', 'srg-ok');
    });
    document.getElementById('srg-patient').addEventListener('change', function () {
      syncConfirm('srg-patient', 'srg-confirm');
    });

    document.getElementById('surgery-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('srg-submit');
      btn.disabled = true;
      clearMsg('srg-error', 'srg-ok');
      try {
        var encounter = document.getElementById('srg-encounter').value.trim();
        var team = document.getElementById('srg-team').value.trim();
        var scheduled = document.getElementById('srg-scheduled').value.replace('T', ' ');
        var s = await Waras.api('/api/operasi', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('srg-patient').value,
            encounter_id: encounter || undefined,
            room: document.getElementById('srg-room').value,
            procedure: document.getElementById('srg-procedure').value.trim(),
            procedure_code: document.getElementById('srg-code').value.trim(),
            operator: document.getElementById('srg-operator').value.trim(),
            team: team || undefined,
            scheduled_at: scheduled
          }
        });
        showOk('srg-ok', 'Jadwal ' + (s.id || '') + ' tersimpan.');
        Waras.toast('Jadwal operasi tersimpan.');
        await loadOperasi();
      } catch (e) {
        showErr('srg-error', 'Simpan jadwal gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('surgery-status-btn').addEventListener('click', async function () {
      if (!selectedSurgery) return;
      clearMsg('surgery-error', 'surgery-ok');
      try {
        await Waras.api('/api/operasi/' + encodeURIComponent(selectedSurgery.id) + '/status', {
          method: 'POST',
          body: { status: document.getElementById('surgery-status').value }
        });
        showOk('surgery-ok', 'Status diubah.');
        Waras.toast('Status operasi diubah.');
        await loadOperasi();
      } catch (e) {
        showErr('surgery-error', 'Ubah status gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });

    var dlvBtn = document.getElementById('btn-new-delivery');
    var dlvCard = document.getElementById('delivery-form-card');
    dlvBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      dlvCard.style.display = 'block';
      fillPatientSelect('dlv-patient', 'dlv-confirm');
      document.getElementById('dlv-patient').focus();
    });
    document.getElementById('dlv-cancel').addEventListener('click', function () {
      dlvCard.style.display = 'none';
      clearMsg('dlv-error', 'dlv-ok');
    });
    document.getElementById('dlv-patient').addEventListener('change', function () {
      syncConfirm('dlv-patient', 'dlv-confirm');
    });

    document.getElementById('delivery-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('dlv-submit');
      btn.disabled = true;
      clearMsg('dlv-error', 'dlv-ok');
      try {
        var d = await Waras.api('/api/bersalin', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('dlv-patient').value,
            age_years: Number(document.getElementById('dlv-age').value),
            gravida_para: document.getElementById('dlv-gp').value.trim(),
            dilation_cm: Number(document.getElementById('dlv-dilation').value),
            fetal_heart_rate: Number(document.getElementById('dlv-fhr').value),
            midwife: document.getElementById('dlv-midwife').value.trim(),
            doctor: document.getElementById('dlv-doctor').value.trim(),
            plan: document.getElementById('dlv-plan').value.trim(),
            room: document.getElementById('dlv-room').value
          }
        });
        showOk('dlv-ok', 'Persalinan ' + (d.id || '') + ' tercatat.');
        Waras.toast('Persalinan tercatat.');
        await loadOperasi();
      } catch (e) {
        showErr('dlv-error', 'Simpan persalinan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('delivery-status-btn').addEventListener('click', async function () {
      if (!selectedDelivery) return;
      clearMsg('delivery-error', 'delivery-ok');
      try {
        await Waras.api('/api/bersalin/' + encodeURIComponent(selectedDelivery.id) + '/status', {
          method: 'POST',
          body: { status: document.getElementById('delivery-status').value }
        });
        showOk('delivery-ok', 'Status diubah.');
        Waras.toast('Status persalinan diubah.');
        await loadOperasi();
      } catch (e) {
        showErr('delivery-error', 'Ubah status gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });
  });
})();
