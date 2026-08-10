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

/// CSRF 토큰을 fleet_csrf 쿠키에서 추출 (더블 서밋 패턴).
function getCsrfToken() {
  const match = document.cookie
    .split('; ')
    .find(c => c.startsWith('fleet_csrf='));
  return match ? match.split('=').slice(1).join('=') : '';
}

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

/// 사이드바 하단의 #sidebar-user-menu 컨테이너에 사용자 메뉴를 렌더링한다.
/// 모든 인증된 페이지가 공유하는 단일 진입점 — 페이지마다 중복 구현하지 않는다.
/// data-rendered 플래그로 중복 초기화만 막고, 컨테이너가 이미 존재해도 항상 채운다
/// (과거 버전은 getElementById('user-menu')로 조기 반환하여 정적 placeholder가 있는
/// 페이지에서 메뉴가 영영 렌더링되지 않는 버그가 있었다).
function renderUserMenu() {
  if (!currentUser) return;
  const container = document.getElementById('sidebar-user-menu');
  if (!container || container.dataset.rendered === 'true') return;

  container.dataset.rendered = 'true';
  container.innerHTML = `
    <span>${escapeHtml(currentUser.username || '')}</span>
    <button id="logout-btn" type="button">Sign out</button>
  `;

  document.getElementById('logout-btn').addEventListener('click', async () => {
    await fetch('/logout', {
      method: 'POST',
      headers: { 'X-CSRF-Token': getCsrfToken() },
    });
    window.location.href = '/login';
  });
}

// ── 사이드바 모바일 드로어 토글 ──────────────────────────────────────────

/// 모든 인증된 페이지에서 공유하는 사이드바 partial의 햄버거 버튼 / 백드롭을 초기화한다.
function initSidebarToggle() {
  const toggle = document.getElementById('sidebar-toggle');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (!toggle) return;

  const close = () => {
    document.body.classList.remove('sidebar-open');
    toggle.setAttribute('aria-expanded', 'false');
  };
  const open = () => {
    document.body.classList.add('sidebar-open');
    toggle.setAttribute('aria-expanded', 'true');
  };

  toggle.addEventListener('click', () => {
    document.body.classList.contains('sidebar-open') ? close() : open();
  });
  if (backdrop) backdrop.addEventListener('click', close);

  // 사이드바 링크 클릭 시(모바일) 드로어 닫기 — 페이지 이동으로 자연히 닫히지만
  // 같은 페이지 내 앵커/재클릭 대비.
  document.querySelectorAll('.sidebar-link').forEach(link => {
    link.addEventListener('click', close);
  });
}

// ── 데이터 fetch ──────────────────────────────────────────────────────

function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleTimeString();
}

/// 토큰 수를 사람이 읽기 쉬운 형태로 포맷 (1234 → "1.2K").
function fmtTokens(n) {
  if (!n || n === 0) return '—';
  if (n < 1000) return String(n);
  if (n < 1_000_000) return (n / 1000).toFixed(1) + 'K';
  return (n / 1_000_000).toFixed(1) + 'M';
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
    const tokens = data.tokens;
    setMetric('metric-tokens-total', tokens ? fmtTokens(tokens.total_tokens) : '—');
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
      const tokenStr = t.token_usage ? fmtTokens(t.token_usage.total_tokens) : '—';
      row.innerHTML = `
        <div title="${escapeHtml(t.id)}">${idShort}</div>
        <div><span class="phase ${t.phase}">${t.phase}</span></div>
        <div>${escapeHtml((t.prompt || '').slice(0, 60))}</div>
        <div>${escapeHtml(t.model || '—')}</div>
        <div>${t.worker_id ? String(t.worker_id).slice(0, 8) : '—'}</div>
        <div>${tokenStr}</div>
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
  // 0. 사이드바 모바일 드로어는 모든 인증된 페이지에서 공통으로 초기화.
  initSidebarToggle();

  // 1. 인증된 사용자 정보 로드 (401 → 자동 리다이렉트). 사이드바 사용자 메뉴도 여기서 렌더링.
  await loadCurrentUser();
  if (!currentUser) return;

  // 2. Overview 대시보드 위젯(#overview-grid)이 있는 페이지(index.html)에서만
  //    메트릭/워커/태스크 폴링과 SSE 스트림을 시작한다. 다른 페이지는 자체 스크립트가
  //    자신의 데이터를 관리하므로 여기서 중복 폴링/SSE 연결을 만들지 않는다.
  if (document.getElementById('overview-grid')) {
    await refreshAll();
    startEventStream();
    setInterval(refreshAll, 5000);
  }
})();
