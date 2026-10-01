import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import activate from './main.mjs';
import { runCli } from './bridge.mjs';

const executable = resolve('target/debug/ailoom');

test('worker → real CLI: configure, preview, apply; no HTTP', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'ailoom-orca-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const root = join(dir, 'project with spaces');
  const source = join(dir, 'source');
  const dataRoot = join(dir, 'data');
  await mkdir(root);
  execFileSync('git', ['init', '--quiet', root]);
  const cli = args => JSON.parse(execFileSync(executable, ['--json', '--data-root', dataRoot, ...args], { encoding: 'utf8' }));
  cli(['source', '--dir', source]);
  cli(['init', '--root', root, '--local-path', '../source', '--project', 'a', '--role', 'dev', '--target', 'cursor']);
  const commands = new Map();
  const storage = new Map();
  const notices = [];
  activate({
    commands: { register: (name, fn) => commands.set(name, fn) },
    log() { throw new Error('host log unavailable'); },
    host: { call: async (name, args) => {
      if (name === 'storage.get') return { value: storage.get(args.key) ?? null };
      if (name === 'storage.set') { storage.set(args.key, args.value); return { ok: true }; }
      if (name === 'notifications.show') { notices.push(args); throw new Error('host notification unavailable'); }
      throw new Error(name);
    } },
  });
  await assert.rejects(commands.get('ailoom-status')(), /绑定/);
  await commands.get('ailoom-configure')({ root, executable, dataRoot });
  await commands.get('ailoom-status')();
  await assert.rejects(commands.get('ailoom-sync')(), /预览/);
  const plan = await commands.get('ailoom-plan')();
  assert.ok(plan.result.summary.create > 0);
  await commands.get('ailoom-sync')();
  await assert.rejects(commands.get('ailoom-sync')(), /预览/);
  const next = await commands.get('ailoom-plan')();
  assert.equal(next.result.summary.create, 0);
  assert.equal(next.result.summary.update, 0);
  assert.ok(notices.some(n => n.body.includes('应用完成')));
});

test('invalid actions, paths and missing CLI produce actionable errors', async () => {
  await assert.rejects(runCli({ action: 'shell' }), /不支持/);
  await assert.rejects(runCli({ action: 'status', root: '.' }), /绝对路径/);
  await assert.rejects(runCli({ action: 'version', executable: '/does-not-exist/ailoom' }), /找不到 AILoom/);
  const version = await runCli({ executable, action: 'version' });
  assert.equal(version.name, 'ailoom');
});
