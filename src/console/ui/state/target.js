// AIL-086：TargetContext —— repoId/worktreeId/scopeId/kind/root/generation。
// 所有修改动作带目标；所有异步结果带发起时 generation；generation 不匹配即丢弃。
// AIL-121：目录优先目标契约 —— directoryTarget() 生成唯一只读快照；
// 视图种类 viewKind 区分「当前目录」与「项目共享设置」；generation 在任何目标/视图
// 切换时递增，旧读取结果与旧计划一律作废。

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

// ---------------------------------------------------------------------------
// AIL-121 目标契约
// 快照字段：projectId / name / worktreeId / rootPath / relativeDir /
// resolvedPath / viewKind（'directory' | 'project-shared'）+ generation。
// 兼容别名：repo_id=projectId、wt_id=worktreeId、path=resolvedPath、kind，
// 供 scopes/library 引用对话框等既有消费方继续工作；新代码一律读契约字段。
// ---------------------------------------------------------------------------

export function directoryTarget({ projectId, name, worktreeId, rootPath, relativeDir = null, kind }) {
  const rel = relativeDir ? String(relativeDir).replace(/^\/+|\/+$/g, '') : null;
  const resolvedPath = rel ? trimSlash(rootPath) + '/' + rel : trimSlash(rootPath);
  return {
    projectId, name, worktreeId, rootPath, relativeDir: rel || null, resolvedPath,
    viewKind: 'directory',
    kind,
    repo_id: projectId, wt_id: worktreeId, path: resolvedPath,
  };
}

export function sharedSettingsTarget({ projectId, name, rootPath, kind }) {
  return {
    projectId, name, worktreeId: null, rootPath, relativeDir: null,
    resolvedPath: trimSlash(rootPath), viewKind: 'project-shared',
    kind,
    repo_id: projectId, wt_id: null, path: trimSlash(rootPath),
  };
}

function trimSlash(p) { return String(p || '').replace(/\/+$/, '') || '/'; }

/// 当前编辑层（对 resolver 五层键的映射）。用户不可选择层；
/// 层由目标种类与相对目录唯一决定：
///   Git 目录视图：Worktree 根 → worktree_override；子目录 → worktree_subproject(rel)
///   非 Git 根：repo_default（沿用现有项目默认存储，不迁移）
///   非 Git 子目录：worktree_subproject(rel)（登记在 Worktree "root" 下）
///   项目共享设置：repo_default
export function layerOfTarget(t) {
  if (!t) return null;
  if (t.viewKind === 'project-shared') return { key: 'repo_default', subproject: null };
  if (t.kind === 'nongit' && !t.relativeDir) return { key: 'repo_default', subproject: null };
  if (t.relativeDir) return { key: 'worktree_subproject', subproject: t.relativeDir };
  return { key: 'worktree_override', subproject: null };
}

/// 解析器五层键（低→高）。UI 不另造优先级，只翻译服务端 trace。
export const SUBPROJECT_KEYS = { repo_subproject: true, worktree_subproject: true };

export function originKey(origin) {
  return typeof origin === 'string' ? origin : origin ? Object.keys(origin)[0] : '';
}

export function originPath(origin) {
  const k = originKey(origin);
  return SUBPROJECT_KEYS[k] ? origin?.[k]?.path ?? null : null;
}

/// 某条 trace 是否属于「本层」：键相同，且子项目层必须路径相同。
export function atLayer(origin, layer) {
  if (!layer || originKey(origin) !== layer.key) return false;
  if (SUBPROJECT_KEYS[layer.key]) return originPath(origin) === layer.subproject;
  return true;
}

/// 恢复继承 / 差异比较共用的上游计算：trace 低→高，取「排除本层后」
/// 最高的非 inherit 表态。恢复继承的真实结果 = 剩余各层自顶向下解析：
/// 本层之上的层仍然生效，因此必须取最高剩余表态，而不是「本层以下」。
export function upstreamOf(trace, layer) {
  const rest = (trace || []).filter(e => !atLayer(e.origin, layer) && e.choice !== 'inherit');
  return rest.length ? rest[rest.length - 1] : null;
}

/// 差异模式（目录视角）：here=本层表态；up=排除本层后的真实上游。
export function diffOf(trace, layer) {
  const mine = (trace || []).filter(e => atLayer(e.origin, layer) && e.choice !== 'inherit');
  const here = mine.length ? mine[mine.length - 1].choice : null;
  const up = upstreamOf(trace, layer);
  const mode = here === 'disable' ? 'removed-here'
    : here === 'enable' ? (up?.choice === 'enable' ? 'same-on' : 'added-here')
    : up?.choice === 'enable' ? 'follow-on'
    : up?.choice === 'disable' ? 'follow-off' : 'off';
  return { here, upstream: up, mode };
}
