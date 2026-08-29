    let allProjects = [];
    let sortKey = 'created_at';
    let sortDir = 'desc';

    async function fetchProjects() {
      try {
        const resp = await fetch('api/projects');
        if (!resp.ok) {
          // project:read가 없으면 403 — 빈 목록 대신 이유를 보여준다.
          if (resp.status === 403) {
            document.getElementById('project-table').style.display = 'none';
            const empty = document.getElementById('empty-state');
            empty.querySelector('h3').textContent = 'Not permitted';
            empty.querySelector('p').textContent = 'You do not have permission to view projects.';
            empty.style.display = 'block';
          }
          return;
        }
        allProjects = await resp.json();
        render();
      } catch(e) { console.error('fetch projects:', e); }
    }

    function compareProjects(a, b, key, dir) {
      let va, vb;
      if (key === 'created_at') {
        va = new Date(a.created_at).getTime();
        vb = new Date(b.created_at).getTime();
      } else {
        va = String(a[key] ?? '').toLowerCase();
        vb = String(b[key] ?? '').toLowerCase();
      }
      const cmp = va < vb ? -1 : va > vb ? 1 : 0;
      return dir === 'asc' ? cmp : -cmp;
    }

    function updateSortIndicators() {
      document.querySelectorAll('#project-table-header .sortable').forEach(cell => {
        const active = cell.dataset.sortKey === sortKey;
        cell.classList.toggle('sort-active', active);
        cell.dataset.sortDir = active ? sortDir : '';
      });
    }

    document.querySelectorAll('#project-table-header .sortable').forEach(cell => {
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

    // active/draining/archived — styles.css에 전용 badge 클래스가 없는
    // 값이라(#48이 신설한 상태) 기존 팔레트에 매핑한다.
    function statusBadge(status) {
      const cls = status === 'active' ? 'badge-online'
                : status === 'draining' ? 'badge-pending'
                : 'badge-cancelled';
      return '<span class="badge '+cls+'">'+escapeHtml(status)+'</span>';
    }

    function render() {
      const table = document.getElementById('project-table');
      const empty = document.getElementById('empty-state');
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!allProjects || allProjects.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      const projects = [...allProjects].sort((a, b) => compareProjects(a, b, sortKey, sortDir));
      for (const p of projects) {
        const row = document.createElement('div');
        row.className = 'row';
        row.style.cursor = 'pointer';
        row.onclick = () => window.location.href = 'projects/' + encodeURIComponent(p.id);
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(p.name)}</div>
          <div>${statusBadge(p.status)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(p.description || '—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(p.created_by || '—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(p.created_at)}</div>
        `;
        table.appendChild(row);
      }
    }


    fetchProjects();
    setInterval(fetchProjects, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
