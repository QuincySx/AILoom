// 2026-09-21 UX 成熟度第 1 轮：旧六步向导（批准目录/Worktree/逐条保存）与「新建项目」
// 主流程两套概念并存，且内部术语直出。改为单一三步引导页：讲清链路 + 直达入口，
// 不再承载配置动作本身（登记走项目列表、入库走资源库、生效走项目页）。

import { api } from '../services/api.js';
import { PageHeader } from '../components/layout.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);

  async function render() {
    let repos = [];
    try { repos = (await api.state()).repos || []; } catch { /* 引导页读不到状态也照常展示 */ }
    const hasRepos = repos.length > 0;

    // Three numbered rows, each with its own way in; no cards, no brand banner.
    const steps = [
      ['添加项目', '选择本地文件夹或 Git 仓库。', '#/projects', '添加项目'],
      ['导入能力', '从 GitHub、skills.sh、本地文件夹或 CC Switch 导入技能、规则和子代理。', '#/library', '打开资源库'],
      ['添加并应用', '为目录选择工具和能力，预览改动后应用，再新开 AI 会话。', '#/projects', '打开项目'],
    ];
    root.innerHTML = `
      ${PageHeader({title:'开始使用',description:'三个步骤，让 AI 理解你的工作。'})}
      <ol class="wizard onboarding-steps">${steps.map(([title, text, go, label], index) => `<li class="step ${(hasRepos ? 2 : 0) === index ? 'active' : ''}"><span class="onboarding-number" aria-hidden="true">${index + 1}</span><span class="onboarding-copy"><strong>${title}</strong><span>${text}</span></span><button data-go="${go}" ${(hasRepos ? 2 : 0) === index ? 'data-variant="default"' : 'data-variant="ghost"'}>${label}</button></li>`).join('')}</ol>`;
    root.querySelectorAll('[data-go]').forEach((button) => { button.onclick = () => { location.hash = button.dataset.go; }; });
  }

  render();
  return { destroy() { root.remove(); } };
}
