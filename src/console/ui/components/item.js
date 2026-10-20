import { esc } from '../services/api.js';
import { icon } from './icons.js';

let menuSequence = 0;

export function MoreActions(label, actions, triggerText = '⋯') {
  const id = `item-actions-${++menuSequence}`;
  return `<button class="${triggerText==='⋯'?'item-menu-trigger':'action-menu-trigger'}" ${triggerText==='⋯'?'':'data-variant="default"'} popovertarget="${id}" aria-label="${esc(label)}">${esc(triggerText)}</button><div class="item-menu" id="${id}" popover>${actions}</div>`;
}

export function installItemMenus(container) {
  function position(menu) {
    const trigger=menu.previousElementSibling;
    const anchor=trigger.getBoundingClientRect();
    if(!trigger.getClientRects().length||anchor.bottom<=0||anchor.top>=window.innerHeight){menu.hidePopover();return;}
    const width=Math.min(240,window.innerWidth-32);
    menu.style.width=width+'px';
    const height=menu.getBoundingClientRect().height;
    menu.style.left=Math.max(16,Math.min(anchor.right-width,window.innerWidth-width-16))+'px';
    menu.style.top=Math.max(16,anchor.bottom+height+8<window.innerHeight?anchor.bottom+4:anchor.top-height-4)+'px';
  }
  const reposition=event=>{
    if(event?.target?.closest?.('.item-menu'))return;
    container.querySelectorAll('.item-menu:popover-open').forEach(position);
  };
  window.addEventListener('resize',reposition);
  window.addEventListener('scroll',reposition,true);
  container.addEventListener('toggle',event=>{
    if(event.target.matches?.('.item-menu')&&event.newState==='open')position(event.target);
  },true);
  container.addEventListener('click',event=>{
    const action=event.target.closest('.item-menu button,.item-menu a');
    if(action&&!action.disabled&&!action.matches('[role=combobox]')) {
      const menu=action.closest('.item-menu');
      menu.hidePopover();
      menu.previousElementSibling.focus();
    }
  },true);
}

// Show meaningful source locations, never the AILoom storage path or an ordinary skill root.
export function resourcePath(entry) {
  const source=entry.source_meta||(typeof entry.source==='object'&&entry.source)||{};
  let path=source.repo_path??entry.repo_path;
  if(path==null&&source.source_kind==='local')path=source.discovery_entry?.replace(/^local-dir:/,'');
  if(path==null&&entry.readonly)path=entry.path;
  if(!path)return '';
  path=String(path).replace(/\\/g,'/').replace(/\/SKILL\.md$/,'').replace(/^\.\//,'').replace(/\/$/,'');
  const name=entry.name||path.split('/').pop();
  if(path==='.'||path===name||['skills/','resources/skills/','.agents/skills/','.claude/skills/'].some(root=>path===root+name))return '';
  const ordinaryHostRoot=/^(?:~|\/(?:Users|home)\/[^/]+|\/root|[a-z]:\/Users\/[^/]+)\/\.(?:agents|claude|codex|cc-switch)\/skills\/[^/]+$/i;
  if(ordinaryHostRoot.test(path)||/\/ailoom\/(?:store|cache|library)\//.test(path))return '';
  return path;
}
export const resourceIcon = icon('file');

// All text is escaped here. leading/actions/feedback/titleMarkup are local UI slots.
// path may be a list: an install location followed by the real location it links to.
function pathMarkup(path) {
  const paths=[].concat(path||[]).filter(p=>p&&(p.text??p));
  // A part may be {text, title} when its tooltip says more than the path, e.g. the links it passes through.
  const one=p=>{const text=p.text??p,title=p.title??text;return `<small class="ui-item-path" title="${esc(title)}"><bdi>${esc(text)}</bdi></small>`;};
  if(paths.length<2)return paths.map(one).join('');
  return `<span class="ui-item-paths">${paths.map(one).join('<span class="ui-item-arrow" aria-label="指向">→</span>')}</span>`;
}

export function Item({ title, description = '', meta = '', path = '', leading = '', actions = '', feedback = '', titleMarkup = '', className = '', attributes = '', density = 'resource', tag = 'article', primaryAction = null, trailing = '' }) {
  // A primary action is one real link/button, spanning the content surface.
  // Secondary controls are siblings, never nested inside it or covered by an overlay.
  const content = `${leading ? `<span class="ui-item-leading">${leading}</span>` : ''}<span class="ui-item-content"><span class="ui-item-title">${titleMarkup || esc(title)}</span>${description ? `<span class="ui-item-description">${esc(description)}</span>` : ''}${pathMarkup(path)}${meta ? `<small class="ui-item-meta">${esc(meta)}</small>` : ''}</span>`;
  const primaryTag=primaryAction?.tag==='label'?'label':primaryAction?.href?'a':'button';
  const main = primaryAction
    ? `<${primaryTag} class="ui-item-primary item-surface" ${primaryAction.href ? `href="${esc(primaryAction.href)}"` : primaryTag==='button'?'type="button"':''} ${primaryAction.attributes || ''} ${primaryAction.disabled ? 'disabled' : ''} aria-label="${esc(primaryAction.label || title)}">${content}${trailing ? `<span class="ui-item-trailing">${trailing}</span>` : ''}${primaryTag==='label'?'':icon('chevron-right','icon ui-item-chevron')}</${primaryTag}>`
    : content;
  return `<${tag} class="ui-item ${density === 'regular' ? 'ui-item-regular' : ''} ${primaryAction ? 'ui-item-interactive' : ''} ${tag === 'label' ? 'ui-item-selectable item-surface' : ''} ${className}" ${attributes}>
    <div class="ui-item-main">${main}${actions ? `<div class="ui-item-actions">${actions}</div>` : ''}</div>${feedback ? `<div class="ui-item-feedback">${feedback}</div>` : ''}</${tag}>`;
}

export function paginate(items, requestedPage = 1, pageSize = 10) {
  const total = items.length, pageCount = Math.max(1, Math.ceil(total / pageSize));
  const page = Math.max(1, Math.min(pageCount, requestedPage));
  const start = (page - 1) * pageSize;
  return { items: items.slice(start, start + pageSize), total, page, pageCount, from: total ? start + 1 : 0, to: Math.min(total, start + pageSize) };
}

export function Pagination({ total, page, pageCount, from, to }, disabled = false) {
  if (!total || pageCount <= 1) return '';
  return `<nav class="pagination" aria-label="列表分页"><span>${from}–${to} / ${total}</span><div><span aria-live="polite" aria-atomic="true">${page} / ${pageCount}</span><button data-page="${page - 1}" ${disabled || page <= 1 ? 'disabled' : ''}>上一页</button><button data-page="${page + 1}" ${disabled || page >= pageCount ? 'disabled' : ''}>下一页</button></div></nav>`;
}

export function bindPagination(container, change) {
  container.querySelectorAll('[data-page]').forEach(button => {
    button.onclick = () => {
      if(button.disabled)return;
      const index=[...container.querySelectorAll('[data-page]')].indexOf(button);
      change(Number(button.dataset.page));
      const buttons=[...container.querySelectorAll('[data-page]')];
      (buttons[index]&&!buttons[index].disabled?buttons[index]:buttons.find(b=>!b.disabled))?.focus();
    };
  });
}
