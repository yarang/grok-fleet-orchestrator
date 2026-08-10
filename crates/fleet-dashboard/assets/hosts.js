    async function fetchHosts() {
      try {
        const resp = await fetch('/api/hosts');
        const hosts = await resp.json();
        render(hosts);
      } catch(e) { console.error('fetch hosts:', e); }
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      const d = new Date(iso);
      const now = new Date();
      const diff = (now - d) / 1000;
      if (diff < 60) return Math.floor(diff)+'s ago';
      if (diff < 3600) return Math.floor(diff/60)+'m ago';
      if (diff < 86400) return Math.floor(diff/3600)+'h ago';
      return d.toLocaleDateString();
    }

    function grokBadge(version) {
      if (!version) return '<span style="color:var(--err);font-size:13px;">not installed</span>';
      return '<span style="color:var(--ok);font-size:13px;">'+version+'</span>';
    }

    function render(hosts) {
      const table = document.getElementById('host-table');
      const empty = document.getElementById('empty-state');

      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!hosts || hosts.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const h of hosts) {
        const row = document.createElement('div');
        row.className = 'row';
        row.style.cursor = 'pointer';
        row.onclick = () => window.location.href = '/hosts/' + encodeURIComponent(h.hostname);
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(h.hostname)}</div>
          <div><span class="badge badge-${h.status}">${h.status}</span></div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${h.worker_name||'—'}</div>
          <div>${grokBadge(h.grok_version)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${h.fleet_worker_version||'—'}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${h.os_type||'—'}</div>
          <div style="font-size:13px;font-family:var(--font-mono);">${h.arch||'—'}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(h.last_heartbeat_at)}</div>
        `;
        table.appendChild(row);
      }
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    fetchHosts();
    setInterval(fetchHosts, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}

  
