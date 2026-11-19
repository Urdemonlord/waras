// Clinical Workspace (RME) — pasien dari ?pasien=RM-000812.
// Encounter aktif = encounter terbaru pasien (bukan hardcode).
// Tab: Encounter aktif menampilkan panel klinis; Ringkasan/Riwayat/Diagnosis/
// Obat menampilkan panel terkait; Laboratorium/Radiologi/Tindakan/Dokumen
// adalah modul Fase 2/3 — hanya catatan "segera", bukan layar palsu.
(function () {
  'use strict';

  var VITAL_LABEL = {
    tekanan_darah: 'Tekanan darah', nadi: 'Nadi', suhu: 'Suhu',
    spo2: 'SpO2', gula_darah: 'Gula darah'
  };
  var GROUP_LABEL = {
    utama: 'Diagnosis utama kunjungan', sekunder: 'Diagnosis sekunder',
    kronis: 'Diagnosis kronis', riwayat: 'Riwayat diagnosis',
    tindakan_medis: 'Tindakan medis'
  };

  function getParam(name) {
    var m = new RegExp('[?&]' + name + '=([^&]*)').exec(location.search);
    return m ? decodeURIComponent(m[1]) : null;
  }

  function ageOn(birthDate) {
    if (!birthDate) return '';
    var b = new Date(birthDate + 'T00:00:00');
    var t = new Date(Waras.TODAY + 'T00:00:00');
    var age = t.getFullYear() - b.getFullYear();
    if (t.getMonth() < b.getMonth() || (t.getMonth() === b.getMonth() && t.getDate() < b.getDate())) age--;
    return age + ' thn';
  }

  async function load() {
    var patientId = getParam('pasien') || 'RM-000812';
    var patient = await Waras.api('/api/patients/' + encodeURIComponent(patientId));
    // GET /api/patients/:id mengembalikan pasien + alergi + kondisi +
    // encounter terakhir; bentuknya dinormalisasi di sini.
    var allergies = patient.allergies || patient.alergi || [];
    var conditions = patient.conditions || [];
    var lastEncounter = patient.last_encounter || patient.encounter_terakhir || null;

    var encounters = Waras.asList(await Waras.api('/api/encounters?patient_id=' + encodeURIComponent(patientId)));
    // Encounter aktif = terbaru (tanggal + jam), fallback ke encounter terakhir dari pasien.
    encounters.sort(function (a, b) { return String(b.date + (b.time || '')).localeCompare(String(a.date + (a.time || ''))); });
    var enc = encounters[0] || lastEncounter;
    if (!enc) throw new Error('Pasien belum punya encounter.');

    // --- Banner identitas + alergi ---
    var penAllergy = allergies.find(function (a) {
      return (a.substance_class || '').toLowerCase() === 'penicillin' ||
        String(a.substance || '').toLowerCase().indexOf('penisilin') >= 0;
    });
    document.getElementById('banner-allergy').style.display = penAllergy ? '' : 'none';
    document.getElementById('banner-name').textContent =
      patient.name + ', ' + ageOn(patient.birth_date) + ' · ' +
      (String(patient.gender).toLowerCase().indexOf('perempuan') >= 0 ? 'Perempuan' : 'Laki-laki');
    document.getElementById('banner-meta').textContent =
      'Encounter ' + enc.id + ' · ' + Waras.fmtDateId(enc.date) + ' ' + (enc.time || '') + ' · ' + enc.doctor_name;
    document.getElementById('page-title').textContent = 'Clinical Workspace / ' + patient.id;
    document.getElementById('page-sub').textContent =
      'Clinical Workspace · Encounter ' + enc.id + ' · Rawat Jalan · ' + enc.doctor_name;
    document.getElementById('who-name').textContent =
      patient.name + ', ' + ageOn(patient.birth_date) + ' · ' +
      (String(patient.gender).toLowerCase().indexOf('perempuan') >= 0 ? 'Perempuan' : 'Laki-laki');
    document.getElementById('who-meta').textContent =
      'NIK ' + patient.nik + ' · No. RM ' + patient.id +
      (patient.bpjs_number ? ' · BPJS ' + patient.bpjs_number : '') + ' · ' +
      (patient.address || '') + (patient.blood_type ? ' · Gol. darah ' + patient.blood_type : '') +
      ' · FHIR Patient/' + patient.id.replace('RM-', '').replace(/^0+/, '');
    document.getElementById('who-badges').innerHTML =
      (penAllergy ? '<span class="badge red">● Alergi: Penisilin</span>' : '') +
      (conditions.some(function (c) { return String(c.display || '').toLowerCase().indexOf('diabetes') >= 0 || String(c.display || '').toLowerCase().indexOf('dm tipe') >= 0; })
        ? ' <span class="badge amber">● DM tipe 2</span>' : '');
    document.getElementById('who-avatar').textContent = Waras.initials(patient.name);
    document.getElementById('encounter-line').innerHTML =
      '<strong>Encounter aktif — ' + Waras.fmtDateId(enc.date) + ' · ' + Waras.labelFor(enc.status) +
      ' · ' + Waras.esc(enc.poli) + ' · ' + Waras.esc(enc.doctor_name) + '</strong><br>' +
      '<span class="muted">Clinical documentation: SOAP, tanda vital, diagnosis, tindakan, dan resep — dalam satu konteks kunjungan.</span>';
    document.getElementById('fhir-tag').textContent = 'FHIR · ' + enc.id;

    // --- Alert keamanan obat: dari resep yang diblokir, bukan hardcode ---
    var rxList = Waras.asList(await Waras.api('/api/prescriptions?patient_id=' + encodeURIComponent(patientId)));
    var blocked = rxList.find(function (r) { return String(r.status).toLowerCase() === 'diblokir'; });
    var alertEl = document.getElementById('allergy-alert');
    if (penAllergy && blocked) {
      var replacement = blocked.replaced_by
        ? rxList.find(function (r) { return r.id === blocked.replaced_by; }) : null;
      document.getElementById('alert-text').textContent =
        'Resep ' + Waras.drugDose(blocked.drug_name, blocked.dosage) + ' DIBLOKIR otomatis' +
        (blocked.blocked_reason ? ' (' + blocked.blocked_reason + ')' : '') +
        (replacement ? '. Diganti ' + Waras.drugDose(replacement.drug_name, replacement.dosage) +
          ' — menunggu konfirmasi dokter.' : '.');
      alertEl.style.display = '';
    } else if (penAllergy) {
      document.getElementById('alert-text').textContent =
        'Pasien alergi Penisilin (kritisitas ' + Waras.labelFor(penAllergy.criticality) +
        '). Periksa kelas obat sebelum meresepkan.';
      alertEl.style.display = '';
    } else {
      alertEl.style.display = 'none';
    }

    // --- Tanda vital ---
    var vitals = Waras.asList(await Waras.api('/api/encounters/' + encodeURIComponent(enc.id) + '/vitals'));
    document.getElementById('vitals-body').innerHTML = vitals.map(function (o) {
      var prev = o.previous_value
        ? Waras.esc(o.previous_value) + (o.previous_date ? ' · ' + Waras.fmtDateId(o.previous_date) : '') : '—';
      var time = o.measured_at ? String(o.measured_at).slice(11, 16).replace(':', '.') : '';
      return '<tr><td>' + Waras.esc(VITAL_LABEL[String(o.code).toLowerCase()] || o.code) + '</td>' +
        '<td><strong>' + Waras.esc(o.value) + ' ' + Waras.esc(o.unit) + '</strong></td>' +
        '<td>' + Waras.esc(time) + '</td><td>' + Waras.esc(o.measured_by) + '</td><td>' + prev + '</td></tr>';
    }).join('') || '<tr><td colspan="5" class="empty-note">Belum ada tanda vital pada encounter ini.</td></tr>';

    // --- SOAP ---
    var soap = await Waras.api('/api/encounters/' + encodeURIComponent(enc.id) + '/soap');
    var versions = soap.versions || [];
    fillSoap('s', soap.subjective, soap);
    fillSoap('o', soap.objective, soap);
    fillSoap('a', soap.assessment, soap);
    fillSoap('p', soap.plan, soap);
    document.getElementById('soap-note').textContent =
      'Draft tersimpan otomatis ' + (soap.draft_saved_at ? String(soap.draft_saved_at).slice(11, 16).replace(':', '.') : '—') +
      ' · Riwayat versi (' + (versions.length || soap.version || 1) + ')' +
      (String(soap.status).toLowerCase() === 'draft' ? ' · TTE BSrE dibubuhkan setelah P lengkap.' : ' · Lengkap.');
    document.getElementById('soap-versions').textContent = 'Riwayat versi (' + (versions.length || soap.version || 1) + ')';
    document.getElementById('encounter-id-hidden').value = enc.id;

    // --- Diagnosis (dari GET /api/patients/:id/conditions) ---
    if (!conditions.length) conditions = Waras.asList(await Waras.api('/api/patients/' + encodeURIComponent(patientId) + '/conditions'));
    var order = ['utama', 'sekunder', 'kronis', 'riwayat', 'tindakan_medis'];
    var byGroup = {};
    conditions.forEach(function (c) {
      var g = String(c.group).toLowerCase();
      (byGroup[g] = byGroup[g] || []).push(c);
    });
    var diagHtml = order.map(function (g) {
      var list = byGroup[g];
      if (!list) return '';
      var rows = list.map(function (c) {
        return Waras.esc(c.code) + ' · ' + Waras.esc(c.display) +
          (c.encounter_id ? ' — Encounter ' + Waras.esc(c.encounter_id) : '') +
          ' · ' + Waras.pillFor(c.status) + ' · ' + Waras.esc(c.clinician);
      }).join('<br>');
      return '<div class="diag-row"><div class="muted">' + Waras.esc(GROUP_LABEL[g] || g) + '</div><div>' + rows + '</div><div></div></div>';
    }).join('') || '<p class="empty-note">Belum ada diagnosis.</p>';
    document.getElementById('diag-list').innerHTML = diagHtml;
    var diagFull = document.getElementById('diag-list-full');
    if (diagFull) diagFull.innerHTML = diagHtml;
    document.getElementById('diag-summary').innerHTML = conditions.slice(0, 3).map(function (c) {
      return '<div class="diag-row"><div class="muted">' + Waras.esc(GROUP_LABEL[String(c.group).toLowerCase()] || c.group) + '</div><div>' +
        Waras.esc(c.code) + ' · ' + Waras.esc(c.display) + '</div><div>' + Waras.pillFor(c.status) + '</div></div>';
    }).join('');

    // --- Timeline riwayat kunjungan ---
    var timelineHtml = encounters.map(function (e) {
      return '<li><span class="t-date">' + Waras.fmtDateId(e.date) + '</span><br>' +
        'Encounter ' + Waras.esc(e.poli) + ' — ' + Waras.esc(e.doctor_name) + ' · ' + Waras.pillFor(e.status) + '</li>';
    }).join('');
    document.getElementById('timeline-list').innerHTML = timelineHtml;
    var timelineRw = document.getElementById('timeline-list-riwayat');
    if (timelineRw) timelineRw.innerHTML = timelineHtml;

    // --- Panel FHIR: relasi dari data nyata (id pasien + encounter) ---
    var fhir = await Waras.api('/api/encounters/' + encodeURIComponent(enc.id) + '/fhir').catch(function () { return null; });
    var bundleTypes = [];
    if (fhir && Array.isArray(fhir.entry)) {
      fhir.entry.forEach(function (en) {
        var rt = en.resource && en.resource.resourceType;
        if (rt && bundleTypes.indexOf(rt) < 0) bundleTypes.push(rt);
      });
    }
    document.getElementById('fhir-lines').innerHTML =
      '<code>Patient/' + Waras.esc(patient.id.replace('RM-', '').replace(/^0+/, '')) + '</code> → ' +
      '<code>Encounter ' + Waras.esc(enc.id) + '</code><br>' +
      '→ <code>Observation</code> (tanda vital) · <code>Condition</code> (diagnosis)<br>' +
      '→ <code>MedicationRequest</code> (resep — diganti bila diblokir alergi)<br>' +
      '→ <code>Procedure</code> (tindakan medis)<br>' +
      '<span class="muted">Tiap Condition terikat Encounter-nya; diagnosis kronis atau riwayat ' +
      'tidak terikat satu kunjungan. Bundle FHIR R4' +
      (bundleTypes.length ? ' berisi: ' + bundleTypes.map(Waras.esc).join(', ') + '.' : ' dimuat dari /api/encounters/:id/fhir.') + '</span>';

    // --- Obat (resep pasien) ---
    renderRx(rxList);
    // --- Daftar obat untuk form resep ---
    var drugs = Waras.asList(await Waras.api('/api/drugs'));
    var sel = document.getElementById('rx-drug');
    sel.innerHTML = drugs.map(function (d) {
      return '<option value="' + Waras.esc(d.name) + '" data-class="' + Waras.esc(d.drug_class) + '">' +
        Waras.esc(d.name) + ' (' + Waras.esc(d.drug_class) + ')</option>';
    }).join('');
  }

  function fillSoap(key, text, soap) {
    var el = document.getElementById('soap-' + key);
    if (el) el.textContent = text || '—';
  }

  function soapText(id) {
    var t = document.getElementById(id).textContent;
    return t === '—' ? '' : t;
  }

  function renderRx(list) {
    var el = document.getElementById('rx-list');
    el.innerHTML = list.map(function (r) {
      return '<tr><td>' + Waras.esc(r.id) + '</td><td>' + Waras.esc(r.drug_name) + '</td>' +
        '<td>' + Waras.esc(r.dosage) + ', ' + Waras.esc(r.duration) + '</td>' +
        '<td>' + Waras.esc(r.doctor) + '</td><td>' + Waras.pillFor(r.status) + '</td>' +
        '<td class="muted">' + Waras.esc(r.blocked_reason || (r.replaced_by ? 'Diganti ' + r.replaced_by : '—')) + '</td></tr>';
    }).join('') || '<tr><td colspan="6" class="empty-note">Belum ada resep untuk pasien ini.</td></tr>';
  }

  document.addEventListener('DOMContentLoaded', function () {
    load().catch(function (e) { Waras.toast('RME gagal dimuat: ' + e.message, true); });

    // Tab workspace (9 tab).
    document.querySelectorAll('.tab[data-tab]').forEach(function (tab) {
      tab.addEventListener('click', function () {
        document.querySelectorAll('.tab[data-tab]').forEach(function (t) { t.classList.remove('active'); });
        tab.classList.add('active');
        var name = tab.getAttribute('data-tab');
        document.querySelectorAll('.tab-panel').forEach(function (p) {
          p.classList.toggle('active', p.getAttribute('data-panel') === name);
        });
      });
    });

    // Edit SOAP per seksi -> PUT /api/encounters/:id/soap.
    document.querySelectorAll('.soap-box [data-edit]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var key = btn.getAttribute('data-edit');
        var box = btn.closest('.soap-box');
        var p = box.querySelector('p');
        var ta = box.querySelector('textarea');
        if (ta.style.display === 'none' || !ta.style.display) {
          ta.value = p.textContent === '—' ? '' : p.textContent;
          ta.style.display = '';
          p.style.display = 'none';
          btn.textContent = 'Simpan';
        } else {
          saveSoap(key, ta.value, box, p, ta, btn);
        }
      });
    });
    async function saveSoap(key, value, box, p, ta, btn) {
      btn.disabled = true;
      try {
        var encId = document.getElementById('encounter-id-hidden').value;
        var body = {
          subjective: soapText('soap-s'),
          objective: soapText('soap-o'),
          assessment: soapText('soap-a'),
          plan: soapText('soap-p')
        };
        body[{ s: 'subjective', o: 'objective', a: 'assessment', p: 'plan' }[key]] = value;
        var saved = await Waras.api('/api/encounters/' + encodeURIComponent(encId) + '/soap', { method: 'PUT', body: body });
        p.textContent = value || '—';
        p.style.display = '';
        ta.style.display = 'none';
        btn.textContent = 'Edit';
        document.getElementById('soap-note').textContent =
          'Draft tersimpan otomatis · Riwayat versi (' + (saved.version || 1) + ') · TTE BSrE dibubuhkan setelah P lengkap.';
        document.getElementById('soap-versions').textContent = 'Riwayat versi (' + (saved.version || 1) + ')';
        Waras.toast('SOAP tersimpan sebagai draft v' + (saved.version || 1) + '.');
      } catch (e) {
        Waras.toast('Gagal menyimpan SOAP: ' + e.message, true);
      } finally { btn.disabled = false; }
    }

    // Form resep -> POST /api/prescriptions; 422 = diblokir aturan keamanan obat.
    var rxForm = document.getElementById('rx-form');
    var rxErr = document.getElementById('rx-error');
    var rxOk = document.getElementById('rx-ok');
    rxForm.addEventListener('submit', async function (ev) {
      ev.preventDefault();
      rxErr.style.display = 'none';
      rxOk.style.display = 'none';
      var sel = document.getElementById('rx-drug');
      var opt = sel.options[sel.selectedIndex];
      try {
        var created = await Waras.api('/api/prescriptions', {
          method: 'POST',
          body: {
            encounter_id: document.getElementById('encounter-id-hidden').value,
            drug_name: sel.value,
            drug_class: opt ? opt.getAttribute('data-class') : '',
            dosage: document.getElementById('rx-dosage').value.trim(),
            duration: document.getElementById('rx-duration').value.trim()
          }
        });
        rxOk.textContent = 'Resep ' + created.id + ' (' + created.drug_name + ') dibuat — status ' +
          Waras.labelFor(created.status) + '.';
        rxOk.style.display = 'block';
        var patientId = getParam('pasien') || 'RM-000812';
        var rxList = Waras.asList(await Waras.api('/api/prescriptions?patient_id=' + encodeURIComponent(patientId)));
        renderRx(rxList);
      } catch (e) {
        // 422: resep diblokir (mis. alergi) — tampilkan pesan server apa adanya.
        rxErr.innerHTML = '<strong>DIBLOKIR — resep tidak dibuat.</strong><br>' + Waras.esc(e.message);
        rxErr.style.display = 'block';
      }
    });

    document.getElementById('btn-simpan').addEventListener('click', function () {
      Waras.toast('SOAP sudah tersimpan otomatis sebagai draft (lihat tombol Edit per seksi).');
    });
    document.getElementById('btn-batal').addEventListener('click', function () { location.reload(); });
    document.getElementById('btn-cetak').addEventListener('click', function () { window.print(); });
    document.getElementById('btn-unduh').addEventListener('click', function () {
      Waras.toast('Unduh RME (PDF) segera hadir di Fase 2.');
    });
    document.getElementById('btn-tutup').addEventListener('click', function () { location.href = '/dashboard'; });
  });
})();
