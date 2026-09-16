// AIL-098：组件状态样例矩阵页（仅验收/开发用，fixture 数据驱动，不写任何生产状态）。
// 覆盖：正常/加载/空/失败/禁用/长内容 + 冲突/离线/过期/部分失败。
// 注意：生产页面禁止假数据回退——本页是唯一允许 fixture 的入口。

import { esc } from '../services/api.js';
import { Button } from '../components/button.js';
import { Field } from '../components/field.js';
import { TriStateSelect } from '../components/triState.js';
import { StatusBadge } from '../components/badge.js';
import { DataTable } from '../components/dataTable.js';
import { DiffView } from '../components/diffView.js';
import { ConflictPanel } from '../components/conflictPanel.js';

const HOST_STATES = ['needs-new-session', 'needs-approval', 'host-unverified', 'deployed', 'missing'];
const LONG = '长内容样例：'.repeat(40);
const mounted = [];
let root = null;

function section(title) {
  const el = document.createElement('div');
  el.className = 'step';
  const h = document.createElement('h2');
  h.textContent = title;
  el.appendChild(h);
  root.appendChild(el);
  return el;
}

function row(parent, label, mountFn) {
  const line = document.createElement('p');
  const tag = document.createElement('span');
  tag.className = 'badge';
  tag.textContent = label;
  const slot = document.createElement('span');
  slot.style.display = 'inline-block';
  slot.style.minWidth = '55%';
  line.append(tag, slot);
  parent.appendChild(line);
  mounted.push(mountFn(slot));
}

function plainTable(parent, label, props) {
  const tag = document.createElement('p');
  tag.innerHTML = `<span class="badge">${esc(label)}</span>`;
  parent.appendChild(tag);
  const slot = document.createElement('div');
  parent.appendChild(slot);
  mounted.push(DataTable(slot, props));
}

export function mount(container, ctx) {
  root = document.createElement('div');
  container.appendChild(root);

  // Button：正常/禁用/pending
  const sBtn = section('Button');
  row(sBtn, '正常', (slot) => Button(slot, { label: '正常按钮', onPress: () => {} }));
  row(sBtn, '禁用', (slot) => Button(slot, { label: '禁用按钮', disabled: true, onPress: () => {} }));
  row(sBtn, 'pending（点击后 2 秒）', (slot) =>
    Button(slot, {
      label: '点我进入 pending',
      pendingLabel: '请求中…',
      onPress: () => new Promise((r) => setTimeout(r, 2000)),
    }));

  // Field：正常/错误/hint/长内容/多行
  const sField = section('Field/Input');
  row(sField, '正常', (slot) => Field(slot, { label: '名称', value: '示例值' }));
  row(sField, '错误', (slot) => Field(slot, { label: '名称', value: '非法名', error: '名称包含非法字符（定位到字段）' }));
  row(sField, 'hint', (slot) => Field(slot, { label: '名称', hint: '资源 ID 形如 personal/skill/personal/xxx' }));
  row(sField, '长内容', (slot) => Field(slot, { label: '长值', value: LONG }));
  row(sField, '多行', (slot) => Field(slot, { label: '正文', multi: true, value: '# 标题\n\n正文内容' }));

  // TriStateSelect：三态 × effective + 禁用
  const sTri = section('TriStateSelect');
  row(sTri, 'inherit（生效：启用/团队声明）', (slot) =>
    TriStateSelect(slot, { value: 'inherit', effective: { enabled: true, originLabel: '团队声明' } }));
  row(sTri, 'enable（生效：启用/仓库默认）', (slot) =>
    TriStateSelect(slot, { value: 'enable', effective: { enabled: true, originLabel: '个人仓库默认' } }));
  row(sTri, 'disable（生效：停用/工作树覆盖）', (slot) =>
    TriStateSelect(slot, { value: 'disable', effective: { enabled: false, originLabel: '工作树覆盖' } }));
  row(sTri, '禁用控件', (slot) => TriStateSelect(slot, { value: 'enable', disabled: true }));

  // StatusBadge：宿主全状态 + 工作树状态
  const sBadge = section('StatusBadge');
  for (const st of HOST_STATES) {
    row(sBadge, st, (slot) => StatusBadge(slot, { domain: 'host', status: st }));
  }
  for (const st of ['active', 'missing', 'bare', 'detached']) {
    row(sBadge, `worktree:${st}`, (slot) => StatusBadge(slot, { domain: 'worktree', status: st }));
  }

  // DataTable：正常/空/加载/失败/长内容
  const sTable = section('DataTable');
  plainTable(sTable, '正常', {
    rows: [
      { id: 'personal/skill/personal/alpha', kind: 'skill', description: 'alpha 技能' },
      { id: 'personal/mcp/personal/probe', kind: 'mcp', description: '探针服务' },
    ],
    rowKey: (r) => r.id,
    columns: [
      { key: 'id', label: '资源 ID' },
      { key: 'kind', label: '类型' },
      { key: 'description', label: '说明' },
    ],
  });
  plainTable(sTable, '空', {
    rows: [],
    rowKey: (r) => r.id,
    columns: [{ key: 'id', label: 'ID' }],
    empty: '个人库为空',
  });
  plainTable(sTable, '加载', { loading: true });
  plainTable(sTable, '失败', { error: '无法连接本地服务（可能已退出）' });
  plainTable(sTable, '长内容', {
    rows: [{ id: 'x'.repeat(120), kind: 'skill', description: LONG }],
    rowKey: (r) => r.id,
    columns: [
      { key: 'id', label: '资源 ID' },
      { key: 'kind', label: '类型' },
      { key: 'description', label: '说明' },
    ],
  });

  // DiffView：有差异
  const sDiff = section('DiffView');
  const d1 = document.createElement('div');
  sDiff.appendChild(d1);
  mounted.push(DiffView(d1, {
    path: 'AGENTS.override.md',
    operation: 'update',
    before: '# 规范 v1\n- 旧条款\n',
    after: '# 规范 v2\n- 新条款A\n- 新条款B\n',
  }));

  // ConflictPanel：冲突（不自动覆盖）
  const sConflict = section('ConflictPanel');
  const slot = document.createElement('div');
  sConflict.appendChild(slot);
  mounted.push(ConflictPanel(slot, {
    title: '草稿已被其他会话修改',
    detail: 'revision 7 ≠ 本地 base 5。',
    path: 'profile.toml（个人配置）',
    local: 'hosts: claude=enable（本地修改）',
    server: 'hosts: claude=disable（服务器最新）',
    onReload: () => {},
    onMerge: () => {},
  }));

  // 计划/任务边界状态文案样例：过期/部分失败/离线/中断
  const sStates = section('计划/任务边界状态（文案样例）');
  sStates.insertAdjacentHTML(
    'beforeend',
    `<p><span class="badge bad">stale</span> 计划已过期（配置/源/目标已变化），旧计划拒绝应用</p>
     <p><span class="badge warn">partial</span> 撤销：恢复 2 项，冲突保留 1 项（应用后被用户修改，不覆盖）</p>
     <p><span class="badge bad">offline</span> 无法连接本地服务；草稿与任务记录已本地保留，恢复后可继续（不自动 POST）</p>
     <p><span class="badge warn">interrupted</span> 服务重启时任务中断；不自动重放，可重新发起计划/应用</p>`,
  );

  return {
    destroy() {
      mounted.forEach((m) => m?.destroy?.());
      mounted.length = 0;
      root.remove();
      root = null;
    },
  };
}
