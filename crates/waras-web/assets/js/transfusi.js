// Transfusi darah: GET /api/transfusi/stock, GET /api/transfusi/requests,
// POST /api/transfusi/requests, POST /api/transfusi/requests/{id}/crossmatch,
// POST /api/transfusi/requests/{id}/status (FASE4.md §5.1).
// Kartu stok diagregasi per golongan dari baris blood_stock (komponen PRC);
// angka layar desain (A+ 18 kantong dst.) TIDAK dipakai — seed kontrak:
// 2 kantong tiap golongan, O+ 1 (Menipis), AB- 0 (Habis).
(function () {
  'use strict';

  var TYPES = ['A+', 'A-', 'B+', 'B-', 'O+', 'O-', 'AB+', 'AB-'];
  var REQ_KIND = { menunggu: 'amber', crossmatch: 'teal', siap: 'teal', diberikan: 'green', batal: 'neutral' };

  var stock = [];
  var requests = [];
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
  function stockStatus(units) {
    if (units <= 0) return { label: 'Habis', kind: 'red' };
    if (units <= 2) return { label: 'Menipis', kind: 'amber' };
    return { label: 'Tersedia', kind: 'green' };
  }

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }

  function renderStock() {
    var grid = document.getElementById('blood-grid');
    var perType = {};
    TYPES.forEach(function (t) { perType[t] = 0; });
    stock.forEach(function (row) {
      if (perType[row.blood_type] !== undefined) perType[row.blood_type] += Number(row.units) || 0;
    });
    grid.innerHTML = TYPES.map(function (t) {
      var st = stockStatus(perType[t]);
      return '<div class="kpi"><div class="kpi-label">Golongan ' + t + '</div>' +
        '<div class="kpi-value">' + Waras.fmtNum(perType[t]) + ' <span style="font-size:14px;font-weight:600">kantong</span></div>' +
        '<div class="kpi-sub">' + Waras.statusPill(st.label, st.kind) + '</div></div>';
    }).join('');
    // Kantong untuk crossmatch: hanya yang masih ada unitnya.
    var bagSel = document.getElementById('xm-bag');
    var bags = stock.filter(function (row) { return (Number(row.units) || 0) > 0; });
    bagSel.innerHTML = bags.map(function (row) {
      return '<option value="' + Waras.esc(row.id) + '">' + Waras.esc(row.id) + ' — ' + Waras.esc(row.blood_type) + ' ' +
        Waras.esc(row.component) + ' (' + Waras.fmtNum(row.units) + ' kantong, exp ' + Waras.esc(row.expires_at || '') + ')</option>';
    }).join('') || '<option value="">Stok kosong</option>';
  }

  function renderRequests(map) {
    var filter = document.getElementById('req-filter').value;
    var tbody = document.getElementById('req-body');
    var list = requests.filter(function (r) {
      return !filter || String(r.status).toLowerCase() === filter.toLowerCase();
    });
    if (!list.length) {
      tbody.innerHTML = '<tr><td colspan="8" class="empty-note">Belum ada permintaan transfusi' + (filter ? ' berstatus ' + Waras.esc(filter) : '') + '.</td></tr>';
      return;
    }
    tbody.innerHTML = list.map(function (r) {
      var k = String(r.status || '').toLowerCase();
      var next = k === 'crossmatch' ? 'Siap' : (k === 'siap' ? 'Diberikan' : null);
      return '<tr data-id="' + Waras.esc(r.id) + '"><td>' + Waras.esc(r.id) + '</td><td>' +
        Waras.esc(r.patient_name || Waras.patientName(map, r.patient_id)) + '</td><td>' + Waras.esc(r.blood_type) +
        '</td><td>' + Waras.esc(r.component) + '</td><td>' + Waras.fmtNum(r.units) + ' kantong</td><td>' +
        Waras.esc(r.reason || '') + '</td><td>' + Waras.statusPill(String(r.status), REQ_KIND[k] || 'neutral') + '</td><td>' +
        (next ? '<button type="button" class="btn sm" data-status="' + Waras.esc(r.id) + '" data-next="' + next + '">&rarr; ' + next + '</button>' : '') +
        '</td></tr>';
    }).join('');
    tbody.querySelectorAll('button[data-status]').forEach(function (btn) {
      btn.addEventListener('click', async function () {
        btn.disabled = true;
        try {
          await Waras.api('/api/transfusi/requests/' + encodeURIComponent(btn.getAttribute('data-status')) + '/status', {
            method: 'POST', body: { status: btn.getAttribute('data-next') }
          });
          Waras.toast('Permintaan diubah menjadi ' + btn.getAttribute('data-next') + '.');
          await loadAll();
        } catch (e) {
          Waras.toast('Ubah status gagal: ' + (e && e.message ? e.message : ''), true);
          btn.disabled = false;
        }
      });
    });
    // Pilihan permintaan untuk crossmatch: yang belum Diberikan/Batal.
    var reqSel = document.getElementById('xm-request');
    var open = requests.filter(function (r) {
      var k = String(r.status || '').toLowerCase();
      return k !== 'diberikan' && k !== 'batal';
    });
    var cur = reqSel.value;
    reqSel.innerHTML = open.map(function (r) {
      return '<option value="' + Waras.esc(r.id) + '">' + Waras.esc(r.id) + ' — ' +
        Waras.esc(r.patient_name || Waras.patientName(map, r.patient_id)) + ' (' + Waras.esc(r.blood_type) + ' ' +
        Waras.esc(r.component) + ' ×' + Waras.fmtNum(r.units) + ')</option>';
    }).join('') || '<option value="">Tidak ada permintaan terbuka</option>';
    if (cur) reqSel.value = cur;
  }

  // Riwayat crossmatch TIDAK punya endpoint GET di kontrak (§5.1 hanya
  // POST); yang tampil di sini adalah hasil POST di sesi ini.
  var xmHistory = {};

  function renderCrossmatches(requestId) {
    var box = document.getElementById('xm-list');
    if (!requestId) {
      box.innerHTML = '<p class="empty-note">Pilih permintaan untuk melihat riwayat crossmatch.</p>';
      return;
    }
    var list = xmHistory[requestId] || [];
    if (!list.length) {
      box.innerHTML = '<p class="empty-note">Belum ada crossmatch untuk ' + Waras.esc(requestId) + ' di sesi ini.</p>';
      return;
    }
    box.innerHTML = list.map(function (x) {
      return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
        Waras.esc(x.bag_id) + '</strong> · ' + Waras.esc(x.result) + ' · ' + Waras.esc(x.tested_by || '') +
        ' · ' + Waras.esc(x.tested_at || '') + '</div>';
    }).join('');
  }

  async function loadAll() {
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      stock = Waras.asList(await Waras.api('/api/transfusi/stock'));
      renderStock();
    } catch (e) {
      document.getElementById('blood-grid').innerHTML = '<div class="kpi"><div class="kpi-label">Stok darah</div><div class="kpi-value">&mdash;</div><div class="kpi-sub">Stok belum tersedia (endpoint belum aktif).</div></div>';
    }
    try {
      requests = Waras.asList(await Waras.api('/api/transfusi/requests'));
      renderRequests(map);
    } catch (e) {
      document.getElementById('req-body').innerHTML = '<tr><td colspan="8" class="empty-note">Permintaan transfusi belum tersedia (endpoint /api/transfusi/requests belum aktif).</td></tr>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    getPatients().then(function (map) {
      var sel = document.getElementById('req-patient');
      sel.innerHTML = Object.keys(map).sort().map(function (id) {
        return '<option value="' + Waras.esc(id) + '">' + Waras.esc(map[id].name) + ' (' + Waras.esc(id) + ')</option>';
      }).join('');
    }).catch(function () {});
    loadAll();
    document.getElementById('req-filter').addEventListener('change', function () {
      getPatients().then(renderRequests).catch(function () { renderRequests({}); });
    });
    document.getElementById('xm-request').addEventListener('change', function () {
      renderCrossmatches(this.value);
    });
    document.getElementById('req-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('req-error', 'req-ok');
      try {
        var r = await Waras.api('/api/transfusi/requests', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('req-patient').value,
            blood_type: document.getElementById('req-blood').value,
            component: document.getElementById('req-component').value,
            units: Number(document.getElementById('req-units').value),
            reason: document.getElementById('req-reason').value.trim()
          }
        });
        showOk('req-ok', 'Permintaan ' + (r.id || '') + ' dibuat (Menunggu).');
        Waras.toast('Permintaan transfusi dibuat.');
        await loadAll();
      } catch (e) {
        showErr('req-error', 'Permintaan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
    document.getElementById('xm-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('xm-error', 'xm-ok');
      var reqId = document.getElementById('xm-request').value;
      try {
        var xm = await Waras.api('/api/transfusi/requests/' + encodeURIComponent(reqId) + '/crossmatch', {
          method: 'POST',
          body: {
            bag_id: document.getElementById('xm-bag').value,
            result: document.getElementById('xm-result').value
          }
        });
        (xmHistory[reqId] = xmHistory[reqId] || []).push(xm.crossmatch || xm);
        showOk('xm-ok', 'Crossmatch disimpan' + (String((xm.crossmatch || xm).result || '') === 'Cocok' ? ' — permintaan menjadi Crossmatch.' : '.'));
        Waras.toast('Crossmatch disimpan.');
        await loadAll();
        renderCrossmatches(reqId);
      } catch (e) {
        showErr('xm-error', 'Crossmatch gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
