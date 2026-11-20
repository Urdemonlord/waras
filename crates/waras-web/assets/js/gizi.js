// Gizi: GET /api/gizi/assessments, POST /api/gizi/assessments,
// GET /api/gizi/diets, POST /api/gizi/diets,
// POST /api/gizi/diets/{id}/status (FASE4.md §5.1).
// IMT dihitung JS sebelum kirim dengan rumus yang sama seperti server
// (berat / tinggi_m^2); kategori: <18,5 Kurus; 18,5–24,9 Normal;
// 25–29,9 Gemuk; >=30 Obesitas. Server tetap menghitung ulang.
(function () {
  'use strict';

  var DIET_KIND = { aktif: 'green', selesai: 'neutral' };
  var RISK_KIND = { rendah: 'green', sedang: 'amber', tinggi: 'red' };

  var patientsCache = null;

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
  function bmiCategory(bmi) {
    if (bmi < 18.5) return 'Kurus';
    if (bmi < 25) return 'Normal';
    if (bmi < 30) return 'Gemuk';
    return 'Obesitas';
  }
  function bmiOf(weight, heightCm) {
    var h = Number(heightCm) / 100;
    if (!h || !weight) return null;
    return Number(weight) / (h * h);
  }

  async function getPatients() {
    if (patientsCache) return patientsCache;
    patientsCache = await Waras.patientsMap();
    return patientsCache;
  }
  function fillPatientSelects(map) {
    var opts = Object.keys(map).sort().map(function (id) {
      return '<option value="' + Waras.esc(id) + '">' + Waras.esc(map[id].name) + ' (' + Waras.esc(id) + ')</option>';
    }).join('');
    document.getElementById('gz-patient').innerHTML = opts;
    document.getElementById('diet-patient').innerHTML = opts;
  }

  function updateBmiPreview() {
    var w = Number(document.getElementById('gz-weight').value);
    var h = Number(document.getElementById('gz-height').value);
    var el = document.getElementById('bmi-preview');
    var bmi = bmiOf(w, h);
    if (bmi == null || !isFinite(bmi)) {
      el.textContent = 'Isi berat dan tinggi untuk melihat IMT.';
      return;
    }
    el.textContent = 'IMT ' + bmi.toLocaleString('id-ID', { maximumFractionDigits: 1 }) + ' (' + bmiCategory(bmi) + ').';
  }

  async function loadAssessments() {
    var tbody = document.getElementById('gz-body');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/gizi/assessments'));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="9" class="empty-note">Belum ada asesmen gizi.</td></tr>';
        return;
      }
      tbody.innerHTML = list.map(function (a) {
        var bmi = a.bmi != null ? Number(a.bmi) : bmiOf(a.weight_kg, a.height_cm);
        return '<tr><td>' + Waras.esc(Waras.patientName(map, a.patient_id)) + '</td><td>' +
          Waras.esc(a.weight_kg) + ' kg</td><td>' + Waras.esc(a.height_cm) + ' cm</td><td>' +
          (bmi != null ? bmi.toLocaleString('id-ID', { maximumFractionDigits: 1 }) : '—') + '</td><td>' +
          Waras.esc(a.imt_category || (bmi != null ? bmiCategory(bmi) : '')) + '</td><td>' +
          Waras.statusPill(String(a.risk || ''), RISK_KIND[String(a.risk || '').toLowerCase()] || 'neutral') + '</td><td>' +
          Waras.esc(a.diet || '') + '</td><td>' + Waras.esc(a.assessed_by || '') + '</td><td>' +
          Waras.esc(String(a.created_at || '').slice(0, 10)) + '</td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="9" class="empty-note">Asesmen gizi belum tersedia (endpoint /api/gizi/assessments belum aktif).</td></tr>';
    }
  }

  async function loadDiets() {
    var box = document.getElementById('diet-list');
    var map = {};
    try { map = await getPatients(); } catch (e) { /* nama tampil sebagai id */ }
    try {
      var list = Waras.asList(await Waras.api('/api/gizi/diets'));
      var active = list.filter(function (d) { return String(d.status).toLowerCase() === 'aktif'; });
      if (!active.length) {
        box.innerHTML = '<p class="empty-note">Belum ada diet aktif.</p>';
        return;
      }
      box.innerHTML = active.map(function (d) {
        return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px">' +
          '<strong>' + Waras.esc(Waras.patientName(map, d.patient_id)) + '</strong> — ' +
          Waras.esc(d.diet_type) + ' · ' + Waras.fmtNum(d.calories || 0) + ' kkal' +
          ' <span style="float:right">' + Waras.statusPill('Aktif', 'green') +
          ' <button type="button" class="btn sm" data-diet-done="' + Waras.esc(d.id) + '">Selesai</button></span><br>' +
          '<span class="muted">Mulai ' + Waras.esc(d.start_date || '') + (d.notes ? ' · ' + Waras.esc(d.notes) : '') + '</span></div>';
      }).join('');
      box.querySelectorAll('button[data-diet-done]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
          btn.disabled = true;
          try {
            await Waras.api('/api/gizi/diets/' + encodeURIComponent(btn.getAttribute('data-diet-done')) + '/status', {
              method: 'POST', body: { status: 'Selesai' }
            });
            Waras.toast('Diet ditandai selesai.');
            await loadDiets();
          } catch (e) {
            Waras.toast('Ubah status diet gagal: ' + (e && e.message ? e.message : ''), true);
            btn.disabled = false;
          }
        });
      });
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Diet aktif belum tersedia (endpoint /api/gizi/diets belum aktif).</p>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    getPatients().then(fillPatientSelects).catch(function () {});
    loadAssessments();
    loadDiets();
    document.getElementById('gz-weight').addEventListener('input', updateBmiPreview);
    document.getElementById('gz-height').addEventListener('input', updateBmiPreview);
    document.getElementById('gz-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('gz-error', 'gz-ok');
      try {
        var a = await Waras.api('/api/gizi/assessments', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('gz-patient').value,
            weight_kg: Number(document.getElementById('gz-weight').value),
            height_cm: Number(document.getElementById('gz-height').value),
            risk: document.getElementById('gz-risk').value,
            diet: document.getElementById('gz-diet').value.trim(),
            notes: document.getElementById('gz-notes').value.trim()
          }
        });
        var bmi = a.bmi != null ? Number(a.bmi) : null;
        showOk('gz-ok', 'Asesmen disimpan' + (bmi != null ? ' — IMT ' + bmi.toLocaleString('id-ID', { maximumFractionDigits: 1 }) + ' (' + (a.imt_category || '') + ').' : '.'));
        Waras.toast('Asesmen gizi disimpan.');
        await loadAssessments();
      } catch (e) {
        showErr('gz-error', 'Asesmen gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
    document.getElementById('diet-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('diet-error', 'diet-ok');
      try {
        await Waras.api('/api/gizi/diets', {
          method: 'POST',
          body: {
            patient_id: document.getElementById('diet-patient').value,
            diet_type: document.getElementById('diet-type').value,
            calories: Number(document.getElementById('diet-calories').value),
            notes: document.getElementById('diet-notes').value.trim(),
            start_date: document.getElementById('diet-start').value
          }
        });
        showOk('diet-ok', 'Diet disimpan (Aktif).');
        Waras.toast('Diet disimpan.');
        await loadDiets();
      } catch (e) {
        showErr('diet-error', 'Diet gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
