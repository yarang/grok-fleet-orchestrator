    let allTasks = [];
    let currentFilter = 'all';
    // 로드맵 #23 — 페이지 크기. "Load more" 클릭 시 100씩 늘려 재조회한다.
    // 백엔드(`/api/tasks`)는 limit/offset을 완전히 지원하지만(#11) 프론트가
    // 그동안 limit=200 고정 단발 조회만 했다 — 태스크가 그 이상 쌓이는
    // 배포에서는 오래된 태스크가 그냥 안 보이는 문제였다.
    const PAGE_SIZE = 100;
    let pageSize = PAGE_SIZE;
    // 총 개수를 알려주는 엔드포인트가 없으므로, limit보다 1개 더 요청해서
    // "더 있음"을 판단하고 실제로는 pageSize개만 렌더링한다.
    let hasMore = false;

    async function fetchTasks() {
      try {
        const resp = await fetch('/api/tasks?limit=' + (pageSize + 1) + '&offset=0');
        const data = await resp.json();
        hasMore = data.length > pageSize;
        allTasks = hasMore ? data.slice(0, pageSize) : data;
        render();
      } catch (e) { console.error('fetch tasks:', e); }
    }

    document.getElementById('load-more-btn').addEventListener('click', () => {
      pageSize += PAGE_SIZE;
      fetchTasks();
    });

    function fmtTokens(t) {
      if (!t) return '—';
      const total = t.total_tokens || 0;
      if (total >= 1e6) return (total/1e6).toFixed(1)+'M';
      if (total >= 1e3) return (total/1e3).toFixed(1)+'k';
      return total;
    }

    function fmtDuration(secs) {
      if (!secs) return '—';
      if (secs < 60) return secs.toFixed(1)+'s';
      if (secs < 3600) return (secs/60).toFixed(1)+'m';
      return (secs/3600).toFixed(1)+'h';
    }

    function fmtTime(iso) {
      const d = new Date(iso);
      const now = new Date();
      const diff = (now - d) / 1000;
      if (diff < 60) return Math.floor(diff)+'s ago';
      if (diff < 3600) return Math.floor(diff/60)+'m ago';
      if (diff < 86400) return Math.floor(diff/3600)+'h ago';
      return d.toLocaleDateString();
    }

    function updatePaginationUI() {
      const summary = document.getElementById('pagination-summary');
      const loadMoreBtn = document.getElementById('load-more-btn');
      summary.textContent = hasMore
        ? `Showing ${allTasks.length} tasks`
        : (allTasks.length > 0 ? `All ${allTasks.length} tasks loaded` : '');
      loadMoreBtn.style.display = hasMore ? 'inline-flex' : 'none';
    }

    function render() {
      updatePaginationUI();

      const filtered = currentFilter === 'all'
        ? allTasks
        : allTasks.filter(t => t.phase === currentFilter);

      const table = document.getElementById('task-table');
      const empty = document.getElementById('empty-state');

      // 헤더 제외 기존 행 제거
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (filtered.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const t of filtered) {
        const row = document.createElement('div');
        row.className = 'row';
        row.style.cursor = 'pointer';
        row.onclick = () => window.location.href = '/tasks/' + encodeURIComponent(t.id);
        const prompt = t.prompt.length > 60 ? t.prompt.substring(0,60)+'…' : t.prompt;
        row.innerHTML = `
          <div style="font-family:var(--font-mono);font-size:12px;color:var(--primary);">${t.id.substring(0,8)}</div>
          <div><span class="badge badge-${t.phase}">${t.phase}</span></div>
          <div style="font-size:14px;">${escapeHtml(prompt)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${t.model||'—'}</div>
          <div style="font-family:var(--font-mono);font-size:12px;">${t.worker_id?t.worker_id.substring(0,8):'—'}</div>
          <div style="font-size:13px;">${fmtTokens(t.token_usage)}</div>
          <div style="font-size:13px;">${fmtDuration(t.duration_secs)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(t.created_at)}</div>
        `;
        table.appendChild(row);
      }
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    // 필터 버튼 이벤트
    document.querySelectorAll('.pill-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.pill-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        currentFilter = btn.dataset.filter;
        render();
      });
    });

    // 권한 없는 사용자(viewer)에게는 "+ New Task" 버튼을 숨긴다 — 눌러도 서버가
    // 403을 반환하겠지만, 애초에 할 수 없는 액션을 보여주지 않는 편이 낫다.
    async function hideNewTaskIfNoPermission() {
      try {
        const resp = await fetch('/api/me');
        const me = await resp.json();
        if (!(me.permissions || []).includes('task:create')) {
          const btn = document.getElementById('new-task-link');
          if (btn) btn.style.display = 'none';
        }
      } catch (e) { console.error('hideNewTaskIfNoPermission', e); }
    }
    hideNewTaskIfNoPermission();

    // 주기적 새로고침
    fetchTasks();
    setInterval(fetchTasks, 5000);

    // SSE 연결 상태
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
      es.addEventListener('task_created', () => fetchTasks());
      es.addEventListener('task_dispatched', () => fetchTasks());
      es.addEventListener('task_completed', () => fetchTasks());
      es.addEventListener('task_failed', () => fetchTasks());
    } catch(e) {}

  
