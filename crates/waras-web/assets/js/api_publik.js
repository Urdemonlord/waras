// API Publik: GET /api/api-keys, POST /api/api-keys,
// POST /api/api-keys/{id}/revoke (FASE4.md §5.2).
// Kunci penuh HANYA SEKALI ditampilkan di modal Waras.modal (tombol salin
// + peringatan "tidak bisa dilihat lagi"); cabut kunci pakai konfirmasi
// modal. Kunci TIDAK PERNAH disimpan/disimpan ulang di klien.
(function () {
  'use strict';

  var keys = [];

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

  function renderKeys() {
    var tbody = document.getElementById('keys-body');
    if (!keys.length) {
      tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Belum ada kunci API. Buat kunci pertama untuk integrasi.</td></tr>';
      return;
    }
    tbody.innerHTML = keys.map(function (k) {
      var revoked = k.revoked === 1 || k.revoked === true;
      var scopes = Array.isArray(k.scopes) ? k.scopes.join(', ') : String(k.scopes || '');
      return '<tr><td>' + Waras.esc(k.name) + '</td><td><code>' + Waras.esc(k.key_prefix || '') + '&hellip;</code></td><td>' +
        Waras.esc(scopes) + '</td><td>' + Waras.esc(String(k.created_at || '').slice(0, 10)) + '</td><td>' +
        Waras.esc(k.last_used_at ? String(k.last_used_at).slice(0, 16).replace('T', ' ') : '—') + '</td><td>' +
        (revoked ? Waras.statusPill('Dicabut', 'red') : Waras.statusPill('Aktif', 'green')) + '</td><td>' +
        (revoked ? '' : '<button type="button" class="btn sm" data-revoke="' + Waras.esc(k.id) + '" data-name="' + Waras.esc(k.name) + '">Cabut</button>') +
        '</td></tr>';
    }).join('');
    tbody.querySelectorAll('button[data-revoke]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var id = btn.getAttribute('data-revoke');
        var name = btn.getAttribute('data-name');
        Waras.modal({
          title: 'Cabut kunci API?',
          bodyHtml: '<p style="margin:0">Kunci <strong>' + Waras.esc(name) + '</strong> akan dicabut dan ' +
            'tidak bisa dipakai lagi. Integrasi yang memakai kunci ini langsung gagal (401). Tindakan ini tidak bisa dibatalkan.</p>',
          actions: [
            { label: 'Batal', kind: '' },
            {
              label: 'Cabut kunci', kind: 'danger',
              onClick: async function () {
                try {
                  await Waras.api('/api/api-keys/' + encodeURIComponent(id) + '/revoke', { method: 'POST' });
                  Waras.toast('Kunci ' + name + ' dicabut.');
                  await loadKeys();
                } catch (e) {
                  Waras.toast('Cabut kunci gagal: ' + (e && e.message ? e.message : ''), true);
                }
              }
            }
          ]
        });
      });
    });
  }

  async function loadKeys() {
    // Hanya Admin boleh mengelola kunci (ManageApiKeys). Cek peran dulu
    // lewat /api/auth/me supaya halaman TIDAK menembak /api/api-keys
    // untuk peran lain — tembakan itu 403 dan tercatat sebagai console
    // error di browser meski sudah ditangkap.
    try {
      var me = await Waras.api('/api/auth/me');
      var role = (me.user || me).role;
      if (role !== 'admin') {
        document.getElementById('keys-body').innerHTML = '<tr><td colspan="7" class="empty-note">Hanya Admin yang dapat mengelola kunci API. Peran Anda tidak memiliki akses ini.</td></tr>';
        var nb = document.getElementById('btn-new-key');
        if (nb) nb.style.display = 'none';
        return;
      }
    } catch (e) {
      document.getElementById('keys-body').innerHTML = '<tr><td colspan="7" class="empty-note">Kunci API belum tersedia (endpoint /api/api-keys belum aktif).</td></tr>';
      return;
    }
    try {
      keys = Waras.asList(await Waras.api('/api/api-keys'));
      renderKeys();
    } catch (e) {
      document.getElementById('keys-body').innerHTML = '<tr><td colspan="7" class="empty-note">Kunci API belum tersedia (endpoint /api/api-keys belum aktif).</td></tr>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadKeys();
    document.getElementById('btn-new-key').addEventListener('click', function (ev) {
      ev.preventDefault();
      document.getElementById('key-form-card').style.display = 'block';
      document.getElementById('key-name').focus();
    });
    document.getElementById('key-cancel').addEventListener('click', function () {
      document.getElementById('key-form-card').style.display = 'none';
      clearMsg('key-error', 'key-ok');
    });
    document.getElementById('key-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('key-error', 'key-ok');
      var scopes = Array.prototype.slice.call(document.querySelectorAll('.key-scope:checked')).map(function (c) { return c.value; });
      try {
        var data = await Waras.api('/api/api-keys', {
          method: 'POST',
          body: { name: document.getElementById('key-name').value.trim(), scopes: scopes }
        });
        var fullKey = data.key || (data.api_key && data.api_key.key);
        var created = data.api_key || data;
        document.getElementById('key-form-card').style.display = 'none';
        ev.target.reset();
        showOk('key-ok', '');
        if (fullKey) {
          Waras.modal({
            title: 'Kunci API dibuat',
            bodyHtml: '<p style="margin:0 0 8px">Salin kunci ini sekarang. <strong>Kunci penuh tidak bisa dilihat lagi</strong> setelah modal ini ditutup &mdash; hanya prefix yang tersimpan.</p>' +
              '<pre style="background:var(--neutral-bg);border:1px solid var(--border);border-radius:8px;padding:12px;font-size:13px;word-break:break-all;white-space:pre-wrap;margin:0">' +
              Waras.esc(fullKey) + '</pre>',
            actions: [
              { label: 'Tutup', kind: '' },
              {
                label: 'Salin kunci', kind: 'primary',
                onClick: async function () {
                  try {
                    await copyText(fullKey);
                    Waras.toast('Kunci API disalin.');
                  } catch (e) {
                    Waras.toast('Salin kunci gagal.', true);
                  }
                }
              }
            ]
          });
        } else {
          Waras.toast('Kunci dibuat, tetapi kunci penuh tidak dikembalikan server.', true);
        }
        await loadKeys();
      } catch (e) {
        showErr('key-error', 'Buat kunci gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
