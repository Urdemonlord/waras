// Rawat Inap: GET /api/ranap/beds?ward=, GET /api/ranap/admissions,
// POST /api/ranap/admissions, GET /api/ranap/admissions/{id},
// POST /api/ranap/admissions/{id}/discharge|transfer|notes.
// Semua angka dari API (FASE3.md §5). Peta bed 3A-03 mengikuti tabel
// admisi (Terisi), bukan gambar desain (FASE3.md §8.3).
// Endpoint Fase 3 belum ada saat halaman ini dibuat: loader menangkap
// 404/503 dan menampilkan "belum tersedia" — halaman tidak blank/crash.
(function () {
  'use strict';

  var BED_STYLE = {
    terisi: 'background:var(--red-bg);border:1px solid #FECACA;color:var(--red)',
    kosong: 'background:var(--green-bg);border:1px solid #BBF7D0;color:var(--green)',
    dibersihkan: 'background:var(--amber-bg);border:1px solid #FDE68A;color:var(--amber)'
  };
  var BED_DOT = { terisi: 'var(--red)', kosong: 'var(--green)', dibersihkan: 'var(--amber)' };
  var STATUS_KIND = { dirawat: 'teal', pulang: 'green', dirujuk: 'neutral' };
  var STATUS_LABEL = { dirawat: 'Dirawat', pulang: 'Pulang', dirujuk: 'Dirujuk' };

  var wards = [];            // [{ward_id, ward_name, kind, floor?, beds:[]}]
  var selectedWard = null;
  var bedWard = {};          // bed_id -> "Melati 3A-01"
  var emptyBeds = [];        // [WardBed Kosong]
  var selectedAdmission = null;
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
  function bedStatusKey(s) {
    return String(s == null ? '' : s).toLowerCase();
  }
  function bedLabel(b) {
    return (b && (b.ward_name || b.ward_id || '')) + ' ' + (b ? b.id : '');
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

  // GET /api/ranap/beds mengembalikan [{ward_id, ward_name, kind, beds:[]}];
  // bila server mengembalikan daftar bed datar, kelompokkan per ward di sini.
  function normalizeWards(data) {
    var list = Waras.asList(data);
    if (!list.length) return [];
    if (list[0].beds) return list;
    var byWard = {};
    list.forEach(function (b) {
      var w = b.ward_id || '—';
      if (!byWard[w]) byWard[w] = { ward_id: w, ward_name: w, kind: '', beds: [] };
      byWard[w].beds.push(b);
    });
    return Object.keys(byWard).map(function (k) { return byWard[k]; });
  }

  function renderWardTabs() {
    var box = document.getElementById('ward-tabs');
    box.innerHTML = wards.map(function (w) {
      var active = selectedWard && w.ward_id === selectedWard.ward_id;
      return '<button type="button" class="btn sm' + (active ? ' primary' : '') +
        '" data-ward="' + Waras.esc(w.ward_id) + '">' + Waras.esc(w.ward_name || w.ward_id) + '</button>';
    }).join('');
    box.querySelectorAll('button[data-ward]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        selectedWard = wards.filter(function (w) { return w.ward_id === btn.getAttribute('data-ward'); })[0];
        renderWardTabs();
        renderBedGrid();
      });
    });
  }

  function renderBedGrid() {
    var label = document.getElementById('bedmap-ward-label');
    var grid = document.getElementById('bed-grid');
    if (!selectedWard) {
      label.textContent = '—';
      grid.innerHTML = '<p class="empty-note">Belum ada bangsal.</p>';
      return;
    }
    var kind = selectedWard.kind ? ' (' + selectedWard.kind + ')' : '';
    var floor = selectedWard.floor ? ' · ' + selectedWard.floor : '';
    label.textContent = 'Ruang ' + (selectedWard.ward_name || selectedWard.ward_id) + kind + floor;
    var beds = selectedWard.beds || [];
    if (!beds.length) {
      grid.innerHTML = '<p class="empty-note">Belum ada tempat tidur di bangsal ini.</p>';
      return;
    }
    grid.innerHTML = beds.map(function (b) {
      var k = bedStatusKey(b.status);
      var style = BED_STYLE[k] || 'background:var(--neutral-bg);border:1px solid var(--border)';
      var dot = BED_DOT[k] || 'var(--neutral)';
      var statusLabel = b.status || '—';
      return '<div style="' + style + ';border-radius:8px;padding:10px 12px;font-size:13px">' +
        '<strong>' + Waras.esc(b.id) + '</strong><br>' +
        '<span style="color:' + dot + '">●</span> ' + Waras.esc(statusLabel) + '</div>';
    }).join('');
  }

  function renderOccupancy() {
    var box = document.getElementById('occupancy-list');
    var totalTerisi = 0, totalBeds = 0;
    box.innerHTML = wards.map(function (w) {
      var beds = w.beds || [];
      var terisi = beds.filter(function (b) { return bedStatusKey(b.status) === 'terisi'; }).length;
      var total = beds.length;
      totalTerisi += terisi; totalBeds += total;
      var pct = total ? Math.round(terisi * 100 / total) : 0;
      var kind = w.kind ? ' (' + w.kind + ')' : '';
      return '<div style="margin-bottom:12px"><div style="display:flex;justify-content:space-between;font-size:13px"><strong>' +
        Waras.esc(w.ward_name || w.ward_id) + kind + '</strong></div>' +
        '<div class="muted" style="font-size:12px">Terisi ' + Waras.fmtNum(terisi) + ' / ' + Waras.fmtNum(total) + ' tempat tidur</div>' +
        '<div style="height:8px;background:var(--neutral-bg);border-radius:4px;margin-top:4px;overflow:hidden">' +
        '<div style="height:100%;width:' + pct + '%;background:var(--accent)"></div></div></div>';
    }).join('') || '<p class="empty-note">Belum ada data hunian.</p>';
    setText('occupancy-total', totalBeds ?
      'Total terisi ' + Waras.fmtNum(totalTerisi) + ' / ' + Waras.fmtNum(totalBeds) + ' tempat tidur' : '—');
  }

  async function loadBeds() {
    try {
      var data = await Waras.api('/api/ranap/beds');
      wards = normalizeWards(data);
      bedWard = {};
      emptyBeds = [];
      wards.forEach(function (w) {
        (w.beds || []).forEach(function (b) {
          bedWard[b.id] = (w.ward_name || w.ward_id) + ' ' + b.id;
          if (bedStatusKey(b.status) === 'kosong') emptyBeds.push(b);
        });
      });
      if (!selectedWard && wards.length) selectedWard = wards[0];
      renderWardTabs();
      renderBedGrid();
      renderOccupancy();
      fillBedSelects();
    } catch (e) {
      document.getElementById('ward-tabs').innerHTML =
        '<span class="pill neutral"><i class="dot"></i>Peta bed belum tersedia</span>';
      document.getElementById('bed-grid').innerHTML =
        '<p class="empty-note">Peta tempat tidur belum tersedia (endpoint /api/ranap/beds belum aktif).</p>';
      document.getElementById('occupancy-list').innerHTML =
        '<p class="empty-note">Ringkasan hunian belum tersedia.</p>';
    }
  }

  function fillBedSelects() {
    var opts = emptyBeds.length ? emptyBeds.map(function (b) {
      return '<option value="' + Waras.esc(b.id) + '">' + Waras.esc(bedLabel(b)) + '</option>';
    }).join('') : '<option value="">Tidak ada bed kosong</option>';
    var adm = document.getElementById('adm-bed');
    if (adm) adm.innerHTML = opts;
    var tr = document.getElementById('transfer-bed');
    if (tr) tr.innerHTML = opts;
  }

  async function loadAdmissions() {
    var tbody = document.getElementById('ranap-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/ranap/admissions?status=Dirawat'));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Belum ada pasien rawat inap.</td></tr>';
        document.getElementById('fall-chips').innerHTML = '<span class="muted">Tidak ada.</span>';
        return;
      }
      tbody.innerHTML = list.map(function (a) {
        var k = String(a.status == null ? '' : a.status).toLowerCase();
        var diag = (a.diagnosis_code || '') + (a.diagnosis_display ? ' ' + a.diagnosis_display : '');
        return '<tr data-id="' + Waras.esc(a.id) + '" style="cursor:pointer"><td>' +
          Waras.esc(a.patient_id || '') + '</td><td>' +
          Waras.esc(a.patient_name || Waras.patientName(map, a.patient_id)) + '</td><td>' +
          Waras.esc(bedWard[a.bed_id] || a.bed_id || '') + '</td><td>' +
          Waras.esc(diag) + '</td><td>' +
          Waras.esc(Waras.fmtDateId(a.admitted_date)) + '</td><td>' +
          Waras.esc(a.dpjp || '') + '</td><td>' +
          Waras.statusPill(STATUS_LABEL[k] || String(a.status), STATUS_KIND[k] || 'neutral') + '</td></tr>';
      }).join('');
      tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
        tr.addEventListener('click', function () { selectAdmission(tr.getAttribute('data-id')); });
      });
      var falls = list.filter(function (a) { return a.fall_risk; });
      document.getElementById('fall-chips').innerHTML = falls.length ? falls.map(function (a) {
        return '<span class="pill blue"><i class="dot"></i>' +
          Waras.esc(Waras.patientName(map, a.patient_id)) + ' — ' +
          Waras.esc(bedWard[a.bed_id] || a.bed_id || '') + '</span>';
      }).join(' ') : '<span class="muted">Tidak ada.</span>';
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Pasien rawat inap belum tersedia (endpoint /api/ranap/admissions belum aktif).</td></tr>';
    }
  }

  async function selectAdmission(id) {
    var box = document.getElementById('care-list');
    var actions = document.getElementById('detail-actions');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/ranap/admissions/' + encodeURIComponent(id));
      var adm = data.admission || data;
      selectedAdmission = adm;
      var notes = Waras.asList(data.care_notes);
      setText('detail-label', Waras.patientName(map, adm.patient_id) + ' · ' +
        (bedWard[adm.bed_id] || adm.bed_id || '') + ' · ' + (adm.id || id));
      setText('care-confirm', confirmLine(map, adm.patient_id));
      box.innerHTML = notes.length ? notes.map(function (n) {
        return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
          '<strong>' + Waras.esc(Waras.patientName(map, adm.patient_id)) + '</strong> · ' +
          Waras.esc(n.note || '') + '<br><span class="muted">' +
          Waras.esc(n.recorded_by || '') + ' · ' + Waras.esc(n.recorded_at || '') + '</span></div>';
      }).join('') : '<p class="empty-note">Belum ada catatan perawatan untuk admisi ini.</p>';
      actions.style.display = 'block';
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Detail admisi belum tersedia (' + Waras.esc(e.message || 'endpoint belum aktif') + ').</p>';
      actions.style.display = 'none';
    }
  }

  async function fillPatientSelect() {
    var sel = document.getElementById('adm-patient');
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncAdmConfirm();
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncAdmConfirm() {
    var sel = document.getElementById('adm-patient');
    var opt = sel.options[sel.selectedIndex];
    setText('adm-confirm', opt && opt.value ?
      'Pasien: ' + opt.textContent.replace(' · ', ' (') + ')' : 'Pasien: —');
  }

  async function reloadAll() {
    await loadBeds();
    await loadAdmissions();
  }

  document.addEventListener('DOMContentLoaded', function () {
    reloadAll();

    var newBtn = document.getElementById('btn-new-admission');
    var card = document.getElementById('admission-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect();
      document.getElementById('adm-patient').focus();
    });
    document.getElementById('adm-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('adm-error', 'adm-ok');
    });
    document.getElementById('adm-patient').addEventListener('change', syncAdmConfirm);

    document.getElementById('admission-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('adm-submit');
      btn.disabled = true;
      clearMsg('adm-error', 'adm-ok');
      try {
        var encounter = document.getElementById('adm-encounter').value.trim();
        var adm = await Waras.api('/api/ranap/admissions', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('adm-patient').value,
            bed_id: document.getElementById('adm-bed').value,
            encounter_id: encounter || undefined,
            diagnosis_code: document.getElementById('adm-diag-code').value.trim(),
            diagnosis_display: document.getElementById('adm-diag-display').value.trim(),
            dpjp: document.getElementById('adm-dpjp').value.trim(),
            fall_risk: document.getElementById('adm-fall-risk').checked
          }
        });
        showOk('adm-ok', 'Admisi ' + (adm.id || '') + ' tersimpan.');
        Waras.toast('Admisi tersimpan.');
        await reloadAll();
      } catch (e) {
        showErr('adm-error', 'Simpan admisi gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('care-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      if (!selectedAdmission) return;
      var btn = document.getElementById('care-submit');
      btn.disabled = true;
      clearMsg('care-error', 'care-ok');
      try {
        await Waras.api('/api/ranap/admissions/' + encodeURIComponent(selectedAdmission.id) + '/notes', {
          method: 'POST',
          body: { note: document.getElementById('care-note').value.trim() }
        });
        showOk('care-ok', 'Catatan ditambahkan.');
        document.getElementById('care-note').value = '';
        await selectAdmission(selectedAdmission.id);
      } catch (e) {
        showErr('care-error', 'Tambah catatan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('transfer-btn').addEventListener('click', async function () {
      if (!selectedAdmission) return;
      clearMsg('detail-error', 'detail-ok');
      try {
        await Waras.api('/api/ranap/admissions/' + encodeURIComponent(selectedAdmission.id) + '/transfer', {
          method: 'POST',
          body: { bed_id: document.getElementById('transfer-bed').value }
        });
        showOk('detail-ok', 'Mutasi bed berhasil.');
        Waras.toast('Mutasi bed berhasil.');
        await reloadAll();
        await selectAdmission(selectedAdmission.id);
      } catch (e) {
        showErr('detail-error', 'Mutasi bed gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });

    document.getElementById('discharge-btn').addEventListener('click', async function () {
      if (!selectedAdmission) return;
      if (!window.confirm('Pulangkan pasien ini? Tempat tidur akan berstatus Dibersihkan.')) return;
      clearMsg('detail-error', 'detail-ok');
      try {
        await Waras.api('/api/ranap/admissions/' + encodeURIComponent(selectedAdmission.id) + '/discharge', {
          method: 'POST', body: {}
        });
        showOk('detail-ok', 'Pasien dipulangkan.');
        Waras.toast('Pasien dipulangkan.');
        await reloadAll();
      } catch (e) {
        showErr('detail-error', 'Pulang gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });
  });
})();
