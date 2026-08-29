    let allHosts = [];
    // 로드맵 #14 — 컬럼 정렬 상태 (tasks.js와 동일한 패턴).
    let sortKey = 'hostname';
    let sortDir = 'asc';

    async function fetchHosts() {
      try {
        const resp = await fetch('api/hosts');
        allHosts = await resp.json();
        render();
      } catch(e) { console.error('fetch hosts:', e); }
    }

    function compareHosts(a, b, key, dir) {
      let va, vb;
      if (key === 'last_heartbeat_at') {
        va = a.last_heartbeat_at ? new Date(a.last_heartbeat_at).getTime() : -1;
        vb = b.last_heartbeat_at ? new Date(b.last_heartbeat_at).getTime() : -1;
      } else {
        va = String(a[key] ?? '').toLowerCase();
        vb = String(b[key] ?? '').toLowerCase();
      }
      const cmp = va < vb ? -1 : va > vb ? 1 : 0;
      return dir === 'asc' ? cmp : -cmp;
    }

    function updateSortIndicators() {
      document.querySelectorAll('#host-table-header .sortable').forEach(cell => {
        const active = cell.dataset.sortKey === sortKey;
        cell.classList.toggle('sort-active', active);
        cell.dataset.sortDir = active ? sortDir : '';
      });
    }

    document.querySelectorAll('#host-table-header .sortable').forEach(cell => {
      cell.addEventListener('click', () => {
        const key = cell.dataset.sortKey;
        if (sortKey === key) {
          sortDir = sortDir === 'asc' ? 'desc' : 'asc';
        } else {
          sortKey = key;
          sortDir = 'asc';
        }
        updateSortIndicators();
        render();
      });
    });
    updateSortIndicators();

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
      // 헬퍼 **안에서** 이스케이프한다. 호출부(`${grokBadge(h.grok_version)}`)는
      // 이미 HTML 조각을 받는 자리라 거기서 감쌀 수 없고, `grok_version`은
      // 워커가 스스로 보고하는 검증되지 않은 문자열이다.
      return '<span style="color:var(--ok);font-size:13px;">'+escapeHtml(version)+'</span>';
    }

    function render() {
      const table = document.getElementById('host-table');
      const empty = document.getElementById('empty-state');

      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!allHosts || allHosts.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      // 로드맵 #14 — 컬럼 정렬.
      const hosts = [...allHosts].sort((a, b) => compareHosts(a, b, sortKey, sortDir));

      for (const h of hosts) {
        const row = document.createElement('div');
        row.className = 'row';
        row.style.cursor = 'pointer';
        row.onclick = () => window.location.href = 'hosts/' + encodeURIComponent(h.hostname);
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(h.hostname)}</div>
          <div><span class="badge badge-${escapeHtml(h.status)}">${escapeHtml(h.status)}</span></div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(h.worker_name||'—')}</div>
          <div>${grokBadge(h.grok_version)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(h.fleet_worker_version||'—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(h.os_type||'—')}</div>
          <div style="font-size:13px;font-family:var(--font-mono);">${escapeHtml(h.arch||'—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(h.last_heartbeat_at)}</div>
        `;
        table.appendChild(row);
      }
    }


    fetchHosts();
    setInterval(fetchHosts, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}

  
