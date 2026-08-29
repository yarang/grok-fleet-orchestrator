// AgentTemplate 생성 폼 (로드맵 #92).
//
// 여기서 만드는 것은 **정체성뿐**이고 상태는 항상 `draft`다. 본문(role prompt,
// tools, skills)은 상세 화면의 revision 폼이 담당한다 — `#86`이 정체성과 본문을
// 두 계층으로 나눈 이유가 그것이고, 이 폼이 본문 칸을 갖지 않는 것이 그 분리를
// 화면에서 지키는 방식이다.

const form = document.getElementById('template-form');
const status = document.getElementById('submit-status');
const submitBtn = document.getElementById('submit-btn');

form.addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const name = document.getElementById('name').value.trim();
  const description = document.getElementById('description').value.trim();
  const projectId = document.getElementById('project-id').value.trim();
  if (!name) return;

  status.textContent = 'Creating…';
  status.style.color = 'var(--ink-muted-48)';
  submitBtn.disabled = true;
  try {
    const payload = { name };
    if (description) payload.description = description;
    // 빈 칸은 키를 아예 빼서 보낸다 — `project_id: null`과 "글로벌"이 서버에서
    // 같은 뜻이지만, 보내지 않는 쪽이 의도를 덜 흐린다.
    if (projectId) payload.project_id = projectId;

    const resp = await fetch('api/agent-templates', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrfToken() },
      body: JSON.stringify(payload),
    });
    const rawText = await resp.text();
    let body = null;
    try { body = JSON.parse(rawText); } catch (_) { /* JSON이 아니면 null 유지 */ }
    if (!resp.ok) {
      const msg = (body && body.error && body.error.message) || rawText || ('HTTP ' + resp.status);
      status.textContent = 'Error: ' + msg;
      status.style.color = 'var(--badge-failed, #c0392b)';
      return;
    }
    status.textContent = 'Created';
    status.style.color = 'var(--badge-online, #1a7f37)';
    setTimeout(() => {
      window.location.href = 'agent-templates/' + encodeURIComponent(body.id);
    }, 600);
  } catch (e) {
    status.textContent = 'Error: ' + e;
    status.style.color = 'var(--badge-failed, #c0392b)';
  } finally {
    submitBtn.disabled = false;
  }
});

const pill = document.getElementById('status-pill');
try {
  const es = new EventSource('api/events/stream');
  es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
  es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
} catch (e) {}
