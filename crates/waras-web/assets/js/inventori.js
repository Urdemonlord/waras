// Inventori: GET /api/inventori/items, POST /api/inventori/items,
// GET /api/inventori/moves, POST /api/inventori/items/{id}/moves
// (FASE4.md §5.1). Status stok memakai ambang yang sama dengan Farmasi
// (<=12 Kritis, <=30 Segera = segera restock, else Aman) — pakai
// Waras.pillFor yang sudah memetakan ketiganya.
(function () {
  'use strict';

  var items = [];

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
  function stockStatusOf(item) {
    if (item.stock_status) return item.stock_status;
    var s = Number(item.stock) || 0;
    if (s <= 12) return 'Kritis';
    if (s <= 30) return 'Segera';
    return 'Aman';
  }

  async function loadItems() {
    var tbody = document.getElementById('inv-body');
    try {
      items = Waras.asList(await Waras.api('/api/inventori/items'));
      if (!items.length) {
        tbody.innerHTML = '<tr><td colspan="8" class="empty-note">Belum ada barang inventori.</td></tr>';
      } else {
        tbody.innerHTML = items.map(function (it) {
          return '<tr><td>' + Waras.esc(it.id) + '</td><td>' + Waras.esc(it.name) + '</td><td>' +
            Waras.esc(it.category) + '</td><td class="num">' + Waras.fmtNum(it.stock) + '</td><td>' +
            Waras.esc(it.unit) + '</td><td class="num">' + Waras.fmtNum(it.min_stock) + '</td><td>' +
            Waras.esc(it.location || '') + '</td><td>' + Waras.pillFor(stockStatusOf(it)) + '</td></tr>';
        }).join('');
      }
      var sel = document.getElementById('move-item');
      sel.innerHTML = items.map(function (it) {
        return '<option value="' + Waras.esc(it.id) + '">' + Waras.esc(it.name) + ' — stok ' + Waras.fmtNum(it.stock) + ' ' + Waras.esc(it.unit) + '</option>';
      }).join('') || '<option value="">Belum ada barang</option>';
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="8" class="empty-note">Inventori belum tersedia (endpoint /api/inventori/items belum aktif).</td></tr>';
    }
  }

  async function loadMoves() {
    var box = document.getElementById('moves-list');
    try {
      var list = Waras.asList(await Waras.api('/api/inventori/moves'));
      if (!list.length) {
        box.innerHTML = '<p class="empty-note">Belum ada mutasi stok.</p>';
        return;
      }
      var names = {};
      items.forEach(function (it) { names[it.id] = it.name; });
      box.innerHTML = list.slice(0, 10).map(function (m) {
        var sign = m.kind === 'Keluar' ? '−' : (m.kind === 'Masuk' ? '+' : '=');
        return '<div style="padding:8px 0;border-bottom:1px solid #F1F2F4;font-size:13px"><strong>' +
          Waras.esc(names[m.item_id] || m.item_id) + '</strong> · ' + Waras.esc(m.kind) + ' ' + sign +
          Waras.fmtNum(m.qty) + '<br><span class="muted">' + Waras.esc(m.note || '') + ' · ' +
          Waras.esc(m.by_name || '') + ' · ' + Waras.esc(String(m.created_at || '').slice(0, 16)) + '</span></div>';
      }).join('');
    } catch (e) {
      box.innerHTML = '<p class="empty-note">Mutasi belum tersedia (endpoint /api/inventori/moves belum aktif).</p>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadItems().then(loadMoves);
    document.getElementById('btn-new-item').addEventListener('click', function (ev) {
      ev.preventDefault();
      document.getElementById('item-form-card').style.display = 'block';
      document.getElementById('item-name').focus();
    });
    document.getElementById('item-cancel').addEventListener('click', function () {
      document.getElementById('item-form-card').style.display = 'none';
      clearMsg('item-error', 'item-ok');
    });
    document.getElementById('item-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('item-error', 'item-ok');
      try {
        var it = await Waras.api('/api/inventori/items', {
          method: 'POST',
          body: {
            name: document.getElementById('item-name').value.trim(),
            category: document.getElementById('item-category').value,
            unit: document.getElementById('item-unit').value.trim(),
            stock: Number(document.getElementById('item-stock').value),
            min_stock: Number(document.getElementById('item-min').value),
            location: document.getElementById('item-location').value.trim(),
            expiry: document.getElementById('item-expiry').value
          }
        });
        showOk('item-ok', 'Barang ' + (it.id || '') + ' disimpan.');
        Waras.toast('Barang inventori disimpan.');
        await loadItems();
      } catch (e) {
        showErr('item-error', 'Simpan barang gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
    document.getElementById('move-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('move-error', 'move-ok');
      var itemId = document.getElementById('move-item').value;
      try {
        await Waras.api('/api/inventori/items/' + encodeURIComponent(itemId) + '/moves', {
          method: 'POST',
          body: {
            kind: document.getElementById('move-kind').value,
            qty: Number(document.getElementById('move-qty').value),
            note: document.getElementById('move-note').value.trim()
          }
        });
        showOk('move-ok', 'Mutasi disimpan, stok diperbarui.');
        Waras.toast('Mutasi stok disimpan.');
        await loadItems();
        await loadMoves();
      } catch (e) {
        showErr('move-error', 'Mutasi gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
