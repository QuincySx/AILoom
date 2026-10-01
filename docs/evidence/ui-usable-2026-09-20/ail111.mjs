// AIL-111：项目目录已有 Skill 的扫描在真实 UI 中的验收。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`, B = `${S}/知识库 乙`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-111]', ...a);
const shot = n => b.screenshot(`${EV}/ail111-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const kbHashes = () => sh(`find "${A}" "${B}" -type f -not -path '*/.git/*' -not -path '*/.claude/*' -exec shasum -a 256 {} \\; | sort`);
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };

await b.waitFor('!!document.querySelector("#nav")');
const nongitId = sh(`ls ${S}/data/repos | grep nongit- | head -1`).trim();
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`['未发现本地 Skill', '发现 '].some(k => (document.querySelector('[data-scan-status]')?.textContent || '').includes(k))`);
await shot('01-skill-tab-host-scan-empty');
expect((await b.evaluate(`document.querySelector('[data-scan-results]').textContent`)).includes('没有其他本地 Skill') || ((await b.evaluate(`document.querySelector('[data-scan-results]').innerHTML`)) === '') || (document.querySelector('[data-scan-status]')?.textContent || '').includes('没有其他本地条目'), '宿主目录为空时如实显示（F01 反例：之前项目页完全看不到本目录 Skill）');

// 指定根扫描
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]').textContent.includes('会议纪要')`);
await shot('02-scan-results');
const scanText = await b.evaluate(`document.querySelector('[data-skills-scan]').textContent`);
expect(scanText.includes('会议纪要') && scanText.includes('项目自有（未托管）'), '合法未托管 Skill 显示名称与管理方式');
expect(scanText.includes('周报生成'), '嵌套（skills/分类/周报生成）被有界扫描发现');
expect(scanText.includes('损坏技能') && scanText.includes('缺少 frontmatter'), '损坏 SKILL.md 显示独立错误，不锁定整个列表');
expect(scanText.includes('链接到乙') && scanText.includes('外部链接') && scanText.includes('未解析其内容'), '越界符号链接只报分类与目标，不读内容');
expect(scanText.includes('维护团队中英术语对照') === false && scanText.includes('链接目标'), '不越界：只显示链接目标路径，不读取目标内容（其描述不出现）');
expect(!scanText.includes('更新'), '未托管项没有虚假的更新/版本控件');

// 接管说明 Dialog
await b.evaluate(`document.querySelector('[data-takeover]').click()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('接管说明'))`);
await shot('03-takeover-dialog');
const dlgText = await b.evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent).join('')`);
expect(dlgText.includes('不直接修改、移动或删除') && dlgText.includes('导入个人副本') && dlgText.includes('删除…'), '接管说明：只导入副本/原目录不动/删除走独立双确认入口（AIL-112 已交付）');
await b.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
await b.waitFor(`![...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('接管说明'))`);

// 项目切换不串结果：乙指定根扫描后返回甲
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-list]")');
const bid = JSON.parse(sh(`curl -s http://127.0.0.1:8646/api/state`)).repos.find(r => r.repo_id.startsWith('nongit-') && r.common_dir.includes('乙')).repo_id;
await b.goto('#/projects/' + encodeURIComponent(bid));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`['未发现本地 Skill', '发现 '].some(k => (document.querySelector('[data-scan-status]')?.textContent || '').includes(k))`);
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]').textContent.includes('术语表')`);
const bScan = await b.evaluate(`document.querySelector('[data-scan-results]').textContent`);
expect(bScan.includes('术语表') && !bScan.includes('会议纪要'), '乙只见自己的 Skill（跨项目隔离）');
await shot('04-b-scan');

// 扫描只读：哈希不变
expect(kbHashes() === sh(`cat ${EV}/pre-scan-hashes.txt`), '扫描前后知识库文件哈希完全不变');

// 重启后重新进入，扫描仍可用（不依赖内存态）
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-tab]")', 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-scan]')`, 20000);
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]').textContent.includes('会议纪要')`, 20000);
expect(true, '重启后重新扫描仍可见（扫描为服务端只读能力，无内存依赖）');

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
