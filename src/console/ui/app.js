// AIL-087：应用壳 —— bootstrap、hash 路由、TargetBar、连接状态、跨页恢复。
// 切页销毁页面监听/轮询；服务端任务不受页面销毁影响。

import { api, esc } from './services/api.js';
import { loadDraft, draft } from './state/draft.js';
import { currentTarget, setTarget } from './state/target.js';
import { notify, subscribe } from './state/store.js';
import { confirmAction } from './components/dialog.js';
import { installSelects } from './components/select.js';

import * as pageOverview from './pages/overview.js';
import * as pageSamples from './pages/samples.js';
import * as pageOnboarding from './pages/onboarding.js';
import * as pageScopes from './pages/scopes.js';
import * as pageLibrary from './pages/library.js';
import * as pageSources from './pages/sources.js';
import * as pageWorkflows from './pages/workflows.js';
import * as pageTasks from './pages/tasks.js';
import * as pageInstructions from './pages/instructions.js';
import * as pageProjects from './pages/projects.js';

const ROUTES = {
  '#/projects': { title: '项目', mount: pageProjects.mount },
  '#/overview': { title: '总览', mount: pageOverview.mount, hidden: true },
  '#/samples': { title: '组件样例', mount: pageSamples.mount, hidden: true },
  '#/onboarding': { title: '首次设置', mount: pageOnboarding.mount, hidden: true },
  '#/scopes': { title: '仓库与作用域', mount: pageScopes.mount, hidden: true },
  '#/library': { title: '全局资源中心', mount: pageLibrary.mount },
  '#/sources': { title: '更新中心', mount: pageSources.mount, hidden: true },
  '#/workflows': { title: '流程工作台', mount: pageWorkflows.mount },
  '#/tasks': { title: '任务', mount: pageTasks.mount },
  '#/instructions': { title: '个人指令', mount: pageInstructions.mount, hidden: true },
};

let currentPage = null;
let activeRoute = null;
let routeVersion = 0;

function shell() {
  const nav = document.createElement('nav');
  nav.className = 'sidebar';
  nav.setAttribute('aria-label', '主导航');
  nav.id = 'nav';
  const bar = document.createElement('div');
  bar.id = 'targetBar';
  bar.className = 'muted';
  const app = document.createElement('main');
  app.id = 'app';
  document.body.append(nav, bar, app);
  installSelects(app);
  renderNav();
  subscribe('target', renderTargetBar);
  setInterval(checkConnection, 10000);
}

function renderNav() {
  const nav = document.querySelector('#nav');
  if (!nav) return;
  nav.innerHTML = '<div class="brand">AILoom<span>你的 AI 资源工作台</span></div><div class="nav-label">工作空间</div>';
  for (const [route, def] of Object.entries(ROUTES)) {
    if (def.hidden) continue;
    const b = document.createElement('button');
    b.textContent = def.title;
    if (location.hash === route || (route === '#/projects' && location.hash.startsWith('#/projects/'))) { b.className = 'on'; b.setAttribute('aria-current', 'page'); }
    b.onclick = () => { location.hash = route; };
    nav.appendChild(b);
  }
  nav.insertAdjacentHTML('beforeend', '<div class="sidebar-foot">本地运行 · 仅本机可访问<br>资源由你选择，项目由你确认。</div>');
}

function renderTargetBar() {
  const bar = document.querySelector('#targetBar');
  if (!bar) return;
  const t = currentTarget();
  bar.textContent = t
    ? `当前项目：${t.name || t.repo_id} · ${t.path}`
    : '全局工作空间 · 项目配置与资源管理相互独立';
}

async function checkConnection() {
  try {
    await api.serverInfo();
  } catch {
    // 断线：仅状态提示，不自动 POST；草稿/任务本地保留
    notify('本地服务连接中断；草稿与任务记录已本地保留，恢复后可继续');
  }
}

async function route() {
  const version = ++routeVersion;
  const projectMatch = location.hash.match(/^#\/projects\/([^/]+)$/);
  if (!projectMatch && (!location.hash || !ROUTES[location.hash])) { location.replace('#/projects'); return; }
  if (activeRoute === location.hash) return;
  if (currentPage?.isDirty?.()) {
    const proceed = await confirmAction('当前页面有未保存内容。确定放弃这些修改并离开？', {title:'离开当前页面', confirmLabel:'放弃并离开'});
    if (version !== routeVersion) return;
    if (!proceed) { history.replaceState(null, '', activeRoute); return; }
  }
  const def = projectMatch ? {title:'项目配置', mount:pageProjects.mount} : ROUTES[location.hash];
  document.title = `AILoom 本地控制台 · ${def.title}`;
  const app = document.querySelector('#app');
  currentPage?.destroy?.();
  currentPage = null;
  app.innerHTML = '';
  currentPage = def.mount(app, projectMatch ? {projectId:decodeURIComponent(projectMatch[1])} : {});
  activeRoute = location.hash;
  renderNav();
}

window.addEventListener('hashchange', route);
window.addEventListener('beforeunload', (event) => {
  if (currentPage?.isDirty?.()) { event.preventDefault(); event.returnValue = ''; }
});

(async function init() {
  shell();
  await loadDraft();
  const d = draft();
  if (d.target) setTarget(d.target);
  else if (d.repo) setTarget({ repo_id: d.repo.repo_id, path: d.repo.current_worktree || d.repo.root, kind: d.repo.kind || 'git' });
  route();
  renderTargetBar();
})();
