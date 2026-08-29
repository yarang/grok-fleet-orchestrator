// AgentTemplate 상세 (로드맵 #92).
//
// 이 화면의 규칙은 하나다 — **상태 기계를 여기서 다시 구현하지 않는다.**
// 어떤 전이가 가능한지(`allowed_transitions`)도, 새 revision을 붙일 수 있는지
// (`accepts_new_revisions`)도 서버가 파생 필드로 실어 보낸다. JS가 자기 표를
// 들면 코어의 표와 갈라지고, 갈라진 쪽이 화면에서는 조용히 이긴다.

// 경로가 /agent-templates/:id — 마지막 세그먼트가 id.
const templateId = decodeURIComponent(window.location.pathname.split('/').filter(Boolean).pop());

const STATUS_BADGE = {
  draft: 'badge-pending',
  published: 'badge-online',
  deprecated: 'badge-provisioned',
  retired: 'badge-offline',
  discarded: 'badge-cancelled',
};

let template = null;
let permissions = new Set();
// `retired` 확인 단계에서 받아 둔 dependent 스냅숏. 서버가 이 해시를 그대로
// 되돌려받아야 전이를 허용한다 — 확인 화면을 보는 동안 의존 Agent 집합이
// 바뀌었다면 409로 거절되고, 그때는 다시 읽어야 한다.
let pendingRetire = null;

const txStatus = document.getElementById('transition-status');
const retirePanel = document.getElementById('retire-confirm');

function setStatusLine(el, msg, kind) {
  el.textContent = msg;
  if (kind === 'error') el.style.color = 'var(--badge-failed, #c0392b)';
  else if (kind === 'ok') el.style.color = 'var(--badge-online, #1a7f37)';
  else el.style.color = 'var(--ink-muted-48)';
}

async function readError(resp) {
  const rawText = await resp.text();
  let body = null;
  try { body = JSON.parse(rawText); } catch (_) { /* JSON이 아니면 null 유지 */ }
  return (body && body.error && body.error.message) || rawText || ('HTTP ' + resp.status);
}

function statusBadge(status) {
  const cls = STATUS_BADGE[status] || 'badge-cancelled';
  return '<span class="badge ' + cls + '">' + escapeHtml(status) + '</span>';
}

function fmtTime(iso) {
  if (!iso) return '';
  return new Date(iso).toLocaleString();
}

async function loadPermissions() {
  try {
    const resp = await fetch('api/me');
    if (!resp.ok) return;
    const me = await resp.json();
    permissions = new Set(me.permissions || []);
  } catch (e) {
    console.error('fetch me:', e);
  }
}

async function loadTemplate() {
  const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId));
  if (!resp.ok) {
    // 404와 403을 구분해 보여 준다. 목록을 받아 클라이언트에서 걸렀다면 둘 다
    // "빈 결과"가 되어 이 구분이 불가능하다 — 그래서 단건 조회가 따로 있다.
    const nf = document.getElementById('not-found');
    nf.style.display = 'block';
    if (resp.status === 403) {
      nf.querySelector('h3').textContent = 'Not permitted';
      document.getElementById('not-found-detail').textContent =
        'You do not have permission to view agent templates.';
    } else {
      document.getElementById('not-found-detail').textContent = await readError(resp);
    }
    return false;
  }
  template = await resp.json();
  document.getElementById('detail-body').style.display = 'block';
  return true;
}

function renderHeader() {
  document.getElementById('template-name').textContent = template.name;
  const scope = template.project_id ? 'project ' + template.project_id : 'global';
  document.getElementById('template-meta').innerHTML =
    statusBadge(template.status) +
    ' &nbsp;·&nbsp; ' + escapeHtml(scope) +
    ' &nbsp;·&nbsp; created by ' + escapeHtml(template.created_by || 'unknown') +
    ' &nbsp;·&nbsp; ' + escapeHtml(fmtTime(template.created_at));
  document.getElementById('template-description').textContent = template.description || '';
}

function renderTransitions() {
  const box = document.getElementById('transition-buttons');
  box.innerHTML = '';
  retirePanel.style.display = 'none';
  pendingRetire = null;

  if (!permissions.has('agent_template:lifecycle')) {
    box.innerHTML = '<span style="font-size:13px;color:var(--ink-muted-48);">'
      + 'Lifecycle changes require the <code>agent_template:lifecycle</code> capability.</span>';
    return;
  }
  if (template.allowed_transitions.length === 0) {
    box.innerHTML = '<span style="font-size:13px;color:var(--ink-muted-48);">'
      + 'This is a terminal state — no transitions remain.</span>';
    return;
  }
  for (const target of template.allowed_transitions) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'btn';
    btn.textContent = 'Move to ' + target;
    // 값은 data-*로 싣고 핸들러는 addEventListener로 단다. 속성 문자열에
    // 값을 끼워 넣으면 HTML 파서가 문자 참조를 먼저 풀어 JS로 넘긴다.
    btn.dataset.target = target;
    btn.addEventListener('click', () => beginTransition(btn.dataset.target));
    box.appendChild(btn);
  }
}

async function beginTransition(target) {
  if (target === 'retired') {
    // retire만 의존 집합 확인을 거친다. 이 템플릿을 pin한 Agent가 있는데도
    // 모르고 은퇴시키는 것을 막는 것이 이 단계의 목적이다.
    setStatusLine(txStatus, 'Checking dependents…', 'muted');
    try {
      const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId) + '/dependents');
      if (!resp.ok) { setStatusLine(txStatus, 'Error: ' + await readError(resp), 'error'); return; }
      const dep = await resp.json();
      pendingRetire = dep;
      const n = dep.agent_ids.length;
      document.getElementById('retire-summary').textContent = n === 0
        ? 'No Agent pins a revision of this template. Retiring is safe.'
        : n + ' Agent(s) still pin a revision of this template. They keep running; '
          + 'retiring only blocks new pins.';
      // 라벨은 의존자 수를 따라간다 — 0인데 "anyway"라고 하면 있지도 않은
      // 위험을 경고하는 셈이라, 진짜 경고여야 할 때의 무게가 깎인다.
      document.getElementById('retire-go').textContent = n === 0 ? 'Retire' : 'Retire anyway';
      retirePanel.style.display = 'block';
      setStatusLine(txStatus, '', 'muted');
    } catch (e) {
      setStatusLine(txStatus, 'Error: ' + e, 'error');
    }
    return;
  }
  await postStatus(target, null);
}

async function postStatus(target, hash) {
  setStatusLine(txStatus, 'Applying…', 'muted');
  try {
    const payload = { status: target };
    if (hash) payload.dependent_set_hash = hash;
    const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId) + '/status', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrfToken() },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      const msg = await readError(resp);
      setStatusLine(txStatus, resp.status === 409
        ? 'Conflict: ' + msg + ' — re-check dependents and try again.'
        : 'Error: ' + msg, 'error');
      return;
    }
    setStatusLine(txStatus, 'Now ' + target, 'ok');
    await refresh();
  } catch (e) {
    setStatusLine(txStatus, 'Error: ' + e, 'error');
  }
}

document.getElementById('retire-go').addEventListener('click', () => {
  if (!pendingRetire) return;
  const hash = pendingRetire.dependent_set_hash;
  retirePanel.style.display = 'none';
  pendingRetire = null;
  postStatus('retired', hash);
});
document.getElementById('retire-cancel').addEventListener('click', () => {
  retirePanel.style.display = 'none';
  pendingRetire = null;
  setStatusLine(txStatus, '', 'muted');
});

function renderRevisions(revisions) {
  const table = document.getElementById('revision-table');
  table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());
  const empty = document.getElementById('revision-empty');

  if (revisions.length === 0) {
    table.style.display = 'none';
    empty.style.display = 'block';
    return;
  }
  empty.style.display = 'none';
  table.style.display = 'grid';

  const canRevoke = permissions.has('agent_template:revision_revoke');
  for (const r of revisions) {
    const row = document.createElement('div');
    row.className = 'row';
    const prompt = r.role_prompt.length > 120 ? r.role_prompt.slice(0, 120) + '…' : r.role_prompt;
    const revoked = r.revoked_at
      ? '<span class="badge badge-cancelled">revoked</span>'
      : '';
    row.innerHTML = `
      <div style="font-weight:600;" title="${escapeHtml(r.content_hash)}">${escapeHtml(String(r.content_revision))}</div>
      <div style="font-size:13px;white-space:pre-wrap;">${escapeHtml(prompt)}</div>
      <div style="font-size:13px;">${escapeHtml(r.tools.join(', ') || '—')}</div>
      <div style="font-size:13px;">${escapeHtml(r.skills.join(', ') || '—')}</div>
      <div style="font-size:13px;color:var(--ink-muted-48);">${escapeHtml(fmtTime(r.created_at))}</div>
      <div data-cell="action">${revoked}</div>`;
    if (!r.revoked_at && canRevoke) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'btn';
      btn.textContent = 'Revoke';
      btn.dataset.revisionId = r.id;
      btn.addEventListener('click', () => revokeRevision(btn.dataset.revisionId, btn));
      row.querySelector('[data-cell="action"]').appendChild(btn);
    }
    table.appendChild(row);
  }
}

async function revokeRevision(revisionId, btn) {
  btn.disabled = true;
  try {
    const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId)
      + '/revisions/' + encodeURIComponent(revisionId) + '/revoke', {
      method: 'POST',
      headers: { 'X-CSRF-Token': getCsrfToken() },
    });
    if (!resp.ok) {
      setStatusLine(txStatus, 'Error: ' + await readError(resp), 'error');
      return;
    }
    await refresh();
  } catch (e) {
    setStatusLine(txStatus, 'Error: ' + e, 'error');
  } finally {
    btn.disabled = false;
  }
}

function renderRevisionForm() {
  const panel = document.getElementById('revision-form-panel');
  const closed = document.getElementById('revision-form-closed');
  if (!template.accepts_new_revisions) {
    panel.style.display = 'none';
    closed.style.display = 'block';
    closed.textContent = 'A ' + template.status + ' template does not accept new revisions.';
    return;
  }
  if (!permissions.has('agent_template:update')) {
    panel.style.display = 'none';
    closed.style.display = 'block';
    closed.textContent = 'Adding a revision requires the agent_template:update capability.';
    return;
  }
  closed.style.display = 'none';
  panel.style.display = 'block';
}

function parseList(raw) {
  return raw.split(',').map(s => s.trim()).filter(s => s.length > 0);
}

document.getElementById('revision-form').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const status = document.getElementById('revision-status');
  const btn = document.getElementById('revision-submit');
  const rolePrompt = document.getElementById('role-prompt').value;
  if (!rolePrompt.trim()) return;

  setStatusLine(status, 'Saving…', 'muted');
  btn.disabled = true;
  try {
    const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId) + '/revisions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrfToken() },
      body: JSON.stringify({
        role_prompt: rolePrompt,
        tools: parseList(document.getElementById('tools').value),
        skills: parseList(document.getElementById('skills').value),
      }),
    });
    if (!resp.ok) {
      setStatusLine(status, 'Error: ' + await readError(resp), 'error');
      return;
    }
    setStatusLine(status, 'Added', 'ok');
    document.getElementById('role-prompt').value = '';
    document.getElementById('tools').value = '';
    document.getElementById('skills').value = '';
    await refresh();
  } catch (e) {
    setStatusLine(status, 'Error: ' + e, 'error');
  } finally {
    btn.disabled = false;
  }
});

async function loadRevisions() {
  try {
    const resp = await fetch('api/agent-templates/' + encodeURIComponent(templateId) + '/revisions');
    if (!resp.ok) { renderRevisions([]); return; }
    renderRevisions(await resp.json());
  } catch (e) {
    console.error('fetch revisions:', e);
  }
}

async function refresh() {
  if (!await loadTemplate()) return;
  renderHeader();
  renderTransitions();
  renderRevisionForm();
  await loadRevisions();
}

(async () => {
  await loadPermissions();
  await refresh();
})();

const pill = document.getElementById('status-pill');
try {
  const es = new EventSource('api/events/stream');
  es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
  es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
} catch (e) {}
