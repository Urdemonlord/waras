// Portal Pasien — halaman mandiri, TIDAK memakai /js/app.js.
// Alasan: pasien anonim tidak punya token staf; Waras.api() mengalihkan
// 401 -> Login, sehingga form login pasien tidak akan pernah terlihat.
// Token pasien disimpan di localStorage kunci TERPISAH 'waras-portal-token'
// (JANGAN menimpa token staf 'waras_token').
// Mode staf: /portal?pasien=RM-... memakai token staf biasa + API staf
// (read-only, label "Mode staf — lihat sebagai").
(function () {
  'use strict';

  var PORTAL_KEY = 'waras-portal-token';
  var STAFF_KEY = 'waras_token';

  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }
  function fmtDateId(iso) {
    if (!iso) return '—';
    var d = String(iso).slice(0, 10).split('-');
    if (d.length !== 3) return String(iso);
    var bulan = ['Januari', 'Februari', 'Maret', 'April', 'Mei', 'Juni', 'Juli', 'Agustus', 'September', 'Oktober', 'November', 'Desember'];
    return Number(d[2]) + ' ' + bulan[Number(d[1]) - 1] + ' ' + d[0];
  }
  function get(key) {
    try { return localStorage.getItem(key); } catch (e) { return null; }
  }
  function set(key, val) {
    try {
      if (val) localStorage.setItem(key, val);
      else localStorage.removeItem(key);
    } catch (e) {}
  }
  function toast(msg, isErr) {
    var el = document.getElementById('toast');
    if (!el) { el = document.createElement('div'); el.id = 'toast'; document.body.appendChild(el); }
    el.textContent = msg;
    el.className = 'on' + (isErr ? ' err' : '');
    setTimeout(function () { el.className = ''; }, 4000);
  }
  // Fetch JSON mentah: TIDAK mengalihkan 401 (portal yang mengatur sendiri).
  async function fetchJson(path, token) {
    var headers = { 'Content-Type': 'application/json' };
    if (token) headers['Authorization'] = 'Bearer ' + token;
    var res = await fetch(path, { headers: headers });
    var data = null;
    try { data = await res.json(); } catch (e) { /* tanpa body */ }
    if (!res.ok) {
      var err = new Error((data && data.error && data.error.message) || ('HTTP ' + res.status));
      err.status = res.status;
      throw err;
    }
    return data;
  }
  function asList(data) {
    if (Array.isArray(data)) return data;
    if (data && Array.isArray(data.data)) return data.data;
    if (data && typeof data === 'object') {
      for (var k in data) { if (Array.isArray(data[k])) return data[k]; }
    }
    return [];
  }
  function showLogin() {
    document.getElementById('portal-login').style.display = 'block';
    document.getElementById('portal-main').style.display = 'none';
    document.getElementById('portal-logout-top').style.display = 'none';
    document.getElementById('portal-sub').textContent = 'Masuk dengan NIK dan kata sandi portal.';
  }
  function showMain() {
    document.getElementById('portal-login').style.display = 'none';
    document.getElementById('portal-main').style.display = 'block';
    document.getElementById('portal-logout-top').style.display = '';
  }
  function logout() {
    // Hapus sesi di server juga (POST /api/auth/logout) — sebelumnya
    // logout hanya menghapus token di localStorage dan sesi server
    // tetap berlaku sampai kedaluwarsa 12 jam (Fase A/A4).
    var token = get(PORTAL_KEY);
    if (token) {
      fetch('/api/auth/logout', {
        method: 'POST',
        headers: { 'Authorization': 'Bearer ' + token }
      }).catch(function () { /* best-effort */ });
    }
    set(PORTAL_KEY, null);
    showLogin();
  }

  // POST JSON mentah dengan token opsional (dipakai form kata sandi).
  function postJson(path, token, body) {
    var headers = { 'Content-Type': 'application/json' };
    if (token) headers['Authorization'] = 'Bearer ' + token;
    return fetch(path, { method: 'POST', headers: headers, body: JSON.stringify(body) });
  }

  // Ganti kata sandi portal (mode pasien): POST
  // /api/portal/change-password dengan kata sandi lama + baru.
  async function submitChangePassword(ev) {
    ev.preventDefault();
    var errEl = document.getElementById('portal-change-error');
    var okEl = document.getElementById('portal-change-ok');
    errEl.classList.remove('on');
    okEl.style.display = 'none';
    var cur = document.getElementById('portal-current-password').value;
    var nw = document.getElementById('portal-new-password').value;
    var nw2 = document.getElementById('portal-new-password2').value;
    if (nw !== nw2) {
      errEl.textContent = 'Kata sandi baru tidak sama dengan ulangannya.';
      errEl.classList.add('on');
      return;
    }
    var btn = ev.target.querySelector('button[type="submit"]');
    btn.disabled = true;
    try {
      var res = await postJson('/api/portal/change-password', get(PORTAL_KEY), { current_password: cur, new_password: nw });
      var data = null;
      try { data = await res.json(); } catch (e) { /* tanpa body */ }
      if (!res.ok) {
        if (res.status === 429) throw new Error('Terlalu banyak percobaan ganti kata sandi, coba lagi nanti.');
        if (res.status === 401) throw new Error('Kata sandi lama tidak cocok.');
        throw new Error((data && data.error && data.error.message) || ('HTTP ' + res.status));
      }
      okEl.style.display = '';
      ev.target.reset();
    } catch (e) {
      errEl.textContent = e && e.message ? e.message : 'Ganti kata sandi gagal.';
      errEl.classList.add('on');
    } finally {
      btn.disabled = false;
    }
  }

  // Set kata sandi portal pasien (mode staf, hanya Admin — server
  // mengembalikan 403 untuk peran lain): POST
  // /api/patients/{id}/portal-password.
  var staffPatientId = null;
  async function submitStaffSetPassword(ev) {
    ev.preventDefault();
    var errEl = document.getElementById('portal-staff-set-error');
    var okEl = document.getElementById('portal-staff-set-ok');
    errEl.classList.remove('on');
    okEl.style.display = 'none';
    var btn = ev.target.querySelector('button[type="submit"]');
    btn.disabled = true;
    try {
      var res = await postJson(
        '/api/patients/' + encodeURIComponent(staffPatientId || '') + '/portal-password',
        get(STAFF_KEY),
        { password: document.getElementById('portal-staff-new-password').value }
      );
      var data = null;
      try { data = await res.json(); } catch (e) { /* tanpa body */ }
      if (!res.ok) {
        if (res.status === 403) throw new Error('Hanya Admin yang boleh set kata sandi portal.');
        if (res.status === 404) throw new Error('Pasien tidak ditemukan.');
        throw new Error((data && data.error && data.error.message) || ('HTTP ' + res.status));
      }
      okEl.style.display = '';
      ev.target.reset();
    } catch (e) {
      errEl.textContent = e && e.message ? e.message : 'Set kata sandi gagal.';
      errEl.classList.add('on');
    } finally {
      btn.disabled = false;
    }
  }

  function renderPatient(p) {
    document.getElementById('portal-name').textContent = p.name || '—';
    var meta = [];
    if (p.id) meta.push('No. RM ' + p.id);
    if (p.nik) meta.push('NIK ' + p.nik);
    if (p.blood_type) meta.push('Gol. darah ' + p.blood_type);
    if (p.payer) meta.push(p.payer === 'BPJS' ? 'BPJS aktif' : p.payer);
    if (p.birth_date) meta.push('Lahir ' + fmtDateId(p.birth_date));
    document.getElementById('portal-meta').textContent = meta.join(' · ') || '—';
  }

  function renderAppointments(list) {
    var box = document.getElementById('portal-appointments');
    if (!list.length) { box.innerHTML = '<p class="empty-note">Belum ada janji.</p>'; return; }
    box.innerHTML = list.slice(0, 3).map(function (a) {
      var when = (a.date ? fmtDateId(a.date) : '') + (a.time ? ' · ' + esc(a.time) : '');
      return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
        esc(a.poli || 'Kontrol') + '</strong><br><span class="muted">' + when +
        (a.doctor ? ' · ' + esc(a.doctor) : '') + '</span>' +
        (a.booking_code ? '<br><span style="color:var(--accent-text);font-weight:600">Lihat kode booking ' + esc(a.booking_code) + '</span>' : '') +
        '</div>';
    }).join('');
  }
  function renderQueue(list) {
    var box = document.getElementById('portal-queue');
    if (!list.length) { box.innerHTML = '<p class="empty-note">Tidak ada antrean hari ini.</p>'; return; }
    var q = list[0];
    box.innerHTML = '<div style="display:flex;gap:16px;align-items:center"><span style="font-size:32px;font-weight:700;color:var(--accent-text)">' +
      esc(q.number || q.queue_number || '—') + '</span><span style="font-size:13px">' +
      esc(q.poli || '') + '<br><span class="muted">' + esc(q.status || '') + '</span></span></div>';
  }
  function renderLab(list) {
    var box = document.getElementById('portal-lab');
    if (!list.length) { box.innerHTML = '<p class="empty-note">Belum ada hasil lab.</p>'; return; }
    box.innerHTML = list.slice(0, 3).map(function (o) {
      var results = asList(o.results || o.result || []);
      var line = results.length ? results.map(function (r) {
        return esc(r.name || r.code || '') + ' ' + esc(r.value || '') + ' ' + esc(r.unit || '');
      }).join(' · ') : esc(o.status || '');
      return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
        esc(o.test_name || o.name || o.id || 'Pemeriksaan lab') + '</strong> · ' +
        fmtDateId(o.date || o.created_at) + '<br><span class="muted">' + line + '</span></div>';
    }).join('');
  }
  function renderPrescriptions(list) {
    var box = document.getElementById('portal-prescriptions');
    var active = list.filter(function (p) {
      var k = String(p.status || '').toLowerCase();
      return k === 'aktif' || k === 'menunggu_konfirmasi';
    });
    if (!active.length) { box.innerHTML = '<p class="empty-note">Tidak ada e-resep aktif.</p>'; return; }
    box.innerHTML = active.slice(0, 3).map(function (p) {
      return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
        esc(p.drug || p.drug_name || '') + '</strong> · ' + esc(p.dosage || '') +
        '<br><span class="muted">Ambil di Farmasi RSUD Waras</span></div>';
    }).join('');
  }

  async function loadPatientMode(token) {
    try {
      var me = await fetchJson('/api/portal/me', token);
      var patient = me.patient || me;
      renderPatient(patient);
      var appts = asList(await fetchJson('/api/portal/appointments', token).catch(function () { return []; }));
      renderAppointments(appts);
      var queue = asList(await fetchJson('/api/portal/queue', token).catch(function () { return []; }));
      renderQueue(queue);
      var lab = asList(await fetchJson('/api/portal/lab', token).catch(function () { return []; }));
      renderLab(lab);
      var rx = asList(await fetchJson('/api/portal/prescriptions', token).catch(function () { return []; }));
      renderPrescriptions(rx);
      document.getElementById('portal-change-card').style.display = '';
      document.getElementById('portal-staff-set-card').style.display = 'none';
      showMain();
    } catch (e) {
      if (e.status === 401) {
        set(PORTAL_KEY, null);
        showLogin();
        toast('Sesi portal berakhir, silakan masuk kembali.', true);
      } else {
        toast('Portal belum tersedia: ' + (e && e.message ? e.message : ''), true);
      }
    }
  }

  async function loadStaffMode(patientId) {
    var token = get(STAFF_KEY);
    if (!token) {
      document.getElementById('portal-sub').textContent = 'Mode staf membutuhkan login staf. Masuk dulu di halaman Login.';
      return;
    }
    document.getElementById('portal-staff-label').style.display = '';
    document.getElementById('portal-staff-label').textContent = 'Mode staf — lihat sebagai ' + patientId;
    try {
      var data = await fetchJson('/api/patients/' + encodeURIComponent(patientId), token);
      var patient = data.patient || data;
      renderPatient(patient);
      var queue = asList(await fetchJson('/api/queue?date=2026-11-19', token).catch(function () { return []; }))
        .filter(function (q) { return q.patient_id === patientId; });
      renderQueue(queue);
      var lab = asList(await fetchJson('/api/lab/orders?date=2026-11-19', token).catch(function () { return []; }))
        .filter(function (o) { return o.patient_id === patientId; });
      renderLab(lab);
      var rx = asList(await fetchJson('/api/prescriptions?patient_id=' + encodeURIComponent(patientId), token).catch(function () { return []; }));
      renderPrescriptions(rx);
      renderAppointments([]);
      document.getElementById('portal-appointments').innerHTML = '<p class="empty-note">Janji pasien hanya tampil di mode pasien.</p>';
      document.getElementById('portal-change-card').style.display = 'none';
      document.getElementById('portal-staff-set-card').style.display = '';
      showMain();
    } catch (e) {
      if (e.status === 401) {
        document.getElementById('portal-sub').textContent = 'Sesi staf berakhir. Masuk dulu di halaman Login.';
      } else {
        toast('Gagal memuat data pasien: ' + (e && e.message ? e.message : ''), true);
      }
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    // Listener form kata sandi dipasang di semua mode (bukan hanya di
    // jalur tanpa token di bawah — mode pasien auto-login dan mode staf
    // keluar lebih awal dari fungsi ini).
    document.getElementById('portal-change-form').addEventListener('submit', submitChangePassword);
    document.getElementById('portal-staff-set-form').addEventListener('submit', submitStaffSetPassword);
    var params = new URLSearchParams(location.search);
    var staffPatient = params.get('pasien');
    staffPatientId = staffPatient;
    if (staffPatient) { loadStaffMode(staffPatient); return; }
    var token = get(PORTAL_KEY);
    if (token) { loadPatientMode(token); return; }
    showLogin();
    document.getElementById('portal-login-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      var errEl = document.getElementById('portal-error');
      errEl.classList.remove('on');
      try {
        var res = await fetch('/api/auth/patient-login', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            nik: document.getElementById('portal-nik').value.trim(),
            password: document.getElementById('portal-password').value
          })
        });
        var data = null;
        try { data = await res.json(); } catch (e) { /* tanpa body */ }
        if (!res.ok) {
          if (res.status === 429) throw new Error('Terlalu banyak percobaan login, coba lagi nanti.');
          throw new Error('NIK atau kata sandi tidak cocok.');
        }
        set(PORTAL_KEY, data.token);
        await loadPatientMode(data.token);
      } catch (e) {
        errEl.textContent = e && e.message ? e.message : 'Login gagal.';
        errEl.classList.add('on');
      } finally {
        btn.disabled = false;
      }
    });
    document.getElementById('portal-logout').addEventListener('click', logout);
    document.getElementById('portal-logout-top').addEventListener('click', logout);
  });
})();
