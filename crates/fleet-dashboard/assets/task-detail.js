    function getTaskId() {
      const path = window.location.pathname;
      const parts = path.split('/');
      return parts[parts.length - 1];
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
        const resp = await fetch('/api/tasks/' + encodeURIComponent(id));
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
          ['Worker', t.worker_id ? t.worker_id.substring(0,8) : '—'],
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
          document.getElementById('task-output').textContent = output.chunks.join('');
        } else if (t.phase === 'completed') {
          document.getElementById('task-output').textContent = '(task completed with no stdout/stderr output)';
        } else if (t.phase === 'failed') {
          document.getElementById('task-output').textContent = '(task failed — see audit log for details)';
        } else {
          document.getElementById('task-output').textContent = '(task is ' + t.phase + ' — output will appear here when available)';
        }
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

    loadTask();

  
