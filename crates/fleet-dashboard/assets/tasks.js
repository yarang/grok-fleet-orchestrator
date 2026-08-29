    let allThreads = []; // [{thread_id, root: TaskSummary|null, members: TaskSummary[]}]
    let allTasks = [];   // allThreads의 members를 펼친 것 — 필터 옵션 채우기 등 기존 로직이 씀.
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
    // #96: 그룹 목록에서 이 정렬은 "스레드끼리의 순서"에 쓰인다(스레드 내부
    // 순서는 대화 읽는 순서로 고정 — 아래 그룹핑 절 참고).
    let sortKey = 'created_at';
    let sortDir = 'desc';
    // 로드맵 #23 — 페이지 크기. "Load more" 클릭 시 100씩 늘려 재조회한다.
    // #96: 페이지의 단위는 Task가 아니라 스레드다 — 스레드 하나가 페이지
    // 경계에서 잘려 그룹의 절반만 보이는 화면을 피하려는 설계 문서의 결정.
    const PAGE_SIZE = 100;
    let pageSize = PAGE_SIZE;
    // 총 개수를 알려주는 엔드포인트가 없으므로, limit보다 1개 더 요청해서
    // "더 있음"을 판단하고 실제로는 pageSize개만 렌더링한다.
    let hasMore = false;

    // #96 — 헤더 클릭으로 접은 스레드의 id 집합. re-render를 넘나들며
    // 유지해야 폴링(5초)마다 사용자가 접어둔 그룹이 다시 펼쳐지지 않는다.
    const collapsedThreads = new Set();

    // #96 — terminal Task만 삭제 대상이다. fleet-core::TaskPhase의 직렬화
    // 값과 일치해야 한다(crates/fleet-core/src/task.rs).
    const TERMINAL_PHASES = new Set(['completed', 'failed', 'cancelled']);

    async function fetchTasks() {
      try {
        const [resp, names] = await Promise.all([
          fetch('api/task-threads?limit=' + (pageSize + 1) + '&offset=0'),
          getWorkerNameMap(),
        ]);
        const data = await resp.json();
        workerNames = names;
        const threads = data.threads || [];
        hasMore = threads.length > pageSize;
        allThreads = hasMore ? threads.slice(0, pageSize) : threads;
        allTasks = allThreads.flatMap(th => th.members);
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
      // #96 — 페이지 단위는 스레드지만, 실제로 몇 건의 Task가 로드됐는지도
      // 함께 보여준다("스레드 47개(112 태스크)"처럼 스레드 하나가 여러 Task를
      // 품을 수 있어 두 숫자가 다르기 때문).
      const taskCount = allThreads.reduce((n, th) => n + th.members.length, 0);
      summary.textContent = hasMore
        ? `Showing ${allThreads.length} threads (${taskCount} tasks)`
        : (allThreads.length > 0 ? `All ${allThreads.length} threads loaded (${taskCount} tasks)` : '');
      loadMoreBtn.style.display = hasMore ? 'inline-flex' : 'none';
    }

    function matchesFilters(t) {
      if (currentFilter !== 'all' && t.phase !== currentFilter) return false;
      if (currentSearch && !(t.prompt || '').toLowerCase().includes(currentSearch.toLowerCase())) return false;
      if (currentWorker && t.worker_id !== currentWorker) return false;
      if (currentModel && t.model !== currentModel) return false;
      return true;
    }

    // #96 — 스레드 하나의 렌더링에 필요한 파생 값을 계산한다. 그룹핑 모델은
    // docs/ui-dashboard/ui-design.md §3.3을 그대로 따른다:
    //   - 그룹의 정체성은 thread_id 값 자체(루트 행이 아니다) — 루트가
    //     삭제돼도 구성원은 "헤더 없는 그룹"으로 살아남는다.
    //   - 스레드 내부는 항상 created_at 오름차순(대화를 읽는 순서)이고,
    //     이건 컬럼 정렬 UI의 영향을 받지 않는다 — 정렬 UI는 스레드끼리의
    //     순서만 바꾼다.
    //   - "가장 최근 활동"은 필터와 무관하게 스레드 전체 구성원 기준으로
    //     고정한다 — 필터로 어느 구성원이 보이든 스레드가 목록에서 갑자기
    //     순서를 바꾸면 사용자가 방금 본 위치를 잃는다.
    function deriveThread(th) {
      const sortedMembers = [...th.members].sort((a, b) => compareTasks(a, b, 'created_at', 'asc'));
      const latestActivity = sortedMembers.length
        ? Math.max(...sortedMembers.map(m => new Date(m.created_at).getTime()))
        : 0;
      // 정렬 키가 created_at이 아닐 때 쓸 대표 Task. 루트가 있으면 루트,
      // 없으면(루트 삭제됨) 가장 최근 구성원 — 대표가 없으면 어떤 값으로
      // 스레드를 정렬해야 할지 정의할 수 없기 때문.
      const representative = th.root || sortedMembers[sortedMembers.length - 1];
      const filteredMembers = sortedMembers.filter(matchesFilters);
      return {
        thread_id: th.thread_id,
        root: th.root,
        members: sortedMembers,
        filteredMembers,
        latestActivity,
        representative,
      };
    }

    function renderDeleteCell(t) {
      if (!canDelete || !TERMINAL_PHASES.has(t.phase)) return '<div></div>';
      return `<div><button type="button" class="btn btn-sm btn-danger task-delete-btn" data-task-id="${escapeHtml(t.id)}" title="Delete task">🗑</button></div>`;
    }

    function taskRowHtml(t, indent) {
      const prompt = t.prompt.length > 60 ? t.prompt.substring(0,60)+'…' : t.prompt;
      const idCellClass = indent ? 'task-id-cell indent' : 'task-id-cell';
      return `
          <div class="${idCellClass}" style="font-family:var(--font-mono);font-size:12px;color:var(--primary);">${escapeHtml(t.id.substring(0,8))}</div>
          <div><span class="badge badge-${escapeHtml(t.phase)}">${escapeHtml(t.phase)}</span></div>
          <div style="font-size:14px;">${escapeHtml(prompt)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(t.model||'—')}</div>
          <div style="font-family:var(--font-mono);font-size:12px;" title="${escapeHtml(t.worker_id||'')}">${escapeHtml(workerLabel(t.worker_id, workerNames))}</div>
          <div style="font-size:13px;">${fmtTokens(t.token_usage)}</div>
          <div style="font-size:13px;">${fmtDuration(t.duration_secs)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(t.created_at)}</div>
          ${renderDeleteCell(t)}
      `;
    }

    function makeTaskRow(t, indent) {
      const row = document.createElement('div');
      row.className = 'row';
      row.style.cursor = 'pointer';
      row.onclick = () => window.location.href = 'tasks/' + encodeURIComponent(t.id);
      row.innerHTML = taskRowHtml(t, indent);
      const delBtn = row.querySelector('.task-delete-btn');
      if (delBtn) {
        delBtn.addEventListener('click', (ev) => {
          ev.stopPropagation();
          deleteTask(t.id);
        });
      }
      return row;
    }

    function makeThreadHeaderRow(thread) {
      const total = thread.members.length;
      const shown = thread.filteredMembers.length;
      const countLabel = shown < total ? `${shown} / ${total} 표시 중` : `${total} tasks`;
      const collapsed = collapsedThreads.has(thread.thread_id);
      const header = document.createElement('div');
      header.className = 'row thread-header' + (collapsed ? ' collapsed' : '') + (thread.root ? '' : ' deleted-root');
      const label = thread.root
        ? escapeHtml(thread.root.id.substring(0, 8))
        : `(최초 태스크 삭제됨) · ${escapeHtml(thread.thread_id.substring(0, 4))}`;
      // 헤더가 보여줄 대표 상태 — 가장 최근 구성원의 phase.
      const latest = thread.members[thread.members.length - 1];
      header.innerHTML = `
        <span class="thread-caret">▾</span>
        <span>${label}</span>
        <span style="color:var(--ink-muted-48);">· ${countLabel} · ${escapeHtml(latest.phase)}</span>
      `;
      header.addEventListener('click', () => {
        if (collapsedThreads.has(thread.thread_id)) {
          collapsedThreads.delete(thread.thread_id);
        } else {
          collapsedThreads.add(thread.thread_id);
        }
        render();
      });
      return header;
    }

    function render() {
      updatePaginationUI();

      // #96 — 스레드 단위로 파생값을 먼저 계산하고, 필터에 하나도 걸리지
      // 않는 스레드는 통째로 뺀다(빈 그룹을 그리지 않는다).
      let derived = allThreads.map(deriveThread).filter(th => th.filteredMembers.length > 0);

      // 정렬: created_at 키는 스레드의 "가장 최근 활동"(필터 무관, 전체
      // 구성원 기준) 기준. 그 외 키는 대표 Task(루트, 없으면 최신 구성원)로
      // 기존 compareTasks 규칙을 그대로 적용한다.
      derived.sort((a, b) => {
        if (sortKey === 'created_at') {
          const cmp = a.latestActivity < b.latestActivity ? -1 : a.latestActivity > b.latestActivity ? 1 : 0;
          return sortDir === 'asc' ? cmp : -cmp;
        }
        return compareTasks(a.representative, b.representative, sortKey, sortDir);
      });

      const table = document.getElementById('task-table');
      const empty = document.getElementById('empty-state');

      // 헤더 제외 기존 행 제거
      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (derived.length === 0) {
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

      for (const thread of derived) {
        // 구성원 1건(대부분의 Task) — 들여쓰기·펼침 없이 평범한 한 행.
        // 그룹 전체가 1건일 때만 해당하고("total===1"), 필터로 나머지가
        // 걸러져 1건만 남은 경우는 아래 다중 구성원 분기로 간다(§3.3:
        // 필터가 일부에만 걸려도 스레드는 남고 헤더에 n/m을 적는다).
        if (thread.members.length === 1) {
          table.appendChild(makeTaskRow(thread.filteredMembers[0], false));
          continue;
        }
        table.appendChild(makeThreadHeaderRow(thread));
        if (!collapsedThreads.has(thread.thread_id)) {
          for (const t of thread.filteredMembers) {
            table.appendChild(makeTaskRow(t, true));
          }
        }
      }
    }


    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    // #96 — 삭제 UI 계약(docs/architecture/tasks/management.md 삭제 계약 절,
    // docs/ui-dashboard/ui-design.md §3.3): 무엇이 함께 사라지는지 확인
    // 다이얼로그에서 명시하고, 거부 이유는 구분해서 보여준다.
    async function deleteTask(id) {
      const confirmed = window.confirm(
        'Delete this task?\n\n' +
        '- Its output and telemetry will be permanently deleted.\n' +
        '- Matching event log entries remain, but become anonymous (no task reference).\n' +
        '- Child tasks survive, but lose the link to this parent.\n' +
        '- Its idempotency key is released and becomes reusable.\n\n' +
        'This cannot be undone.'
      );
      if (!confirmed) return;

      try {
        const resp = await fetch('api/tasks/' + encodeURIComponent(id), {
          method: 'DELETE',
          headers: { 'X-CSRF-Token': getCsrf() },
        });
        if (resp.status === 204) {
          fetchTasks();
          return;
        }
        const rawText = await resp.text();
        let body = null;
        try { body = JSON.parse(rawText); } catch (_) {}
        const msg = (body && body.error && body.error.message) || rawText || ('HTTP ' + resp.status);
        // 서버는 별도 에러 코드 없이 conflict 하나로 묶어 보내므로, 메시지
        // 문구로 세 가지 거부 사유를 구분한다(handlers.rs::delete_task_api).
        let friendly;
        if (resp.status === 403) {
          friendly = 'You do not have permission to delete this task.';
        } else if (msg.includes('not terminal')) {
          friendly = 'This task is still running — cancel it first.';
        } else if (msg.includes('blocked by pending dependents')) {
          friendly = msg.charAt(0).toUpperCase() + msg.slice(1) + '.';
        } else {
          friendly = msg;
        }
        alert('Delete failed: ' + friendly);
      } catch (e) {
        alert('Delete failed: ' + e.message);
      }
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

    // 권한 없는 사용자(viewer)에게는 "+ New Task" 버튼을 숨기고, 삭제 버튼
    // 열도 감춘다 — 눌러도 서버가 403을 반환하겠지만, 애초에 할 수 없는
    // 액션을 보여주지 않는 편이 낫다(기존 hideNewTaskIfNoPermission 관례를
    // task:delete로 확장).
    let canDelete = false;
    async function loadPermissions() {
      try {
        const resp = await fetch('api/me');
        const me = await resp.json();
        const perms = me.permissions || [];
        if (!perms.includes('task:create')) {
          const btn = document.getElementById('new-task-link');
          if (btn) btn.style.display = 'none';
        }
        canDelete = perms.includes('task:delete');
        render();
      } catch (e) { console.error('loadPermissions', e); }
    }
    loadPermissions();

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
