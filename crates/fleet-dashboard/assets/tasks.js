    let allTasks = [];
    // worker_id(UUID) → name. app.js의 getWorkerNameMap()이 /api/workers를
    // 캐싱하며 채워준다 — 필터 드롭다운과 테이블 Worker 열 둘 다 여기서 읽는다.
    let workerNames = {};
    let currentFilter = 'all';
    // 로드맵 #14 — 고급 필터 상태(상태 필터 pill과 별개로 함께 적용된다).
    let currentSearch = '';
    let currentWorker = '';
    let currentModel = '';
    // 로드맵 #14 — 정렬 상태. 기본값은 페이지 부제("newest first")와
    // 일치하도록 created_at 내림차순.
    let sortKey = 'created_at';
    let sortDir = 'desc';
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
        const [resp, names] = await Promise.all([
          fetch('api/tasks?limit=' + (pageSize + 1) + '&offset=0'),
          getWorkerNameMap(),
        ]);
        const data = await resp.json();
        workerNames = names;
        hasMore = data.length > pageSize;
        allTasks = hasMore ? data.slice(0, pageSize) : data;
        populateFilterOptions();
        render();
      } catch (e) { console.error('fetch tasks:', e); }
    }

    document.getElementById('load-more-btn').addEventListener('click', () => {
      pageSize += PAGE_SIZE;
      fetchTasks();
    });

    // ── 로드맵 #14: 고급 필터 (검색 / worker / model) ────────────────────

    /// worker/model 드롭다운 옵션을 현재 로드된 태스크 목록에서 동적으로
    /// 채운다 — 별도 백엔드 엔드포인트 없이 클라이언트에서 유일값만 추출.
    /// 재조회(폴링/Load more)할 때마다 다시 호출되므로, 사용자가 이미 골라둔
    /// 값은 그 옵션이 여전히 존재하는 한 유지한다.
    function populateFilterOptions() {
      const workerSel = document.getElementById('filter-worker');
      const modelSel = document.getElementById('filter-model');
      const workers = [...new Set(allTasks.map(t => t.worker_id).filter(Boolean))].sort();
      const models = [...new Set(allTasks.map(t => t.model).filter(Boolean))].sort();

      // displayFn이 있으면 그걸로 보여줄 텍스트를 결정하고(예: worker_id → name),
      // 없으면 기존처럼 값 자체를 잘라서 보여준다(model처럼 이미 사람이 읽을 수
      // 있는 짧은 문자열). 어느 쪽이든 <option value>는 항상 원본 값(필터링 키)
      // 그대로 유지한다.
      const buildOptions = (label, values, displayFn) =>
        `<option value="">${label}</option>` +
        values.map(v => {
          const display = displayFn ? displayFn(v) : (v.length > 12 ? v.slice(0, 8) + '…' : v);
          return `<option value="${escapeHtml(v)}">${escapeHtml(display)}</option>`;
        }).join('');

      const prevWorker = workerSel.value;
      const prevModel = modelSel.value;
      workerSel.innerHTML = buildOptions('All workers', workers, w => workerLabel(w, workerNames));
      modelSel.innerHTML = buildOptions('All models', models);
      if (workers.includes(prevWorker)) workerSel.value = prevWorker;
      if (models.includes(prevModel)) modelSel.value = prevModel;
    }

    document.getElementById('filter-search').addEventListener('input', (e) => {
      currentSearch = e.target.value.trim();
      render();
    });
    document.getElementById('filter-worker').addEventListener('change', (e) => {
      currentWorker = e.target.value;
      render();
    });
    document.getElementById('filter-model').addEventListener('change', (e) => {
      currentModel = e.target.value;
      render();
    });

    // ── 로드맵 #14: 컬럼 정렬 ────────────────────────────────────────────

    function compareTasks(a, b, key, dir) {
      let va, vb;
      switch (key) {
        case 'tokens':
          va = (a.token_usage && a.token_usage.total_tokens) || 0;
          vb = (b.token_usage && b.token_usage.total_tokens) || 0;
          break;
        case 'duration_secs':
          va = a.duration_secs ?? -1;
          vb = b.duration_secs ?? -1;
          break;
        case 'created_at':
          va = new Date(a.created_at).getTime();
          vb = new Date(b.created_at).getTime();
          break;
        default:
          va = String(a[key] ?? '').toLowerCase();
          vb = String(b[key] ?? '').toLowerCase();
      }
      const cmp = va < vb ? -1 : va > vb ? 1 : 0;
      return dir === 'asc' ? cmp : -cmp;
    }

    function updateSortIndicators() {
      document.querySelectorAll('#task-table-header .sortable').forEach(cell => {
        const active = cell.dataset.sortKey === sortKey;
        cell.classList.toggle('sort-active', active);
        cell.dataset.sortDir = active ? sortDir : '';
      });
    }

    document.querySelectorAll('#task-table-header .sortable').forEach(cell => {
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

      let filtered = currentFilter === 'all'
        ? allTasks
        : allTasks.filter(t => t.phase === currentFilter);

      // 로드맵 #14 — 고급 필터. 상태 pill 필터와 AND로 함께 적용된다.
      if (currentSearch) {
        const q = currentSearch.toLowerCase();
        filtered = filtered.filter(t => (t.prompt || '').toLowerCase().includes(q));
      }
      if (currentWorker) {
        filtered = filtered.filter(t => t.worker_id === currentWorker);
      }
      if (currentModel) {
        filtered = filtered.filter(t => t.model === currentModel);
      }

      // 로드맵 #14 — 컬럼 정렬. allTasks 자체가 아니라 필터링된 결과를
      // 정렬해야 필터+정렬이 함께 걸린 상태로 일관되게 보인다.
      filtered = [...filtered].sort((a, b) => compareTasks(a, b, sortKey, sortDir));

      const table = document.getElementById('task-table');
      const empty = document.getElementById('empty-state');

      // 헤더 제외 기존 행 제거
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (filtered.length === 0) {
        // 로드맵 #14 — 필터/검색 때문에 결과가 0인 것과, 애초에 태스크가
        // 하나도 없는 것을 구분해서 안내한다.
        const filtersActive = currentFilter !== 'all' || currentSearch || currentWorker || currentModel;
        empty.querySelector('h3').textContent = filtersActive
          ? 'No matching tasks'
          : 'No tasks found';
        empty.querySelector('p').textContent = filtersActive
          ? 'Try adjusting or clearing the filters above.'
          : 'Tasks will appear here when submitted via MCP or API.';
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
        row.onclick = () => window.location.href = 'tasks/' + encodeURIComponent(t.id);
        const prompt = t.prompt.length > 60 ? t.prompt.substring(0,60)+'…' : t.prompt;
        row.innerHTML = `
          <div style="font-family:var(--font-mono);font-size:12px;color:var(--primary);">${t.id.substring(0,8)}</div>
          <div><span class="badge badge-${t.phase}">${t.phase}</span></div>
          <div style="font-size:14px;">${escapeHtml(prompt)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${t.model||'—'}</div>
          <div style="font-family:var(--font-mono);font-size:12px;" title="${escapeHtml(t.worker_id||'')}">${escapeHtml(workerLabel(t.worker_id, workerNames))}</div>
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
        const resp = await fetch('api/me');
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
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
      es.addEventListener('task_created', () => fetchTasks());
      es.addEventListener('task_dispatched', () => fetchTasks());
      es.addEventListener('task_completed', () => fetchTasks());
      es.addEventListener('task_failed', () => fetchTasks());
    } catch(e) {}

  
