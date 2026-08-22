    let allEvents = [];
    let currentFilter = 'all';
    let workerNames = {};

    async function fetchActivity() {
      try {
        // 작업·워커 생명주기 이벤트. 인증/권한 감사 로그는 별개이며 /api/audit이 담당한다.
        const [resp, names] = await Promise.all([
          fetch('api/events?limit=200'),
          getWorkerNameMap(),
        ]);
        const data = await resp.json();
        // /api/events는 { events, count } 형태로 감싸서 반환한다.
        allEvents = data.events ?? [];
        workerNames = names;
        render();
      } catch(e) { console.error('fetch activity:', e); }
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      const d = new Date(iso);
      const now = new Date();
      const diff = (now - d) / 1000;
      if (diff < 60) return Math.floor(diff)+'s ago';
      if (diff < 3600) return Math.floor(diff/60)+'m ago';
      return d.toLocaleString();
    }

    function getEventType(ev) {
      return ev.event?.type || 'unknown';
    }

    function getTaskId(ev) {
      return ev.event?.task_id ? String(ev.event.task_id).substring(0,8) : '—';
    }

    function getWorkerId(ev) {
      return workerLabel(ev.event?.worker_id, workerNames);
    }

    function getDetails(ev) {
      const e = ev.event;
      if (!e) return '—';
      const parts = [];
      if (e.created_by) parts.push('by: '+e.created_by);
      if (e.exit_code !== undefined) parts.push('exit: '+e.exit_code);
      if (e.error) parts.push('error: '+e.error.substring(0,60));
      if (e.attempts) parts.push('attempts: '+e.attempts);
      return parts.join(', ') || '—';
    }

    function matchesFilter(ev) {
      if (currentFilter === 'all') return true;
      const type = getEventType(ev);
      if (currentFilter === 'task_created') return type.startsWith('task');
      if (currentFilter === 'worker') return type.startsWith('worker');
      if (currentFilter === 'login') return type.includes('login') || type.includes('session');
      return true;
    }

    function render() {
      const filtered = allEvents.filter(matchesFilter);
      const table = document.getElementById('activity-table');
      const empty = document.getElementById('empty-state');

      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (filtered.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const ev of filtered) {
        const row = document.createElement('div');
        row.className = 'row';
        const type = getEventType(ev);
        row.innerHTML = `
          <div style="font-family:var(--font-mono);font-size:12px;color:var(--ink-muted-48);">${ev.seq}</div>
          <div style="font-family:var(--font-mono);font-size:12px;font-weight:600;">${escapeHtml(type)}</div>
          <div style="font-family:var(--font-mono);font-size:12px;color:var(--primary);">${getTaskId(ev)}</div>
          <div style="font-family:var(--font-mono);font-size:12px;">${getWorkerId(ev)}</div>
          <div style="font-size:13px;color:var(--ink-muted-80);">${escapeHtml(getDetails(ev))}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(ev.event?.at)}</div>
        `;
        table.appendChild(row);
      }
    }

    function escapeHtml(s) {
      if (!s) return '';
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    document.querySelectorAll('.pill-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.pill-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        currentFilter = btn.dataset.filter;
        render();
      });
    });

    fetchActivity();
    setInterval(fetchActivity, 10000);

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
      es.onmessage = () => fetchActivity();
    } catch(e) {}

  
