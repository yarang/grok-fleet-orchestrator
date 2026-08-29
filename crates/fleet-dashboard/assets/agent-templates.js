// AgentTemplate 목록 (로드맵 #92). projects.js와 같은 구조 — 정렬은 클라이언트,
// 필터는 서버다. 필터를 서버에 맡기는 이유는 `project_scope`가 3-상태이기
// 때문이다: 전체 / 글로벌만 / 특정 프로젝트. 쿼리스트링은 "없음"과 "NULL"을
// 구분하지 못해 `project_id`와 `global` 두 파라미터로 펼쳐져 있고, 그 해석은
// 서버만 갖고 있다. 목록을 다 받아 와서 거르면 그 규칙이 여기에 복제된다.

let allTemplates = [];
let sortKey = 'created_at';
let sortDir = 'desc';

const STATUS_BADGE = {
  draft: 'badge-pending',
  published: 'badge-online',
  deprecated: 'badge-provisioned',
  retired: 'badge-offline',
  discarded: 'badge-cancelled',
};

function currentQuery() {
  const params = new URLSearchParams();
  const status = document.getElementById('filter-status').value;
  if (status) params.set('status', status);
  // `global=true`와 `project_id`는 서버가 상호 배타로 거절한다. 이 화면은
  // `project_id`를 보내지 않으므로 그 충돌이 생길 수 없다.
  if (document.getElementById('filter-scope').value === 'global') params.set('global', 'true');
  const qs = params.toString();
  return qs ? 'api/agent-templates?' + qs : 'api/agent-templates';
}

async function fetchTemplates() {
  try {
    const resp = await fetch(currentQuery());
    if (!resp.ok) {
      if (resp.status === 403) {
        document.getElementById('template-table').style.display = 'none';
        const empty = document.getElementById('empty-state');
        empty.style.display = 'block';
        empty.querySelector('h3').textContent = 'Not permitted';
        empty.querySelector('p').textContent = 'You do not have permission to view agent templates.';
      }
      return;
    }
    allTemplates = await resp.json();
    render();
  } catch (e) {
    console.error('fetch agent templates:', e);
  }
}

function compareTemplates(a, b, key, dir) {
  let av, bv;
  if (key === 'created_at') {
    av = new Date(a.created_at).getTime();
    bv = new Date(b.created_at).getTime();
  } else {
    av = String(a[key] ?? '').toLowerCase();
    bv = String(b[key] ?? '').toLowerCase();
  }
  if (av < bv) return dir === 'asc' ? -1 : 1;
  if (av > bv) return dir === 'asc' ? 1 : -1;
  return 0;
}

function updateSortIndicators() {
  document.querySelectorAll('#template-table-header .sortable').forEach(cell => {
    const active = cell.dataset.sortKey === sortKey;
    cell.classList.toggle('sort-active', active);
    if (active) { cell.dataset.sortDir = sortDir; } else { delete cell.dataset.sortDir; }
  });
}

function fmtTime(iso) {
  const d = new Date(iso);
  const secs = Math.floor((Date.now() - d.getTime()) / 1000);
  if (secs < 60) return secs + 's ago';
  if (secs < 3600) return Math.floor(secs / 60) + 'm ago';
  if (secs < 86400) return Math.floor(secs / 3600) + 'h ago';
  return d.toLocaleDateString();
}

function statusBadge(status) {
  const cls = STATUS_BADGE[status] || 'badge-cancelled';
  return '<span class="badge ' + cls + '">' + escapeHtml(status) + '</span>';
}

function render() {
  const table = document.getElementById('template-table');
  table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());
  const empty = document.getElementById('empty-state');

  if (allTemplates.length === 0) {
    table.style.display = 'none';
    empty.style.display = 'block';
    return;
  }
  empty.style.display = 'none';
  table.style.display = 'grid';

  const sorted = [...allTemplates].sort((a, b) => compareTemplates(a, b, sortKey, sortDir));
  for (const t of sorted) {
    const row = document.createElement('div');
    row.className = 'row';
    row.style.cursor = 'pointer';
    row.onclick = () => window.location.href = 'agent-templates/' + encodeURIComponent(t.id);
    const scope = t.project_id
      ? '<span title="' + escapeHtml(t.project_id) + '">project</span>'
      : '<span style="font-weight:600;">global</span>';
    row.innerHTML = `
      <div style="font-weight:600;">${escapeHtml(t.name)}</div>
      <div>${statusBadge(t.status)}</div>
      <div style="font-size:13px;">${scope}</div>
      <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(t.description || '—')}</div>
      <div style="font-size:13px;">${escapeHtml(t.created_by || '—')}</div>
      <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(fmtTime(t.created_at))}</div>`;
    table.appendChild(row);
  }
  updateSortIndicators();
}

document.querySelectorAll('#template-table-header .sortable').forEach(cell => {
  cell.addEventListener('click', () => {
    const key = cell.dataset.sortKey;
    if (sortKey === key) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortKey = key;
      sortDir = key === 'created_at' ? 'desc' : 'asc';
    }
    render();
  });
});

// 필터가 바뀌면 정렬 상태는 그대로 두고 다시 받아 온다.
['filter-status', 'filter-scope'].forEach(id => {
  document.getElementById(id).addEventListener('change', fetchTemplates);
});

fetchTemplates();
setInterval(fetchTemplates, 10000);

const pill = document.getElementById('status-pill');
try {
  const es = new EventSource('api/events/stream');
  es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
  es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
} catch (e) {}
