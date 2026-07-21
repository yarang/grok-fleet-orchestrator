// Fleet Orchestrator Dashboard — 순수 JS + htmx 없이 직접 fetch.
// 모든 데이터는 5초마다 폴링, 이벤트는 SSE로 실시간 수신.

const API = {
  overview: '/api/overview',
  workers: '/api/workers?limit=50',
  tasks: '/api/tasks?limit=20',
  events: '/api/events?limit=50',
  eventsStream: '/api/events/stream',
};

function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleTimeString();
}

function setStatusPill(online) {
  const pill = document.getElementById('status-pill');
  pill.textContent = online ? 'live' : 'disconnected';
  pill.classList.toggle('online', online);
}

async function fetchJSON(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json();
}

async function refreshOverview() {
  try {
    const data = await fetchJSON(API.overview);
    document.getElementById('metric-workers').textContent = `${data.workers.online}/${data.workers.total}`;
    document.getElementById('metric-tasks-active').textContent = data.tasks.pending + data.tasks.dispatched;
    document.getElementById('metric-tasks-today').textContent = data.tasks.total;
    document.getElementById('metric-failures').textContent = data.tasks.failed;
    setStatusPill(true);
  } catch (e) {
    console.error('overview', e);
    setStatusPill(false);
  }
}

async function refreshWorkers() {
  try {
    const workers = await fetchJSON(API.workers);
    const list = document.getElementById('worker-list');
    // 헤더 보존.
    const header = list.querySelector('.row.header');
    list.innerHTML = '';
    list.appendChild(header);

    if (workers.length === 0) {
      const emptyRow = document.createElement('div');
      emptyRow.className = 'row';
      emptyRow.style.gridTemplateColumns = '1fr';
      emptyRow.style.textAlign = 'center';
      emptyRow.style.color = 'var(--muted)';
      emptyRow.style.padding = '24px';
      emptyRow.textContent = 'No workers online';
      list.appendChild(emptyRow);
    } else {
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
    }
  } catch (e) {
    console.error('workers', e);
  }
}

async function refreshTasks() {
  try {
    const tasks = await fetchJSON(API.tasks);
    const list = document.getElementById('task-list');
    const header = list.querySelector('.row.header');
    list.innerHTML = '';
    list.appendChild(header);

    if (tasks.length === 0) {
      const emptyRow = document.createElement('div');
      emptyRow.className = 'row';
      emptyRow.style.gridTemplateColumns = '1fr';
      emptyRow.style.textAlign = 'center';
      emptyRow.style.color = 'var(--muted)';
      emptyRow.style.padding = '24px';
      emptyRow.textContent = 'No recent tasks';
      list.appendChild(emptyRow);
    } else {
      for (const t of tasks) {
        const row = document.createElement('div');
        row.className = 'row';
        const idShort = t.id.slice(0, 8);
        row.innerHTML = `
          <div title="${t.id}">${idShort}</div>
          <div><span class="phase ${t.phase}">${t.phase}</span></div>
          <div>${escapeHtml(t.prompt.slice(0, 80))}</div>
          <div>${t.worker_id ? t.worker_id.slice(0, 8) : '—'}</div>
          <div>${fmtTime(t.created_at)}</div>
        `;
        list.appendChild(row);
      }
    }
  } catch (e) {
    console.error('tasks', e);
  }
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, c => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  }[c]));
}

let eventCount = 0;

function startEventStream() {
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
        <span class="event-type">${type}</span>
        <span>${escapeHtml(JSON.stringify(ev).slice(0, 200))}</span>
      `;
      log.insertBefore(line, log.firstChild);
      // 최대 100개 라인 보존.
      while (log.children.length > 100) {
        log.removeChild(log.lastChild);
      }
      eventCount++;
      counter.textContent = `(${eventCount})`;
    } catch (err) {
      console.error('event parse', err);
    }
  });

  source.onerror = () => {
    setStatusPill(false);
    // EventSource는 자동 재연결.
  };

  source.onopen = () => {
    setStatusPill(true);
  };
}

async function refreshEventsFallback() {
  // SSE 미지원 환경용 폴링 폴백.
  try {
    const data = await fetchJSON(API.events);
    const log = document.getElementById('event-log');
    log.innerHTML = '';
    for (const entry of (data.events || []).reverse()) {
      const ev = entry.event || entry;
      const type = ev.type || 'unknown';
      const time = fmtTime(entry.event?.at);
      const line = document.createElement('div');
      line.className = 'event-line';
      line.innerHTML = `
        <span class="event-time">${time}</span>
        <span class="event-type">${type}</span>
        <span>${escapeHtml(JSON.stringify(ev).slice(0, 200))}</span>
      `;
      log.appendChild(line);
    }
  } catch (e) {
    console.error('events fallback', e);
  }
}

async function refreshAll() {
  await Promise.allSettled([
    refreshOverview(),
    refreshWorkers(),
    refreshTasks(),
  ]);
}

// 초기 로드.
refreshAll();
startEventStream();

// 5초마다 폴링 (SSE와 병행).
setInterval(refreshAll, 5000);
