import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

// Exercise the actual inline script without installing a browser/npm dependency.
// DOM and Orca are fakes here; this is not a real host acceptance test.
const source = (await readFile(new URL('./panel.html', import.meta.url), 'utf8')).match(/<script>([\s\S]*?)<\/script>/)[1];
const tick = () => new Promise(resolve => setImmediate(resolve));
function boot(transport) {
  const nodes = new Map();
  const element = id => {
    if (!nodes.has(id)) nodes.set(id, { value: '', disabled: true, hidden: true, textContent: '', listeners: {}, addEventListener(event, fn) { this.listeners[event] = fn; } });
    return nodes.get(id);
  };
  const buttons = ['status', 'plan', 'sync', 'recover'].map(action => Object.assign(element(action), { dataset: { action } }));
  let receive;
  const parent = { postMessage(message) { if (message.type !== 'orca-panel-action') return; Promise.resolve().then(() => transport(message.params)).then(value => receive({ source: parent, data: { type: 'orca-panel-action-result', requestId: message.requestId, ok: true, value } }), error => receive({ source: parent, data: { type: 'orca-panel-action-result', requestId: message.requestId, ok: false, error: error.message } })); } };
  vm.runInNewContext(source, { window: { addEventListener: (_, fn) => { receive = fn; } }, parent, document: { getElementById: element, querySelectorAll: () => buttons }, setTimeout, clearTimeout });
  return element;
}
test('panel binds, previews, applies and renders result as text', async () => {
  const calls = [];
  const el = boot(async ({commandId,args}) => {
    calls.push(commandId);
    if (commandId === 'ailoom-configuration') return null;
    if (commandId === 'ailoom-configure') return args;
    return { root: '/project', result: { summary: { create: 1 }, text: '<script>unsafe</script>' } };
  });
  await tick(); assert.equal(el('bind').disabled, false); assert.equal(el('sync').disabled, true);
  el('root').value='/project';el('executable').value='/bin/ailoom';
  el('configuration').listeners.submit({ preventDefault() {} });await tick();
  assert.equal(el('plan').disabled,false);
  el('plan').listeners.click();await tick();assert.equal(el('sync').disabled,false);
  assert.ok(el('result').textContent.includes('<script>unsafe</script>'));
  el('sync').listeners.click();await tick();assert.equal(el('sync').disabled,true);
  assert.deepEqual(calls,['ailoom-configuration','ailoom-configure','ailoom-plan','ailoom-sync']);
});
test('unsupported host keeps all write controls disabled', async () => {
 const el=boot(async()=>{throw new Error('unknown method');});await tick();
 assert.equal(el('bind').disabled,true);assert.equal(el('sync').disabled,true);
 assert.match(el('message').textContent,/宿主补丁/);
});
