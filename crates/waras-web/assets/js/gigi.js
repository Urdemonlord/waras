// Rekam medis gigi: GET /api/gigi/records, POST /api/gigi/records,
// POST /api/gigi/records/{id}/status (FASE4.md §5.1).
// Odontogram: grid 32 gigi notasi FDI (11–18, 21–28, 31–38, 41–48),
// kotak berlabel nomor (bukan SVG gigi realistis, sesuai gaya Swiss
// klinis). Warna menurut status rekam: Rencana kuning, Berjalan teal,
// Selesai hijau. Klik gigi memfilter daftar rekam ke gigi itu.
(function () {
  'use strict';

  var TEETH_UPPER = ['11', '12', '13', '14', '15', '16', '17', '18', '21', '22', '23', '24', '25', '26', '27', '28'];
  var TEETH_LOWER = ['41', '42', '43', '44', '45', '46', '47', '48', '31', '32', '33', '34', '35', '36', '37', '38'];
  var STATUS_KIND = { rencana: 'amber', berjalan: 'teal', selesai: 'green' };
  var STATUS_CLASS = { rencana: 'st-rencana', berjalan: 'st-berjalan', selesai: 'st-selesai' };

  var records = [];
  var selectedTooth = null;
  var patientsCache = null;

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

  // Status "terkuat" per gigi: Berjalan > Rencana > Selesai.
  function toothStatusMap() {
    var rank = { berjalan: 3, rencana: 2, selesai: 1 };
    var map = {};
    records.forEach(function (r) {
      var k = String(r.status || '').toLowerCase();
      if (!rank[k]) return;
      if (!map[r.tooth_number] || rank[k] > rank[map[r.tooth_number]]) map[r.tooth_number] = k;
    });
    return map;
  }

  function renderOdontogram() {
    var box = document.getElementById('odontogram');
    var statusByTooth = toothStatusMap();
    var all = TEETH_UPPER.concat(TEETH_LOWER);
    box.innerHTML = all.map(function (t) {
      var k = statusByTooth[t];
      var cls = k ? ' ' + (STATUS_CLASS[k] || '') : '';
      var sel = selectedTooth === t ? ' sel' : '';
      return '<button type="button" class="tooth' + cls + sel + '" data-tooth="' + t + '" title="Gigi ' + t +
        (k ? ' — ' + k : ' — belum ada rekam') + '">' + t + '</button>';
    }).join('');
    box.querySelectorAll('.tooth').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var t = btn.getAttribute('data-tooth');
        selectedTooth = selectedTooth === t ? null : t;
        renderOdontogram();
        renderRecords();
        renderSummary();
      });
    });
  }

  function renderSummary() {
    var el = document.getElementById('odonto-summary');
    if (!selectedTooth) {
      el.textContent = 'Pilih gigi untuk melihat diagnosisnya.';
      return;
    }
    var list = records.filter(function (r) { return r.tooth_number === selectedTooth; });
    if (!list.length) {
      el.textContent = 'Gigi ' + selectedTooth + ': belum ada rekam.';
      return;
    }
    el.textContent = 'Gigi ' + selectedTooth + ': ' + list.map(function (r) {
      return (r.diagnosis_code ? r.diagnosis_code + ' ' : '') + (r.diagnosis || '') + ' (' + (r.status || '') + ')';
    }).join('; ') + '.';
  }

  function renderRecords() {
    var tbody = document.getElementById('gigi-body');
    var label = document.getElementById('gigi-filter-label');
    var list = selectedTooth
      ? records.filter(function (r) { return r.tooth_number === selectedTooth; })
      : records;
    label.textContent = selectedTooth ? 'Filter: gigi ' + selectedTooth : '';
    if (!list.length) {
      tbody.innerHTML = '<tr><td colspan="8" class="empty-note">' +
        (selectedTooth ? 'Belum ada rekam untuk gigi ' + Waras.esc(selectedTooth) + '.' : 'Belum ada rekam gigi.') +
        '</td></tr>';
      return;
    }
    getPatients().then(function (map) {
      tbody.innerHTML = list.map(function (r) {
        var k = String(r.status || '').toLowerCase();
        var next = k === 'rencana' ? 'Berjalan' : (k === 'berjalan' ? 'Selesai' : null);
        return '<tr><td><strong>' + Waras.esc(r.tooth_number) + '</strong></td><td>' +
          Waras.esc(Waras.patientName(map, r.patient_id)) + '</td><td>' + Waras.esc(r.diagnosis || '') +
          '</td><td>' + Waras.esc(r.diagnosis_code || '') + '</td><td>' + Waras.esc(r.treatment || '') +
          (r.treatment_code ? ' (' + Waras.esc(r.treatment_code) + ')' : '') + '</td><td>' +
          Waras.esc(r.dentist || '') + '</td><td>' + Waras.statusPill(String(r.status), STATUS_KIND[k] || 'neutral') +
          '</td><td>' + (next ? '<button type="button" class="btn sm" data-status="' + Waras.esc(r.id) + '" data-next="' + next + '">&rarr; ' + next + '</button>' : '') + '</td></tr>';
      }).join('');
      tbody.querySelectorAll('button[data-status]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
          btn.disabled = true;
          try {
            await Waras.api('/api/gigi/records/' + encodeURIComponent(btn.getAttribute('data-status')) + '/status', {
              method: 'POST', body: { status: btn.getAttribute('data-next') }
            });
            Waras.toast('Status rekam diubah menjadi ' + btn.getAttribute('data-next') + '.');
            await loadRecords();
          } catch (e) {
            Waras.toast('Ubah status gagal: ' + (e && e.message ? e.message : ''), true);
            btn.disabled = false;
          }
        });
      });
    }).catch(function () { /* nama tampil sebagai id */ });
  }

  function renderToday() {
    var box = document.getElementById('gigi-today');
    var today = Waras.TODAY;
    var list = records.filter(function (r) { return String(r.created_at || '').slice(0, 10) === today; });
    if (!list.length) {
      box.innerHTML = '<p class="empty-note">Belum ada tindakan gigi hari ini.</p>';
      return;
    }
    getPatients().then(function (map) {
      box.innerHTML = list.map(function (r) {
        var k = String(r.status || '').toLowerCase();
        return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
          Waras.esc(r.treatment || '') + ' — gigi ' + Waras.esc(r.tooth_number) + '</strong><br>' +
          '<span class="muted">' + Waras.esc(Waras.patientName(map, r.patient_id)) + ' · ' + Waras.esc(r.dentist || '') + '</span>' +
          ' <span style="float:right">' + Waras.statusPill(String(r.status), STATUS_KIND[k] || 'neutral') + '</span></div>';
      }).join('');
    }).catch(function () { /* nama tampil sebagai id */ });
  }

  async function loadRecords() {
    try {
      records = Waras.asList(await Waras.api('/api/gigi/records'));
      renderOdontogram();
      renderRecords();
      renderSummary();
      renderToday();
    } catch (e) {
      document.getElementById('gigi-body').innerHTML = '<tr><td colspan="8" class="empty-note">Rekam gigi belum tersedia (endpoint /api/gigi/records belum aktif).</td></tr>';
      document.getElementById('gigi-today').innerHTML = '<p class="empty-note">Tindakan gigi belum tersedia.</p>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    getPatients().then(function (map) {
      document.getElementById('gigi-patient').innerHTML = Object.keys(map).sort().map(function (id) {
        return '<option value="' + Waras.esc(id) + '">' + Waras.esc(map[id].name) + ' (' + Waras.esc(id) + ')</option>';
      }).join('');
    }).catch(function () {});
    loadRecords();
    document.getElementById('odonto-clear').addEventListener('click', function (ev) {
      ev.preventDefault();
      selectedTooth = null;
      renderOdontogram();
      renderRecords();
      renderSummary();
    });
    document.getElementById('gigi-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('gigi-error', 'gigi-ok');
      try {
        var r = await Waras.api('/api/gigi/records', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('gigi-patient').value,
            tooth_number: document.getElementById('gigi-tooth').value.trim(),
            diagnosis: document.getElementById('gigi-diagnosis').value.trim(),
            diagnosis_code: document.getElementById('gigi-diagnosis-code').value.trim(),
            treatment: document.getElementById('gigi-treatment').value.trim(),
            treatment_code: document.getElementById('gigi-treatment-code').value.trim(),
            dentist: document.getElementById('gigi-dentist').value.trim()
          }
        });
        showOk('gigi-ok', 'Rekam ' + (r.id || '') + ' disimpan (Rencana).');
        Waras.toast('Rekam gigi disimpan.');
        await loadRecords();
      } catch (e) {
        showErr('gigi-error', 'Rekam gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
