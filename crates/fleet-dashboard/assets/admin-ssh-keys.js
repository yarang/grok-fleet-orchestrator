    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    async function fetchKeys() {
      try {
        const resp = await fetch('/api/ssh-keys');
        if (!resp.ok) { console.error('fetch keys:', resp.status); return; }
        const keys = await resp.json();
        render(keys);
      } catch(e) { console.error('fetch keys:', e); }
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      return new Date(iso).toLocaleString();
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    function render(keys) {
      const table = document.getElementById('key-table');
      const empty = document.getElementById('empty-state');
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!keys || keys.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const k of keys) {
        const row = document.createElement('div');
        row.className = 'row';
        const fp = k.fingerprint.substring(0, 8) + '…' + k.fingerprint.substring(k.fingerprint.length - 8);
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(k.name)}</div>
          <div><span class="badge badge-provisioned">${escapeHtml(k.key_type)}</span></div>
          <div style="font-family:monospace;font-size:12px;color:var(--ink-muted-48,#888);">${fp}</div>
          <div style="font-size:13px;color:var(--ink-muted-48,#888);">${fmtTime(k.created_at)}</div>
          <div><button class="btn btn-sm btn-danger" onclick="deleteKey('${escapeHtml(k.name)}')">Delete</button></div>
        `;
        table.appendChild(row);
      }
    }

    async function deleteKey(name) {
      if (!confirm('Delete SSH key "' + name + '"? This cannot be undone.')) return;
      try {
        const resp = await fetch('/api/ssh-keys/' + encodeURIComponent(name), {
          method: 'DELETE',
          headers: { 'X-CSRF-Token': getCsrf() }
        });
        if (!resp.ok) { const t = await resp.text(); alert('Failed: ' + t); return; }
        fetchKeys();
      } catch(e) { alert('Error: ' + e.message); }
    }

    document.getElementById('upload-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const fd = new FormData(form);
      const body = { name: fd.get('name'), private_key: fd.get('private_key') };
      const msg = document.getElementById('upload-msg');
      msg.style.display = 'none';
      try {
        const resp = await fetch('/api/ssh-keys', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrf() },
          body: JSON.stringify(body)
        });
        if (resp.ok) {
          form.reset();
          document.getElementById('upload-panel').style.display = 'none';
          document.getElementById('show-upload-btn').style.display = '';
          fetchKeys();
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

    document.getElementById('show-upload-btn').style.display = 'none';

    // 권한 확인 (사용자 메뉴는 app.js의 renderUserMenu()가 #sidebar-user-menu에 렌더링).
    fetch('/api/me').then(r => r.json()).then(me => {
      if (!(me.permissions||[]).includes('host:provision')) {
        document.getElementById('show-upload-btn').style.display = 'none';
      } else {
        document.getElementById('show-upload-btn').style.display = '';
      }
    }).catch(()=>{});

    fetchKeys();

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
  
