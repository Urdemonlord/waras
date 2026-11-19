// Billing & Kasir — GET /api/dashboard (pendapatan + belum lunas),
// GET /api/invoices, POST /api/invoices/:id/pay.
// CATATAN (agregat statis berlabel, bukan dari API Fase 1 — lihat CONTRACT §4):
// "Piutang BPJS Rp 212 jt" dan "Deposit rawat inap Rp 46 jt" adalah agregat
// desain dari klaim BPJS + deposit; API Fase 1 belum mengekspos agregat itu,
// jadi nilainya tampil sebagai label statis sampai ada endpointnya.
(function () {
  'use strict';

  var selectedInvoice = null;
  var payMethod = 'tunai';

  async function loadSummary() {
    var k = await Waras.api('/api/dashboard');
    document.getElementById('sum-revenue').textContent = Waras.fmtJt(k.revenue_today_idr);
    document.getElementById('sum-revenue-sub').textContent = Waras.fmtNum(k.revenue_transactions) + ' transaksi';
    document.getElementById('sum-unpaid').textContent = Waras.fmtJt(k.unpaid_invoices_idr);
    document.getElementById('sum-unpaid-sub').textContent = Waras.fmtNum(k.unpaid_invoices_count) + ' tagihan';
  }

  async function loadInvoices() {
    var tbody = document.getElementById('inv-body');
    var list = Waras.asList(await Waras.api('/api/invoices'));
    var map = await Waras.patientsMap();
    tbody.innerHTML = list.map(function (inv) {
      return '<tr class="clickable" data-inv="' + Waras.esc(inv.id) + '">' +
        '<td>' + Waras.esc(inv.id) + '</td>' +
        '<td>' + Waras.esc(Waras.patientName(map, inv.patient_id)) + '</td>' +
        '<td>' + Waras.esc(Waras.labelFor(inv.payer)) + '</td>' +
        '<td>' + Waras.esc(inv.service) + '</td>' +
        '<td class="num">' + Waras.fmtRp(inv.amount) + '</td>' +
        '<td>' + Waras.pillFor(inv.status) + '</td></tr>';
    }).join('') || '<tr><td colspan="6" class="empty-note">Belum ada tagihan.</td></tr>';

    // Panel bayar: hanya tagihan yang belum lunas.
    var unpaid = list.filter(function (inv) { return String(inv.status).toLowerCase() !== 'lunas'; });
    var sel = document.getElementById('pay-invoice');
    sel.innerHTML = unpaid.map(function (inv) {
      return '<option value="' + Waras.esc(inv.id) + '">' + Waras.esc(inv.id) + ' · ' +
        Waras.esc(Waras.patientName(map, inv.patient_id)) + ' · ' + Waras.fmtRp(inv.amount) + '</option>';
    }).join('');
    selectInvoice(unpaid[0] || null, map);
    sel.addEventListener('change', function () {
      var inv = list.find(function (x) { return x.id === sel.value; });
      selectInvoice(inv || null, map);
    });
    tbody.querySelectorAll('tr.clickable').forEach(function (tr) {
      tr.addEventListener('click', function () {
        var inv = list.find(function (x) { return x.id === tr.getAttribute('data-inv'); });
        if (inv && String(inv.status).toLowerCase() !== 'lunas') {
          sel.value = inv.id;
          selectInvoice(inv, map);
          document.getElementById('pay-panel').scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
      });
    });
  }

  function selectInvoice(inv, map) {
    selectedInvoice = inv;
    var label = document.getElementById('pay-label');
    var btn = document.getElementById('pay-btn');
    var amount = document.getElementById('pay-amount');
    if (!inv) {
      label.textContent = 'Semua tagihan sudah lunas.';
      btn.disabled = true;
      amount.value = '';
      return;
    }
    label.textContent = 'Tagihan terpilih: ' + inv.id + ' · ' + Waras.patientName(map, inv.patient_id) +
      ' · ' + Waras.fmtRp(inv.amount);
    btn.disabled = false;
    btn.textContent = 'Terima Pembayaran ' + Waras.fmtRp(inv.amount);
    amount.value = inv.amount;
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadSummary().catch(function (e) { Waras.toast('Ringkasan gagal dimuat: ' + e.message, true); });
    loadInvoices().catch(function (e) { Waras.toast('Tagihan gagal dimuat: ' + e.message, true); });

    document.querySelectorAll('.method[data-method]').forEach(function (m) {
      m.addEventListener('click', function () {
        document.querySelectorAll('.method[data-method]').forEach(function (x) { x.classList.remove('active'); });
        m.classList.add('active');
        payMethod = m.getAttribute('data-method');
        if (selectedInvoice) {
          document.getElementById('pay-btn').textContent =
            'Terima Pembayaran ' + Waras.fmtRp(document.getElementById('pay-amount').value || selectedInvoice.amount);
        }
      });
    });

    document.getElementById('pay-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      if (!selectedInvoice) return;
      var btn = document.getElementById('pay-btn');
      btn.disabled = true;
      try {
        await Waras.api('/api/invoices/' + encodeURIComponent(selectedInvoice.id) + '/pay', {
          method: 'POST',
          body: { method: payMethod, amount: Number(document.getElementById('pay-amount').value) || selectedInvoice.amount }
        });
        Waras.toast('Pembayaran ' + selectedInvoice.id + ' diterima.');
        await loadSummary();
        await loadInvoices();
      } catch (e) {
        Waras.toast('Pembayaran gagal: ' + e.message, true);
        btn.disabled = false;
      }
    });
    document.getElementById('btn-simpan').addEventListener('click', function () {
      Waras.toast('Tidak ada perubahan tagihan untuk disimpan.');
    });
    document.getElementById('btn-batal').addEventListener('click', function () { loadInvoices(); });
    document.getElementById('btn-unduh').addEventListener('click', async function () {
      try {
        var list = Waras.asList(await Waras.api('/api/invoices'));
        var map = await Waras.patientsMap();
        var csv = 'No. Tagihan,Pasien,Penjamin,Layanan,Jumlah,Status\n' + list.map(function (inv) {
          return [inv.id, Waras.patientName(map, inv.patient_id), Waras.labelFor(inv.payer), inv.service, inv.amount, Waras.labelFor(inv.status)]
            .map(function (v) { return '"' + String(v).replace(/"/g, '""') + '"'; }).join(',');
        }).join('\n');
        var a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }));
        a.download = 'tagihan-' + Waras.TODAY + '.csv';
        a.click();
        URL.revokeObjectURL(a.href);
      } catch (e) { Waras.toast('Unduh gagal: ' + e.message, true); }
    });
    document.getElementById('btn-cetak').addEventListener('click', function () { window.print(); });
    document.getElementById('btn-kwitansi').addEventListener('click', function () { window.print(); });
    document.getElementById('btn-tutup').addEventListener('click', function () { location.href = '/dashboard'; });
  });
})();
