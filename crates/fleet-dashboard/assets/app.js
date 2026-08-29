// Fleet Orchestrator Dashboard — 쿠키 기반 세션 (Phase 9.1).
// 인증은 HttpOnly 쿠키로 자동 처리. fetch는 credentials: 'same-origin' (기본값).
// 401 시 /login 으로 리다이렉트. 별도 prompt 없음(로드맵 #14의 다크 모드
// 명시적 선호만 localStorage에 저장 — 인증/세션과는 무관).

const API = {
  overview: 'api/overview',
  workers: 'api/workers?limit=50',
  tasks: 'api/tasks?limit=20',
  events: 'api/events?limit=50',
  eventsStream: 'api/events/stream',
  me: 'api/me',
};

// ── worker_id → name 매핑 (모든 페이지 공용) ────────────────────────────
//
// Task/Event는 worker_id(UUID)만 갖고 있어 그대로 찍으면 가독성이 떨어진다.
// /api/workers가 이미 id·name을 함께 주므로, 한 번 fetch해서 페이지 생명주기
// 동안 캐싱하고 각 페이지 렌더링에서 UUID 대신 name을 보여주는 데 쓴다.
// 워커가 삭제/미등록이라 맵에 없으면(또는 fetch 자체가 실패하면) 기존처럼
// UUID 앞 8자로 조용히 폴백 — 이 매핑은 표시용 개선이지 필수 데이터가 아니다.
let _workerNameCache = null;
async function getWorkerNameMap() {
  if (_workerNameCache) return _workerNameCache;
  try {
    const workers = await fetchJSON(API.workers);
    const map = {};
    for (const w of workers) map[w.id] = w.name;
    _workerNameCache = map;
    return map;
  } catch (e) {
    console.error('getWorkerNameMap', e);
    return {};
  }
}

// workerId가 매핑에 있으면 name, 없으면 UUID 앞 8자(기존 폴백)를 반환.
function workerLabel(workerId, nameMap) {
  if (!workerId) return '—';
  return (nameMap && nameMap[workerId]) || String(workerId).slice(0, 8);
}

// ── 다크 모드: 저장된 명시적 선호를 즉시 적용 (로드맵 #14) ───────────────
//
// 이 파일은 매 페이지의 </body> 바로 앞에서 로드되므로, 여기서 최대한
// 일찍(다른 초기화보다 먼저, 스크립트 최상단에서) 적용해야 깜빡임(FOUC)이
// 짧다. `initThemeToggle()`(아래 §다크 모드 섹션)은 토글 버튼 UI를
// 만드는 부분만 담당 — 실제 테마 적용은 여기서 끝낸다.
const THEME_KEY = 'fleet-theme';

function currentEffectiveTheme() {
  const explicit = localStorage.getItem(THEME_KEY);
  if (explicit === 'light' || explicit === 'dark') return explicit;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyExplicitTheme(theme) {
  if (theme === 'light' || theme === 'dark') {
    document.documentElement.setAttribute('data-theme', theme);
  } else {
    document.documentElement.removeAttribute('data-theme');
  }
}

applyExplicitTheme(localStorage.getItem(THEME_KEY));

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
      window.location.href = 'login';
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
    await fetch('logout', {
      method: 'POST',
      headers: { 'X-CSRF-Token': getCsrfToken() },
    });
    window.location.href = 'login';
  });
}

// ── 다크 모드: 토글 버튼 UI (로드맵 #14) ─────────────────────────────
//
// 실제 테마 적용(`applyExplicitTheme`)은 이 파일 최상단에서 이미 끝났다 —
// 여기서는 사이드바 하단에 토글 버튼을 동적으로 만들어 붙이기만 한다. 모든
// HTML 페이지가 사이드바 마크업을 각자 복제하고 있어(공용 템플릿 없음)
// 정적 버튼을 마크업에 추가하는 대신 여기서 동적으로 생성 — 페이지 14곳을
// 일일이 고칠 필요가 없다.
function initThemeToggle() {
  const footer = document.querySelector('.sidebar-footer');
  if (!footer) return;

  const btn = document.createElement('button');
  btn.id = 'theme-toggle';
  btn.type = 'button';
  btn.className = 'theme-toggle-btn';

  const updateLabel = () => {
    const dark = currentEffectiveTheme() === 'dark';
    btn.textContent = dark ? '☀️ Light mode' : '🌙 Dark mode';
    btn.setAttribute('aria-label', dark ? 'Switch to light mode' : 'Switch to dark mode');
  };
  updateLabel();

  btn.addEventListener('click', () => {
    const next = currentEffectiveTheme() === 'dark' ? 'light' : 'dark';
    localStorage.setItem(THEME_KEY, next);
    applyExplicitTheme(next);
    updateLabel();
  });

  footer.insertBefore(btn, footer.firstChild);
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
    window.location.href = 'login';
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
        <div><span class="status-pill ${escapeHtml(w.status)}">${escapeHtml(w.status)}</span></div>
        <div>${escapeHtml(String(w.active_tasks))}/${escapeHtml(String(w.max_concurrent))}</div>
        <div>${escapeHtml(w.circuit_state)}</div>
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
    const [tasks, workerNames] = await Promise.all([fetchJSON(API.tasks), getWorkerNameMap()]);
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
        <div title="${escapeHtml(t.id)}">${escapeHtml(idShort)}</div>
        <div><span class="phase ${escapeHtml(t.phase)}">${escapeHtml(t.phase)}</span></div>
        <div>${escapeHtml((t.prompt || '').slice(0, 60))}</div>
        <div>${escapeHtml(t.model || '—')}</div>
        <div title="${escapeHtml(t.worker_id || '')}">${escapeHtml(workerLabel(t.worker_id, workerNames))}</div>
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
  // 0. 사이드바 모바일 드로어 / 다크 모드 토글은 모든 인증된 페이지에서 공통으로 초기화.
  initSidebarToggle();
  initThemeToggle();

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
