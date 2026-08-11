    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    // 현재 온라인 워커들의 'model' 라벨을 모아 드롭다운을 채운다. 워커가 없거나
    // 라벨이 없으면 "Any available worker" 하나만 남는다 — 자유 텍스트 대체
    // 수단은 없으므로, 필요하면 라벨을 먼저 워커에 달아야 한다.
    async function populateModelOptions() {
      const select = document.getElementById('model-select');
      try {
        const resp = await fetch('/api/workers');
        if (!resp.ok) return;
        const workers = await resp.json();
        const models = new Set();
        for (const w of (workers || [])) {
          const m = (w.labels || {}).model;
          if (m) models.add(m);
        }
        for (const m of Array.from(models).sort()) {
          const opt = document.createElement('option');
          opt.value = m;
          opt.textContent = m;
          select.appendChild(opt);
        }
        if (models.size === 0) {
          const hint = document.getElementById('model-hint');
          if (hint) hint.textContent = 'No online worker advertises a "model" label right now — leave as "Any" or add one after labeling a worker.';
        }
      } catch (e) { console.error('populateModelOptions', e); }
    }

    document.getElementById('task-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const data = new URLSearchParams(new FormData(form));
      data.set('csrf_token', getCsrf());

      const status = document.getElementById('submit-status');
      const submitBtn = form.querySelector('button[type="submit"]');
      status.textContent = 'Submitting…';
      status.style.color = 'var(--ink-muted-48)';
      submitBtn.disabled = true;

      try {
        const resp = await fetch('/api/tasks', {
          method: 'POST',
          body: data,
          headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        });
        const body = await resp.json().catch(() => null);
        if (!resp.ok) {
          const msg = (body && body.error && body.error.message) || (await resp.text());
          status.textContent = 'Error: ' + msg;
          status.style.color = 'var(--badge-failed, #c0392b)';
          return;
        }
        if (body && body.dispatched) {
          status.textContent = 'Dispatched — task ' + body.task_id;
          status.style.color = 'var(--badge-online, #1a7f37)';
        } else {
          status.textContent = 'Created (queued) — task ' + (body && body.task_id) + (body && body.warning ? ': ' + body.warning : '');
          status.style.color = 'var(--badge-degraded, #b08800)';
        }
        setTimeout(() => { window.location.href = '/tasks/' + encodeURIComponent(body.task_id); }, 900);
      } catch (e) {
        status.textContent = 'Error: ' + e.message;
        status.style.color = 'var(--badge-failed, #c0392b)';
      } finally {
        submitBtn.disabled = false;
      }
    });

    populateModelOptions();

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
