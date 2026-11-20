// Kepegawaian: GET /api/kepegawaian/staff, POST /api/kepegawaian/staff,
// POST /api/kepegawaian/staff/{id}/active, GET /api/kepegawaian/shifts,
// POST /api/kepegawaian/shifts (FASE4.md §5.1).
// Nonaktifkan pegawai memakai konfirmasi Waras.modal (pemakaian modal
// ke-3, setelah kunci API sekali + cabut kunci).
(function () {
  'use strict';

  var staff = [];

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
  function isActive(s) {
    return s.active === 1 || s.active === true;
  }

  async function loadStaff() {
    var tbody = document.getElementById('staff-body');
    var title = document.getElementById('staff-title');
    try {
      staff = Waras.asList(await Waras.api('/api/kepegawaian/staff'));
      title.textContent = 'Daftar pegawai — ' + Waras.fmtNum(staff.length) + ' orang';
      if (!staff.length) {
        tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Belum ada data pegawai.</td></tr>';
      } else {
        tbody.innerHTML = staff.map(function (s) {
          var active = isActive(s);
          return '<tr><td>' + Waras.esc(s.nip || '') + '</td><td>' + Waras.esc(s.name) + '</td><td>' +
            Waras.esc(s.role) + '</td><td>' + Waras.esc(s.poli || '') + '</td><td>' +
            Waras.esc(s.phone || '') + '</td><td>' +
            (active ? Waras.statusPill('Aktif', 'green') : Waras.statusPill('Nonaktif', 'neutral')) + '</td><td>' +
            '<button type="button" class="btn sm" data-toggle="' + Waras.esc(s.id) + '" data-name="' +
            Waras.esc(s.name) + '" data-active="' + (active ? '1' : '0') + '">' +
            (active ? 'Nonaktifkan' : 'Aktifkan') + '</button></td></tr>';
        }).join('');
      }
      tbody.querySelectorAll('button[data-toggle]').forEach(function (btn) {
        btn.addEventListener('click', function () {
          var id = btn.getAttribute('data-toggle');
          var name = btn.getAttribute('data-name');
          var turningOff = btn.getAttribute('data-active') === '1';
          Waras.modal({
            title: turningOff ? 'Nonaktifkan pegawai?' : 'Aktifkan pegawai?',
            bodyHtml: '<p style="margin:0">Pegawai <strong>' + Waras.esc(name) + '</strong> akan ' +
              (turningOff ? 'dinonaktifkan (tidak bisa dijadwalkan shift; datanya tetap tersimpan).' : 'diaktifkan kembali.') + '</p>',
            actions: [
              { label: 'Batal', kind: '' },
              {
                label: turningOff ? 'Nonaktifkan' : 'Aktifkan', kind: turningOff ? 'danger' : 'primary',
                onClick: async function () {
                  try {
                    await Waras.api('/api/kepegawaian/staff/' + encodeURIComponent(id) + '/active', {
                      method: 'POST', body: { active: !turningOff }
                    });
                    Waras.toast(name + (turningOff ? ' dinonaktifkan.' : ' diaktifkan.'));
                    await loadStaff();
                  } catch (e) {
                    Waras.toast('Ubah status gagal: ' + (e && e.message ? e.message : ''), true);
                  }
                }
              }
            ]
          });
        });
      });
      var sel = document.getElementById('shift-staff');
      sel.innerHTML = staff.filter(isActive).map(function (s) {
        return '<option value="' + Waras.esc(s.id) + '">' + Waras.esc(s.name) + ' — ' + Waras.esc(s.role) + '</option>';
      }).join('') || '<option value="">Belum ada pegawai aktif</option>';
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="7" class="empty-note">Daftar pegawai belum tersedia (endpoint /api/kepegawaian/staff belum aktif).</td></tr>';
    }
  }

  async function loadShifts() {
    var date = document.getElementById('shift-date').value || Waras.TODAY;
    var tbody = document.getElementById('shift-body');
    var names = {};
    staff.forEach(function (s) { names[s.id] = s.name; });
    try {
      var list = Waras.asList(await Waras.api('/api/kepegawaian/shifts?date=' + encodeURIComponent(date)));
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="3" class="empty-note">Belum ada shift pada ' + Waras.esc(date) + '.</td></tr>';
        return;
      }
      tbody.innerHTML = list.map(function (sh) {
        return '<tr><td>' + Waras.esc(sh.staff_name || names[sh.staff_id] || sh.staff_id) + '</td><td>' +
          Waras.esc(sh.shift) + '</td><td>' + Waras.esc(sh.poli || '') + '</td></tr>';
      }).join('');
    } catch (e) {
      tbody.innerHTML = '<tr><td colspan="3" class="empty-note">Jadwal shift belum tersedia (endpoint /api/kepegawaian/shifts belum aktif).</td></tr>';
    }
  }

  document.addEventListener('DOMContentLoaded', function () {
    loadStaff().then(loadShifts);
    document.getElementById('shift-date').addEventListener('change', loadShifts);
    document.getElementById('btn-new-staff').addEventListener('click', function (ev) {
      ev.preventDefault();
      document.getElementById('staff-form-card').style.display = 'block';
      document.getElementById('staff-name').focus();
    });
    document.getElementById('staff-cancel').addEventListener('click', function () {
      document.getElementById('staff-form-card').style.display = 'none';
      clearMsg('staff-error', 'staff-ok');
    });
    document.getElementById('staff-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('staff-error', 'staff-ok');
      try {
        var s = await Waras.api('/api/kepegawaian/staff', {
          method: 'POST',
          body: {
            name: document.getElementById('staff-name').value.trim(),
            nip: document.getElementById('staff-nip').value.trim(),
            role: document.getElementById('staff-role').value,
            poli: document.getElementById('staff-poli').value.trim(),
            phone: document.getElementById('staff-phone').value.trim()
          }
        });
        showOk('staff-ok', 'Pegawai ' + (s.id || '') + ' disimpan.');
        Waras.toast('Pegawai disimpan.');
        await loadStaff();
      } catch (e) {
        showErr('staff-error', 'Simpan pegawai gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
    document.getElementById('shift-form').addEventListener('submit', async function (ev) {
      ev.preventDefault();
      var btn = ev.target.querySelector('button[type="submit"]');
      btn.disabled = true;
      clearMsg('shift-error', 'shift-ok');
      try {
        await Waras.api('/api/kepegawaian/shifts', {
          method: 'POST',
          body: {
            staff_id: document.getElementById('shift-staff').value,
            date: document.getElementById('shift-date-new').value,
            shift: document.getElementById('shift-kind').value,
            poli: document.getElementById('shift-poli').value.trim()
          }
        });
        showOk('shift-ok', 'Jadwal shift disimpan.');
        Waras.toast('Jadwal shift disimpan.');
        await loadShifts();
      } catch (e) {
        showErr('shift-error', 'Jadwal gagal: ' + (e && e.message ? e.message : 'kesalahan tidak diketahui'));
      } finally {
        btn.disabled = false;
      }
    });
  });
})();
