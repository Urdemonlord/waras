// Casemix & Klaim INA-CBG: GET /api/casemix,
// POST /api/casemix/group, GET /api/casemix/{id},
// GET /api/casemix/{id}/export.
// Semua angka dari API (FASE3.md §5, §8.5: KPI dihitung dari klaim
// nyata, BUKAN 486 / Rp 1,42 miliar di desain). Grouping lokal
// (perkiraan) — tarif contoh, bukan tarif resmi Kemenkes (FASE3 §8.7).
// Tombol "Kirim ke E-Klaim" HANYA membuka penjelasan jujur + salin
// JSON (FASE3 §7): WARAS tidak mengirim langsung, tidak ada endpoint
// submit, tidak ada URL E-Klaim yang ditebak.
// Catatan: Claim (waras-core) tidak punya kolom pasien/diagnosis;
// pasien diurai dari invoice/SEP bila klaim menautkannya, diagnosis
// dari grouping_note ("Diagnosis utama: ..."). Bila agen B memperkaya
// respons dengan patient_name/diagnosis, kolom itu dipakai dulu.
(function () {
  'use strict';

  var CLAIM_KIND = {
    dibayar: 'green', terverifikasi: 'green', diajukan: 'amber',
    pending: 'amber', draft: 'amber', terkirim: 'teal',
    diterima: 'green', ditolak: 'red'
  };
  var CLAIM_LABEL = {
    dibayar: 'Dibayar', terverifikasi: 'Terverifikasi', diajukan: 'Diajukan',
    pending: 'Pending', draft: 'Draft', terkirim: 'Terkirim',
    diterima: 'Diterima', ditolak: 'Ditolak'
  };

  var claims = [];
  var selectedClaim = null;
  var patientsCache = null;
  var invoicePatientCache = {};
  var sepPatientCache = {};

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
  function claimPill(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return Waras.statusPill(CLAIM_LABEL[k] || String(v), CLAIM_KIND[k] || 'neutral');
  }
  function diagnosisOf(c) {
    if (c.diagnosis_display || c.diagnosis_code) {
      return ((c.diagnosis_code || '') + ' ' + (c.diagnosis_display || '')).trim();
    }
    var note = c.grouping_note || '';
    var m = note.match(/Diagnosis utama:\s*([^.]+)/i);
    return m ? m[1].trim() : '—';
  }

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }

  async function patientIdOfClaim(c) {
    if (c.patient_id) return c.patient_id;
    if (c.invoice_id) {
      if (invoicePatientCache[c.invoice_id] !== undefined) return invoicePatientCache[c.invoice_id];
      try {
        var inv = await Waras.api('/api/invoices/' + encodeURIComponent(c.invoice_id));
        var pid = (inv.invoice || inv).patient_id || null;
        invoicePatientCache[c.invoice_id] = pid;
        return pid;
      } catch (e) { invoicePatientCache[c.invoice_id] = null; }
    }
    if (c.sep_id) {
      if (sepPatientCache[c.sep_id] !== undefined) return sepPatientCache[c.sep_id];
      try {
        var sep = await Waras.api('/api/seps/' + encodeURIComponent(c.sep_id));
        var spid = (sep.sep || sep).patient_id || null;
        sepPatientCache[c.sep_id] = spid;
        return spid;
      } catch (e) { sepPatientCache[c.sep_id] = null; }
    }
    return null;
  }

  async function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
    var ta = document.createElement('textarea');
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
  }

  async function exportClaimJson(id) {
    var data = await Waras.api('/api/casemix/' + encodeURIComponent(id) + '/export');
    return JSON.stringify(data, null, 2);
  }

  async function loadCasemix() {
    var tbody = document.getElementById('casemix-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var data = await Waras.api('/api/casemix');
      var kpis = data.kpis || {};
      claims = Waras.asList(data.claims !== undefined ? data.claims : data);
      var total = Number(kpis.total || claims.length) || 0;
      var verified = Number(kpis.terverifikasi || 0);
      setText('kpi-total', Waras.fmtNum(total));
      setText('kpi-verified', Waras.fmtNum(verified));
      setText('kpi-verified-sub', total ?
        ((verified * 100 / total).toLocaleString('id-ID', { maximumFractionDigits: 1 }) + '% dari total') : '—');
      setText('kpi-paid', Waras.fmtRp(kpis.dibayar_idr || 0));
      setText('kpi-pending', Waras.fmtNum(kpis.pending || 0));
      if (!claims.length) {
        tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Belum ada berkas klaim.</td></tr>';
        document.getElementById('claim-notes').innerHTML = '<p class="empty-note">Belum ada catatan klaim.</p>';
        return;
      }
      // Nama pasien diurai paralel (invoice/SEP), lalu tabel dirender.
      var names = await Promise.all(claims.map(async function (c) {
        if (c.patient_name) return c.patient_name;
        var pid = await patientIdOfClaim(c);
        return pid ? Waras.patientName(map, pid) : '—';
      }));
      tbody.innerHTML = claims.map(function (c, i) {
        return '<tr data-id="' + Waras.esc(c.id) + '" style="cursor:pointer"><td>' +
          Waras.esc(c.id) + '</td><td>' + Waras.esc(names[i]) + '</td><td>' +
          Waras.esc(diagnosisOf(c)) + '</td><td>' + Waras.esc(c.ina_cbg_code || '') + '</td><td>' +
          Waras.fmtRp(c.amount || 0) + '</td><td>' + claimPill(c.status) + '</td>' +
          '<td><button type="button" class="btn sm" data-copy="' + Waras.esc(c.id) + '">Salin JSON</button></td></tr>';
      }).join('');
      tbody.querySelectorAll('tr[data-id]').forEach(function (tr) {
        tr.addEventListener('click', function (ev) {
          if (ev.target && ev.target.getAttribute('data-copy')) return;
          selectClaim(tr.getAttribute('data-id'));
        });
      });
      tbody.querySelectorAll('button[data-copy]').forEach(function (btn) {
        btn.addEventListener('click', async function (ev) {
          ev.stopPropagation();
          try {
            await copyText(await exportClaimJson(btn.getAttribute('data-copy')));
            Waras.toast('JSON klaim disalin.');
          } catch (e) {
            Waras.toast('Salin JSON gagal: ' + (e && e.message ? e.message : ''), true);
          }
        });
      });
      renderNotes();
    } catch (e) {
      setText('kpi-total', '—'); setText('kpi-verified', '—');
      setText('kpi-paid', '—'); setText('kpi-pending', '—');
      tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Berkas klaim belum tersedia (endpoint /api/casemix belum aktif).</td></tr>';
      document.getElementById('claim-notes').innerHTML = '<p class="empty-note">Catatan klaim belum tersedia.</p>';
    }
  }

  function renderNotes() {
    var box = document.getElementById('claim-notes');
    var withNotes = claims.filter(function (c) { return c.grouping_note; });
    if (!withNotes.length) {
      box.innerHTML = '<p class="empty-note">Belum ada catatan grouping.</p>';
      return;
    }
    box.innerHTML = withNotes.map(function (c) {
      var sel = selectedClaim && selectedClaim.id === c.id;
      return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px' +
        (sel ? ';background:var(--accent-soft);border-radius:4px;padding-left:8px' : '') + '">' +
        '<strong>' + Waras.esc(c.id) + '</strong> · ' + Waras.esc(c.grouping_note) + '</div>';
    }).join('');
  }

  function selectClaim(id) {
    var c = null;
    claims.forEach(function (x) { if (x.id === id) c = x; });
    if (!c) return;
    selectedClaim = c;
    renderNotes();
  }

  function openEklaimModal() {
    var modal = document.getElementById('eklaim-modal');
    clearMsg('eklaim-error', 'eklaim-ok');
    if (!selectedClaim) {
      Waras.toast('Pilih dulu klaim di tabel.', true);
      return;
    }
    setText('eklaim-claim', 'Klaim: ' + selectedClaim.id + ' · ' +
      (selectedClaim.ina_cbg_code || '') + ' · ' + Waras.fmtRp(selectedClaim.amount || 0));
    var pre = document.getElementById('eklaim-json');
    pre.textContent = 'Memuat JSON berkas klaim…';
    modal.style.display = 'flex';
    exportClaimJson(selectedClaim.id).then(function (json) {
      pre.textContent = json;
    }).catch(function (e) {
      pre.textContent = 'JSON berkas klaim belum tersedia (' + (e && e.message ? e.message : 'endpoint belum aktif') + ').';
    });
  }
  function closeEklaimModal() {
    document.getElementById('eklaim-modal').style.display = 'none';
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadCasemix();

    var newBtn = document.getElementById('btn-new-group');
    var card = document.getElementById('group-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      document.getElementById('group-encounter').focus();
    });
    document.getElementById('group-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      clearMsg('group-error', 'group-ok');
    });
    document.getElementById('btn-fix-claim').addEventListener('click', function (ev) {
      ev.preventDefault();
      if (!selectedClaim) {
        Waras.toast('Pilih dulu klaim di tabel.', true);
        return;
      }
      card.style.display = 'block';
      var note = selectedClaim.grouping_note || '';
      document.getElementById('group-encounter').focus();
      Waras.toast(selectedClaim.id + ': ' + (note || 'tidak ada catatan grouping.'));
    });

    // "Cek pasien": encounter -> pasien, lalu baris konfirmasi §6.
    document.getElementById('group-check').addEventListener('click', async function () {
      var encId = document.getElementById('group-encounter').value.trim();
      if (!encId) { setText('group-confirm', 'Pasien: — (isi No. Encounter dulu)'); return; }
      setText('group-confirm', 'Pasien: memeriksa encounter…');
      try {
        var enc = await Waras.api('/api/encounters/' + encodeURIComponent(encId));
        var encounter = enc.encounter || enc;
        var pid = encounter.patient_id;
        if (!pid) { setText('group-confirm', 'Pasien: — (encounter tanpa pasien)'); return; }
        var p = await Waras.api('/api/patients/' + encodeURIComponent(pid));
        var patient = p.patient || p;
        setText('group-confirm', 'Pasien: ' + patient.name + ' (' + patient.id + ')');
      } catch (e) {
        setText('group-confirm', 'Pasien: — (encounter tidak ditemukan)');
      }
    });

    document.getElementById('group-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('group-submit');
      btn.disabled = true;
      clearMsg('group-error', 'group-ok');
      try {
        var claim = await Waras.api('/api/casemix/group', {
          method: 'POST',
          body: { encounter_id: document.getElementById('group-encounter').value.trim() }
        });
        var code = claim.code || claim.ina_cbg_code || '';
        var tariff = claim.tariff_idr != null ? claim.tariff_idr : claim.amount;
        showOk('group-ok', 'Grouping selesai: ' + (claim.id || '') + ' → ' + code +
          (tariff != null ? ' · ' + Waras.fmtRp(tariff) : '') + ' (perkiraan).');
        Waras.toast('Grouping klaim selesai.');
        await loadCasemix();
      } catch (e) {
        showErr('group-error', 'Grouping gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById('btn-eklaim').addEventListener('click', function (ev) {
      ev.preventDefault();
      openEklaimModal();
    });
    document.getElementById('eklaim-close').addEventListener('click', closeEklaimModal);
    document.getElementById('eklaim-modal').addEventListener('click', function (ev) {
      if (ev.target === this) closeEklaimModal();
    });
    document.getElementById('eklaim-copy').addEventListener('click', async function () {
      if (!selectedClaim) return;
      clearMsg('eklaim-error', 'eklaim-ok');
      try {
        await copyText(await exportClaimJson(selectedClaim.id));
        showOk('eklaim-ok', 'JSON disalin — tempel di aplikasi E-Klaim.');
      } catch (e) {
        showErr('eklaim-error', 'Salin JSON gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      }
    });
  });
})();
