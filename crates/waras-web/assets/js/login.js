// Login — POST /api/auth/login, simpan token, ke /dashboard.
// Kolom dibiarkan kosong; kredensial seed didokumentasikan di README, bukan di halaman.
(function () {
  'use strict';
  document.addEventListener('DOMContentLoaded', function () {
    if (Waras.token()) { location.href = '/dashboard'; return; }
    var form = document.getElementById('login-form');
    var err = document.getElementById('login-error');
    var btn = document.getElementById('login-btn');
    form.addEventListener('submit', async function (ev) {
      ev.preventDefault();
      err.classList.remove('on');
      btn.disabled = true;
      btn.textContent = 'Memeriksa…';
      try {
        var data = await Waras.api('/api/auth/login', {
          method: 'POST',
          body: {
            username: document.getElementById('username').value.trim(),
            password: document.getElementById('password').value
          }
        });
        Waras.setToken(data.token);
        location.href = '/dashboard';
      } catch (e) {
        err.textContent = e.message || 'Masuk gagal. Periksa NIP/nama pengguna dan kata sandi.';
        err.classList.add('on');
        btn.disabled = false;
        btn.textContent = 'Masuk';
      }
    });
  });
})();
