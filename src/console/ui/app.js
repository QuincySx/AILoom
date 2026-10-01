import './components/pathText.js';
// AIL-087：应用壳 —— bootstrap、hash 路由、TargetBar、连接状态、跨页恢复。
// 切页销毁页面监听/轮询；服务端任务不受页面销毁影响。

import { api, esc } from './services/api.js';
import { loadDraft, draft } from './state/draft.js';
import { currentTarget, setTarget } from './state/target.js';
import { notify, subscribe } from './state/store.js';
import { Dialog, confirmAction } from './components/dialog.js';
import { installSelects } from './components/select.js';

import * as pageSamples from './pages/samples.js';
import * as pageOnboarding from './pages/onboarding.js';
import * as pageLibrary from './pages/library.js';
import * as pageTasks from './pages/tasks.js';
import * as pageProjects from './pages/projects.js';
import * as pageProjectSettings from './pages/projectSettings.js';
import * as pageNativeFiles from './pages/nativeFiles.js';
import * as pageWorkspace from './pages/workspace.js';

const ROUTES = {
  '#/projects': { title: '我的目录', mount: pageWorkspace.mount },
  '#/projects/manage': { title: '管理目录', mount: pageProjects.mount, hidden: true },
  '#/samples': { title: '组件样例', mount: pageSamples.mount, hidden: true },
  '#/onboarding': { title: '开始使用', mount: pageOnboarding.mount, hidden: true },
  '#/native-files': { title: '全局规则与 Agent', mount: pageNativeFiles.mount },
  '#/library': { title: '资源库', mount: pageLibrary.mount },
  '#/tasks': { title: '操作记录', mount: pageTasks.mount, secondary: true },
};

let currentPage = null;
let activeRoute = null;
let routeVersion = 0;
let serviceStopped = false;

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
  const isActive = (route) => location.hash === route || (route === '#/projects' && location.hash.startsWith('#/projects/'));
  const button = (route, def) => {
    const b = document.createElement('button');
    b.textContent = def.title;
    if (isActive(route)) { b.className = 'on'; b.setAttribute('aria-current', 'page'); }
    b.onclick = () => { location.hash = route; };
    return b;
  };
  nav.innerHTML = '<div class="brand">AILoom<span>你的 AI 资源工作台</span></div><div class="nav-label">工作空间</div>';
  for (const [route, def] of Object.entries(ROUTES)) {
    if (def.hidden || def.secondary) continue;
    nav.appendChild(button(route, def));
  }
  // 辅助入口：操作记录（配置操作留痕），与主工作区分组呈现
  const auxiliary = Object.entries(ROUTES).filter(([, def]) => def.secondary);
  if (auxiliary.length) {
    nav.insertAdjacentHTML('beforeend', '<div class="nav-label">历史</div>');
    for (const [route, def] of auxiliary) nav.appendChild(button(route, def));
  }
  nav.insertAdjacentHTML('beforeend', '<div class="sidebar-foot">本地运行 · 仅本机可访问<br>资源由你选择，项目由你确认。</div>');
  const service = document.createElement('button');
  service.className = 'service-menu-button';
  service.textContent = '服务';
  service.onclick = openService;
  nav.append(service);
}

async function openService() {
  const body = document.createElement('div');
  body.innerHTML = '<p role="status">正在读取…</p>';
  let busy = false, closed = false;
  const dialog = Dialog(document.body, {title:'网页服务', content:body, canClose:()=>!busy, onClose:()=>{closed=true;}});
  try {
    const state = await api.serviceStatus();
    if (closed) return;
    body.innerHTML = `<p>运行中 · 端口 ${esc(state.port)}</p>
      <label><input type="checkbox" data-autostart ${state.autostart.enabled?'checked':''} ${state.autostart.supported?'':'disabled'}> 登录时自动启动</label>
      <p class="muted">关闭网页后仍在后台运行，CLI 可独立使用。</p>
      <p data-service-message role="status"></p>
      <button class="danger" data-stop-service>停止服务</button>`;
    const toggle = body.querySelector('[data-autostart]');
    const stop = body.querySelector('[data-stop-service]');
    const message = body.querySelector('[data-service-message]');
    if (!state.autostart.supported) message.textContent = '此系统暂不支持登录自启动。';
    toggle.onchange = async () => {
      const enabled = toggle.checked;
      busy = true; toggle.disabled = true; stop.disabled = true;
      try {
        const result = await api.serviceAutostart(enabled);
        toggle.checked = result.enabled;
        message.textContent = enabled ? '已开启，下次登录生效。' : '已关闭，当前服务继续运行。';
      } catch(e) { toggle.checked = !enabled; message.textContent = e.message; }
      finally { busy=false; toggle.disabled=false; stop.disabled=false; }
    };
    stop.onclick = async () => {
      const text = currentPage?.isDirty?.()
        ? '有未保存的修改。放弃修改并停止网页服务？'
        : '停止后网页将不可用，CLI 仍可使用。';
      if (!await confirmAction(text, {title:'停止网页服务',confirmLabel:'停止服务',destructive:true})) return;
      busy=true; stop.disabled=true; toggle.disabled=true;
      try {
        await api.shutdown();
        serviceStopped=true;
        currentPage?.destroy?.(); currentPage=null;
        document.querySelector('#app').innerHTML='<p role="status">停止请求已提交，现有任务完成后退出。</p><p>重新打开：<code>ailoom web</code></p>';
        document.querySelectorAll('#nav button').forEach(b=>b.disabled=true);
        dialog.close();
      } catch(e) {
        message.textContent=e.message;stop.disabled=false;toggle.disabled=!state.autostart.supported;
      } finally {busy=false;}
    };
  } catch(e) { if(!closed) body.textContent=e.message; }
}

function renderTargetBar() {
  const bar = document.querySelector('#targetBar');
  if (!bar) return;
  const t = currentTarget();
  bar.textContent = t
    ? `当前项目：${t.name || t.repo_id}${t.viewKind === 'project-shared' ? ' · 共享设置' : t.resolvedPath ? ' · ' + t.resolvedPath : ' · ' + t.path}`
    : '全局工作空间 · 项目配置与资源管理相互独立';
}

async function checkConnection() {
  if (serviceStopped) return;
  try {
    await api.serverInfo();
  } catch {
    // 断线：仅状态提示，不自动 POST；草稿/任务本地保留
    notify('本地服务连接中断；草稿与任务记录已本地保留，恢复后可继续');
  }
}

async function route() {
  if (serviceStopped) return;
  const version = ++routeVersion;
  // 已下线的旧入口：保留重定向，旧书签不失效
  if (['#/scopes','#/overview','#/sources','#/workflows','#/instructions'].includes(location.hash)) { location.replace('#/projects'); return; }
  // AIL-124：项目内共享设置子路由（#/projects/<id>/settings | /instructions）
  const settingsMatch = location.hash.match(/^#\/projects\/([^/]+)\/(instructions|profile|knowledge)$/);
  const legacyMatch = location.hash.match(/^#\/projects\/([^/]+)\/(settings|advanced)$/);
  if (legacyMatch) { location.replace(`#/projects/${legacyMatch[1]}`); return; }
  const projectMatch = !ROUTES[location.hash] && !settingsMatch && location.hash.match(/^#\/projects\/([^/]+)$/);
  if (!projectMatch && !settingsMatch && (!location.hash || !ROUTES[location.hash])) { location.replace('#/projects'); return; }
  if (activeRoute === location.hash) return;
  if (currentPage?.isDirty?.()) {
    const proceed = await confirmAction('当前页面有未保存内容。确定放弃这些修改并离开？', {title:'离开当前页面', confirmLabel:'放弃并离开'});
    if (version !== routeVersion) return;
    if (!proceed) { history.replaceState(null, '', activeRoute); return; }
  }
  const def = settingsMatch ? {title:'项目设置', mount:pageProjectSettings.mount}
    : projectMatch ? {title:'目录能力', mount:pageWorkspace.mount} : ROUTES[location.hash];
  document.title = `AILoom 本地控制台 · ${def.title}`;
  const app = document.querySelector('#app');
  currentPage?.destroy?.();
  currentPage = null;
  app.innerHTML = '';
  app.classList.toggle('workspace-host', !!projectMatch || location.hash === '#/projects');
  if (settingsMatch) {
    currentPage = def.mount(app, {
      projectId: decodeURIComponent(settingsMatch[1]),
      tab: settingsMatch[2],
    });

  } else if (projectMatch) {
    currentPage = def.mount(app, {projectId:decodeURIComponent(projectMatch[1])});
  } else {
    currentPage = def.mount(app, {});
  }
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
