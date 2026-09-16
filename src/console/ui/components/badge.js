// AIL-083：StatusBadge —— 领域化状态展示；部署不等于调用成功（文案映射统一在此）。

import { esc } from '../services/api.js';

const MAPS = {
  worktree: {
    active: ['ok', 'active'],
    missing: ['bad', 'missing'],
    bare: ['warn', 'bare'],
    detached: ['warn', 'detached'],
  },
  host: {
    'needs-new-session': ['warn', '需新会话'],
    'needs-approval': ['warn', '需宿主批准'],
    'host-unverified': ['bad', '宿主未验证'],
    deployed: ['ok', '已部署'],
    missing: ['bad', '缺失'],
  },
  generic: {},
};

export function StatusBadge(container, props) {
  const el = document.createElement('span');
  container.appendChild(el);
  let cur = props;
  function render(p) {
    const map = MAPS[p.domain ?? 'generic'] ?? {};
    const [cls, label] = map[p.status] ?? (p.status ? ['', p.status] : ['', '']);
    el.className = `badge ${cls}`;
    el.textContent = p.label ?? esc(label);
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { el.remove(); },
  };
}
