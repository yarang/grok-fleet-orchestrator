    async function fetchUsers() {
      try {
        const resp = await fetch('/api/users');
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
        const resp = await fetch('/api/me');
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
          actions += `<button class="btn btn-sm" onclick="toggleUser('${u.id}','${u.enabled}')">${u.enabled ? 'Disable' : 'Enable'}</button> `;
        }
        if (_canDelete) {
          actions += `<button class="btn btn-sm btn-danger" onclick="deleteUser('${u.id}','${escapeHtml(u.username)}')">Delete</button>`;
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
    }

    async function toggleUser(id, currentEnabled) {
      const body = new URLSearchParams({ csrf_token: getCsrf() });
      try {
        const resp = await fetch('/api/users/'+id+'/toggle', { method:'POST', body, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
        if (!resp.ok) { const t = await resp.text(); alert('Failed: ' + t); return; }
        fetchUsers();
      } catch(e) { alert('Error: ' + e.message); }
    }

    async function deleteUser(id, username) {
      if (!confirm('Delete user "' + username + '"? This cannot be undone.')) return;
      const body = new URLSearchParams({ csrf_token: getCsrf() });
      try {
        const resp = await fetch('/api/users/'+id+'/delete', { method:'POST', body, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
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
        const resp = await fetch('/api/users', { method:'POST', body:data, headers:{'Content-Type':'application/x-www-form-urlencoded'} });
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

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    // 권한 확인 후 사용자 목록 로드.
    document.getElementById('show-create-btn').style.display = 'none';
    checkPerms().then(() => fetchUsers());

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}

  
