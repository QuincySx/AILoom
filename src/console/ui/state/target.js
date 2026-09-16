// AIL-086：TargetContext —— repoId/worktreeId/scopeId/kind/root/generation。
// 所有修改动作带目标；所有异步结果带发起时 generation；generation 不匹配即丢弃。

import { get, set } from './store.js';

let generation = 0;

export function currentTarget() {
  return get('target') ?? null;
}

export function currentGeneration() {
  return generation;
}

export function setTarget(t) {
  generation += 1;
  set('target', t ? { ...t, generation } : null);
  // 目标变化：当前 effective / 能力保存标记 / plan / apply 展示关联失效（蓝图状态表）
  set('effective', null);
  set('capEffective', null);
  set('planJob', null);
  set('planView', null);
  set('applyJob', null);
  set('applyView', null);
  return generation;
}

/// 异步发起时捕获 generation；完成后调用方 shouldApply(gen) 决定是否采纳结果。
export function shouldApply(gen) {
  return gen === generation;
}
