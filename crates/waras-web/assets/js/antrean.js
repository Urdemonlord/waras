// Antrean Online & Aplicares: GET /api/antrean-online, POST
// /api/antrean-online/sync, GET /api/beds, POST /api/beds/refresh.
// Semua angka dari API (FASE2.md §6, §2.3). Endpoint Fase 2 belum ada di
// server saat halaman ini dibuat: setiap loader menangkap 404/503 dan
// menampilkan status "belum tersedia" — halaman tidak pernah blank/crash.
// Aksi sinkron/perbarui yang dijawab 503 menampilkan "BPJS belum dikonfigurasi".
(function () {
  'use strict';

  function queueStatusPill(status) {
    var s = String(status || '').toLowerCase();
    if (s === 'hadir') return '<span class="pill green"><i class="dot"></i>Hadir</span>';
    if (s === 'menunggu') return '<span class="pill amber"><i class="dot"></i>Menunggu</span>';
    return Waras.pillFor(status);
  }

  async function loadQueue() {
    var tbody = document.getElementById('queue-body');
    var map = {};
    try { map = await Waras.patientsMap(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/antrean-online'));
      tbody.innerHTML = list.map(function (b) {
        return '<tr><td>' + Waras.esc(b.booking_code || b.id) + '</td>' +
          '<td>' + Waras.esc(Waras.patientName(map, b.patient_id)) + '</td>' +
          '<td>' + Waras.esc(b.poli) + '</td>' +
          '<td>' + Waras.esc(b.estimasi) + '</td>' +
          '<td>' + queueStatusPill(b.status) + '</td></tr>';
      }).join('') || '<tr><td colspan="5" class="empty-note">Belum ada booking antrean online hari ini.</td></tr>';
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="5" class="empty-note">Antrean online belum tersedia (endpoint /api/antrean-online belum aktif).</td></tr>';
    }
  }

  async function loadBeds() {
    var box = document.getElementById('bed-list');
    var note = document.getElementById('bed-note');
    try {
      var list = Waras.asList(await Waras.api('/api/beds'));
      if (!list.length) {
        box.innerHTML = '<p class="empty-note">Belum ada data ketersediaan tempat tidur.</p>';
        note.textContent = '';
        return;
      }
      box.innerHTML = list.map(function (b) {
        var avail = Number(b.available) || 0;
        var total = Number(b.total) || 0;
        var pill = avail > 0
          ? '<span class="pill green"><i class="dot"></i>Tersedia</span>'
          : '<span class="pill red"><i class="dot"></i>Penuh</span>';
        return '<div style="display:flex;align-items:center;gap:8px;padding:10px 0;border-bottom:1px solid #F1F2F4">' +
          '<div style="flex:1"><strong>' + Waras.esc(b.class) + '</strong><br>' +
          '<span class="muted small">Tersedia ' + Waras.fmtNum(avail) + ' / ' + Waras.fmtNum(total) + '</span></div>' +
          pill + '</div>';
      }).join('');
      // Jam pembaruan terakhir dari baris mana pun (bila server mengirimnya).
      var latest = null;
      list.forEach(function (b) { if (b.updated_at && (!latest || String(b.updated_at) > String(latest))) latest = b.updated_at; });
      note.textContent = latest ? 'Terakhir diperbarui ' + Waras.fmtDateId(latest) + '.' : '';
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Ketersediaan tempat tidur belum tersedia (endpoint /api/beds belum aktif).</p>';
      note.textContent = '';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadQueue();
    loadBeds();

    document.getElementById('btn-sync').addEventListener('click', async function (ev) {
      ev.preventDefault();
      try {
        await Waras.api('/api/antrean-online/sync', { method: 'POST' });
        Waras.toast('Antrean online disinkronkan.');
        await loadQueue();
      } catch (e) {
        if (e && e.status === 503) Waras.toast('BPJS belum dikonfigurasi', true);
        else Waras.toast('Sinkronisasi gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'), true);
      }
    });

    document.getElementById('btn-refresh-beds').addEventListener('click', async function (ev) {
      ev.preventDefault();
      try {
        await Waras.api('/api/beds/refresh', { method: 'POST' });
        Waras.toast('Ketersediaan tempat tidur diperbarui.');
        await loadBeds();
      } catch (e) {
        if (e && e.status === 503) Waras.toast('BPJS belum dikonfigurasi', true);
        else Waras.toast('Perbarui gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'), true);
      }
    });
  });
})();
