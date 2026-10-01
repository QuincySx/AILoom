// AIL-110 失联修复验证：失联显示可恢复状态 + 重新检查后恢复。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-110-fix]', ...a);
const shot = n => b.screenshot(`${EV}/ail110-${n}.png`);
const sh = cmd => execSync(cmd).toString();
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };

await b.waitFor('!!document.querySelector("#nav")');
const nongitId = sh(`ls ${S}/data/repos | grep nongit- | head -1`).trim();

// 服务仍是旧二进制：重启以加载新后端
const token = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/shutdown -H "X-AILoom-Session: ${token}" -d '{}'`);
await new Promise(r => setTimeout(r, 1200));
execSync(`cd ${S} && HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root ${S}/data console --port 8646 --no-open >> ${EV}/console.log 2>&1 &`, { shell: '/bin/zsh' });
for (let i = 0; i < 40; i++) { try { execSync('curl -s http://127.0.0.1:8646/api/state'); break; } catch { execSync('sleep 0.5'); } }

const wt = JSON.parse(sh(`curl -s http://127.0.0.1:8646/api/state`)).repos.find(r => r.repo_id === nongitId).worktrees.local;
expect(wt.status === 'active' && wt.path.endsWith('知识库 甲'), '后端为文件夹项目合成 local 状态 active');

// 失联 → 可恢复状态
sh(`mv "${A}" "${A}-失联测试"`);
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor(`!!document.querySelector('[data-recheck]')`, 15000);
const badge = await b.evaluate(`document.querySelector('.config-header .badge')?.textContent`);
const guidance = await b.evaluate(`document.querySelector('.empty-state')?.textContent`);
expect(badge.includes('目录失联'), '徽标显示「目录失联」');
expect(guidance.includes('移回上面的原路径') && guidance.includes('重新登记'), '给出移动回原路径 / 重新登记两种恢复策略');
expect(guidance.includes('身份随路径变化') && guidance.includes('不会自动合并'), '说明身份契约：不静默合并');
await shot('08-a-missing-dir-after-fix');

// 目录回归 → 重新检查恢复
sh(`mv "${A}-失联测试" "${A}"`);
await b.evaluate(`document.querySelector('[data-recheck]').click()`);
await b.waitFor(`!!document.querySelector('[data-entries]') && document.querySelector('[data-entries]').textContent.includes('已启用')`, 15000);
expect(true, '恢复目录后「重新检查」回到正常配置页（宿主状态仍在）');
await shot('09-a-recovered');

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
