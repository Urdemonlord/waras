// BPJS Kesehatan — VClaim & PCare: GET /api/seps, GET /api/referrals,
// GET /api/pcare/visits, POST /api/seps.
// Semua angka dari API (FASE2.md §6, §2.3). Endpoint Fase 2 belum ada di
// server saat halaman ini dibuat: setiap loader menangkap 404/503 dan
// menampilkan status "belum tersedia" — halaman tidak pernah blank/crash.
// POST /api/seps yang dijawab 503 menampilkan "BPJS belum dikonfigurasi".
(function () {
  'use strict';

  var JENIS_LABEL = { rawatjalan: 'Rawat jalan', rawatinap: 'Rawat inap' };
  function jenisLabel(v) {
    var k = String(v == null ? '' : v).toLowerCase();
    return JENIS_LABEL[k] || String(v || '—');
  }
  function sepFailed(s) {
    return String((s && s.status) || '').toLowerCase() === 'gagal';
  }

  function showFormErr(msg) {
    var el = document.getElementById('sep-error');
    var ok = document.getElementById('sep-ok');
    if (ok) ok.classList.remove('on');
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }
  function showFormOk(msg) {
    var el = document.getElementById('sep-ok');
    var err = document.getElementById('sep-error');
    if (err) err.classList.remove('on');
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }

  async function loadSeps() {
    var tbody = document.getElementById('sep-body');
    var map = {};
    try { map = await Waras.patientsMap(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/seps'));
      var failed = list.filter(sepFailed).length;
      var jalan = list.filter(function (s) { return String(s.jenis).toLowerCase() === 'rawatjalan'; }).length;
      var inap = list.filter(function (s) { return String(s.jenis).toLowerCase() === 'rawatinap'; }).length;
      document.getElementById('kpi-sep').textContent = Waras.fmtNum(list.length);
      document.getElementById('kpi-sep-sub').textContent =
        'Rawat jalan ' + Waras.fmtNum(jalan) + ' · Rawat inap ' + Waras.fmtNum(inap);
      document.getElementById('kpi-failed').textContent = Waras.fmtNum(failed);
      document.getElementById('kpi-failed-sub').textContent =
        failed > 0 ? 'Perlu verifikasi peserta' : 'Semua SEP aktif';
      tbody.innerHTML = list.map(function (s) {
        return '<tr><td>' + Waras.esc(s.sep_number || s.id) + '</td>' +
          '<td>' + Waras.esc(Waras.patientName(map, s.patient_id)) + '</td>' +
          '<td>' + Waras.esc(s.bpjs_number) + '</td>' +
          '<td>' + Waras.esc(jenisLabel(s.jenis)) + '</td>' +
          '<td>' + Waras.esc(s.poli) + '</td>' +
          '<td>' + Waras.pillFor(s.status) + '</td></tr>';
      }).join('') || '<tr><td colspan="6" class="empty-note">Belum ada SEP hari ini.</td></tr>';
    } catch (e) {
      document.getElementById('kpi-sep-sub').textContent = 'Belum tersedia';
      document.getElementById('kpi-failed-sub').textContent = 'Belum tersedia';
      tbody.innerHTML = '<tr><td colspan="6" class="empty-note">Daftar SEP belum tersedia (endpoint /api/seps belum aktif).</td></tr>';
    }
  }

  async function loadReferralsAndPcare() {
    var map = {};
    try { map = await Waras.patientsMap(); } catch (e) { /* nama tampil sebagai id */ }
    var refs = null, visits = null;
    try { refs = Waras.asList(await Waras.api('/api/referrals')); } catch (e) { refs = null; }
    try { visits = Waras.asList(await Waras.api('/api/pcare/visits')); } catch (e) { visits = null; }

    var refBox = document.getElementById('ref-list');
    var pcareBox = document.getElementById('pcare-list');
    var summary = document.getElementById('ref-summary');

    if (refs === null && visits === null) {
      summary.textContent = 'Rujukan & kunjungan PCare belum tersedia (endpoint /api/referrals dan /api/pcare/visits belum aktif).';
      refBox.innerHTML = '<p class="empty-note">Belum tersedia.</p>';
      pcareBox.innerHTML = '<p class="empty-note">Belum tersedia.</p>';
      document.getElementById('kpi-ref').textContent = '—';
      document.getElementById('kpi-ref-sub').textContent = 'Belum tersedia';
      document.getElementById('kpi-pcare').textContent = '—';
      return;
    }
    refs = refs || [];
    visits = visits || [];
    document.getElementById('kpi-ref').textContent = Waras.fmtNum(refs.length);
    document.getElementById('kpi-pcare').textContent = Waras.fmtNum(visits.length);
    // Tujuan rujukan terbanyak (dihitung dari data, bukan hardcode).
    var destCount = {};
    refs.forEach(function (r) { destCount[r.to_facility] = (destCount[r.to_facility] || 0) + 1; });
    var top = null, topN = 0;
    Object.keys(destCount).forEach(function (d) { if (destCount[d] > topN) { top = d; topN = destCount[d]; } });
    document.getElementById('kpi-ref-sub').textContent =
      top ? 'Terbanyak ke ' + top + ' (' + Waras.fmtNum(topN) + ')' : 'Belum ada rujukan';
    summary.textContent = 'Rujukan keluar: ' + Waras.fmtNum(refs.length) + ' hari ini. ' +
      'PCare FKTP: ' + Waras.fmtNum(visits.length) + ' kunjungan tercatat.';
    refBox.innerHTML = refs.map(function (r) {
      return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
        '<strong>' + Waras.esc(Waras.patientName(map, r.patient_id)) + '</strong> · ' +
        Waras.esc(r.from_poli) + ' → ' + Waras.esc(r.to_facility) + '<br>' +
        '<span class="muted">' + Waras.esc(r.reason || '') + '</span> ' + Waras.pillFor(r.status) + '</div>';
    }).join('') || '<p class="empty-note">Belum ada rujukan hari ini.</p>';
    pcareBox.innerHTML = visits.map(function (v) {
      return '<div style="padding:6px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
        '<strong>' + Waras.esc(Waras.patientName(map, v.patient_id)) + '</strong> · ' +
        Waras.esc(v.poli) + ' · ' + Waras.esc(Waras.fmtDateId(v.visit_date)) + '<br>' +
        '<span class="muted">' + Waras.esc(v.status || '') + '</span></div>';
    }).join('') || '<p class="empty-note">Belum ada kunjungan PCare hari ini.</p>';
  }

  async function fillPatientSelect() {
    var sel = document.getElementById('sep-patient');
    try {
      var list = Waras.asList(await Waras.api('/api/patients'));
      sel.innerHTML = list.map(function (p) {
        return '<option value="' + Waras.esc(p.id) + '" data-bpjs="' + Waras.esc(p.bpjs_number || '') + '">' +
          Waras.esc(p.name) + ' · ' + Waras.esc(p.id) + '</option>';
      }).join('');
      syncBpjs();
    } catch (e) {
      sel.innerHTML = '<option value="">Daftar pasien belum tersedia</option>';
    }
  }
  function syncBpjs() {
    var sel = document.getElementById('sep-patient');
    var opt = sel.options[sel.selectedIndex];
    var bpjs = document.getElementById('sep-bpjs');
    if (opt && opt.getAttribute('data-bpjs')) bpjs.value = opt.getAttribute('data-bpjs');
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadSeps();
    loadReferralsAndPcare();

    var newBtn = document.getElementById('btn-new-sep');
    var card = document.getElementById('sep-form-card');
    newBtn.addEventListener('click', function (ev) {
      ev.preventDefault();
      card.style.display = 'block';
      fillPatientSelect();
      document.getElementById('sep-patient').focus();
    });
    document.getElementById('sep-cancel').addEventListener('click', function () {
      card.style.display = 'none';
      showFormErr(''); showFormOk('');
    });
    document.getElementById('sep-patient').addEventListener('change', syncBpjs);

    document.getElementById('sep-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = document.getElementById('sep-submit');
      btn.disabled = true;
      showFormErr(''); showFormOk('');
      try {
        var sep = await Waras.api('/api/seps', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('sep-patient').value,
            bpjs_number: document.getElementById('sep-bpjs').value.trim(),
            jenis: document.getElementById('sep-jenis').value,
            poli: document.getElementById('sep-poli').value
          }
        });
        showFormOk('SEP ' + (sep.sep_number || sep.id || '') + ' berhasil diterbitkan.');
        Waras.toast('SEP berhasil diterbitkan.');
        await loadSeps();
      } catch (e) {
        // 503 = BPJS belum dikonfigurasi (FASE2.md §5): pesan yang diminta,
        // tanpa crash; error lain tampil apa adanya.
        if (e && e.status === 503) showFormErr('BPJS belum dikonfigurasi');
        else showFormErr('Terbitkan SEP gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
