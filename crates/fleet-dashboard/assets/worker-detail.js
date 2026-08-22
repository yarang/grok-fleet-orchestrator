    function getWorkerId() {
      const path = window.location.pathname;
      const parts = path.split('/');
      return parts[parts.length - 1];
    }

    function fmtTokens(t) {
      if (!t) return '—';
      const total = t.total_tokens || 0;
      if (total >= 1e6) return (total/1e6).toFixed(1)+'M';
      if (total >= 1e3) return (total/1e3).toFixed(1)+'k';
      return total;
    }

    function fmtDuration(secs) {
      if (!secs) return '—';
      if (secs < 60) return secs.toFixed(1)+'s';
      if (secs < 3600) return (secs/60).toFixed(1)+'m';
      return (secs/3600).toFixed(1)+'h';
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      const d = new Date(iso);
      return d.toLocaleString();
    }

    async function loadWorker() {
      const id = getWorkerId();
      try {
        const resp = await fetch('api/workers/' + id);
        if (!resp.ok) throw new Error('not found');
        const w = await resp.json();

        document.getElementById('worker-name').textContent = w.name || w.id;
        document.getElementById('worker-status').textContent =
          'Status: ' + w.status + ' • Circuit: ' + w.circuit_state;

        const grid = document.getElementById('worker-details');
        const details = [
          ['Status', w.status, 'badge badge-'+w.status],
          ['Active / Max', w.active_tasks + ' / ' + w.max_concurrent],
          ['Circuit', w.circuit_state],
          ['Endpoint', w.endpoint],
          ['Version', w.worker_version || '—'],
          ['Last Seen', fmtTime(w.last_seen)],
          ['Registered', fmtTime(w.registered_at)],
        ];

        grid.innerHTML = details.map(([label, value, cls]) => `
          <div class="detail-item">
            <div class="label">${label}</div>
            <div class="value">${cls ? '<span class="'+cls+'">'+value+'</span>' : value}</div>
          </div>
        `).join('');

        // 라벨
        if (w.labels && Object.keys(w.labels).length > 0) {
          const labels = Object.entries(w.labels).map(([k,v]) => k+'='+v).join(', ');
          grid.innerHTML += `
            <div class="detail-item">
              <div class="label">Labels</div>
              <div class="value" style="font-family:var(--font-mono);font-size:13px;">${escapeHtml(labels)}</div>
            </div>
          `;
        }

        renderTasks(w.recent_tasks || []);
      } catch (e) {
        document.getElementById('worker-name').textContent = 'Worker not found';
      }
    }

    function renderTasks(tasks) {
      const table = document.getElementById('task-table');
      const empty = document.getElementById('empty-state');

      table.querySelectorAll('.row:not(.header)').forEach(r => r.remove());

      if (!tasks || tasks.length === 0) {
        empty.style.display = 'block';
        table.style.display = 'none';
        return;
      }
      empty.style.display = 'none';
      table.style.display = 'grid';

      for (const t of tasks) {
        const row = document.createElement('div');
        row.className = 'row';
        const prompt = t.prompt.length > 50 ? t.prompt.substring(0,50)+'…' : t.prompt;
        row.innerHTML = `
          <div style="font-family:var(--font-mono);font-size:12px;color:var(--primary);">${t.id.substring(0,8)}</div>
          <div><span class="badge badge-${t.phase}">${t.phase}</span></div>
          <div style="font-size:14px;">${escapeHtml(prompt)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${t.model||'—'}</div>
          <div style="font-size:13px;">${fmtTokens(t.token_usage)}</div>
          <div style="font-size:13px;">${fmtDuration(t.duration_secs)}</div>
          <div style="font-size:13px;color:var(--ink-muted-48);">${fmtTime(t.created_at)}</div>
        `;
        table.appendChild(row);
      }
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    loadWorker();

  
