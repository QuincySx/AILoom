import { createHash } from 'node:crypto';
import { projectPath, runCli } from './bridge.mjs';

const digest = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');

export default function activate(orca) {
  let busy = false;
  let preview;
  const notify = async body => {
    try { await orca.host.call('notifications.show', { title: 'AILoom', body: body.slice(0, 1000) }); }
    catch { /* A notification failure must not turn a completed sync into a retry. */ }
  };
  const register = (id, handler) => orca.commands.register(id, async (args = {}) => {
    if (busy) throw new Error('AILoom 正在执行操作，请等待完成');
    busy = true;
    try { return await handler(args ?? {}); }
    catch (error) { await notify(error.message); throw error; }
    finally { busy = false; }
  });
  register('ailoom-configuration', async () => {
    const { value } = await orca.host.call('storage.get', { key: 'configuration' });
    return value ?? null;
  });
  register('ailoom-configure', async args => {
    const root = await projectPath(args.root);
    const config = { root, executable: args.executable || 'ailoom', ...(args.dataRoot ? { dataRoot: args.dataRoot } : {}) };
    await runCli({ ...config, action: 'version' });
    await orca.host.call('storage.set', { key: 'configuration', value: config });
    preview = undefined;
    await notify(`已绑定项目：${root}`);
    return config;
  });
  for (const action of ['status', 'plan', 'sync', 'recover']) {
    register(`ailoom-${action}`, async () => {
      const { value: config } = await orca.host.call('storage.get', { key: 'configuration' });
      if (!config?.root) throw new Error('请先通过 ailoom-configure 绑定项目和 CLI 路径');
      if (action === 'sync') {
        if (!preview || preview.configuration !== digest(config)) throw new Error('请先预览变更，再应用');
        const current = await runCli({ ...config, action: 'plan' });
        if (preview.plan !== digest(current)) {
          preview = undefined;
          throw new Error('项目变更已更新，请重新预览');
        }
      }
      if (action === 'plan' || action === 'sync' || action === 'recover') preview = undefined;
      const result = await runCli({ ...config, action });
      if (action === 'plan' && Buffer.byteLength(JSON.stringify(result)) > 32000) throw new Error('预览过大，请使用独立 CLI 或网页查看完整改动并应用');
      if (action === 'plan') preview = { configuration: digest(config), plan: digest(result) };
      // Full structured result remains available to the command caller.
      try { orca.log(JSON.stringify({ action, root: config.root, summary: result?.summary })); }
      catch { /* Logging cannot change the outcome of a completed operation. */ }
      await notify(`${config.root}\n${{ status: '检查完成', plan: '预览完成，可查看命令结果后应用', sync: '应用完成', recover: '恢复完成' }[action]}`);
      return { root: config.root, result };
    });
  }
}
