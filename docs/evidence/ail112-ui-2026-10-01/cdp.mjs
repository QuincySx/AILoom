// 最小 CDP 驱动：node cdp.mjs <cdpPort> <url> <script.js>；script 导出 async (page) => {}
const [port, url, scriptPath] = process.argv.slice(2);
const tab = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
const ws = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise(r => ws.addEventListener('open', r, { once: true }));
let seq = 0; const pending = new Map(); const errors = [];
ws.addEventListener('message', e => { const m = JSON.parse(e.data);
  if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result); }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.text + ' ' + (m.params.exceptionDetails.exception?.description || ''));
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') errors.push('console.error ' + m.params.args.map(a => a.value ?? a.description).join(' '));
});
const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); ws.send(JSON.stringify({ id, method, params })); });
const page = {
  async eval(expr) { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || JSON.stringify(r.exceptionDetails)); return r.result.value; },
  async waitFor(expr, ms = 10000) { const t = Date.now(); while (Date.now() - t < ms) { if (await page.eval(expr).catch(() => false)) return true; await new Promise(r => setTimeout(r, 100)); }
    throw new Error('timeout: ' + expr + '\n' + await page.eval('document.body.innerText').catch(() => '')); },
  async shot(path) { const s = await send('Page.captureScreenshot', { format: 'png' }); (await import('node:fs')).writeFileSync(path, Buffer.from(s.data, 'base64')); },
  errors,
};
await send('Runtime.enable'); await send('Page.enable');
await send('Page.navigate', { url });
await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
try { await (await import(scriptPath)).default(page); console.log('OK'); }
catch (e) { console.log('FAIL', e.message); process.exitCode = 1; }
finally { if (errors.length) console.log('PAGE ERRORS', errors); await fetch(`http://127.0.0.1:${port}/json/close/${tab.id}`).catch(() => {}); process.exit(); }
