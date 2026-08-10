    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    async function loadSshKeys() {
      const select = document.getElementById('ssh-key-select');
      try {
        const resp = await fetch('/api/ssh-keys');
        if (!resp.ok) { select.innerHTML = '<option value="">No keys available</option>'; return; }
        const keys = await resp.json();
        if (!keys || keys.length === 0) {
          select.innerHTML = '<option value="">No keys — upload one first</option>';
        } else {
          select.innerHTML = keys.map(k =>
            '<option value="' + k.name + '">' + k.name + ' (' + k.key_type + ')</option>'
          ).join('');
        }
      } catch(e) {
        select.innerHTML = '<option value="">Failed to load keys</option>';
      }
    }

    document.getElementById('provision-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const fd = new FormData(form);

      // labels 파싱.
      const labels = {};
      const labelsRaw = fd.get('labels_raw') || '';
      for (const pair of labelsRaw.split(',')) {
        const trimmed = pair.trim();
        if (!trimmed) continue;
        const eq = trimmed.indexOf('=');
        if (eq > 0) {
          labels[trimmed.substring(0, eq)] = trimmed.substring(eq + 1);
        }
      }

      const body = {
        host: fd.get('host'),
        ssh_port: parseInt(fd.get('ssh_port') || '22'),
        ssh_user: fd.get('ssh_user'),
        ssh_key_name: fd.get('ssh_key_name'),
        worker_name: fd.get('worker_name'),
        labels,
        orchestrator_url: fd.get('orchestrator_url'),
        bootstrap_token: fd.get('bootstrap_token') || null,
        fleet_worker_bin: fd.get('fleet_worker_bin') || '',
        dry_run: fd.get('dry_run') === 'on',
      };

      const btn = document.getElementById('submit-btn');
      const result = document.getElementById('provision-result');
      btn.disabled = true;
      btn.textContent = 'Provisioning…';
      result.style.display = 'none';

      try {
        const resp = await fetch('/api/hosts/provision', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrf() },
          body: JSON.stringify(body)
        });
        const data = await resp.json();

        if (resp.ok) {
          let html = '<div style="padding:12px;border-radius:6px;background:rgba(26,125,49,0.1);border:1px solid #1a7d31;">';
          html += '<strong style="color:#1a7d31;">✓ Provisioning ' + (data.succeeded ? 'succeeded' : 'completed with errors') + '</strong>';
          if (data.steps) {
            html += '<ul style="margin:8px 0 0;padding-left:20px;font-size:13px;">';
            for (const s of data.steps) {
              const icon = s.status === 'applied' ? '✓' : s.status === 'skipped' ? '○' : '✗';
              const color = s.status === 'failed' ? '#c61e00' : '#666';
              html += '<li style="color:' + color + ';">' + icon + ' ' + s.name + '</li>';
            }
            html += '</ul>';
          }
          html += '</div>';
          result.innerHTML = html;
        } else {
          result.innerHTML = '<div style="padding:12px;border-radius:6px;background:rgba(198,30,0,0.1);border:1px solid #c61e00;color:#c61e00;"><strong>✗ Failed:</strong> ' + (data.error || 'Unknown error') + '</div>';
        }
        result.style.display = 'block';
      } catch(e) {
        result.innerHTML = '<div style="padding:12px;color:#c61e00;">Error: ' + e.message + '</div>';
        result.style.display = 'block';
      } finally {
        btn.disabled = false;
        btn.textContent = 'Provision Host';
      }
    });

    // 사용자 메뉴는 app.js의 renderUserMenu()가 #sidebar-user-menu에 렌더링한다.
    loadSshKeys();

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('/api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
  
