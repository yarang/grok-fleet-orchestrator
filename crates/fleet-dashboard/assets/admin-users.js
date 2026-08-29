    async function fetchUsers() {
      try {
        const resp = await fetch('api/users');
        const users = await resp.json();
        render(users);
      } catch(e) { console.error('fetch users:', e); }
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      return new Date(iso).toLocaleString();
    }

    let _users = [];
    let _canCreate = false;
    let _canDelete = false;

    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    async function checkPerms() {
      try {
        const resp = await fetch('api/me');
        const me = await resp.json();
        _canCreate = (me.permissions||[]).includes('user:create');
        _canDelete = (me.permissions||[]).includes('user:delete');
        if (_canCreate) {
          document.getElementById('show-create-btn').style.display = '';
        } else {
          document.getElementById('show-create-btn').style.display = 'none';
        }
      } catch(e) { console.error('checkPerms', e); }
    }

    function render(users) {
      _users = users || [];
      const table = document.getElementById('user-table');
      const empty = document.getElementById('empty-state');

      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!users || users.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const u of users) {
        const row = document.createElement('div');
        row.className = 'row';

        let actions = '';
        if (_canCreate) {
          actions += `<button type="button" class="btn btn-sm" data-toggle-user="${escapeHtml(u.id)}">${u.enabled ? 'Disable' : 'Enable'}</button> `;
        }
        if (_canDelete) {
          actions += `<button type="button" class="btn btn-sm btn-danger" data-delete-user="${escapeHtml(u.id)}" data-username="${escapeHtml(u.username)}">Delete</button>`;
        }
        if (!actions) actions = '<span style="font-size:12px;color:var(--ink-muted-48);">—</span>';

        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(u.username)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(u.email||'—')}</div>
          <div>${(u.roles||[]).map(r=>'<span class="badge badge-provisioned">'+escapeHtml(r)+'</span>').join(' ')||'—'}</div>
          <div>${u.enabled ? '<span class="badge badge-online">enabled</span>' : '<span class="badge badge-failed">disabled</span>'}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(u.last_login_at)}</div>
          <div style="display:flex;gap:4px;flex-wrap:wrap;">${actions}</div>
        `;
        table.appendChild(row);
      }

      // 인라인 onclick 금지 — 이유는 `admin-ssh-keys.js`의 같은 배선에 적어 두었다.
      // 여기서는 사용자명이 그 자리에 들어갔으므로 노출이 더 직접적이었다.
      table.querySelectorAll('button[data-toggle-user]').forEach(b => {
        b.addEventListener('click', () => toggleUser(b.getAttribute('data-toggle-user')));
      });
      table.querySelectorAll('button[data-delete-user]').forEach(b => {
        b.addEventListener('click', () =>
          deleteUser(b.getAttribute('data-delete-user'), b.getAttribute('data-username')));
      });
    }

    // `currentEnabled`는 받기만 하고 쓰지 않던 죽은 인자였다. 서버가
    // `POST /api/users/{id}/toggle` 하나로 현재 상태를 보고 뒤집으므로
    // 클라이언트가 알려 줄 필요가 없다. 인라인 onclick을 걷어내면서 함께
    // 지웠다 — 이 인자가 `'${u.enabled}'`로 속성에 박히던 자리였다.
    async function toggleUser(id) {
      const body = new URLSearchParams({ csrf_token: getCsrf() });
      try {
        const resp = await fetch('api/users/'+id+'/toggle', { method:'POST', body, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
        if (!resp.ok) { const t = await resp.text(); alert('Failed: ' + t); return; }
        fetchUsers();
      } catch(e) { alert('Error: ' + e.message); }
    }

    async function deleteUser(id, username) {
      if (!confirm('Delete user "' + username + '"? This cannot be undone.')) return;
      const body = new URLSearchParams({ csrf_token: getCsrf() });
      try {
        const resp = await fetch('api/users/'+id+'/delete', { method:'POST', body, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
        if (!resp.ok) { const t = await resp.text(); alert('Failed: ' + t); return; }
        fetchUsers();
      } catch(e) { alert('Error: ' + e.message); }
    }

    document.getElementById('create-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const data = new URLSearchParams(new FormData(form));
      data.set('csrf_token', getCsrf());
      const msg = document.getElementById('create-msg');
      msg.style.display = 'none';
      try {
        const resp = await fetch('api/users', { method:'POST', body:data, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
        if (resp.ok) {
          form.reset();
          document.getElementById('create-panel').style.display = 'none';
          document.getElementById('show-create-btn').style.display = '';
          fetchUsers();
        } else {
          const t = await resp.text();
          msg.textContent = 'Error: ' + t;
          msg.style.display = 'block';
        }
      } catch(e) {
        msg.textContent = 'Error: ' + e.message;
        msg.style.display = 'block';
      }
    });


    // 권한 확인 후 사용자 목록 로드.
    document.getElementById('show-create-btn').style.display = 'none';
    checkPerms().then(() => fetchUsers());

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}

  
