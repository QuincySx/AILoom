import { existsSync } from 'node:fs';
const DIR = process.env.SKILL_DIR, SHOTS = process.env.SHOTS;
const openDialog = 'document.querySelector("dialog[open]")';
const clickIn = (label) => `(()=>{const b=[...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent.trim()===${JSON.stringify(label)});if(!b)return false;b.click();return true})()`;
export default async (page) => {
  await page.waitFor('!!document.querySelector("[data-skill-delete]")');
  // 1. 取消：零写入
  await page.eval('document.querySelector("[data-skill-delete]").click()');
  await page.waitFor(`${openDialog}?.innerText.includes("local-one")`);
  if (!await page.eval(`${openDialog}.innerText.includes(${JSON.stringify(DIR)})`)) throw new Error('预览未显示精确目录');
  await page.shot(SHOTS + '-1-preview.png');
  await page.eval(clickIn('取消'));
  await page.waitFor(`!${openDialog}`);
  if (!existsSync(DIR)) throw new Error('取消后目录被改动');
  // 2. 名称输错：拒绝，目录保留
  await page.eval('document.querySelector("[data-skill-delete]").click()');
  await page.waitFor(clickIn('继续：输入名称确认'));
  await page.waitFor(`!!document.querySelector("dialog[open] [data-delete-name]")`);
  await page.eval('document.querySelector("dialog[open] [data-delete-name]").value="wrong"');
  await page.eval(clickIn('确认删除'));
  await page.waitFor(`document.querySelector("dialog[open] [data-delete-err]")?.textContent.includes("不一致")`);
  await page.shot(SHOTS + '-2-wrong-name.png');
  if (!existsSync(DIR)) throw new Error('名称错误时目录被删除');
  await page.eval(clickIn('取消'));
  await page.waitFor(`!${openDialog}`);
  // 3. 正确删除：目录移入归档，列表刷新
  await page.eval('document.querySelector("[data-skill-delete]").click()');
  await page.waitFor(clickIn('继续：输入名称确认'));
  await page.waitFor(`!!document.querySelector("dialog[open] [data-delete-name]")`);
  await page.eval('document.querySelector("dialog[open] [data-delete-name]").value="local-one"');
  await page.eval(clickIn('确认删除'));
  await page.waitFor(`document.body.innerText.includes("已删除并移入归档")`);
  await page.shot(SHOTS + '-3-deleted.png');
  if (existsSync(DIR)) throw new Error('确认后目录仍存在');
  if (await page.eval('!!document.querySelector("[data-skill-delete]")')) throw new Error('列表未刷新');
  if (page.errors.length) throw new Error('页面报错: ' + page.errors.join('\n'));
};
