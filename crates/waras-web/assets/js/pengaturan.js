// Pengaturan & Audit: GET /api/pengaturan, PUT /api/pengaturan/{key},
// GET /api/health (kartu mode), GET /api/audit (read-only, terbaru dulu).
// Kolom Hash = 8 karakter pertama entry_hash (FASE4.md §6.1).
// Kunci yang diizinkan: backup_otomatis, sinkron_satusehat, mode_offline
// (on/off), retensi_audit_hari (angka 1..3650) — FASE4.md §4.1.
(function () {
  'use strict';

  var TOGGLES = [
    { key: 'backup_otomatis', label: 'Backup otomatis harian', desc: 'Backup setiap hari 02.00, terenkripsi.' },
    { key: 'sinkron_satusehat', label: 'Sinkron SATUSEHAT otomatis', desc: 'Kirim resource FHIR ke SATUSEHAT saat encounter ditutup.' },
    { key: 'mode_offline', label: 'Mode offline (antrean lokal)', desc: 'Antrean tetap jalan tanpa internet; sinkron saat online.' }
  ];

  function showErr(id, msg) {
    var el = document.getElementById(id);
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }
  function showOk(id, msg) {
    var el = document.getElementById(id);
    if (el) { el.textContent = msg; el.classList.add('on'); }
  }
  function clearSettingMsg() {
    var e = document.getElementById('setting-error'), o = document.getElementById('setting-ok');
    if (e) e.classList.remove('on');
    if (o) o.classList.remove('on');
  }

  async function saveSetting(key, value) {
    clearSettingMsg();
    try {
      await Waras.api('/api/pengaturan/' + encodeURIComponent(key), {
        method: 'PUT', body: { value: value }
      });
      showOk('setting-ok', 'Pengaturan ' + key + ' disimpan (' + value + ').');
      Waras.toast('Pengaturan disimpan.');
    } catch (e) {
      showErr('setting-error', 'Simpan gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      throw e;
    }
  }

  async function loadHealth() {
    var card = document.getElementById('mode-card');
    try {
      var h = await Waras.api('/api/health');
      var mode = String(h.mode || '').toLowerCase();
      var modeLabel = mode === 'postgres' ? 'Mode Rumah Sakit (PostgreSQL)' : 'Mode Klinik (SQLite)';
      card.innerHTML = '<p style="margin:0;line-height:1.8">' + Waras.esc(modeLabel) + '.<br>' +
        'Versi ' + Waras.esc(h.version || '') + ' · Tanggal klinik ' + Waras.esc(h.today || '') + '.<br>' +
        '<span class="muted">Status: ' + Waras.esc(h.status || 'ok') + '</span></p>';
    } catch (e) {
      card.innerHTML = '<p class="empty-note">Mode instalasi belum tersedia (endpoint /api/health belum aktif).</p>';
    }
  }

  async function loadSettings() {
    var box = document.getElementById('setting-toggles');
    try {
      var list = Waras.asList(await Waras.api('/api/pengaturan'));
      var map = {};
      list.forEach(function (s) { map[s.key] = s.value; });
      box.innerHTML = TOGGLES.map(function (t) {
        var on = String(map[t.key] || 'off').toLowerCase() === 'on';
        return '<div class="setting-row"><div class="lbl">' + Waras.esc(t.label) +
          '<span class="desc">' + Waras.esc(t.desc) + '</span></div>' +
          '<label class="switch"><input type="checkbox" data-setting="' + Waras.esc(t.key) + '"' +
          (on ? ' checked' : '') + '><span class="slider"></span></label></div>';
      }).join('');
      box.querySelectorAll('input[data-setting]').forEach(function (input) {
        input.addEventListener('change', async function () {
          var key = input.getAttribute('data-setting');
          var value = input.checked ? 'on' : 'off';
          try {
            await saveSetting(key, value);
          } catch (e) {
            input.checked = !input.checked; // kembalikan bila gagal
          }
        });
      });
      var ret = document.getElementById('retensi');
      if (map.retensi_audit_hari != null) ret.value = map.retensi_audit_hari;
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Pengaturan belum tersedia (endpoint /api/pengaturan belum aktif).</p>';
    }
  }

  async function loadAudit() {
    var tbody = document.getElementById('audit-body');
    try {
      var list = Waras.asList(await Waras.api('/api/audit?limit=50'));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="5" class="empty-note">Belum ada entri audit.</td></tr>';
        return;
      }
      tbody.innerHTML = list.map(function (a) {
        var ts = String(a.timestamp || '');
        var time = ts.length >= 16 ? ts.slice(11, 16) : ts;
        var obj = [a.resource_type, a.resource_id].filter(Boolean).join(' ');
        return '<tr><td>' + Waras.esc(time) + '</td><td>' + Waras.esc(a.actor || '') + '</td><td>' +
          Waras.esc(a.action || '') + '</td><td>' + Waras.esc(obj) + '</td><td><code>' +
          Waras.esc(String(a.entry_hash || '').slice(0, 8)) + '</code></td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="5" class="empty-note">Audit trail belum tersedia (endpoint /api/audit belum aktif).</td></tr>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadHealth();
    loadSettings();
    loadAudit();
    document.getElementById('retensi-save').addEventListener('click', async function () {
      var btn = this;
      btn.disabled = true;
      var val = document.getElementById('retensi').value.trim();
      try {
        await saveSetting('retensi_audit_hari', val);
      } catch (e) {
        /* pesan sudah tampil */
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
