    function getHostname() {
      const path = window.location.pathname;
      const parts = path.split('/');
      return decodeURIComponent(parts[parts.length - 1]);
    }

    function fmtTime(iso) {
      if (!iso) return '—';
      return new Date(iso).toLocaleString();
    }

    async function loadHost() {
      const hostname = getHostname();
      try {
        const resp = await fetch('api/hosts/' + encodeURIComponent(hostname));
        if (!resp.ok) throw new Error('not found');
        const h = await resp.json();

        document.getElementById('host-name').textContent = h.hostname;
        document.getElementById('host-status').textContent =
          'Status: ' + h.status + (h.worker_name ? ' • Worker: ' + h.worker_name : ' (unregistered)');

        const grid = document.getElementById('host-details');
        const details = [
          ['Status', h.status],
          ['Worker', h.worker_name || 'Not registered'],
          ['grok Version', h.grok_version || 'Not installed'],
          ['Worker Version', h.fleet_worker_version || '—'],
          ['SSH', (h.ssh_user||'')+'@'+(h.ssh_host||'')+':'+h.ssh_port],
          ['Last Heartbeat', fmtTime(h.last_heartbeat_at)],
          ['Provisioned', fmtTime(h.provisioned_at)],
        ];

        if (h.os_info) {
          details.push(['OS', h.os_info.os_type || '—']);
          details.push(['Distro', h.os_info.distro || '—']);
          details.push(['Kernel', h.os_info.kernel || '—']);
          details.push(['Arch', h.os_info.arch || '—']);
        }

        if (h.metrics) {
          if (h.metrics.load_avg && h.metrics.load_avg.length > 0) {
            details.push(['Load Avg', h.metrics.load_avg.map(v=>v.toFixed(2)).join(', ')]);
          }
          if (h.metrics.mem_available_mb) details.push(['Mem Available', h.metrics.mem_available_mb+' MB']);
          if (h.metrics.disk_free_mb) details.push(['Disk Free', h.metrics.disk_free_mb+' MB']);
        }

        grid.innerHTML = details.map(([label, value]) => `
          <div class="detail-item">
            <div class="label">${label}</div>
            <div class="value">${escapeHtml(String(value))}</div>
          </div>
        `).join('');

        // 이벤트 타임라인
        const timeline = document.getElementById('event-timeline');
        if (!h.events || h.events.length === 0) {
          timeline.innerHTML = '<li class="empty-state"><p>No events recorded.</p></li>';
        } else {
          timeline.innerHTML = h.events.map(e => `
            <li class="timeline-item">
              <div class="timeline-dot ${e.severity}"></div>
              <div class="timeline-content">
                <div class="event-type">${escapeHtml(e.event_type)}</div>
                <div class="event-time">${fmtTime(e.created_at)}</div>
                ${e.message ? '<div class="event-message">'+escapeHtml(e.message)+'</div>' : ''}
              </div>
            </li>
          `).join('');
        }
      } catch(e) {
        document.getElementById('host-name').textContent = 'Host not found';
      }
    }

    function escapeHtml(s) {
      const d = document.createElement('div');
      d.textContent = s;
      return d.innerHTML;
    }

    loadHost();

  
