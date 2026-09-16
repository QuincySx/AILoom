// AIL-097 页面：个人指令与宿主验证设置。

import { InstructionsPanel } from '../features/instructionsPanel.js';
import { currentTarget, currentGeneration } from '../state/target.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  const slot = document.createElement('div');
  root.appendChild(slot);
  const panel = InstructionsPanel(slot, {
    target: currentTarget(),
    targetGen: currentGeneration(),
  });
  root.innerHTML = '';
  root.appendChild(slot);
  return { destroy() { root.remove(); }, panel };
}
