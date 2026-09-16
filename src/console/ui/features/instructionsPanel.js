// AIL-097：InstructionsPanel —— 个人指令（补充偏好 / 基于公司基线的替代视图）。
// 只读基线展示、个人 patch 编辑、预览应用走 plan/apply、宿主语义需新会话验证。

import { api, esc } from '../services/api.js';
import { Field } from '../components/field.js';

export function InstructionsPanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>个人指令（Claude 追加式 / Codex 替代视图）</h2>
    <p class="muted">公司 AGENTS.md / CLAUDE.md 只读不改：Claude 写独立条目文件；Codex 生成包含公司基线全文的替代视图，
    基线变化后自动重新合成。宿主加载需新会话；无法屏蔽祖先/全局指令。</p>
    <p data-editor></p>
    <p><button data-save>保存个人指令（仅数据区）</button>
       <button data-clear>清除条目</button>
       <span data-msg class="muted"></span></p>
    <div data-baseline class="muted"></div>`;
  const editorSlot = wrap.querySelector('[data-editor]');
  const msg = wrap.querySelector('[data-msg]');
  const baseline = wrap.querySelector('[data-baseline]');
  const field = Field(editorSlot, {
    label: '个人补充（Markdown）',
    multi: true,
    placeholder: '- 个人：回答用中文',
  });
  let cur = props;

  wrap.querySelector('[data-save]').onclick = async () => {
    try {
      await api.instructions({ content: field.value() });
      msg.textContent = '已保存到机器数据区；应用走「预览 + 应用」（宿主新会话后生效）';
      cur.onChanged?.();
    } catch (e) {
      msg.textContent = '失败：' + e.message;
    }
  };
  wrap.querySelector('[data-clear]').onclick = async () => {
    try {
      await api.instructions({ clear: true });
      field.setValue('');
      msg.textContent = '已清除';
    } catch (e) {
      msg.textContent = '失败：' + e.message;
    }
  };

  function render(p) { cur = p; }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    setBaseline(text) {
      baseline.textContent = text
        ? `公司 AGENTS.md 当前内容（只读）：\n${text.slice(0, 2000)}`
        : '';
    },
    setContent(v) { field.setValue(v); },
    destroy() { wrap.remove(); },
  };
}
