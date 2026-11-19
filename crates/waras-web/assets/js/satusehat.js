// SATUSEHAT — FHIR R4: GET /api/bridges, GET /api/satusehat/resources,
// GET /api/outbox?status=dead&limit=10 (+ destination=satusehat untuk jam kirim).
// Semua angka dari API (FASE2.md §6, §2.3). Endpoint Fase 2 belum ada di
// server saat halaman ini dibuat: setiap loader menangkap 404/503 dan
// menampilkan status "belum tersedia" — halaman tidak pernah blank/crash.
(function () {
  'use strict';

  // Resource FHIR prioritas (label statis dari desain v16; angkanya dari API).
  var PRIORITY = ['Patient', 'Encounter', 'Condition', 'Observation',
    'MedicationRequest', 'Procedure', 'Medication', 'AllergyIntolerance'];
  var TOTAL_TARGET = 57; // Target Kemenkes (FASE2.md §6).

  var HEALTH_KIND = {
    online: 'green', aktif: 'green', peringatan: 'amber',
    kritis: 'red', offline: 'neutral', belum_dikonfigurasi: 'neutral'
  };
  var HEALTH_LABEL = {
    online: 'Online', aktif: 'Aktif', peringatan: 'Peringatan',
    kritis: 'Kritis', offline: 'Offline', belum_dikonfigurasi: 'Belum dikonfigurasi'
  };

  function healthOf(b) {
    var h = String((b && b.health) || '').toLowerCase();
    return HEALTH_LABEL[h] || (b && b.health) || '—';
  }
  function healthKind(b) {
    var h = String((b && b.health) || '').toLowerCase();
    return HEALTH_KIND[h] || 'neutral';
  }
  function bridgeName(b) {
    return (b && (b.name || b.destination)) || '—';
  }
  function isSatusehat(b) {
    var d = String((b && b.destination) || '').toLowerCase();
    return d === 'satusehat' || bridgeName(b).toUpperCase().indexOf('SATUSEHAT') !== -1;
  }
  function countsOf(b) {
    return (b && b.counts) || {};
  }
  function num(v) { return Number(v) || 0; }

  function hhmm(iso) {
    // "2026-11-19T09:41:00" -> "09.41" (jam WIB dari desain).
    if (!iso) return '—';
    var m = String(iso).match(/T(\d{2}):(\d{2})/);
    return m ? m[1] + '.' + m[2] : String(iso).slice(0, 10);
  }

  function setKpi(id, text) {
    var el = document.getElementById(id);
    if (el) el.textContent = text;
  }

  async function loadBridges() {
    var pills = document.getElementById('bridge-pills');
    try {
      var list = Waras.asList(await Waras.api('/api/bridges'));
      if (!list.length) {
        pills.innerHTML = '<span class="pill neutral"><i class="dot"></i>Belum ada jembatan</span>';
        return;
      }
      pills.innerHTML = list.map(function (b) {
        return '<span class="pill ' + healthKind(b) + '"><i class="dot"></i>' +
          Waras.esc(bridgeName(b)) + ' · ' + Waras.esc(healthOf(b)) + '</span>';
      }).join('');
      // KPI dari jembatan SATUSEHAT (counts outbox; FASE2.md §5/§6).
      var sat = list.filter(isSatusehat)[0];
      if (sat) {
        var c = countsOf(sat);
        setKpi('kpi-sent', Waras.fmtNum(num(c.acked)));
        setKpi('kpi-dead', Waras.fmtNum(num(c.dead)));
        setKpi('kpi-pending', Waras.fmtNum(num(c.pending)));
      }
    } catch (e) {
      pills.innerHTML = '<span class="pill neutral"><i class="dot"></i>Status jembatan belum tersedia</span>';
      // KPI tetap "—" + catatan di bawah tabel; bukan crash.
      var note = document.getElementById('resource-note');
      if (note) note.textContent = 'Endpoint /api/bridges belum aktif (404/503) — angka akan tampil setelah API Fase 2 tersedia.';
    }
  }

  async function loadResources() {
    var tbody = document.getElementById('resource-body');
    var rows = null;
    var lastSent = {};
    try {
      rows = Waras.asList(await Waras.api('/api/satusehat/resources'));
    } catch (e) {
      rows = null; // endpoint belum ada -> baris tampil dengan "—".
    }
    // Jam kirim terakhir per resource dari outbox SATUSEHAT (bila ada).
    try {
      var jobs = Waras.asList(await Waras.api('/api/outbox?destination=satusehat&limit=50'));
      jobs.forEach(function (j) {
        var rt = j.resource_type;
        if (!rt) return;
        var t = j.updated_at || j.created_at;
        if (t && (!lastSent[rt] || String(t) > String(lastSent[rt]))) lastSent[rt] = t;
      });
    } catch (e) { /* jam kirim opsional; kolom tampil "—" */ }

    var byName = {};
    (rows || []).forEach(function (r) { if (r && r.resource) byName[r.resource] = r; });
    // Resource terpetakan = jumlah tipe FHIR yang diimplementasikan (baris
    // yang dikembalikan server) / 57 target Kemenkes.
    if (rows) setKpi('kpi-mapped', Waras.fmtNum(Object.keys(byName).length) + ' / ' + TOTAL_TARGET);

    var names = PRIORITY.slice();
    Object.keys(byName).forEach(function (n) { if (names.indexOf(n) === -1) names.push(n); });
    tbody.innerHTML = names.map(function (name) {
      var r = byName[name];
      var sent = r ? num(r.sent) : 0;
      var failed = r ? num(r.failed) : 0;
      var pending = r ? num(r.pending) : 0;
      var total = sent + failed + pending;
      var status;
      if (!r) {
        status = '<span class="pill neutral"><i class="dot"></i>Belum tersedia</span>';
      } else if (failed > 0) {
        status = '<span class="pill red"><i class="dot"></i>Gagal ' + Waras.fmtNum(failed) + '</span>';
      } else if (sent > 0) {
        status = '<span class="pill blue"><i class="dot"></i>Terkirim</span>';
      } else if (pending > 0) {
        status = '<span class="pill amber"><i class="dot"></i>Menunggu</span>';
      } else {
        status = '<span class="pill neutral"><i class="dot"></i>Belum ada kiriman</span>';
      }
      return '<tr><td>' + Waras.esc(name) + '</td>' +
        '<td class="num">' + (r ? Waras.fmtNum(total) : '—') + '</td>' +
        '<td>' + Waras.esc(hhmm(lastSent[name])) + '</td>' +
        '<td>' + status + '</td></tr>';
    }).join('');
  }

  async function loadValidationLog() {
    var box = document.getElementById('validation-log');
    var pill = document.getElementById('sandbox-pill');
    try {
      var jobs = Waras.asList(await Waras.api('/api/outbox?status=dead&limit=10'));
      if (!jobs.length) {
        box.innerHTML = '<p class="empty-note">Tidak ada kegagalan validasi.</p>';
        pill.innerHTML = '<span class="pill green"><i class="dot"></i>Sandbox lulus</span>';
        return;
      }
      box.innerHTML = jobs.map(function (j) {
        var t = hhmm(j.updated_at || j.created_at);
        var err = j.last_error || 'Gagal tanpa pesan.';
        return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
          '<strong>' + Waras.esc(t) + '</strong> ' + Waras.esc(j.resource_type || '') +
          ' GAGAL<br><span class="muted">' + Waras.esc(err) + '</span></div>';
      }).join('');
      pill.innerHTML = '';
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Log validasi belum tersedia (endpoint /api/outbox belum aktif).</p>';
      pill.innerHTML = '';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadBridges();
    loadResources();
    loadValidationLog();
  });
})();
