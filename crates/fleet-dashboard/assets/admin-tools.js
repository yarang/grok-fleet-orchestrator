    async function fetchTools() {
      try {
        const resp = await fetch('api/tools');
        const data = await resp.json();
        render(data.tools || []);
      } catch(e) { console.error('fetch tools:', e); }
    }

    function render(tools) {
      const grid = document.getElementById('tools-grid');
      grid.innerHTML = tools.map(t => `
        <div class="detail-item" style="cursor:default;">
          <div class="label" style="font-family:var(--font-mono);color:var(--primary);">${escapeHtml(t.name)}</div>
          <div class="value" style="font-size:14px;font-weight:400;">${escapeHtml(t.description)}</div>
        </div>
      `).join('');
    }


    fetchTools();

    // SSE
    const pill = document.getElementById('status-pill');
    try {
      const es = new EventSource('api/events/stream');
      es.onopen = () => { pill.textContent = 'live'; pill.classList.add('online'); };
      es.onerror = () => { pill.textContent = 'offline'; pill.classList.remove('online'); };
    } catch(e) {}

  
