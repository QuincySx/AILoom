import { esc } from '../services/api.js';
import { icon } from './icons.js';

export const brandMark = `<svg viewBox="0 0 28 28" aria-hidden="true" class="brand-mark"><rect x="2" y="3" width="6" height="22" rx="2" fill="currentColor"/><rect x="20" y="3" width="6" height="22" rx="2" fill="currentColor"/><path class="brand-weft" d="m6 16 16-10v9L6 25z"/></svg>`;

const NAV_ICONS={projects:'grid',library:'library',global:'globe',tasks:'history',help:'help'};
const navIcon=key=>icon(NAV_ICONS[key]);
export function AppHeader(container, { route }) {
  const active = path => path==='#/library' ? route===path||route==='#/library/rules' : route===path||route.startsWith(path+'/');
  const link = (path,label,key) => `<a href="${path}" class="item-surface" ${active(path)?'aria-current="page"':''}>${navIcon(key)}<span>${label}</span></a>`;
  container.className = 'sidebar app-header';
  container.innerHTML = `<button class="app-menu-toggle" aria-label="打开主导航" aria-expanded="false" aria-controls="app-navigation">${icon('menu')}</button><a class="brand" href="#/projects" aria-label="AILoom 项目首页">${brandMark}<span class="brand-name">AILoom</span></a>
    <div class="app-navigation" id="app-navigation"><small class="app-nav-label">工作空间</small><div class="app-nav-primary">${link('#/projects','项目','projects')}${link('#/library','资源库','library')}${link('#/library/global','全局技能','global')}${link('#/tasks','操作记录','tasks')}</div>
    <div class="app-nav-utility">${link('#/onboarding','使用指南','help')}<button data-open-service><span class="service-indicator" aria-hidden="true"></span>本地工作空间</button></div></div><a href="#/onboarding" class="app-mobile-help" aria-label="使用指南">${icon('help')}</a>`;
  const toggle=container.querySelector('.app-menu-toggle');
  const close=()=>{container.classList.remove('menu-open');toggle.setAttribute('aria-expanded','false');toggle.setAttribute('aria-label','打开主导航');};
  toggle.onclick=()=>{const open=container.classList.toggle('menu-open');toggle.setAttribute('aria-expanded',String(open));toggle.setAttribute('aria-label',open?'关闭主导航':'打开主导航');if(open)container.querySelector('.app-nav-primary a')?.focus();};
  container.onkeydown=e=>{if(e.key==='Escape'){close();toggle.focus();}};
  container.querySelectorAll('a,[data-open-service]').forEach(link=>link.addEventListener('click',close));
}

// actions is escaped, application-owned button/link markup, never remote content.
export function PageHeader({ title, description = '', actions = '', size = 'default' }) {
  return `<header class="page-head" data-size="${size}"><div><h1>${esc(title)}</h1>${description ? `<p>${esc(description)}</p>` : ''}</div>${actions ? `<div class="actions">${actions}</div>` : ''}</header>`;
}

export function setBreadcrumb(items) {
  const bar = document.querySelector('#page-breadcrumb');
  if (!bar) return;
  bar.innerHTML = `<nav aria-label="当前位置">${items.map((item,index)=>`${index?'<span aria-hidden="true">/</span>':''}${item.href?`<a href="${esc(item.href)}">${esc(item.label)}</a>`:`<span ${index===items.length-1?'aria-current="page"':''}>${esc(item.label)}</span>`}`).join('')}</nav><a href="#/onboarding" class="breadcrumb-help" aria-label="使用指南">${icon('help')}</a>`;
}
