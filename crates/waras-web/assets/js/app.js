// WARAS web — helper bersama (CONTRACT §6).
// Semua data klinis dimuat dari API lewat fetch; HTML hanya berisi label statis.
// Token sesi disimpan di localStorage; 401 -> kembali ke halaman Login.
(function () {
  'use strict';

  var TOKEN_KEY = 'waras_token';
  // Tanggal klinik (CONTRACT §0): "hari ini" = WARAS_TODAY, default 2026-11-19.
  var TODAY = '2026-11-19';

  function token() {
    try { return localStorage.getItem(TOKEN_KEY); } catch (e) { return null; }
  }
  function setToken(t) {
    try { if (t) localStorage.setItem(TOKEN_KEY, t); else localStorage.removeItem(TOKEN_KEY); } catch (e) {}
  }

  // Panggilan API JSON. Menambah header Bearer; 401 -> hapus token, ke Login.
  async function api(path, opts) {
    opts = opts || {};
    var headers = { 'Content-Type': 'application/json' };
    var t = token();
    if (t) headers['Authorization'] = 'Bearer ' + t;
    var res = await fetch(path, {
      method: opts.method || 'GET',
      headers: headers,
      body: opts.body ? JSON.stringify(opts.body) : undefined
    });
    if (res.status === 401) {
      setToken(null);
      var p = location.pathname;
      if (p !== '/' && p !== '/index.html') location.href = '/';
      throw new Error('Sesi berakhir, silakan masuk kembali.');
    }
    var data = null;
    try { data = await res.json(); } catch (e) { /* respons tanpa body */ }
    if (!res.ok) {
      var msg = (data && data.error && data.error.message) || ('HTTP ' + res.status);
      var err = new Error(msg);
      err.status = res.status;
      err.data = data;
      throw err;
    }
    return data;
  }

  // API mengembalikan daftar sebagai array langsung, atau dibungkus
  // {patients:[...]} / {data:[...]} — normalisasi di satu tempat.
  function asList(data) {
    if (Array.isArray(data)) return data;
    if (data && Array.isArray(data.data)) return data.data;
    if (data && typeof data === 'object') {
      for (var k in data) { if (Array.isArray(data[k])) return data[k]; }
    }
    return [];
  }

  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  // Format rupiah Indonesia: 185000 -> "Rp 185.000".
  function fmtRp(n) {
    n = Number(n) || 0;
    return 'Rp ' + n.toLocaleString('id-ID');
  }
  // Jutaan rupiah untuk kartu ringkasan: 18400000 -> "Rp 18,4 jt".
  function fmtJt(n) {
    var v = (Number(n) || 0) / 1e6;
    return 'Rp ' + v.toLocaleString('id-ID', { maximumFractionDigits: 1 }) + ' jt';
  }
  function fmtNum(n) { return (Number(n) || 0).toLocaleString('id-ID'); }
  function fmtDateId(iso) {
    if (!iso) return '—';
    var d = String(iso).slice(0, 10).split('-');
    if (d.length !== 3) return String(iso);
    var bulan = ['Januari','Februari','Maret','April','Mei','Juni','Juli','Agustus','September','Oktober','November','Desember'];
    return Number(d[2]) + ' ' + bulan[Number(d[1]) - 1] + ' ' + d[0];
  }
  function initials(name) {
    // Gelar dilewati: token berisi titik ("dr.", "Sp.PD", "S.Kep.") atau
    // gelar telanjang ("dr", "Ns", "Apt", "drg", "Bdn"). Inisial dari kata
    // nama pertama + terakhir: "dr. Ahmad Hidayat" -> "AH" (bukan "DA"),
    // "Ns. Sari Wijaya" -> "SW".
    var TITLES = { dr: 1, drg: 1, ns: 1, apt: 1, bdn: 1, prof: 1 };
    var parts = String(name || '?').trim().split(/\s+/);
    var keep = parts.filter(function (w) {
      if (!w || w[0] === '(') return false;
      if (w.indexOf('.') !== -1) return false;
      return !TITLES[w.replace(/,+$/, '').toLowerCase()];
    });
    var a = keep[0] ? keep[0][0] : '?';
    var b = keep.length > 1 ? keep[keep.length - 1][0] : '';
    return (a + b).toUpperCase();
  }

  // Tampilan obat: "Amoxicillin 500 mg" + "1x1". Bila dosage masih membawa
  // kekuatan ("500 mg 1x1", baris lama / ketikan bebas), kekuatan yang
  // dobel dibuang supaya tidak tampil "500 mg 500 mg 1x1". Aturan yang
  // sama dengan `waras_core::dosage::normalize_dosage` di server.
  function drugDose(drug, dosage) {
    var name = String(drug || '');
    var d = String(dosage || '').trim();
    var m = name.match(/(\d+(?:[.,]\d+)?\s*(?:mg|g|mcg|µg|IU|ml|%))\s*$/i);
    if (m) {
      var compact = function (s) { return s.replace(/\s+/g, '').toLowerCase(); };
      if (compact(d).indexOf(compact(m[1])) === 0) {
        var need = compact(m[1]).length, seen = 0, i = 0;
        while (i < d.length && seen < need) {
          if (/\s/.test(d[i])) { i++; continue; }
          seen++; i++;
        }
        d = d.slice(i).trim();
      }
    }
    return d ? name + ' ' + d : name;
  }

  // ---- Status pill (CONTRACT §6: teks berwarna + dot + latar pucat) ----
  // Nilai enum API diserialisasi snake_case (waras-core::types).
  var STATUS_LABEL = {
    menunggu: 'Menunggu', dipanggil: 'Dipanggil', dalam_konsultasi: 'Dalam konsultasi',
    selesai: 'Selesai', tidak_hadir: 'Tidak hadir',
    menunggu_konfirmasi: 'Menunggu konfirmasi', aktif: 'Aktif', diblokir: 'Diblokir',
    diberikan: 'Diberikan', dibatalkan: 'Dibatalkan',
    lunas: 'Lunas', cicilan: 'Cicilan',
    aman: 'Aman', segera: 'Segera', kritis: 'Kritis',
    terkonfirmasi: 'Terkonfirmasi', info: 'INFO', tinggi: 'Tinggi',
    sedang: 'Sedang', rendah: 'Rendah', kronis: 'Kronis',
    draft: 'Draft', lengkap: 'Lengkap', dalam_pemeriksaan: 'Dalam pemeriksaan',
    bpjs: 'BPJS', umum: 'Umum', laki_laki: 'Laki-laki', perempuan: 'Perempuan'
  };
  var STATUS_KIND = {
    menunggu: 'amber', dipanggil: 'teal', dalam_konsultasi: 'teal',
    selesai: 'green', tidak_hadir: 'red',
    menunggu_konfirmasi: 'amber', aktif: 'green', diblokir: 'red',
    diberikan: 'green', dibatalkan: 'neutral',
    lunas: 'green', cicilan: 'amber',
    aman: 'green', segera: 'amber', kritis: 'red',
    terkonfirmasi: 'green', info: 'teal', tinggi: 'amber',
    sedang: 'amber', rendah: 'green', kronis: 'neutral',
    draft: 'amber', lengkap: 'green', dalam_pemeriksaan: 'teal'
  };
  function statusPill(text, kind) {
    return '<span class="pill ' + esc(kind || 'neutral') + '"><i class="dot"></i>' + esc(text) + '</span>';
  }
  function pillFor(value) {
    var key = String(value == null ? '' : value).toLowerCase();
    return statusPill(STATUS_LABEL[key] || String(value), STATUS_KIND[key] || 'neutral');
  }
  function labelFor(value) {
    var key = String(value == null ? '' : value).toLowerCase();
    return STATUS_LABEL[key] || String(value);
  }

  var ROLE_LABEL = {
    dokter_umum: 'Dokter Umum', perawat: 'Perawat', apoteker: 'Apoteker',
    kasir: 'Kasir', admin: 'Admin'
  };
  function roleLabel(role) {
    var key = String(role == null ? '' : role).toLowerCase();
    return ROLE_LABEL[key] || String(role);
  }

  // ---- Toast kecil ----
  var toastTimer = null;
  function toast(msg, isErr) {
    var el = document.getElementById('toast');
    if (!el) { el = document.createElement('div'); el.id = 'toast'; document.body.appendChild(el); }
    el.textContent = msg;
    el.className = 'on' + (isErr ? ' err' : '');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { el.className = ''; }, 4000);
  }

  // ---- Cache peta pasien (id -> pasien), dipakai banyak halaman ----
  var patientsCache = null;
  async function patientsMap() {
    if (patientsCache) return patientsCache;
    var list = asList(await api('/api/patients'));
    var map = {};
    list.forEach(function (p) { map[p.id] = p; });
    patientsCache = map;
    return map;
  }
  function patientName(map, id) {
    return (map && map[id] && map[id].name) || id || '—';
  }

  // ---- Shell: nav aktif, /api/auth/me, pencarian topbar ----
  function initShell() {
    var page = document.body.getAttribute('data-page') || '';
    document.querySelectorAll('.nav-item[data-nav]').forEach(function (a) {
      if (a.getAttribute('data-nav') === page) a.classList.add('active');
    });
    // Item "segera" (Fase 2-4): jangan navigasi, beri tahu saja.
    document.querySelectorAll('.nav-item.soon').forEach(function (a) {
      a.addEventListener('click', function (ev) {
        ev.preventDefault();
        toast('Modul ' + a.textContent.trim().replace(/\s*segera\s*$/i, '') + ' segera hadir (Fase 2–4).');
      });
    });
    // Tautan "segera" di konten (Fase 2-4): jangan navigasi, beri tahu saja.
    document.querySelectorAll('[data-soon]').forEach(function (a) {
      a.addEventListener('click', function (ev) {
        ev.preventDefault();
        toast('Modul ini segera hadir (Fase 2–4).');
      });
    });
    // Profil dari GET /api/auth/me (bukan hardcode).
    var meName = document.getElementById('me-name');
    if (meName) {
      api('/api/auth/me').then(function (me) {
        var user = me.user || me;
        meName.textContent = user.name || '—';
        var roleEl = document.getElementById('me-role');
        if (roleEl) roleEl.textContent = roleLabel(user.role);
        var av = document.getElementById('me-avatar');
        if (av) av.textContent = initials(user.name);
      }).catch(function () { /* biarkan label awal; api() sudah menangani 401 */ });
    }
    // Lonceng: titik merah bila ada notifikasi belum dibaca.
    var bellDot = document.getElementById('bell-dot');
    if (bellDot) {
      api('/api/notifications').then(function (data) {
        var list = asList(data);
        if (list.some(function (n) { return !n.read; })) bellDot.classList.add('on');
      }).catch(function () {});
    }
    initSearch();
    var bell = document.getElementById('bell-btn');
    if (bell) bell.addEventListener('click', function () { location.href = '/dashboard'; });
  }

  // Pencarian topbar -> GET /api/patients?search= (CONTRACT §3).
  function initSearch() {
    var input = document.getElementById('top-search');
    var box = document.getElementById('search-results');
    if (!input || !box) return;
    var timer = null;
    function close() { box.classList.remove('open'); box.innerHTML = ''; }
    input.addEventListener('input', function () {
      clearTimeout(timer);
      var q = input.value.trim();
      if (q.length < 2) { close(); return; }
      timer = setTimeout(function () {
        api('/api/patients?search=' + encodeURIComponent(q)).then(function (data) {
          var list = asList(data).slice(0, 8);
          if (!list.length) {
            box.innerHTML = '<a href="#" onclick="return false">Tidak ada pasien cocok dengan &ldquo;' + esc(q) + '&rdquo;.</a>';
          } else {
            box.innerHTML = list.map(function (p) {
              return '<a href="/rme?pasien=' + encodeURIComponent(p.id) + '"><strong>' + esc(p.name) + '</strong><br><span class="sr-meta">' +
                esc(p.id) + ' · NIK ' + esc(p.nik) + '</span></a>';
            }).join('');
          }
          box.classList.add('open');
        }).catch(function () { close(); });
      }, 300);
    });
    input.addEventListener('keydown', function (ev) { if (ev.key === 'Escape') close(); });
    document.addEventListener('click', function (ev) {
      if (!box.contains(ev.target) && ev.target !== input) close();
    });
    document.addEventListener('keydown', function (ev) {
      if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'k') { ev.preventDefault(); input.focus(); }
    });
  }

  window.Waras = {
    TODAY: TODAY, api: api, asList: asList, esc: esc,
    fmtRp: fmtRp, fmtJt: fmtJt, fmtNum: fmtNum, fmtDateId: fmtDateId,
    initials: initials, drugDose: drugDose, statusPill: statusPill, pillFor: pillFor,
    labelFor: labelFor, roleLabel: roleLabel, toast: toast,
    patientsMap: patientsMap, patientName: patientName,
    token: token, setToken: setToken, initShell: initShell
  };
  document.addEventListener('DOMContentLoaded', initShell);
})();
