// AIL-083：StatusBadge —— 领域化状态展示；部署不等于调用成功（文案映射统一在此）。


const MAPS = {
  worktree: {
    active: ['ok', 'active'],
    missing: ['bad', 'missing'],
    bare: ['warn', 'bare'],
    detached: ['warn', 'detached'],
  },
  host: {
    'needs-new-session': ['warn', '需新会话'],
    'needs-approval': ['warn', '需在 AI 工具中批准'],
    'host-unverified': ['bad', '未确认能加载'],
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
    el.textContent = p.label ?? label;
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { el.remove(); },
  };
}
