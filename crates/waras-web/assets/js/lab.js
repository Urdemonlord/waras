// Laboratorium: GET /api/lab/orders?date=&status=,
// POST /api/lab/orders, GET /api/lab/orders/{id},
// POST /api/lab/orders/{id}/results, POST /api/lab/orders/{id}/status.
// Semua angka dari API (FASE3.md §5). Panel LOINC dari FASE3.md §2
// (label pemeriksaan + kode LOINC, bukan angka hasil). Flag dihitung
// server, bukan dari klien.
// Endpoint Fase 3 belum ada saat halaman ini dibuat: loader menangkap
// 404/503 dan menampilkan "belum tersedia" — halaman tidak blank/crash.
(function () {
  'use strict';

  var ORDER_KIND = { menunggu: 'amber', proses: 'teal', selesai: 'green', kritis: 'red' };
  var ORDER_LABEL = { menunggu: 'Menunggu', proses: 'Proses', selesai: 'Selesai', kritis: 'Kritis' };
  var FLAG_KIND = { normal: 'neutral', tinggi: 'amber', rendah: 'amber', kritis: 'red' };

  var selectedOrder = null;
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
  function orderPill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(ORDER_LABEL[k] || String(v), ORDER_KIND[k] || 'neutral');
  }
  function flagPill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    var label = k === 'kritis' ? 'KRITIS' : (Waras.labelFor(k) || String(v));
    return Waras.statusPill(label, FLAG_KIND[k] || 'neutral');
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
    var sel = document.getElementById('lab-patient');
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncLabConfirm();
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncLabConfirm() {
    var sel = document.getElementById('lab-patient');
    var opt = sel.options[sel.selectedIndex];
    setText('lab-confirm', opt && opt.value ?
      'Pasien: ' + opt.textContent.replace(' · ', ' (') + ')' : 'Pasien: —');
  }
  function syncLoinc() {
    var sel = document.getElementById('lab-test');
    var parts = (sel.value || '').split('|');
    document.getElementById('lab-loinc').value = parts[1] || '';
  }

  async function loadOrders() {
    var tbody = document.getElementById('lab-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/lab/orders?date=' + Waras.TODAY));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Belum ada permintaan lab hari ini.</td></tr>';
      } else {
        tbody.innerHTML = list.map(function (o) {
          return '<tr data-id="' + Waras.esc(o.id) + '" style="cursor:pointer"><td>' +
            Waras.esc(o.id) + '</td><td>' +
            Waras.esc(o.patient_name || Waras.patientName(map, o.patient_id)) + '</td><td>' +
            Waras.esc(o.test_name || '') + '</td><td>' + Waras.esc(o.loinc || '') + '</td><td>' +
            Waras.esc(o.ordered_from || '') + '</td><td>' + orderPill(o.status) + '</td></tr>';
        }).join('');
        tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
          tr.addEventListener('click', function () { selectOrder(tr.getAttribute('data-id'), list); });
        });
      }
      renderCritical(list, map);
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Permintaan lab belum tersedia (endpoint /api/lab/orders belum aktif).</td></tr>';
      document.getElementById('critical-list').innerHTML =
        '<p class="empty-note">Nilai kritis belum tersedia.</p>';
    }
  }

  function renderCritical(list, map) {
    var box = document.getElementById('critical-list');
    var kritis = (list || []).filter(function (o) {
      return String(o.status || '').toLowerCase() === 'kritis';
    });
    if (!kritis.length) {
      box.innerHTML = '<p class="empty-note">Tidak ada nilai kritis hari ini.</p>';
      return;
    }
    box.innerHTML = kritis.map(function (o) {
      return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
        '<strong>' + Waras.esc(o.id) + '</strong> · ' +
        Waras.esc(o.patient_name || Waras.patientName(map, o.patient_id)) + ' · ' +
        Waras.esc(o.test_name || '') + '<br>' +
        '<span class="pill red"><i class="dot"></i>Kritis</span> ' +
        '<span class="muted">Lihat hasil untuk detail analit.</span></div>';
    }).join('');
  }

  async function selectOrder(id, orders) {
    var o = null;
    (orders || []).forEach(function (x) { if (x.id === id) o = x; });
    // Bila dipanggil tanpa daftar (mis. setelah simpan), ambil dari API.
    if (!o) {
      try { o = await Waras.api('/api/lab/orders/' + encodeURIComponent(id)); } catch (e) { o = null; }
    }
    if (!o) return;
    selectedOrder = o.order || o;
    var order = selectedOrder;
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    var tbody = document.getElementById('result-body');
    setText('result-title', order.id + ' · ' + Waras.patientName(map, order.patient_id));
    setText('result-form-title', order.id + ' · ' + Waras.patientName(map, order.patient_id));
    setText('result-confirm', confirmLine(map, order.patient_id));
    document.getElementById('result-form-card').style.display = 'block';
    document.getElementById('result-status-box').style.display = 'block';
    var sel = document.getElementById('lab-status');
    if (sel) sel.value = order.status || 'Menunggu';
    try {
      var data = await Waras.api('/api/lab/orders/' + encodeURIComponent(order.id));
      var results = Waras.asList(data.results);
      if (!results.length) {
        tbody.innerHTML = '<tr><td colspan="4" class="empty-note">Belum ada hasil untuk permintaan ini.</td></tr>';
        return;
      }
      tbody.innerHTML = results.map(function (r) {
        var val = Waras.esc(r.value_text || '') + (r.unit ? ' ' + Waras.esc(r.unit) : '');
        return '<tr><td>' + Waras.esc(r.analyte || '') + '</td><td>' + val + '</td><td>' +
          Waras.esc(r.ref_text || '') + '</td><td>' + flagPill(r.flag) + '</td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="4" class="empty-note">Hasil belum tersedia (' + Waras.esc(e.message || 'endpoint belum aktif') + ').</td></tr>';
    }
  }

  function resultRowHtml() {
    return '<div class="form-grid result-row" style="border:1px solid var(--border);border-radius:8px;padding:12px;margin-bottom:12px">' +
      '<div class="field"><label>Analit</label><input type="text" class="r-analyte" placeholder="Troponin I" required></div>' +
      '<div class="field"><label>Nilai</label><input type="text" class="r-value" placeholder="0,42" required></div>' +
      '<div class="field"><label>Satuan</label><input type="text" class="r-unit" placeholder="ng/mL"></div>' +
      '<div class="field"><label>Nilai rujukan (teks)</label><input type="text" class="r-ref-text" placeholder="&lt; 0,04 ng/mL"></div>' +
      '<div class="field"><label>Batas bawah rujukan</label><input type="number" step="any" class="r-ref-low" placeholder="13,0"></div>' +
      '<div class="field"><label>Batas atas rujukan</label><input type="number" step="any" class="r-ref-high" placeholder="17,0"></div>' +
      '<div class="field"><label>Ambang kritis bawah</label><input type="number" step="any" class="r-crit-low" placeholder=""></div>' +
      '<div class="field"><label>Ambang kritis atas</label><input type="number" step="any" class="r-crit-high" placeholder="0,04"></div>' +
      '</div>';
  }
  function numOrNull(input) {
    if (!input || input.value === '' || input.value == null) return null;
    var n = Number(String(input.value).replace(',', '.'));
    return isNaN(n) ? null : n;
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadOrders();
    document.getElementById('result-rows').innerHTML = resultRowHtml();

    var newBtn = document.getElementById('btn-new-lab');
    var card = document.getElementById('lab-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect();
      document.getElementById('lab-patient').focus();
    });
    document.getElementById('lab-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('lab-error', 'lab-ok');
    });
    document.getElementById('lab-patient').addEventListener('change', syncLabConfirm);
    document.getElementById('lab-test').addEventListener('change', syncLoinc);

    document.getElementById('lab-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('lab-submit');
      btn.disabled = true;
      clearMsg('lab-error', 'lab-ok');
      try {
        var parts = document.getElementById('lab-test').value.split('|');
        var encounter = document.getElementById('lab-encounter').value.trim();
        var order = await Waras.api('/api/lab/orders', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('lab-patient').value,
            encounter_id: encounter || undefined,
            test_name: parts[0],
            loinc: parts[1] || undefined,
            ordered_from: document.getElementById('lab-from').value,
            ordered_by: document.getElementById('lab-by').value.trim()
          }
        });
        showOk('lab-ok', 'Permintaan ' + (order.id || '') + ' tersimpan.');
        Waras.toast('Permintaan lab tersimpan.');
        await loadOrders();
      } catch (e) {
        showErr('lab-error', 'Simpan permintaan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('lab-status-btn').addEventListener('click', async function () {
      if (!selectedOrder) return;
      try {
        await Waras.api('/api/lab/orders/' + encodeURIComponent(selectedOrder.id) + '/status', {
          method: 'POST',
          body: { status: document.getElementById('lab-status').value }
        });
        Waras.toast('Status lab diubah.');
        await loadOrders();
        await selectOrder(selectedOrder.id);
      } catch (e) {
        Waras.toast('Ubah status gagal: ' + (e && e.message ? e.message : ''), true);
      }
    });

    document.getElementById('result-add-row').addEventListener('click', function () {
      document.getElementById('result-rows').insertAdjacentHTML('beforeend', resultRowHtml());
    });
    document.getElementById('result-cancel').addEventListener('click', function () {
      document.getElementById('result-form-card').style.display = 'none';
      clearMsg('result-error', 'result-ok');
    });

    document.getElementById('result-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      if (!selectedOrder) return;
      var btn = document.getElementById('result-submit');
      btn.disabled = true;
      clearMsg('result-error', 'result-ok');
      try {
        var rows = document.querySelectorAll('#result-rows .result-row');
        var results = [];
        rows.forEach(function (row) {
          results.push({
            analyte: row.querySelector('.r-analyte').value.trim(),
            value_text: row.querySelector('.r-value').value.trim(),
            unit: row.querySelector('.r-unit').value.trim(),
            ref_text: row.querySelector('.r-ref-text').value.trim(),
            ref_low: numOrNull(row.querySelector('.r-ref-low')),
            ref_high: numOrNull(row.querySelector('.r-ref-high')),
            critical_low: numOrNull(row.querySelector('.r-crit-low')),
            critical_high: numOrNull(row.querySelector('.r-crit-high'))
          });
        });
        await Waras.api('/api/lab/orders/' + encodeURIComponent(selectedOrder.id) + '/results', {
          method: 'POST',
          body: { results: results }
        });
        showOk('result-ok', 'Hasil tersimpan (flag dihitung server).');
        Waras.toast('Hasil lab tersimpan.');
        await loadOrders();
        await selectOrder(selectedOrder.id);
      } catch (e) {
        showErr('result-error', 'Simpan hasil gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
