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

    // archive 게이트가 막은 사유를 문장으로 옮긴다.
    //
    // 라벨을 만드는 곳은 서버(`fleet_store::ArchiveBlockers::labels`)다. 여기서
    // agents/tasks 목록을 보고 **추론하지 않는 것**이 핵심이다 — 그렇게 하면
    // 게이트가 이 파일에 세 번째로 구현되고, 서버가 조건을 추가할 때마다 조용히
    // 틀린 말을 하게 된다. 실제로 그 형태의 결함이 있었다: 게이트에 Agent 조건이
    // 추가됐는데 이 문구는 "tasks still running"으로 고정돼 있어, Task가 0건인
    // Project에서 없는 Task를 기다리라고 안내했다.
    //
    // Task와 Agent는 해소 방법이 다르므로 문장도 갈라야 한다. `Ready` Agent는
    // 저절로 끝나지 않는다 — 사람이 Stop을 눌러야 한다.
    function drainingMessage(blockedBy) {
      const blockers = Array.isArray(blockedBy) ? blockedBy : [];
      const tasks = blockers.includes('tasks');
      const agents = blockers.includes('agents');
      if (tasks && agents) {
        return 'Draining — unfinished tasks and live agents are blocking archive; wait for the tasks and stop the agents below.';
      }
      if (tasks) {
        return 'Draining — tasks are still running; archive completes once they finish.';
      }
      if (agents) {
        return 'Draining — live agents are still assigned; archive completes once you stop them below.';
      }
      // 서버가 사유를 주지 않은 경우(구버전 응답 등). 틀린 사유를 지어내는 것보다
      // 사유를 말하지 않는 편이 낫다.
      return 'Draining — archive is still blocked.';
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
        // draining에 머무를 수 있다. 사유는 서버가 `archive_blocked_by`로
        // 말해 준다 — 여기서 짐작하지 않는다.
        if (body && body.status === 'draining') {
          status.textContent = drainingMessage(body.archive_blocked_by);
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

    // Agent 섹션 (로드맵 #49, 1단계). 여기 있는 이유는 Agent가 생성 시점에
    // 정해진 하나의 Project에 영구히 속하기 때문이다 — fleet 전역 목록보다
    // 이 화면이 정확한 자리이고, "왜 archive가 draining에 멈춰 있는가"를
    // 같은 화면에서 확인할 수 있다.
    let agentManageAllowed = false;

    async function fetchAgents() {
      const table = document.getElementById('agent-table');
      const empty = document.getElementById('agents-empty');
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());
      let agents = [];
      try {
        const resp = await fetch('api/agents?project_id=' + encodeURIComponent(projectId));
        if (!resp.ok) {
          // agent:read가 없으면 섹션을 숨긴다 — 빈 목록으로 오해하게 두지
          // 않는다(Issue 섹션과 같은 처리).
          if (resp.status === 403) {
            table.style.display = 'none';
            empty.querySelector('h3').textContent = 'Not permitted';
            empty.querySelector('p').textContent = 'You do not have permission to view agents.';
            empty.style.display = 'block';
          }
          return;
        }
        agents = await resp.json();
      } catch (e) { console.error('fetch agents', e); return; }

      if (!agents.length) {
        table.style.display = 'none';
        empty.style.display = 'block';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';
      for (const a of agents) {
        const row = document.createElement('div');
        row.className = 'row';
        const cls = a.status === 'ready' ? 'badge-online' : 'badge-cancelled';
        // 회수는 되돌릴 수 없다 — 다시 Ready로 만드는 경로가 없으므로
        // 이미 stopped면 버튼 자체를 내보내지 않는다.
        const stopBtn = (agentManageAllowed && a.status === 'ready')
          ? '<button type="button" class="btn" data-stop="' + escapeHtml(a.id) + '">Stop</button>'
          : '';
        row.innerHTML = `
          <div style="font-weight:600;">${escapeHtml(a.name)}</div>
          <div><span class="badge ${cls}">${escapeHtml(a.status)}</span></div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(a.created_by || '—')}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(a.created_at)}</div>
          <div>${stopBtn}</div>
        `;
        table.appendChild(row);
      }
      table.querySelectorAll('button[data-stop]').forEach(b => {
        b.addEventListener('click', () => stopAgent(b.getAttribute('data-stop'), b));
      });
    }

    async function stopAgent(agentId, btn) {
      if (!window.confirm('Stop this agent? It cannot be restarted.')) return;
      btn.disabled = true;
      try {
        const resp = await fetch('api/agents/' + encodeURIComponent(agentId), {
          method: 'DELETE',
          headers: { 'X-CSRF-Token': getCsrf() },
        });
        if (!resp.ok) {
          const t = await resp.text();
          window.alert('Failed to stop agent: ' + t);
          btn.disabled = false;
          return;
        }
      } catch (e) {
        window.alert('Failed to stop agent: ' + e.message);
        btn.disabled = false;
        return;
      }
      await fetchAgents();
    }

    document.getElementById('agent-create').addEventListener('submit', async (ev) => {
      ev.preventDefault();
      const status = document.getElementById('agent-create-status');
      const nameEl = document.getElementById('agent-name');
      const descEl = document.getElementById('agent-description');
      status.textContent = 'Creating…';
      status.style.color = 'var(--ink-muted-48)';
      try {
        const resp = await fetch('api/agents', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrf() },
          body: JSON.stringify({
            project_id: projectId,
            name: nameEl.value,
            description: descEl.value || null,
          }),
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
        status.textContent = '';
        nameEl.value = '';
        descEl.value = '';
        await fetchAgents();
      } catch (e) {
        status.textContent = 'Error: ' + e.message;
        status.style.color = 'var(--badge-failed, #c0392b)';
      }
    });

    async function loadAgentPermissions() {
      try {
        const resp = await fetch('api/me');
        if (!resp.ok) return;
        const me = await resp.json();
        const perms = (me && me.permissions) || [];
        agentManageAllowed = perms.indexOf('agent:manage') !== -1;
      } catch (e) { /* 권한을 모르면 생성 폼을 숨긴 채로 둔다 */ }
      document.getElementById('agent-create').style.display =
        agentManageAllowed ? 'flex' : 'none';
    }

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
      await fetchAgents();
      await fetchIssues();
      await fetchTasks();
    }
    loadAgentPermissions().then(refresh);
    setInterval(refresh, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
