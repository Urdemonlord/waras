// Radiologi: GET /api/radiologi/orders?date=&status=,
// POST /api/radiologi/orders, GET /api/radiologi/orders/{id},
// POST /api/radiologi/orders/{id}/report,
// POST /api/radiologi/orders/{id}/status.
// Semua angka dari API (FASE3.md §5). Penampil citra menampilkan
// modalitas + area sebagai TEKS di panel gelap (seperti desain) —
// BUKAN citra asli; citra diagnostik tidak disimpan di WARAS.
// Endpoint Fase 3 belum ada saat halaman ini dibuat: loader menangkap
// 404/503 dan menampilkan "belum tersedia" — halaman tidak blank/crash.
(function () {
  'use strict';

  var ORDER_KIND = { menunggu: 'amber', proses: 'teal', selesai: 'green' };
  var ORDER_LABEL = { menunggu: 'Menunggu', proses: 'Proses', selesai: 'Selesai' };

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
    var sel = document.getElementById('rad-patient');
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncRadConfirm();
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncRadConfirm() {
    var sel = document.getElementById('rad-patient');
    var opt = sel.options[sel.selectedIndex];
    setText('rad-confirm', opt && opt.value ?
      'Pasien: ' + opt.textContent.replace(' · ', ' (') + ')' : 'Pasien: —');
  }

  async function loadOrders() {
    var tbody = document.getElementById('rad-body');
    var reportBody = document.getElementById('report-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/radiologi/orders?date=' + Waras.TODAY));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="4" class="empty-note">Belum ada permintaan radiologi hari ini.</td></tr>';
        reportBody.innerHTML = '<tr><td colspan="5" class="empty-note">Belum ada laporan hasil.</td></tr>';
        return;
      }
      tbody.innerHTML = list.map(function (o) {
        return '<tr data-id="' + Waras.esc(o.id) + '" style="cursor:pointer"><td>' +
          Waras.esc(o.id) + '</td><td>' +
          Waras.esc(o.patient_name || Waras.patientName(map, o.patient_id)) + '</td><td>' +
          Waras.esc(o.modality || '') + '</td><td>' + Waras.esc(o.area || '') + '</td></tr>';
      }).join('');
      tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
        tr.addEventListener('click', function () { selectOrder(tr.getAttribute('data-id'), list); });
      });
      // Laporan hasil: radiolog hanya diketahui setelah GET detail per
      // order; tampilkan status order + radiolog bila sudah ada di list.
      reportBody.innerHTML = list.map(function (o) {
        var exam = (o.modality || '') + ' ' + (o.area || '');
        var rad = o.radiologist || o.report_radiologist || '—';
        return '<tr data-id="' + Waras.esc(o.id) + '" style="cursor:pointer"><td>' +
          Waras.esc(o.id) + '</td><td>' +
          Waras.esc(o.patient_name || Waras.patientName(map, o.patient_id)) + '</td><td>' +
          Waras.esc(exam.trim()) + '</td><td>' + Waras.esc(rad) + '</td><td>' +
          orderPill(o.status) + '</td></tr>';
      }).join('');
      reportBody.querySelectorAll('tr[data-id]').forEach(function (tr) {
        tr.addEventListener('click', function () { selectOrder(tr.getAttribute('data-id'), list); });
      });
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="4" class="empty-note">Permintaan radiologi belum tersedia (endpoint /api/radiologi/orders belum aktif).</td></tr>';
      reportBody.innerHTML = '<tr><td colspan="5" class="empty-note">Laporan hasil belum tersedia.</td></tr>';
    }
  }

  async function selectOrder(id, orders) {
    var o = null;
    (orders || []).forEach(function (x) { if (x.id === id) o = x; });
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    var report = null;
    try {
      var data = await Waras.api('/api/radiologi/orders/' + encodeURIComponent(id));
      var order = data.order || data;
      if (order && order.id) o = order;
      report = data.report || null;
    } catch (e) { /* laporan opsional; penampil tetap terisi dari baris */ }
    if (!o) return;
    selectedOrder = o;
    setText('viewer-title', o.id);
    setText('viewer-text', ((o.modality || '') + ' · ' + (o.area || '')).toUpperCase());
    setText('viewer-sub', Waras.patientName(map, o.patient_id) + ' · ' + (o.id || ''));
    setText('viewer-findings', report ? report.findings : 'Belum ada laporan untuk permintaan ini.');
    setText('viewer-radiologist', report ?
      'Dilaporkan: ' + report.radiologist + ' · ' + (report.reported_at || '') : '—');
    setText('report-form-title', o.id + ' · ' + Waras.patientName(map, o.patient_id));
    setText('report-confirm', confirmLine(map, o.patient_id));
    var sel = document.getElementById('rad-status');
    if (sel) sel.value = o.status || 'Menunggu';
    if (report) {
      document.getElementById('report-findings').value = report.findings || '';
      document.getElementById('report-radiologist').value = report.radiologist || '';
    }
    document.getElementById('report-form-card').style.display = 'block';
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadOrders();

    var newBtn = document.getElementById('btn-new-rad');
    var card = document.getElementById('rad-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect();
      document.getElementById('rad-patient').focus();
    });
    document.getElementById('rad-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('rad-error', 'rad-ok');
    });
    document.getElementById('rad-patient').addEventListener('change', syncRadConfirm);

    document.getElementById('rad-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('rad-submit');
      btn.disabled = true;
      clearMsg('rad-error', 'rad-ok');
      try {
        var encounter = document.getElementById('rad-encounter').value.trim();
        var order = await Waras.api('/api/radiologi/orders', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('rad-patient').value,
            encounter_id: encounter || undefined,
            modality: document.getElementById('rad-modality').value,
            area: document.getElementById('rad-area').value.trim(),
            ordered_from: document.getElementById('rad-from').value,
            ordered_by: document.getElementById('rad-by').value.trim()
          }
        });
        showOk('rad-ok', 'Permintaan ' + (order.id || '') + ' tersimpan.');
        Waras.toast('Permintaan radiologi tersimpan.');
        await loadOrders();
      } catch (e) {
        showErr('rad-error', 'Simpan permintaan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('btn-new-report').addEventListener('click', function (ev) {
      ev.preventDefault();
      if (!selectedOrder) {
        Waras.toast('Pilih dulu permintaan di tabel.', true);
        return;
      }
      document.getElementById('report-form-card').style.display = 'block';
      document.getElementById('report-findings').focus();
    });
    document.getElementById('report-cancel').addEventListener('click', function () {
      document.getElementById('report-form-card').style.display = 'none';
      clearMsg('report-error', 'report-ok');
    });

    document.getElementById('report-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      if (!selectedOrder) return;
      var btn = document.getElementById('report-submit');
      btn.disabled = true;
      clearMsg('report-error', 'report-ok');
      try {
        await Waras.api('/api/radiologi/orders/' + encodeURIComponent(selectedOrder.id) + '/report', {
          method: 'POST',
          body: {
            findings: document.getElementById('report-findings').value.trim(),
            radiologist: document.getElementById('report-radiologist').value.trim()
          }
        });
        showOk('report-ok', 'Laporan tersimpan; order menjadi Selesai.');
        Waras.toast('Laporan radiologi tersimpan.');
        await loadOrders();
        await selectOrder(selectedOrder.id);
      } catch (e) {
        showErr('report-error', 'Simpan laporan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('report-status-btn').addEventListener('click', async function () {
      if (!selectedOrder) return;
      clearMsg('report-error', 'report-ok');
      try {
        await Waras.api('/api/radiologi/orders/' + encodeURIComponent(selectedOrder.id) + '/status', {
          method: 'POST',
          body: { status: document.getElementById('rad-status').value }
        });
        showOk('report-ok', 'Status diubah.');
        Waras.toast('Status radiologi diubah.');
        await loadOrders();
      } catch (e) {
        showErr('report-error', 'Ubah status gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });
  });
})();
