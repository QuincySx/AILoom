// AIL-087：应用壳 —— bootstrap、hash 路由、TargetBar、连接状态、跨页恢复。
// 切页销毁页面监听/轮询；服务端任务不受页面销毁影响。

import { api, esc } from './services/api.js';
import { loadDraft } from './state/draft.js';
import { currentTarget } from './state/target.js';
import { notify } from './state/store.js';

import * as pageOverview from './pages/overview.js';
import * as pageSamples from './pages/samples.js';
import * as pageOnboarding from './pages/onboarding.js';
import * as pageScopes from './pages/scopes.js';
import * as pageLibrary from './pages/library.js';
import * as pageSources from './pages/sources.js';
import * as pageWorkflows from './pages/workflows.js';
import * as pageTasks from './pages/tasks.js';
import * as pageInstructions from './pages/instructions.js';

const ROUTES = {
  '#/overview': { title: '总览', mount: pageOverview.mount },
  '#/samples': { title: '组件样例', mount: pageSamples.mount },
  '#/onboarding': { title: '首次设置', mount: pageOnboarding.mount },
  '#/scopes': { title: '仓库与作用域', mount: pageScopes.mount },
  '#/library': { title: '资源库', mount: pageLibrary.mount },
  '#/sources': { title: '来源与更新', mount: pageSources.mount },
  '#/workflows': { title: '流程工作台', mount: pageWorkflows.mount },
  '#/tasks': { title: '任务', mount: pageTasks.mount },
  '#/instructions': { title: '个人指令', mount: pageInstructions.mount },
};

let currentPage = null;

function shell() {
  const nav = document.createElement('div');
  nav.className = 'nav';
  nav.id = 'nav';
  const bar = document.createElement('div');
  bar.id = 'targetBar';
  bar.className = 'muted';
  const app = document.createElement('div');
  app.id = 'app';
  document.body.append(nav, bar, app);
  renderNav();
  setInterval(renderTargetBar, 2000);
  setInterval(checkConnection, 10000);
}

function renderNav() {
  const nav = document.querySelector('#nav');
  if (!nav) return;
  nav.innerHTML = '';
  for (const [route, def] of Object.entries(ROUTES)) {
    const b = document.createElement('button');
    b.textContent = def.title;
    if (location.hash === route) b.className = 'on';
    b.onclick = () => { location.hash = route; };
    nav.appendChild(b);
  }
}

function renderTargetBar() {
  const bar = document.querySelector('#targetBar');
  if (!bar) return;
  const t = currentTarget();
  bar.textContent = t
    ? `操作目标：${t.repo_id} @ ${t.path}${t.wt_id ? `（工作树 ${t.wt_id}）` : ''}`
    : '操作目标：未选择（在「首次设置」或「仓库与作用域」选择）';
}

async function checkConnection() {
  try {
    await api.serverInfo();
  } catch {
    // 断线：仅状态提示，不自动 POST；草稿/任务本地保留
    notify('本地服务连接中断；草稿与任务记录已本地保留，恢复后可继续');
  }
}

function route() {
  if (!location.hash || !ROUTES[location.hash]) location.hash = '#/overview';
  const def = ROUTES[location.hash];
  document.title = `AILoom 本地控制台 · ${def.title}`;
  const app = document.querySelector('#app');
  currentPage?.destroy?.();
  currentPage = null;
  app.innerHTML = '';
  currentPage = def.mount(app, {});
  renderNav();
}

window.addEventListener('hashchange', route);

(async function init() {
  shell();
  await loadDraft();
  route();
})();
