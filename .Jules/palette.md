## 2024-03-24 - Empty states and ARIA live regions in dynamic dashboards
**Learning:** In dynamic, JS-driven dashboards (like the fleet orchestrator UI), tables that render only headers when empty create confusion, looking broken. Furthermore, dynamic connection status pills must be read by screen readers without user interaction.
**Action:** Always include empty states for lists/tables to guide users when data is absent. Make sure dynamic status elements use `role="status"` and `aria-live="polite"` so state changes are properly announced.
