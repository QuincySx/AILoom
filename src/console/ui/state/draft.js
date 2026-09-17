// AIL-086：草稿状态机 —— 保存 409 冲突显式呈现（保留本地 + 采用服务器/本地覆盖二选一），
// 其余保存错误 toast 反馈；不吞错、不静默覆盖。

import { api } from '../services/api.js';
import { get, set, notify } from './store.js';

export const DEFAULT_DRAFT = {
  step: 1,
  approvedRoot: null,
  repo: null,
  hosts: {},
  hostInfo: null,
  libSummary: null,
  capSaved: false,
  capEffective: null,
  planJob: null,
  planView: null,
  applyJob: null,
  applyView: null,
};

export async function loadDraft() {
  try {
    const d = await api.getDraft();
    set('draftRev', d.revision || 0);
    if (d.draft) set('draft', Object.assign({}, DEFAULT_DRAFT, d.draft));
    else set('draft', { ...DEFAULT_DRAFT });
  } catch (e) {
    set('draft', { ...DEFAULT_DRAFT });
    notify('草稿加载失败：' + e.message);
  }
}

export function draft() {
  return get('draft') ?? { ...DEFAULT_DRAFT };
}

export function patchDraft(patch) {
  set('draft', Object.assign({}, draft(), patch));
}

export async function saveDraft() {
  const rev = get('draftRev') ?? 0;
  try {
    const r = await api.putDraft(rev, draft());
    set('draftRev', r.revision);
    hideConflictBar();
    return true;
  } catch (e) {
    if (e.kind === 'conflict') {
      // U03：冲突显式呈现；本地草稿保留；用户显式选择
      window._serverRev = e.data.current_revision;
      window._serverDraft = e.data.draft;
      showConflictBar(e.data.current_revision);
    } else {
      notify('草稿保存失败：' + e.message);
    }
    return false;
  }
}

export async function resolveConflict(choice) {
  if (choice === 'server' && window._serverDraft) {
    set('draft', Object.assign({}, DEFAULT_DRAFT, window._serverDraft));
  }
  set('draftRev', window._serverRev ?? 0);
  await saveDraft();
  window.dispatchEvent(new CustomEvent('draft:changed'));
}

function showConflictBar(serverRev) {
  const bar = document.querySelector('#conflictBar');
  if (!bar) return;
  bar.innerHTML =
    `草稿已被其他会话修改（revision ${serverRev}）。你的本地草稿已保留： ` +
    `<button data-act="server">采用服务器草稿</button>` +
    ` <button data-act="local">用本地覆盖</button>`;
  bar.style.display = 'block';
  bar.querySelectorAll('button').forEach((b) => {
    b.onclick = () => resolveConflict(b.dataset.act);
  });
}

function hideConflictBar() {
  const bar = document.querySelector('#conflictBar');
  if (bar) bar.style.display = 'none';
}
