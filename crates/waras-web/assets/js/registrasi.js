// Registrasi & Antrean — POST /api/patients lalu POST /api/queue/take;
// GET /api/patients + GET /api/queue untuk tabel pendaftaran hari ini.
// Penjamin BPJS membutuhkan nomor BPJS; poli menentukan prefix nomor antrean
// (A = Poli Umum, sesuai CONTRACT §3).
(function () {
  'use strict';

  var POLI_PREFIX = { 'Poli Umum': 'A', 'Poli Anak': 'B', 'Penyakit Dalam': 'C', 'Gigi': 'D', 'Mata': 'E' };

  async function loadQueuePanel() {
    var queue = Waras.asList(await Waras.api('/api/queue?date=' + Waras.TODAY));
    var map = await Waras.patientsMap();
    // Nomor berikutnya: satu lebih dari nomor terbesar hari ini (prefix A).
    var maxNum = 0;
    queue.forEach(function (q) {
      var m = /^A-(\d+)$/.exec(q.number || '');
      if (m) maxNum = Math.max(maxNum, parseInt(m[1], 10));
    });
    document.getElementById('next-number').textContent = 'A-' + String(maxNum + 1).padStart(3, '0');
    var last = queue.slice(-3).reverse();
    document.getElementById('queue-mini').innerHTML = last.map(function (q) {
      return '<div class="queue-mini"><strong>' + Waras.esc(q.number) + '</strong> · ' +
        Waras.esc(Waras.patientName(map, q.patient_id)) + ' — ' + Waras.pillFor(q.status) + '</div>';
    }).join('') || '<p class="empty-note">Belum ada antrean hari ini.</p>';
    // Tombol "Panggil berikutnya": antrean Menunggu paling awal.
    var btn = document.getElementById('call-next-btn');
    var next = queue.find(function (q) { return String(q.status).toLowerCase() === 'menunggu'; });
    btn.disabled = !next;
    btn.onclick = async function () {
      if (!next) return;
      btn.disabled = true;
      try {
        await Waras.api('/api/queue/' + encodeURIComponent(next.id) + '/call', { method: 'POST' });
        Waras.toast('Memanggil ' + next.number + ' — ' + Waras.patientName(map, next.patient_id) + '.');
        await refreshAll();
      } catch (e) {
        Waras.toast('Gagal memanggil: ' + e.message, true);
        btn.disabled = false;
      }
    };
    return queue;
  }

  async function loadTable(queue) {
    var tbody = document.getElementById('reg-body');
    var map = await Waras.patientsMap();
    var qByPatient = {};
    (queue || []).forEach(function (q) { qByPatient[q.patient_id] = q; });
    var patients = Waras.asList(await Waras.api('/api/patients')).slice(0, 8);
    document.getElementById('reg-count').textContent = patients.length;
    tbody.innerHTML = patients.map(function (p) {
      var q = qByPatient[p.id];
      return '<tr class="clickable" data-pasien="' + Waras.esc(p.id) + '">' +
        '<td>' + Waras.esc(p.id) + '</td>' +
        '<td>' + Waras.esc(p.name) + '</td>' +
        '<td>' + Waras.esc(p.nik) + '</td>' +
        '<td>' + Waras.esc(q ? q.poli : '—') + '</td>' +
        '<td>' + Waras.esc(Waras.labelFor(p.insurance)) + '</td>' +
        '<td>' + Waras.esc(q ? q.time : '—') + '</td>' +
        '<td>' + (q ? Waras.pillFor(q.status) : '<span class="muted">Belum antre</span>') + '</td></tr>';
    }).join('');
    tbody.querySelectorAll('tr.clickable').forEach(function (tr) {
      tr.addEventListener('click', function () {
        location.href = '/rme?pasien=' + encodeURIComponent(tr.getAttribute('data-pasien'));
      });
    });
  }

  async function refreshAll() {
    var queue = await loadQueuePanel();
    await loadTable(queue);
  }

  document.addEventListener('DOMContentLoaded', function () {
    refreshAll().catch(function (e) { Waras.toast('Registrasi gagal dimuat: ' + e.message, true); });

    var penjamin = document.getElementById('penjamin');
    var bpjsRow = document.getElementById('bpjs-row');
    penjamin.addEventListener('change', function () {
      bpjsRow.style.display = penjamin.value === 'bpjs' ? '' : 'none';
    });

    var form = document.getElementById('reg-form');
    var err = document.getElementById('reg-error');
    var ok = document.getElementById('reg-ok');
    form.addEventListener('submit', async function (ev) {
      ev.preventDefault();
      err.classList.remove('on');
      ok.classList.remove('on');
      var btn = document.getElementById('reg-submit');
      btn.disabled = true;
      btn.textContent = 'Menyimpan…';
      try {
        var nik = document.getElementById('nik').value.trim();
        if (!/^\d{16}$/.test(nik)) throw new Error('NIK harus 16 digit angka.');
        var patient = await Waras.api('/api/patients', {
          method: 'POST',
          body: {
            nik: nik,
            name: document.getElementById('nama').value.trim(),
            birth_date: document.getElementById('lahir').value,
            gender: document.getElementById('gender').value,
            address: document.getElementById('alamat').value.trim(),
            insurance: penjamin.value,
            bpjs_number: penjamin.value === 'bpjs' ? document.getElementById('bpjs').value.trim() || null : null,
            blood_type: null,
            phone: null
          }
        });
        var q = await Waras.api('/api/queue/take', {
          method: 'POST',
          body: {
            patient_id: patient.id,
            poli: document.getElementById('poli').value,
            doctor: document.getElementById('dokter').value
          }
        });
        ok.textContent = 'Pasien ' + patient.name + ' terdaftar (' + patient.id + ') — nomor antrean ' + (q.number || '') + '.';
        ok.classList.add('on');
        form.reset();
        bpjsRow.style.display = 'none';
        await refreshAll();
      } catch (e) {
        err.textContent = e.message || 'Pendaftaran gagal.';
        err.classList.add('on');
      } finally {
        btn.disabled = false;
        btn.textContent = 'Simpan & Ambil Nomor Antrean';
      }
    });

    document.getElementById('btn-batal').addEventListener('click', function () {
      form.reset(); bpjsRow.style.display = 'none'; err.classList.remove('on'); ok.classList.remove('on');
    });
    document.getElementById('btn-cetak').addEventListener('click', function () { window.print(); });
    document.getElementById('btn-unduh').addEventListener('click', async function () {
      try {
        var queue = Waras.asList(await Waras.api('/api/queue?date=' + Waras.TODAY));
        var map = await Waras.patientsMap();
        var csv = 'Nomor,Pasien,No RM,Poli,Jam,Status\n' + queue.map(function (q) {
          return [q.number, Waras.patientName(map, q.patient_id), q.patient_id, q.poli, q.time, Waras.labelFor(q.status)]
            .map(function (v) { return '"' + String(v).replace(/"/g, '""') + '"'; }).join(',');
        }).join('\n');
        var a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }));
        a.download = 'antrean-' + Waras.TODAY + '.csv';
        a.click();
        URL.revokeObjectURL(a.href);
      } catch (e) { Waras.toast('Unduh gagal: ' + e.message, true); }
    });
    document.getElementById('btn-tutup').addEventListener('click', function () { location.href = '/dashboard'; });
  });
})();
