// Fleet Orchestrator Dashboard — 쿠키 기반 세션 (Phase 9.1).
// 인증은 HttpOnly 쿠키로 자동 처리. fetch는 credentials: 'same-origin' (기본값).
// 401 시 /login 으로 리다이렉트. 별도 prompt/localStorage 없음.

const API = {
  overview: '/api/overview',
  workers: '/api/workers?limit=50',
  tasks: '/api/tasks?limit=20',
  events: '/api/events?limit=50',
  eventsStream: '/api/events/stream',
  me: '/api/me',
};

// ── 인증 헬퍼 ────────────────────────────────────────────────────────

let currentUser = null;

async function loadCurrentUser() {
  try {
    const r = await fetch(API.me);
    if (r.status === 401) {
      window.location.href = '/login';
      return null;
    }
    if (!r.ok) return null;
    currentUser = await r.json();
    renderUserMenu();
    return currentUser;
  } catch (e) {
    console.error('me', e);
    return null;
  }
}

function renderUserMenu() {
  if (!currentUser) return;
  const header = document.querySelector('header');
  if (!header || document.getElementById('user-menu')) return;

  const menu = document.createElement('div');
  menu.id = 'user-menu';
  menu.style.cssText = 'display:flex; align-items:center; gap:12px;';
  menu.innerHTML = `
    <span style="font-size: 13px; color: var(--ink-muted, #615d59);">
      ${escapeHtml(currentUser.username || '')}
    </span>
    <button id="logout-btn"
            style="background:transparent; border:1px solid var(--hairline,#e6e6e6);
                   border-radius:8px; padding:6px 12px; cursor:pointer;
                   font-size:12px; color: var(--ink-secondary, #31302e);">
      Sign out
    </button>
  `;
  header.appendChild(menu);

  document.getElementById('logout-btn').addEventListener('click', async () => {
    await fetch('/logout', { method: 'POST' });
    window.location.href = '/login';
  });
}

// ── 데이터 fetch ──────────────────────────────────────────────────────

function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleTimeString();
}

function setStatusPill(online) {
  const pill = document.getElementById('status-pill');
  if (!pill) return;
  pill.textContent = online ? 'live' : 'disconnected';
  pill.classList.toggle('online', online);
}

async function fetchJSON(url) {
  const r = await fetch(url);
  if (r.status === 401) {
    // 세션 만료 — 로그인으로.
    window.location.href = '/login';
    throw new Error('session expired');
  }
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json();
}

async function refreshOverview() {
  try {
    const data = await fetchJSON(API.overview);
    setMetric('metric-workers', `${data.workers.online}/${data.workers.total}`);
    setMetric('metric-tasks-active', data.tasks.pending + data.tasks.dispatched);
    setMetric('metric-tasks-today', data.tasks.total);
    setMetric('metric-failures', data.tasks.failed);
    setStatusPill(true);
  } catch (e) {
    console.error('overview', e);
    setStatusPill(false);
  }
}

function setMetric(id, value) {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}

async function refreshWorkers() {
  try {
    const workers = await fetchJSON(API.workers);
    const list = document.getElementById('worker-list');
    if (!list) return;
    const header = list.querySelector('.row.header');
    list.innerHTML = '';
    if (header) list.appendChild(header);
    for (const w of workers) {
      const row = document.createElement('div');
      row.className = 'row';
      row.innerHTML = `
        <div>${escapeHtml(w.name)}</div>
        <div><span class="status-pill ${w.status}">${w.status}</span></div>
        <div>${w.active_tasks}/${w.max_concurrent}</div>
        <div>${w.circuit_state}</div>
        <div>${fmtTime(w.last_seen)}</div>
      `;
      list.appendChild(row);
    }
  } catch (e) {
    console.error('workers', e);
  }
}

async function refreshTasks() {
  try {
    const tasks = await fetchJSON(API.tasks);
    const list = document.getElementById('task-list');
    if (!list) return;
    const header = list.querySelector('.row.header');
    list.innerHTML = '';
    if (header) list.appendChild(header);
    for (const t of tasks) {
      const row = document.createElement('div');
      row.className = 'row';
      const idShort = (t.id || '').slice(0, 8);
      row.innerHTML = `
        <div title="${escapeHtml(t.id)}">${idShort}</div>
        <div><span class="phase ${t.phase}">${t.phase}</span></div>
        <div>${escapeHtml((t.prompt || '').slice(0, 80))}</div>
        <div>${t.worker_id ? String(t.worker_id).slice(0, 8) : '—'}</div>
        <div>${fmtTime(t.created_at)}</div>
      `;
      list.appendChild(row);
    }
  } catch (e) {
    console.error('tasks', e);
  }
}

function escapeHtml(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  }[c]));
}

// ── SSE 이벤트 스트림 ─────────────────────────────────────────────────

let eventCount = 0;

function startEventStream() {
  // EventSource는 쿠키를 자동 전송 (same-origin).
  const source = new EventSource(API.eventsStream);
  const log = document.getElementById('event-log');
  const counter = document.getElementById('event-counter');

  source.addEventListener('fleet_event', (e) => {
    try {
      const entry = JSON.parse(e.data);
      const ev = entry.event || entry;
      const type = ev.type || 'unknown';
      const time = fmtTime(entry.event?.at || new Date().toISOString());
      const line = document.createElement('div');
      line.className = 'event-line';
      line.innerHTML = `
        <span class="event-time">${time}</span>
        <span class="event-type">${escapeHtml(type)}</span>
        <span>${escapeHtml(JSON.stringify(ev).slice(0, 200))}</span>
      `;
      if (log) log.insertBefore(line, log.firstChild);
      while (log && log.children.length > 100) {
        log.removeChild(log.lastChild);
      }
      eventCount++;
      if (counter) counter.textContent = `(${eventCount})`;
    } catch (err) {
      console.error('event parse', err);
    }
  });

  source.onerror = () => setStatusPill(false);
  source.onopen = () => setStatusPill(true);
}

// ── 초기화 ───────────────────────────────────────────────────────────

async function refreshAll() {
  await Promise.allSettled([
    refreshOverview(),
    refreshWorkers(),
    refreshTasks(),
  ]);
}

(async () => {
  // 1. 인증된 사용자 정보 로드 (401 → 자동 리다이렉트).
  await loadCurrentUser();
  if (!currentUser) return;

  // 2. 데이터 초기 로드.
  await refreshAll();
  startEventStream();

  // 3. 5초 폴링 (SSE와 병행).
  setInterval(refreshAll, 5000);
})();
