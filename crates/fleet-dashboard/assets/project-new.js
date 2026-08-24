    function getCsrf() {
      const m = document.cookie.match('(^|;)\\s*fleet_csrf\\s*=\\s*([^;]+)');
      return m ? m.pop() : '';
    }

    document.getElementById('project-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const name = form.querySelector('[name=name]').value.trim();
      const description = form.querySelector('[name=description]').value.trim();

      const status = document.getElementById('submit-status');
      const submitBtn = form.querySelector('button[type="submit"]');
      status.textContent = 'Creating…';
      status.style.color = 'var(--ink-muted-48)';
      submitBtn.disabled = true;

      try {
        // project API는 JSON body를 받으므로 CSRF는 헤더 variant를 쓴다
        // (task 제출 폼의 form-field variant와 다름).
        const resp = await fetch('api/projects', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrf() },
          body: JSON.stringify(description ? { name, description } : { name }),
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
        setTimeout(() => { window.location.href = 'projects/' + encodeURIComponent(body.id); }, 600);
      } catch (e) {
        status.textContent = 'Error: ' + e.message;
        status.style.color = 'var(--badge-failed, #c0392b)';
      } finally {
        submitBtn.disabled = false;
      }
    });

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}
