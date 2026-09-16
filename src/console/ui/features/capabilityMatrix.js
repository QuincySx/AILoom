// AIL-089：CapabilityMatrix —— 宿主×资源三态编辑与生效状态、支持级别展示。
// 单项三态 patch（显式 enable/disable/inherit），保存后以服务端 effective 回显。

import { api, esc } from '../services/api.js';
import { TriStateSelect } from '../components/triState.js';
import * as store from '../state/store.js';

export function CapabilityMatrix(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  let cur = props;
  const controls = new Map();

  wrap.innerHTML = `
    <h2>宿主能力（三态：继承 / 启用 / 禁用）</h2>
    <p data-hosts></p>
    <p class="muted">个人层只影响 AILoom 的部署期望；宿主从全局/祖先目录加载的能力不受本工具控制——
    界面区分「本工具未部署」与「宿主已禁用」。实际生效见能力矩阵与「需新会话」标记。</p>
    <div data-caps></div>
    <p><span data-msg class="muted">已保存 ≠ 已部署；部署走预览+应用。</span></p>`;

  const hostsSlot = wrap.querySelector('[data-hosts]');
  const capsSlot = wrap.querySelector('[data-caps]');

  async function refresh() {
    const target = cur.target;
    if (!target?.path) { hostsSlot.innerHTML = '<span class="muted">先选择操作目标</span>'; return; }
    const gen = cur.targetGen;
    try {
      const eff = await api.effective(target.path);
      if (!cur.accept(gen)) return;
      store.set('effective', eff);
      renderHosts(eff.hosts ?? {});
    } catch (e) {
      hostsSlot.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
    try {
      const caps = await api.capabilities();
      renderCaps(caps.capabilities ?? []);
    } catch { capsSlot.innerHTML = ''; }
  }

  function originLabel(o) {
    if (!o) return '未设置';
    if (typeof o === 'string') {
      return { team_declaration: '团队声明', repo_default: '个人仓库默认', worktree_override: '工作树覆盖' }[o] ?? o;
    }
    const k = Object.keys(o)[0] ?? '';
    const inner = o[k] ?? {};
    return k + (inner.path ? `（${inner.path}）` : '');
  }

  function renderHosts(hosts) {
    hostsSlot.innerHTML = '';
    controls.forEach((c) => c.destroy());
    controls.clear();
    const table = document.createElement('table');
    table.innerHTML = '<tr><th>宿主</th><th>本层选择</th><th>生效（服务端）</th></tr>';
    for (const [host, h] of Object.entries(hosts)) {
      const tr = document.createElement('tr');
      const tdName = document.createElement('td');
      tdName.textContent = host;
      const tdSel = document.createElement('td');
      const tdEff = document.createElement('td');
      tr.append(tdName, tdSel, tdEff);
      table.appendChild(tr);
      const currentLayer = h.selection ?? (h.enabled ? 'enable' : 'inherit');
      const ctl = TriStateSelect(tdSel, {
        value: currentLayer,
        effective: { enabled: h.enabled, originLabel: originLabel(h.origin) },
        onChange: (state) => {
          cur.onPatch?.({ host, state });
        },
      });
      controls.set(host, ctl);
    }
    hostsSlot.appendChild(table);
  }

  function renderCaps(caps) {
    const rows = caps.map((c) => `<tr>
      <td>${esc(c.tool)}</td><td>${esc(c.kind)}</td><td>${esc(c.scope)}</td>
      <td>${c.support === 'native' ? '<span class="badge ok">原生</span>'
          : c.support === 'generated' ? '<span class="badge">生成入口</span>'
          : c.support === 'unsupported' ? '<span class="badge bad">不支持</span>'
          : '<span class="badge warn">未知</span>'}</td>
      <td class="muted">${esc(c.load_mode)}</td>
      <td>${c.requires_new_session ? '<span class="badge warn">需新会话</span>' : '—'}</td>
    </tr>`).join('');
    capsSlot.innerHTML =
      `<table><tr><th>宿主</th><th>资源</th><th>作用域</th><th>支持级别</th><th>加载方式</th><th>生效时机</th></tr>${rows}</table>`;
  }

  function render(p) { cur = p; }
  render(props);
  refresh();
  return { update(next) { render({ ...cur, ...next }); }, refresh, destroy() { wrap.remove(); } };
}

