// 探索/验收辅助：连接 :9231 的 Chrome CDP，执行 JS、截图。
// 用法：node drive.mjs '<js-expression>' [screenshot-path]
import { writeFile } from 'node:fs/promises';

export async function connect(url = 'about:blank') {
  const tab = await (await fetch('http://127.0.0.1:9231/json/new?' + encodeURIComponent(url), { method: 'PUT' })).json();
  const socket = new WebSocket(tab.webSocketDebuggerUrl);
  await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
  let seq = 0;
  const pending = new Map();
  const errors = [];
  socket.addEventListener('message', event => {
    const m = JSON.parse(event.data);
    if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(m.error) : p.resolve(m.result); }
    if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.text + ': ' + JSON.stringify(m.params.exceptionDetails.exception));
  });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
    return r.result.value;
  };
  const waitFor = async (expression, timeout = 10000) => {
    for (let n = 0; n < timeout / 100; n++) {
      if (await evaluate(expression)) return true;
      await new Promise(r => setTimeout(r, 100));
    }
    throw new Error('Timed out: ' + expression + '\n' + await evaluate('document.querySelector("#app")?.innerText?.slice(0,500)'));
  };
  const screenshot = async path => {
    const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    await writeFile(path, Buffer.from(shot.data, 'base64'));
  };
  const key = (key, code, vk, text) => send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: vk, ...(text ? { text, unmodifiedText: text } : {}) });
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
  return {
    tab, send, evaluate, waitFor, screenshot, key, errors,
    viewport: (width, height) => send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false }),
    async goto(hash) { await evaluate(`location.hash=${JSON.stringify(hash)}`); },
    async close() { socket.close(); await fetch('http://127.0.0.1:9231/json/close/' + tab.id); },
  };
}
