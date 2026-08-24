    // 경로가 /projects/:id — 마지막 세그먼트가 id.
    const projectId = decodeURIComponent(window.location.pathname.split('/').filter(Boolean).pop());

    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      return new Date(iso).toLocaleString();
    }

    function statusBadge(status) {
      const cls = status === 'active' ? 'badge-online'
                : status === 'draining' ? 'badge-pending'
                : 'badge-cancelled';
      return '<span class="badge '+cls+'">'+escapeHtml(status)+'</span>';
    }

    async function fetchProject() {
      const resp = await fetch('api/projects/' + encodeURIComponent(projectId));
      if (!resp.ok) {
        document.getElementById('project-name').textContent =
          resp.status === 404 ? 'Project not found' : 'Failed to load project';
        return null;
      }
      const p = await resp.json();
      document.getElementById('project-name').textContent = p.name;
      document.getElementById('project-description').textContent = p.description || '';
      document.getElementById('project-status').innerHTML = statusBadge(p.status);
      document.getElementById('project-created-by').textContent = p.created_by || '—';
      document.getElementById('project-created-at').textContent = fmtTime(p.created_at);

      // archive는 되돌릴 수 없는 방향의 액션이라 이미 archived면 감춘다.
      const btn = document.getElementById('archive-btn');
      btn.style.display = p.status === 'archived' ? 'none' : 'inline-flex';
      btn.textContent = p.status === 'draining' ? 'Retry archive' : 'Archive project';
      return p;
    }

    document.getElementById('archive-btn').addEventListener('click', async () => {
      const btn = document.getElementById('archive-btn');
      const status = document.getElementById('archive-status');
      // 되돌릴 수 없는 방향이므로 확인을 받는다.
      if (!window.confirm('Archive this project? It will stop accepting new tasks.')) return;
      btn.disabled = true;
      status.textContent = 'Archiving…';
      status.style.color = 'var(--ink-muted-48)';
      try {
        const resp = await fetch('api/projects/' + encodeURIComponent(projectId), {
          method: 'DELETE',
          headers: { 'X-CSRF-Token': getCsrf() },
        });
        const rawText = await resp.text();
        let body = null;
        try { body = JSON.parse(rawText); } catch (_) {}
        if (!resp.ok) {
          const msg = (body && body.error && body.error.message) || rawText || ('HTTP ' + resp.status);
          status.textContent = 'Error: ' + msg;
          status.style.color = 'var(--badge-failed, #c0392b)';
          return;
        }
        // draining에 머무를 수 있다 — 비종료 Task가 남아 있으면 archive가
        // 완료되지 않는다. 그 사실을 그대로 알려준다.
        if (body && body.status === 'draining') {
          status.textContent = 'Draining — tasks still running; archive completes once they finish.';
          status.style.color = 'var(--badge-degraded, #b08800)';
        } else {
          status.textContent = 'Archived';
          status.style.color = 'var(--badge-online, #1a7f37)';
        }
        await fetchProject();
      } catch (e) {
        status.textContent = 'Error: ' + e.message;
        status.style.color = 'var(--badge-failed, #c0392b)';
      } finally {
        btn.disabled = false;
      }
    });

    async function fetchIssues() {
      const table = document.getElementById('issue-table');
      const empty = document.getElementById('issues-empty');
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());
      let issues = [];
      try {
        const resp = await fetch('api/issues?project_id=' + encodeURIComponent(projectId));
        if (!resp.ok) {
          // issue:read가 없으면 섹션을 숨긴다 — 빈 목록으로 오해하게 두지 않는다.
          if (resp.status === 403) {
            table.style.display = 'none';
            empty.querySelector('h3').textContent = 'Not permitted';
            empty.querySelector('p').textContent = 'You do not have permission to view issues.';
            empty.style.display = 'block';
          }
          return;
        }
        issues = await resp.json();
      } catch (e) { console.error('fetch issues', e); return; }

      if (!issues.length) {
        table.style.display = 'none';
        empty.style.display = 'block';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';
      for (const i of issues) {
        const row = document.createElement('div');
        row.className = 'row';
        // "진행 중"은 파생 배지다 — Issue 상태가 아니라 비터미널 연관 Task의
        // 존재에서 유도된다(#88: InProgress 상태를 두지 않은 이유).
        const inProgress = i.has_active_tasks
          ? ' <span class="badge badge-dispatched">in progress</span>' : '';
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(i.title)}${inProgress}</div>
          <div><span class="badge badge-pending">${escapeHtml(i.status)}</span></div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(i.severity)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(i.assignee || '—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(i.created_at)}</div>
        `;
        table.appendChild(row);
      }
    }

    async function fetchTasks() {
      const table = document.getElementById('task-table');
      const empty = document.getElementById('tasks-empty');
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());
      let tasks = [];
      try {
        const resp = await fetch('api/tasks');
        if (!resp.ok) return;
        // /api/tasks에는 project 필터가 없어 클라이언트에서 거른다 — 이
        // 페이지가 유일한 소비자라 API를 넓히지 않았다.
        tasks = (await resp.json()).filter(t => t.project_id === projectId);
      } catch (e) { console.error('fetch tasks', e); return; }

      if (!tasks.length) {
        table.style.display = 'none';
        empty.style.display = 'block';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';
      for (const t of tasks) {
        const row = document.createElement('div');
        row.className = 'row';
        row.style.cursor = 'pointer';
        row.onclick = () => window.location.href = 'tasks/' + encodeURIComponent(t.id);
        row.innerHTML = `
          <div style="font-size:13px;">${escapeHtml((t.prompt || '').slice(0, 80))}</div>
          <div><span class="badge badge-${escapeHtml(t.phase)}">${escapeHtml(t.phase)}</span></div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(t.worker_id || '—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(t.created_at)}</div>
        `;
        table.appendChild(row);
      }
    }

    async function refresh() {
      await fetchProject();
      await fetchIssues();
      await fetchTasks();
    }
    refresh();
    setInterval(refresh, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
