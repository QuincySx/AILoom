// AIL-090：ImportPreview / UpdatePanel —— 来源输入、导入预览（目录/GitHub/发现入口）、
// 上游检查与更新冲突。导入不执行脚本；检查不等于应用。

import { api, esc } from '../services/api.js';
import { Button } from '../components/button.js';
import { Field } from '../components/field.js';
import { notify } from '../state/store.js';

export function ImportPreview(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>导入 skill（预览不复制不执行；脚本仅复制、绝不运行）</h2>
    <p class="muted">三种来源：本地目录 · GitHub 仓库（--path 可选）· 发现入口（skills.sh/&lt;owner&gt;/&lt;repo&gt;/&lt;skill&gt;）</p>
    <p data-local></p>
    <p data-git></p>
    <div data-view></div>`;
  const target = props.target;
  const gen = props.targetGen;

  const localDir = Field(wrap.querySelector('[data-local]'), { label: '本地 skill 目录', placeholder: '/Users/me/my-skills/my-skill', width: '50%' });
  const view = wrap.querySelector('[data-view]');
  const gitSlot = wrap.querySelector('[data-git]');
  const gitInput = Field(gitSlot, { label: 'GitHub URL 或发现入口', placeholder: 'skills.sh/vercel-labs/skills/find-skills 或 https://github.com/o/r', width: '50%' });
  const pathInput = Field(gitSlot, { label: '仓库内子目录（可空）', placeholder: 'skills/alpha', width: '24%' });
  const refInput = Field(gitSlot, { label: 'ref（可空）', width: '16%' });

  const btns = document.createElement('p');
  gitSlot.appendChild(btns);
  const previewGitBtn = Button(btns, { label: '预览远程导入', onPress: () => doGit(false) });
  const execGitBtn = Button(btns, { label: '导入远程', onPress: () => doGit(true) });
  const previewLocalBtn = Button(btns, { label: '预览本地', onPress: () => doLocal(false) });
  const execLocalBtn = Button(btns, { label: '导入本地', onPress: () => doLocal(true) });

  function renderPreview(p) {
    view.innerHTML = `<p>技能 <b>${esc(p.skill_name)}</b>（${p.files?.length ?? 0} 文件）脚本：${esc((p.scripts ?? []).join(', ') || '无')}
      冲突：${esc((p.conflicts ?? []).join('; ') || '无')}
      ${p.resolved_commit ? `commit ${esc(p.resolved_commit.slice(0, 12))}` : ''}
      ${p.existing_source ? `<span class="badge warn">同名来自 ${esc(p.existing_source)}</span>` : ''}</p>`;
  }

  async function doLocal(execute) {
    view.innerHTML = '';
    try {
      const v = await api.libraryImport(localDir.value(), undefined, execute);
      if (execute) {
        view.innerHTML = `<p class="badge ok">已导入：${esc(v.report.skill_id)}（脚本未执行）</p>`;
        const t = props.target;
        if (t?.path) {
          await api.select({ resource: v.report.skill_id, state: 'enable', root: t.path });
          notify('已导入并在当前目标默认层启用（保存 ≠ 部署）');
        }
      } else {
        renderPreview(v.preview);
      }
      props.onImported?.();
    } catch (e) {
      view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
  }

  async function doGit(execute) {
    view.innerHTML = '';
    const input = gitInput.value().trim();
    if (!input) return;
    try {
      let v;
      if (/^(https?:\/\/|skills\.sh\/|skill\.sh\/|github)/.test(input) && !input.startsWith('/')) {
        v = await api.libraryImportGit(input, pathInput.value().trim() || undefined, refInput.value().trim() || undefined, undefined, execute);
        v = v.executed ? { executed: true, report: v.report } : { executed: false, preview: v.preview };
      } else {
        v = await api.libraryImportEntry(input, undefined, execute);
        v = v.result ?? v;
      }
      if (v.executed) {
        const report = v.report;
        view.innerHTML = `<p class="badge ok">已导入：${esc(report.skill_id)}（脚本未执行）</p>`;
        const t = props.target;
        if (t?.path) {
          await api.select({ resource: report.skill_id, state: 'enable', root: t.path });
          notify('已导入并在当前目标默认层启用（保存 ≠ 部署）');
        }
      } else {
        const p = v.preview ?? {};
        renderPreview(p);
        if (v.candidates) {
          view.innerHTML += `<p class="badge warn">多 skill 仓库：${esc((v.candidates ?? p.candidates ?? []).join('; '))}</p>`;
        }
        if (v.error) view.innerHTML += `<p class="muted">${esc(v.error)}</p>`;
      }
      props.onImported?.();
    } catch (e) {
      view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
  }

  function render(p) {}
  render(props);
  return { update(next) {}, refresh() {}, destroy() { wrap.remove(); } };
}

// ---------------------------------------------------------------------------
// AIL-090：UpdatePanel —— 上游检查、状态展示与受控更新
// ---------------------------------------------------------------------------
export function UpdatePanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `<h2>来源与更新</h2>
    <p class="muted">检查只比较不应用；本地修改遇上游更新必须显式处理，不盲目覆盖。</p>
    <p><input data-skill placeholder="skill 名（可空=全部）" style="width:30%">
    <button data-check>检查更新</button>
    <button data-update disabled>应用更新</button></p>
    <div data-view class="muted"></div>`;
  const view = wrap.querySelector('[data-view]');
  const skillInput = wrap.querySelector('[data-skill]');
  const checkBtn = wrap.querySelector('[data-check]');
  const updateBtn = wrap.querySelector('[data-update]');
  let lastState = null;

  checkBtn.onclick = async () => {
    view.textContent = '检查中…';
    try {
      const skill = skillInput.value.trim();
      if (skill) {
        const v = await api.checkUpdate(skill);
        renderStatus(v.status);
      } else {
        const v = await api.librarySources();
        const items = (v.items ?? []).filter((i) => !i.legacy);
        if (!items.length) { view.innerHTML = '<span class="muted">没有可检查上游的 skill（全部为本地/未知来源）</span>'; return; }
        view.innerHTML = '<table><tr><th>skill</th><th>来源</th><th>commit</th></tr>' +
          items.map((i) => `<tr><td>${esc(i.skill)}</td><td>${esc(i.source?.repo_url ?? '')}</td><td>${esc((i.source?.resolved_commit ?? '').slice(0, 12))}</td></tr>`).join('') + '</table>';
      }
    } catch (e) {
      view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
  };

  function renderStatus(st) {
    lastState = st;
    const cls = { 'up-to-date': 'ok', 'upstream-new': 'warn', conflict: 'bad', 'local-modified': 'warn', 'upstream-missing': 'bad' }[st.state] ?? '';
    view.innerHTML = `<p><span class="badge ${cls}">${esc(st.state)}</span> ${esc(st.note ?? '')}
      ${st.upstream_commit ? ` · 上游 commit ${esc(st.upstream_commit.slice(0, 12))}` : ''}</p>`;
    updateBtn.disabled = !(st.state === 'upstream-new');
  }

  updateBtn.onclick = async () => {
    if (!lastState) return;
    try {
      const v = await api.updateSkill(lastState.skill, true);
      view.innerHTML = `<p class="badge ok">已更新 ${esc(v.result.skill)}；旧版已备份。部署需重新预览+应用。</p>`;
      props.onUpdated?.();
    } catch (e) {
      view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
    }
  };

  return { update() {}, destroy() { wrap.remove(); } };
}
