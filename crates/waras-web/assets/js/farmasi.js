// Farmasi — GET /api/prescriptions (e-resep), POST /api/prescriptions/:id/dispense,
// GET /api/drugs (stok + KFA). Status stok dari Drug::stock_status() di API.
(function () {
  'use strict';

  async function loadRx() {
    var tbody = document.getElementById('rx-body');
    try {
      var list = Waras.asList(await Waras.api('/api/prescriptions'));
      var map = await Waras.patientsMap();
      tbody.innerHTML = list.map(function (r) {
        var canDispense = String(r.status).toLowerCase() === 'aktif';
        return '<tr><td>' + Waras.esc(r.id) + '</td>' +
          '<td>' + Waras.esc(Waras.patientName(map, r.patient_id)) + '</td>' +
          '<td>' + Waras.esc(Waras.drugDose(r.drug_name, r.dosage)) + '</td>' +
          '<td>' + Waras.esc(r.doctor) + '</td>' +
          '<td>' + Waras.pillFor(r.status) + '</td>' +
          '<td>' + (canDispense
            ? '<button class="btn sm primary" data-dispense="' + Waras.esc(r.id) + '">Berikan</button>'
            : '<span class="muted">—</span>') + '</td></tr>';
      }).join('') || '<tr><td colspan="6" class="empty-note">Belum ada e-resep.</td></tr>';
      tbody.querySelectorAll('[data-dispense]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
          btn.disabled = true;
          try {
            await Waras.api('/api/prescriptions/' + encodeURIComponent(btn.getAttribute('data-dispense')) + '/dispense', { method: 'POST' });
            Waras.toast('Resep ' + btn.getAttribute('data-dispense') + ' diberikan; stok berkurang.');
            await refreshAll();
          } catch (e) {
            Waras.toast('Gagal memberikan resep: ' + e.message, true);
            btn.disabled = false;
          }
        });
      });
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">E-resep tidak dapat dimuat: ' + Waras.esc(e.message) + '</td></tr>';
    }
  }

  async function loadDrugs() {
    try {
      var drugs = Waras.asList(await Waras.api('/api/drugs'));
      // Panel stok: 5 pertama, seperti desain.
      document.getElementById('stock-list').innerHTML = drugs.slice(0, 5).map(function (d) {
        var status = d.stock_status || (d.stock <= 12 ? 'kritis' : d.stock <= 30 ? 'segera' : 'aman');
        return '<div class="queue-mini" style="justify-content:space-between"><div><strong>' + Waras.esc(d.name) + '</strong><br>' +
          '<span class="muted small">Sisa ' + Waras.fmtNum(d.stock) + ' · exp ' + Waras.esc(String(d.expiry).slice(0, 7)) + '</span></div>' +
          Waras.pillFor(status) + '</div>';
      }).join('');
      // Tabel KFA lengkap.
      document.getElementById('kfa-body').innerHTML = drugs.map(function (d) {
        var status = d.stock_status || (d.stock <= 12 ? 'kritis' : d.stock <= 30 ? 'segera' : 'aman');
        return '<tr><td>' + Waras.esc(d.kfa_code) + '</td><td>' + Waras.esc(d.name) + '</td>' +
          '<td>' + Waras.esc(d.form) + '</td><td class="num">' + Waras.fmtNum(d.stock) + '</td>' +
          '<td>' + Waras.esc(d.location) + '</td><td>' + Waras.pillFor(status) + '</td></tr>';
      }).join('');
    } catch (e) {
      Waras.toast('Stok obat gagal dimuat: ' + e.message, true);
    }
  }

  async function refreshAll() { await loadRx(); await loadDrugs(); }

  document.addEventListener('DOMContentLoaded', function () {
    refreshAll();
    document.getElementById('btn-simpan').addEventListener('click', function () {
      refreshAll();
      Waras.toast('Data farmasi dimuat ulang.');
    });
    document.getElementById('btn-batal').addEventListener('click', function () { refreshAll(); });
    document.getElementById('btn-unduh').addEventListener('click', async function () {
      try {
        var list = Waras.asList(await Waras.api('/api/prescriptions'));
        var map = await Waras.patientsMap();
        var csv = 'No. Resep,Pasien,Obat,Dosis,Durasi,Dokter,Status\n' + list.map(function (r) {
          return [r.id, Waras.patientName(map, r.patient_id), r.drug_name, r.dosage, r.duration, r.doctor, Waras.labelFor(r.status)]
            .map(function (v) { return '"' + String(v).replace(/"/g, '""') + '"'; }).join(',');
        }).join('\n');
        var a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }));
        a.download = 'e-resep-' + Waras.TODAY + '.csv';
        a.click();
        URL.revokeObjectURL(a.href);
      } catch (e) { Waras.toast('Unduh gagal: ' + e.message, true); }
    });
    document.getElementById('btn-cetak').addEventListener('click', function () { window.print(); });
    document.getElementById('btn-tutup').addEventListener('click', function () { location.href = '/dashboard'; });
  });
})();
