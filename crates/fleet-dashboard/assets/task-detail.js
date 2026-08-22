    function getTaskId() {
      const path = window.location.pathname;
      const parts = path.split('/');
      return parts[parts.length - 1];
    }

    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      return new Date(iso).toLocaleString();
    }

    function fmtDuration(secs) {
      if (!secs) return '—';
      if (secs < 60) return secs.toFixed(1)+'s';
      if (secs < 3600) return (secs/60).toFixed(1)+'m';
      return (secs/3600).toFixed(1)+'h';
    }

    function fmtTokens(t) {
      if (!t) return '—';
      return (t.total_tokens||0).toLocaleString();
    }

    async function loadTask() {
      const id = getTaskId();
      try {
        const [resp, workerNames] = await Promise.all([
          fetch('api/tasks/' + encodeURIComponent(id)),
          getWorkerNameMap(),
        ]);
        if (!resp.ok) throw new Error('not found');
        const data = await resp.json();
        const t = data.task;
        const output = data.output;

        document.getElementById('task-id').textContent = 'Task ' + id.substring(0,8);
        document.getElementById('task-phase').textContent = 'Phase: ' + t.phase;

        const grid = document.getElementById('task-details');
        const details = [
          ['Status', t.phase],
          ['Created By', t.created_by],
          ['Worker', workerLabel(t.worker_id, workerNames)],
          ['Model', t.model || '—'],
          ['Duration', fmtDuration(t.duration_secs)],
          ['Exit Code', t.exit_code !== null && t.exit_code !== undefined ? t.exit_code : '—'],
          ['Tokens', fmtTokens(t.token_usage)],
          ['Created', fmtTime(t.created_at)],
        ];

        grid.innerHTML = details.map(([label, value]) => `
          <div class="detail-item">
            <div class="label">${label}</div>
            <div class="value">${escapeHtml(String(value))}</div>
          </div>
        `).join('');

        document.getElementById('task-prompt').textContent = t.prompt || '';

        if (output && output.chunks && output.chunks.length > 0) {
          // chunks는 {seq, chunk, written_at} 객체 배열 — chunk 텍스트 필드만
          // 뽑아 이어 붙여야 한다. 예전엔 배열 자체를 join()해서 각 객체가
          // "[object Object]"로 스트링화되는 버그가 있었다 (2026-08-11).
          document.getElementById('task-output').textContent =
            output.chunks.map(function (c) { return c.chunk || ''; }).join('');
        } else if (t.phase === 'completed') {
          document.getElementById('task-output').textContent = '(task completed with no stdout/stderr output)';
        } else if (t.phase === 'failed') {
          document.getElementById('task-output').textContent = '(task failed — see audit log for details)';
        } else {
          document.getElementById('task-output').textContent = '(task is ' + t.phase + ' — output will appear here when available)';
        }
        loadThread(id);
      } catch(e) {
        document.getElementById('task-id').textContent = 'Task not found';
        document.getElementById('task-output').textContent = '';
      }
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    // 이 태스크가 속한 스레드(연속 대화) 히스토리를 불러온다. 태스크 하나뿐인
    // (아직 이어간 적 없는) 스레드는 섹션 자체를 숨긴다 — 매번 "히스토리
    // 없음"을 보여주는 건 잡음이라 판단.
    async function loadThread(currentId) {
      try {
        const resp = await fetch('api/tasks/' + encodeURIComponent(currentId) + '/thread');
        if (!resp.ok) return;
        const data = await resp.json();
        const thread = data.thread || [];
        if (thread.length < 2) return;

        const section = document.getElementById('task-thread-section');
        const grid = document.getElementById('task-thread');
        section.style.display = '';
        grid.innerHTML = thread.map(function (t, i) {
          const isCurrent = t.id === currentId;
          const label = (i === 0 ? 'Root' : 'Reply ' + i) + (isCurrent ? ' (viewing)' : '');
          const promptPreview = (t.prompt || '').slice(0, 80);
          return '<div class="detail-item">'
            + '<div class="label">' + escapeHtml(label) + '</div>'
            + '<div class="value">'
            + (isCurrent
                ? escapeHtml(promptPreview)
                : '<a href="tasks/' + encodeURIComponent(t.id) + '">' + escapeHtml(promptPreview) + '</a>')
            + ' <span style="opacity:0.6;">(' + escapeHtml(t.phase) + ')</span>'
            + '</div></div>';
        }).join('');
      } catch (e) { console.error('loadThread', e); }
    }

    document.getElementById('reply-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const currentId = getTaskId();
      const form = e.target;
      const status = document.getElementById('reply-status');
      const submitBtn = form.querySelector('button[type="submit"]');
      const promptEl = document.getElementById('reply-prompt');

      const data = new URLSearchParams();
      data.set('prompt', promptEl.value);
      data.set('parent_task_id', currentId);
      data.set('csrf_token', getCsrf());

      status.textContent = 'Submitting…';
      status.style.color = 'var(--ink-muted-48)';
      submitBtn.disabled = true;

      try {
        const resp = await fetch('api/tasks', {
          method: 'POST',
          body: data,
          headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        });
        const rawText = await resp.text();
        let body = null;
        try { body = JSON.parse(rawText); } catch (_) { /* not JSON */ }

        if (!resp.ok) {
          const msg = (body && body.error && body.error.message) || rawText || ('HTTP ' + resp.status);
          status.textContent = 'Error: ' + msg;
          status.style.color = 'var(--badge-failed, #c0392b)';
          submitBtn.disabled = false;
          return;
        }
        status.textContent = 'Sent — redirecting…';
        status.style.color = 'var(--badge-online, #1a7f37)';
        window.location.href = 'tasks/' + encodeURIComponent(body.task_id);
      } catch (e) {
        status.textContent = 'Error: ' + e.message;
        status.style.color = 'var(--badge-failed, #c0392b)';
        submitBtn.disabled = false;
      }
    });

    loadTask();

  
