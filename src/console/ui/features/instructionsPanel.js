// AIL-097/106：InstructionsPanel —— 个人指令（补充偏好 / 基于公司基线的替代视图）。
// 只读基线展示、个人 patch 编辑、预览应用走 plan/apply、宿主语义需新会话验证。
// 保存范围为项目默认层（所有 Worktree 继承）；按 Worktree 的指令差异控制台暂未支持（接口缺口，不造假）。

import { api, esc } from '../services/api.js';
import { Field } from '../components/field.js';
import { confirmAction } from '../components/dialog.js';

export function InstructionsPanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>项目说明</h2>
    <p class="muted" data-scope-line>读取项目信息…</p>

    <p data-editor></p>
    <p><button data-save>保存说明</button>
       <button data-clear>清除我的说明…</button>
       <span data-msg class="muted"></span></p>
    <details><summary>查看团队说明</summary><div data-baseline class="muted"></div></details>`;
  const editorSlot = wrap.querySelector('[data-editor]');
  const msg = wrap.querySelector('[data-msg]');
  const baseline = wrap.querySelector('[data-baseline]');
  const scopeLine = wrap.querySelector('[data-scope-line]');
  const field = Field(editorSlot, {
    label: '个人补充（Markdown）',
    multi: true,
    placeholder: '- 个人：回答用中文',
  });
  let cur = props;
  let saved = '';
  let disposed = false;
  let busy = true;
  let baselineText = '';
  const saveButton = wrap.querySelector('[data-save]');
  const clearButton = wrap.querySelector('[data-clear]');
  saveButton.disabled = clearButton.disabled = true;
  if (cur.target?.path) {
    scopeLine.textContent = `告诉 AI 这个项目的背景、约定和要求。适用于整个项目。`;
  }
  // 只读展示上游基线，便于确认“恢复继承”将回到的内容；绝不修改该文件。
  (async () => {
    const root = cur.target?.path;
    if (!root || disposed) return;
    for (const name of ['AGENTS.md', 'CLAUDE.md']) {
      try {
        const v = await api.fsRead(`${root}/${name}`);
        if (disposed) return;
        if (v?.content) {
          baselineText = `文件 ${name}（只读基线）：\n${v.content.slice(0, 1200)}`;
          baseline.textContent = baselineText;
          return;
        }
      } catch { /* 无基线文件是正常情况 */ }
    }
  })();

  api.instructions({root:cur.target?.path, read:true}).then(result => {
    if (disposed) return;
    saved = result.content;
    field.setValue(saved);
    busy = false;
    saveButton.disabled = clearButton.disabled = false;
  }).catch(e => { busy = false; if (!disposed) msg.textContent = '读取失败：' + e.message; });

  wrap.querySelector('[data-save]').onclick = async () => {
    if (busy) return;
    const content = field.value();
    busy = true;
    saveButton.disabled = clearButton.disabled = true;
    try {
      await api.instructions({root:cur.target?.path, content});
      saved = content;
      msg.textContent = '已保存。到项目中应用后，新开 AI 会话使用。';
      cur.onChanged?.();
    } catch (e) {
      msg.textContent = '失败：' + e.message;
    } finally {
      busy = false;
      saveButton.disabled = clearButton.disabled = false;
    }
  };
  wrap.querySelector('[data-clear]').onclick = async () => {
    const excerpt = baselineText ? `\n\n清除后将回到公司基线（摘要）：\n${baselineText.slice(0, 200)}` : '\n\n清除后将回到公司基线（当前项目未读到基线文件）。';
    if (busy || !await confirmAction('清除你添加的项目说明？团队说明会保留。' + excerpt, {title:'清除我的说明', confirmLabel:'清除', destructive:true})) return;
    busy = true;
    saveButton.disabled = clearButton.disabled = true;
    try {
      await api.instructions({root:cur.target?.path, clear:true});
      saved = '';
      field.setValue('');
      msg.textContent = '已清除。到项目中应用改动。';
      cur.onChanged?.();
    } catch (e) {
      msg.textContent = '失败：' + e.message;
    } finally {
      busy = false;
      saveButton.disabled = clearButton.disabled = false;
    }
  };

  function render(p) { cur = p; }
  render(props);
  return {
    isDirty: () => busy || field.value() !== saved,
    update(next) { render({ ...cur, ...next }); },
    setBaseline(text) {
      baselineText = text || '';
      baseline.textContent = text
        ? `团队说明：\n${text.slice(0, 2000)}`
        : '';
    },
    setContent(v) { field.setValue(v); },
    destroy() { disposed = true; wrap.remove(); },
  };
}
