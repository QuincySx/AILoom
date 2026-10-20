// AIL-152：全局 Skill —— 部署到用户级目录，所有项目可见。
// 只管理 AILoom 部署的条目；目录里已有的同名条目保留，用户确认后替换（原条目移入归档，可还原）。

import { api, esc } from '../services/api.js';
import { Dialog, confirmAction } from '../components/dialog.js';
import { notify } from '../state/store.js';
import { PageHeader } from '../components/layout.js';
import { Item, resourcePath } from '../components/item.js';

// Official marks: claude.ai/favicon.svg; developers.openai.com/codex/ (OpenAI mark).
const AGENT_LOGOS = {
  claude: '<svg viewBox="0 0 248 248" aria-hidden="true" focusable="false"><path d="M52.4285 162.873L98.7844 136.879L99.5485 134.602L98.7844 133.334H96.4921L88.7237 132.862L62.2346 132.153L39.3113 131.207L17.0249 130.026L11.4214 128.844L6.2 121.873L6.7094 118.447L11.4214 115.257L18.171 115.847L33.0711 116.911L55.485 118.447L71.6586 119.392L95.728 121.873H99.5485L100.058 120.337L98.7844 119.392L97.7656 118.447L74.5877 102.732L49.4995 86.1905L36.3823 76.62L29.3779 71.7757L25.8121 67.2858L24.2839 57.3608L30.6515 50.2716L39.3113 50.8623L41.4763 51.4531L50.2636 58.1879L68.9842 72.7209L93.4357 90.6804L97.0015 93.6343L98.4374 92.6652L98.6571 91.9801L97.0015 89.2625L83.757 65.2772L69.621 40.8192L63.2534 30.6579L61.5978 24.632C60.9565 22.1032 60.579 20.0111 60.579 17.4246L67.8381 7.49965L71.9133 6.19995L81.7193 7.49965L85.7946 11.0443L91.9074 24.9865L101.714 46.8451L116.996 76.62L121.453 85.4816L123.873 93.6343L124.764 96.1155H126.292V94.6976L127.566 77.9197L129.858 57.3608L132.15 30.8942L132.915 23.4505L136.608 14.4708L143.994 9.62643L149.725 12.344L154.437 19.0788L153.8 23.4505L150.998 41.6463L145.522 70.1215L141.957 89.2625H143.994L146.414 86.7813L156.093 74.0206L172.266 53.698L179.398 45.6635L187.803 36.802L193.152 32.5484H203.34L210.726 43.6549L207.415 55.1159L196.972 68.3492L188.312 79.5739L175.896 96.2095L168.191 109.585L168.882 110.689L170.738 110.53L198.755 104.504L213.91 101.787L231.994 98.7149L240.144 102.496L241.036 106.395L237.852 114.311L218.495 119.037L195.826 123.645L162.07 131.592L161.696 131.893L162.137 132.547L177.36 133.925L183.855 134.279H199.774L229.447 136.524L237.215 141.605L241.8 147.867L241.036 152.711L229.065 158.737L213.019 154.956L175.45 145.977L162.587 142.787H160.805V143.85L171.502 154.366L191.242 172.089L215.82 195.011L217.094 200.682L213.91 205.172L210.599 204.699L188.949 188.394L180.544 181.069L161.696 165.118H160.422V166.772L164.752 173.152L187.803 207.771L188.949 218.405L187.294 221.832L181.308 223.959L174.813 222.777L161.187 203.754L147.305 182.486L136.098 163.345L134.745 164.2L128.075 235.42L125.019 239.082L117.887 241.8L111.902 237.31L108.718 229.984L111.902 215.452L115.722 196.547L118.779 181.541L121.58 162.873L123.291 156.636L123.14 156.219L121.773 156.449L107.699 175.752L86.304 204.699L69.3663 222.777L65.291 224.431L58.2867 220.768L58.9235 214.27L62.8713 208.48L86.304 178.705L100.44 160.155L109.551 149.507L109.462 147.967L108.959 147.924L46.6977 188.512L35.6182 189.93L30.7788 185.44L31.4156 178.115L33.7079 175.752L52.4285 162.873Z" fill="currentColor"/></svg>',
  agents: '<svg viewBox="100 100 411 411" aria-hidden="true" focusable="false"><path fill-rule="evenodd" clip-rule="evenodd" d="M252.794 108.802C289.191 99.0484 326.265 110.305 351.148 135.135C385.113 126.072 422.85 134.862 449.492 161.505C476.136 188.149 484.925 225.888 475.862 259.85V259.854C500.696 284.735 511.95 321.81 502.198 358.207C492.447 394.602 464.161 421.084 430.215 430.217C421.083 464.162 394.603 492.448 358.206 502.199C321.812 511.951 284.734 500.693 259.852 475.864C225.887 484.927 188.15 476.137 161.507 449.495C134.864 422.851 126.073 385.111 135.136 351.149C110.304 326.266 99.0496 289.192 108.801 252.795C118.552 216.4 146.84 189.918 180.784 180.785C189.917 146.841 216.396 118.553 252.794 108.802ZM374.292 407.145C374.292 411.271 372.092 415.086 368.517 417.148L283.723 466.102C302.487 480.585 327.555 486.459 352.217 479.852C386.997 470.532 410.068 439.312 410.555 405.006V317.717C410.555 315.08 409.125 312.621 406.843 311.303L374.292 292.509V407.145ZM251.868 415.897C248.296 417.959 243.893 417.959 240.317 415.897L155.526 366.942C152.366 390.436 159.811 415.08 177.866 433.136H177.863C203.325 458.594 241.896 462.962 271.85 446.232L347.449 402.586C349.735 401.268 351.148 398.8 351.148 396.163V358.579L251.868 415.897ZM368.602 220.628C366.319 219.309 363.474 219.318 361.191 220.637L328.641 239.431L427.921 296.749C431.496 298.811 433.697 302.627 433.697 306.752V404.661C455.622 395.654 473.244 376.881 479.851 352.218C489.169 317.442 473.668 281.85 444.201 264.274L368.602 220.628ZM177.303 206.34C155.377 215.348 137.756 234.122 131.148 258.783C121.832 293.561 137.331 329.153 166.799 346.727L242.398 390.373C244.68 391.692 247.525 391.684 249.807 390.366L282.357 371.572L183.078 314.253C179.504 312.189 177.303 308.375 177.303 304.251V206.34ZM259.849 279.145V331.858L305.5 358.213L351.15 331.858V279.145L305.5 252.789L259.849 279.145ZM327.276 144.9C308.512 130.418 283.445 124.543 258.782 131.15C224.002 140.471 200.931 171.691 200.445 205.995V293.286C200.445 295.923 201.875 298.381 204.158 299.7L236.707 318.493V203.856C236.707 199.731 238.909 195.916 242.483 193.853L327.276 144.9ZM433.137 177.867C407.675 152.407 369.103 148.038 339.149 164.769L263.55 208.415C261.265 209.734 259.852 212.202 259.852 214.838V252.423L359.132 195.105C362.703 193.041 367.108 193.041 370.682 195.105L455.473 244.06C458.635 220.567 451.189 195.922 433.135 177.867H433.137Z" fill="currentColor"></path></svg>',
};

// Agents known to read the shared `~/.agents/skills` directory. They light up together with one switch;
// a longer list stacks in the group and fans out on hover.
const SHARED_READERS = [{ name: 'Codex', logo: AGENT_LOGOS.agents }];
const TARGET_NAME = { claude: 'Claude Code', agents: '通用目录' };

const ACTION_LABEL = { create: '新增', update: '更新', delete: '移除', restore: '恢复', conflict: '跳过（同名）' };

function tilde(path, home) {
  const p = String(path || '');
  return home && (p === home || p.startsWith(home + '/')) ? '~' + p.slice(home.length) : p;
}
const when = (iso) => { const d = new Date(iso); return Number.isNaN(d.getTime()) ? '' : d.toLocaleString(); };

export function GlobalSkills(container) {
  const root = document.createElement('section');
  root.className = 'global-skills';
  container.append(root);
  let alive = true, version = 0, data = null, search = '', busy = false;
  let addDialog=null,detailDialog=null,addBody=null,addError='',addGroup='',addState='all',sourceFilter='',statusFilter='';
  const addTargets=new Map(),addSelected=new Set();
  let addSubmitting=false;
  const q = (s) => root.querySelector(s);
  root.innerHTML = `${PageHeader({title:'全局技能',description:'按工具直接启用或关闭，影响所有项目。',actions:''})}<span data-global-count class="sr-only"></span>
    <div class="toolbar global-toolbar"><input data-search type="search" aria-label="搜索已添加的全局技能" placeholder="搜索名称或描述"><select data-source-filter aria-label="全局技能来源"><option value="">全部来源</option></select><select data-status-filter aria-label="全局技能状态"><option value="">全部状态</option><option value="attention">需要处理</option><option value="enabled">已启用</option><option value="pending">待应用</option></select><button data-add-skill data-variant="default" disabled>添加技能</button></div>
    <button data-apply disabled hidden>应用</button><div class="global-status"><p data-status role="status"></p><button data-refresh data-variant="ghost" hidden>重新读取</button></div>
    <div class="global-list"><div data-list></div></div>
    <div data-foreign></div>
    <div data-archive></div><details class="global-target-section"><summary>存储位置</summary><div data-targets></div></details>`;

  const message = (s) => { if (alive) q('[data-status]').textContent = s; };

  function deployedState(id) {
    const deployed = (data.deployed || []).some((d) => d.resource_id === id);
    const pending = (data.actions || []).some((a) => a.resource_id === id && a.action !== 'noop');
    const conflict = (data.actions || []).some((a) => a.resource_id === id && a.action === 'conflict');
    const skill=(data.skills||[]).find(s=>s.id===id)||{id,global:true};
    const complete=[...selectedTargets(skill)].every(t=>targetDeployed(id,t));
    return conflict ? ['同名冲突', 'warn'] : pending ? ['待应用', ''] : deployed ? complete?['已启用', 'current']:['部分开启','warn'] : ['未部署', 'warn'];
  }
  const hasTargets=()=>data?.targets?.some(t=>t.supported);
  const selectedTargets=skill=>new Set(skill.targets||data.skill_targets?.[skill.id]||(skill.global?(data.targets||[]).filter(t=>t.enabled).map(t=>t.key):[]));
  const targetAllowed=(skill,key)=>!skill.available_targets||skill.available_targets.includes(key);
  const targetDeployed=(id,key)=>(data.deployed||[]).some(d=>d.resource_id===id&&d.tool===(key==='agents'?'codex':key));
  function draftTargets(skill) {
    if(skill.global)return selectedTargets(skill);
    if(!addTargets.has(skill.id))addTargets.set(skill.id,new Set((data.targets||[]).filter(t=>t.enabled&&t.supported&&targetAllowed(skill,t.key)).map(t=>t.key)));
    return addTargets.get(skill.id);
  }
  const orderedTargets=()=>[...(data.targets||[])].sort((a,b)=>(a.key!=='claude')-(b.key!=='claude'));
  const readersOf=t=>t.key==='agents'?SHARED_READERS.map(r=>r.name).join('、')+' 等读取':'';
  function targetButton(skill,t,chosen,adding=false) {
    const on=chosen.has(t.key),name=TARGET_NAME[t.key]||t.label,readers=readersOf(t);
    const hint=!targetAllowed(skill,t.key)?'技能不支持此工具':!t.supported?'目录不受支持':on?(adding?'已选择，点击取消':targetDeployed(skill.id,t.key)?'已部署，点击取消':'已选择，尚未部署'):'未选择，点击选择';
    const disabled=busy||!t.supported||!targetAllowed(skill,t.key)&&(adding||!on)||adding&&skill.global||!adding&&!skill.global;
    const marks=t.key==='agents'
      ?`<span class="agent-stack">${SHARED_READERS.slice(0,3).map(r=>`<span class="agent-mark" title="${esc(r.name)}">${r.logo}</span>`).join('')}${SHARED_READERS.length>3?`<span class="agent-more">+${SHARED_READERS.length-3}</span>`:''}</span>`
      :AGENT_LOGOS[t.key];
    return `<button type="button" class="${t.key==='agents'?'agent-group':'agent-icon-toggle'}" ${adding?'data-global-add-target-skill':'data-global-target-skill'}="${esc(skill.id)}" data-target-key="${esc(t.key)}" aria-pressed="${on}" ${disabled?'disabled':''} aria-label="${esc(skill.name+' · '+name+(readers?'（'+readers+'）':'')+' · '+hint)}" title="${esc(name+(readers?'：'+readers:'')+' · '+hint)}">${marks}</button>`;
  }
  function targetChoices(skill,chosen,adding=false) {
    return `<div class="global-skill-targets" role="group" aria-label="${esc(skill.name)} 开启到哪些工具">${orderedTargets().map(t=>targetButton(skill,t,chosen,adding)).join('')}</div>`;
  }
  // Skill settings list where the skill is deployed: one row per location, each with its own switch.
  function locationRows(skill) {
    const chosen=selectedTargets(skill);
    return orderedTargets().map(t=>{
      const conflict=(data.foreign||[]).some(f=>f.conflicts_with===skill.id&&f.target===t.key);
      const state=conflict?'检测到已有文件':!t.supported?'目录不受支持':targetDeployed(skill.id,t.key)?'已部署':chosen.has(t.key)?'尚未部署':'未开启';
      return Item({title:TARGET_NAME[t.key]||t.label,description:readersOf(t),meta:state,className:'global-location'+(conflict?' global-location-conflict':''),actions:targetButton(skill,t,chosen)});
    }).join('');
  }
  function addedSkills() {
    const skills=new Map((data.skills||[]).map(s=>[s.id,s]));
    const selected=new Set([...(data.selected||[]),...(data.skills||[]).filter(s=>s.global).map(s=>s.id)]);
    const deployed=new Set((data.deployed||[]).map(s=>s.resource_id));
    for(const id of new Set([...selected,...deployed]))if(!skills.has(id))skills.set(id,{id,name:id.split('/').pop(),source:'来源暂不可用',global:selected.has(id),unavailable:true});
    return [...skills.values()].filter(s=>selected.has(s.id)||deployed.has(s.id));
  }

  function render() {
    q('[data-targets]').innerHTML = (data.targets || []).map((t) => Item({title:TARGET_NAME[t.key]||t.label,description:readersOf(t),path:tilde(t.dir, data.home),meta:t.supported?'':'目录不受支持',className:'global-target'})).join('');
    const added=addedSkills();
    const sources=[...new Set(added.map(s=>s.source||'资源库'))];
    q('[data-source-filter]').innerHTML='<option value="">全部来源</option>'+sources.map(source=>`<option ${source===sourceFilter?'selected':''}>${esc(source)}</option>`).join('');
    const rows=added.filter(s=>{
      const [state,tone]=s.global?deployedState(s.id):['待移除','warn'];
      return (!sourceFilter||(s.source||'资源库')===sourceFilter)&&(!statusFilter||(statusFilter==='enabled'?state==='已启用':statusFilter==='attention'?tone==='warn':state==='待应用'))&&`${s.name} ${s.id} ${s.source} ${s.description||''}`.toLowerCase().includes(search.trim().toLowerCase());
    });
    q('[data-global-count]').textContent=`${added.length} 个`;
    q('[data-refresh]').disabled=busy;q('[data-add-skill]').disabled=busy||!hasTargets();q('[data-search]').disabled=busy;
    q('[data-list]').innerHTML=rows.length?rows.map(s=>{
      const [state,cls]=s.global?deployedState(s.id):['待移除','warn'];
      const conflict=(data.foreign||[]).findIndex(f=>f.conflicts_with===s.id);
      const conflictTool=conflict>=0?((data.targets||[]).find(target=>target.key===data.foreign[conflict].target)?.label||data.foreign[conflict].target):'';
      const feedback=cls==='warn'?`<span class="global-warning">${state==='同名冲突'?esc(conflictTool)+' 存在同名冲突':s.unavailable?'来源暂不可用，选择保留。':state==='待移除'?'尚未完成移除':'工具文件尚未更新。'}</span>${conflict>=0?`<button data-takeover="${conflict}">处理冲突</button>`:!s.unavailable?`<button data-retry-skill="${esc(s.id)}">重试应用</button>`:''}`:'';
      return Item({title:s.name||s.id,primaryAction:{attributes:`data-global-detail="${esc(s.id)}"`,label:'查看技能 '+(s.name||s.id)},description:s.description,path:resourcePath(s),className:'native-file-row',attributes:`data-global-skill="${esc(s.id)}"`,actions:`${s.unavailable?'':targetChoices(s,selectedTargets(s))}<span class="workspace-resource-status ${cls} sr-only">${state}</span>${s.global?'':`<button class="global-remove" data-toggle="${esc(s.id)}" ${busy?'disabled':''}>重试移除</button>`}`,feedback});
    }).join(''):`<div class="empty-state"><h3>${added.length?'没有匹配的全局技能':'还没有添加全局技能'}</h3>${added.length?'':'<p>点击“添加技能”，从资源库中挑选。</p>'}</div>`;
    root.querySelectorAll('[data-global-detail]').forEach(button=>{button.onclick=()=>showDetail(added.find(s=>s.id===button.dataset.globalDetail));});
    root.querySelectorAll('[data-retry-skill]').forEach(b=>{b.onclick=()=>{const skill=added.find(s=>s.id===b.dataset.retrySkill);return setAndApply(skill,true,[...selectedTargets(skill)]);};});
    const foreign = data.foreign || [];
    const home = data.home;
    const foreignShown = foreign.filter((f) => `${f.name} ${f.path} ${f.location?.real_path || ''}`.toLowerCase().includes(search.toLowerCase()));
    const conflictRows=foreignShown.filter(f=>f.conflicts_with&&!added.some(s=>s.id===f.conflicts_with)),otherRows=foreignShown.filter(f=>!f.conflicts_with);
    // One line per entry, like the skills above: name, directory, install path, and for a link the real path.
    const targetName=key=>TARGET_NAME[key]||(data.targets||[]).find(tg=>tg.key===key)?.label||key;
    // At most two parts: where it is installed and, for a link, where it really lives. Links passed on the way go in the tooltip.
    const real=(target,loc)=>{const text=tilde(target,home),via=(loc.via||[]).map(v=>tilde(v,home));return via.length?{text,title:`${text}（经过 ${via.join('、')}）`}:text;};
    const paths=(loc,path)=>[tilde(path,home),...(loc?.kind==='link'?[real(loc.real_path,loc)]:loc?.kind==='broken_link'?[real(loc.points_to,loc)]:[])];
    const foreignRow=f=>{const i=foreign.indexOf(f);return Item({title:f.name,description:targetName(f.target)+(f.conflicts_with?'，与所选技能同名':''),path:paths(f.location,f.path),meta:f.location?.kind==='broken_link'?'链接失效':'',className:'global-location-row'+(f.conflicts_with?' global-conflict':''),actions:f.conflicts_with?`<button data-takeover="${i}" ${busy?'disabled':''}>替换为所选技能</button>`:''});};
    q('[data-foreign]').innerHTML = `${conflictRows.length?`<section class="global-conflicts"><div class="workspace-section-title"><h2>需要处理</h2><small>${conflictRows.length} 个同名技能</small></div>${conflictRows.map(foreignRow).join('')}</section>`:''}${foreign.length?`<details class="global-other"><summary>其他工具安装的技能<small>${otherRows.length} 个</small></summary>${otherRows.map(foreignRow).join('')||'<p class="muted">没有匹配的条目</p>'}</details>`:''}`;
    const archive = data.archive || [];
    q('[data-archive]').innerHTML = archive.length ? `<section class="global-archive"><div class="workspace-section-title"><h2>原有技能备份</h2><small>${archive.length} 个</small></div>${archive.map((a,i)=>Item({title:a.name,description:when(a.at),path:paths(a.location,a.original),className:'global-location-row',actions:`<button data-restore="${i}" ${busy?'disabled':''}>还原</button>`})).join('')}</section>` : '';
    const pending = data.pending || 0;
    q('[data-apply]').disabled = busy || !pending;
    const hasConflicts=(data.actions||[]).some(a=>a.action==='conflict');
    q('[data-apply]').hidden=!pending&&!hasConflicts;
    q('[data-apply]').textContent = pending ? `应用待处理项（${pending}）` : '有同名冲突';
    bind(rows, foreign, archive);
    renderAdd();
  }

  function showDetail(skill) {
    if(!skill)return;
    detailDialog?.destroy();
    const body=document.createElement('div');
    body.innerHTML=`<p>${esc(skill.description||'暂无用途说明')}</p><p class="muted">来自${esc(skill.source||'资源库')}</p>${resourcePath(skill)?`<p class="path">${esc(resourcePath(skill))}</p>`:''}<section class="global-detail-targets"><div class="workspace-section-title"><h2>部署位置</h2><small>${orderedTargets().length} 个位置</small></div>${locationRows(skill)}</section><p class="field-error" data-detail-error role="alert"></p>`;
    body.querySelectorAll('[data-global-target-skill]').forEach(box=>{
      box.onclick=async()=>{
        if(busy)return;
        const chosen=selectedTargets(skill);chosen.has(box.dataset.targetKey)?chosen.delete(box.dataset.targetKey):chosen.add(box.dataset.targetKey);
        body.querySelectorAll('button').forEach(b=>b.disabled=true);
        const ok=await setAndApply(skill,chosen.size>0,[...chosen]);
        if(!alive)return;
        if(ok)detailDialog.close();else {const error=q('[data-status]').textContent;showDetail(data.skills.find(s=>s.id===skill.id)||skill);root.querySelector('[data-detail-error]').textContent=error;}
      };
    });
    detailDialog=Dialog(root,{title:skill.name||skill.id,content:body,canClose:()=>!busy,actions:[{label:'关闭'},{label:'移除全局技能',variant:'destructive',onAction:()=>setAndApply(skill,false)}]});
  }

  function bind(rows, foreign, archive) {
    root.querySelectorAll('[data-global-target-skill]').forEach(box=>{if(box.closest?.('.global-detail-targets'))return;box.onclick=async()=>{
      if(busy||box.disabled)return;
      const skill=rows.find(s=>s.id===box.dataset.globalTargetSkill);if(!skill)return;
      const chosen=selectedTargets(skill);chosen.has(box.dataset.targetKey)?chosen.delete(box.dataset.targetKey):chosen.add(box.dataset.targetKey);
      await setAndApply(skill,chosen.size>0,[...chosen]);
      root.querySelectorAll('[data-global-target-skill]').forEach(b=>{if(b.dataset.globalTargetSkill===skill.id&&b.dataset.targetKey===box.dataset.targetKey)b.focus();});
    };});
    root.querySelectorAll('[data-toggle]').forEach((b) => {
      b.onclick = () => {
        const s = rows.find((r) => r.id === b.dataset.toggle);
        return setAndApply(s,false);
      };
    });
    root.querySelectorAll('[data-takeover]').forEach((b) => {
      b.onclick = () => {
        const f = foreign[Number(b.dataset.takeover)];
        const loc = f.location || {};
        const keep = loc.kind === 'link' ? `\n只移走链接，${tilde(loc.real_path, data.home)} 不受影响。` : '';
        const content = document.createElement('div');
        const text = document.createElement('div');
        text.className = 'confirmation-message';
        text.innerHTML = `<strong>${esc(f.name)}</strong><span class="replacement-flow"><span>当前版本</span><span aria-hidden="true">→</span><span>资源库版本</span></span><span class="muted">原有版本保留，可还原。${esc(keep)}</span><details><summary>原有文件位置</summary><span class="path">${esc(tilde(f.path,data.home))}</span></details>`;
        const error = document.createElement('p');
        error.className = 'field-error';
        error.setAttribute('role', 'alert');
        content.append(text, error);
        Dialog(document.body, {
          title: '替换同名 Skill', content, canClose: () => !busy,
          actions: [
            { label: '取消', onAction: () => !busy },
            { label: '替换', variant: 'default', onAction: async () => {
              const ok = await act(() => api.globalTakeover(f.target, f.name), '已移开原来的，点「应用」完成替换');
              if (!ok) error.textContent = q('[data-status]').textContent;
              return ok;
            } },
          ],
        });
      };
    });
    root.querySelectorAll('[data-restore]').forEach((b) => {
      b.onclick = () => {
        const a = archive[Number(b.dataset.restore)];
        act(() => api.globalRestore(a.id), '已还原');
      };
    });
  }

  function addGroups() {
    const groups=new Map();
    for(const skill of data.skills||[]) {
      const url=skill.repository_url,key=skill.repository_key||url||(skill.source&&skill.source!=='资源库'?'source:'+skill.source:'local:personal');
      if(!groups.has(key)) {
        let title=key==='local:personal'?'本地与个人资源':skill.source||url,host='';
        if(url)try {
          const parsed=new URL(url.replace(/^git@([^:]+):/,'ssh://git@$1/'));
          if(['https:','http:','ssh:','git:'].includes(parsed.protocol)){title=parsed.pathname.replace(/\/$/,'').replace(/\.git$/,'').replace(/^\//,'')||parsed.host;host=parsed.host;}
        }catch { /* Local paths retain the source label. */ }
        groups.set(key,{key,title,host,skills:[]});
      }
      groups.get(key).skills.push(skill);
    }
    const compare=(a,b)=>String(a).localeCompare(String(b),'zh-CN',{numeric:true});
    for(const group of groups.values())group.skills.sort((a,b)=>compare(a.name,b.name)||compare(a.id,b.id));
    return [...groups.values()].sort((a,b)=>(a.key==='local:personal')-(b.key==='local:personal')||compare(a.title,b.title)||compare(a.host,b.host));
  }
  function renderAdd() {
    if(!addBody||!data)return;
    const aq=s=>addBody.querySelector(s),query=aq('[data-global-add-search]').value.trim().toLowerCase();
    const groups=addGroups(),total=(data.skills||[]).length;
    if(addGroup&&!groups.some(g=>g.key===addGroup))addGroup='';
    const nav=(key,title,count,host='')=>`<button data-global-add-group="${esc(key)}" class="item-surface" aria-pressed="${addGroup===key}" ${busy?'disabled':''}><span><strong>${esc(title)}</strong>${host?`<small>${esc(host)}</small>`:''}</span><span class="badge">${count}</span></button>`;
    aq('[data-global-add-groups]').innerHTML=nav('','全部资源',total)+groups.map(g=>nav(g.key,g.title,g.skills.length,g.host)).join('');
    aq('[data-global-add-states]').innerHTML=[['all','全部'],['available','未添加'],['added','已添加']].map(([value,label])=>`<button data-global-add-state="${value}" aria-pressed="${addState===value}" ${busy?'disabled':''}>${label}</button>`).join('');
    const shown=groups.filter(g=>!addGroup||g.key===addGroup).map(g=>({...g,visible:g.skills.filter(s=>(addState==='all'||(addState==='added')===!!s.global)&&`${s.name} ${s.id} ${s.source} ${s.description||''} ${g.title} ${g.host}`.toLowerCase().includes(query))})).filter(g=>g.visible.length);
    const count=shown.reduce((n,g)=>n+g.visible.length,0);
    aq('[data-global-add-list]').innerHTML=count?shown.map(g=>`<section><h3 class="global-add-group-heading">${esc(g.title)}</h3>${g.visible.map((s,index)=>{
      const inputId='global-choice-'+index+'-'+encodeURIComponent(s.id);
      return Item({title:s.name,primaryAction:{tag:'label',attributes:`for="${esc(inputId)}"`,label:'选择 '+s.name},description:s.description,path:resourcePath(s),density:'regular',className:'global-add-row',leading:`<input id="${esc(inputId)}" type="checkbox" data-global-add-id="${esc(s.id)}" aria-label="${esc('选择 '+s.name)}" ${s.global||busy||addSubmitting?'disabled':''} ${s.global||addSelected.has(s.id)?'checked':''}>`,actions:s.global?'<span class="badge">已添加</span>':targetChoices(s,draftTargets(s),true)});
    }).join('')}</section>`).join(''):`<div class="empty-state"><h3>${total?'没有匹配的技能':'资源库里还没有 Skill'}</h3>${total?'<button data-global-add-clear>清除筛选</button>':'<a href="#/library" data-add-library>到资源库安装</a>'}</div>`;
    aq('[data-global-add-count]').textContent=`显示 ${count} / ${total} 项`;
    aq('[data-global-add-error]').textContent=addError;
    aq('[data-global-add-search]').disabled=busy||addSubmitting;
    aq('[data-global-add-cancel]').disabled=busy||addSubmitting;
    aq('[data-global-add-source]').disabled=busy||addSubmitting;
    aq('[data-global-selected-count]').textContent=`已选 ${addSelected.size} 项`;
    aq('[data-global-add-submit]').disabled=busy||addSubmitting||!addSelected.size||[...addSelected].some(id=>!draftTargets(data.skills.find(s=>s.id===id)).size);
    if(busy||addSubmitting)addBody.querySelectorAll('input,button,select').forEach(el=>el.disabled=true);
    addDialog?.update({canClose:()=>!busy&&!addSubmitting});
    aq('[data-add-library]')?.addEventListener('click',()=>addDialog.close());
    aq('[data-global-add-clear]')?.addEventListener('click',()=>{if(busy)return;addGroup='';addState='all';aq('[data-global-add-search]').value='';renderAdd();});
  }
  function showAdd() {
    if(!data||busy||!hasTargets())return;
    if(!addDialog) {
      addBody=document.createElement('div');addBody.className='global-add-picker';
      addBody.innerHTML='<div class="global-add-toolbar"><input data-global-add-search type="search" aria-label="搜索要添加的 Skill" placeholder="搜索名称、用途或来源…"><div data-global-add-states role="group" aria-label="添加状态"></div><button data-global-add-source class="global-add-source-toggle" aria-expanded="false">选择来源</button><span data-global-add-count class="muted global-candidate-count" role="status" aria-live="polite"></span></div><p data-global-add-error class="field-error" role="alert"></p><div class="global-add-catalog"><aside aria-label="按仓库与合集过滤"><h3>仓库与合集</h3><div data-global-add-groups tabindex="0" aria-label="选择仓库或合集"></div></aside><div><div data-global-add-list tabindex="0" aria-label="可添加的全局技能"></div></div></div><footer class="dialog-actions"><span data-global-selected-count role="status" aria-live="polite"></span><button data-global-add-cancel>取消</button><button class="primary" data-global-add-submit disabled>添加并开启</button></footer>';
      addDialog=Dialog(root,{title:'添加全局技能',content:addBody,open:false,keepMounted:true,canClose:()=>!busy&&!addSubmitting});
      addBody.querySelector('[data-global-add-cancel]').onclick=()=>{if(!busy&&!addSubmitting)addDialog.close();};
      addBody.querySelector('[data-global-add-submit]').onclick=async()=>{
        if(busy||addSubmitting||!addSelected.size)return;
        const plan=[...addSelected].map(id=>({skill:data.skills.find(s=>s.id===id),targets:[...draftTargets(data.skills.find(s=>s.id===id))]}));
        if(plan.some(item=>!item.targets.length))return;
        addSubmitting=true;renderAdd();const errors=[];
        try {for(const {skill,targets} of plan){if(!alive)break;const ok=await setAndApply(skill,true,targets);if(ok)addSelected.delete(skill.id);else errors.push(addError);}}
        finally {addSubmitting=false;if(alive){addError=errors.join('；');renderAdd();if(!errors.length)addDialog.close();}}
        return !errors.length;
      };
      addBody.querySelector('[data-global-add-source]').onclick=()=>{const open=addBody.classList.toggle('sources-open');addBody.querySelector('[data-global-add-source]').setAttribute('aria-expanded',String(open));};
      addBody.querySelector('[data-global-add-search]').oninput=()=>{addError='';renderAdd();};
      addBody.querySelector('[data-global-add-groups]').onclick=event=>{
        const button=event.target.closest('[data-global-add-group]');if(!button||busy)return;
        addGroup=button.dataset.globalAddGroup;addBody.classList.remove('sources-open');addBody.querySelector('[data-global-add-source]').setAttribute('aria-expanded','false');renderAdd();
        addBody.querySelector('[data-global-add-list]').scrollTop=0;
        const switcher=addBody.querySelector('[data-global-add-source]');
        if(switcher.getClientRects?.().length)switcher.focus();else addBody.querySelectorAll('[data-global-add-group]').forEach(b=>{if(b.dataset.globalAddGroup===addGroup)b.focus();});
      };
      addBody.querySelector('[data-global-add-states]').onclick=event=>{
        const button=event.target.closest('[data-global-add-state]');if(!button||busy)return;
        addState=button.dataset.globalAddState;renderAdd();
        addBody.querySelector('[data-global-add-list]').scrollTop=0;
        addBody.querySelectorAll('[data-global-add-state]').forEach(b=>{if(b.dataset.globalAddState===addState)b.focus();});
      };
      addBody.querySelector('[data-global-add-list]').onclick=event=>{
        const box=event.target.closest('[data-global-add-target-skill]');
        if(box?.dataset.globalAddTargetSkill){
          if(busy||addSubmitting||box.disabled)return;
          const skill=data.skills.find(s=>s.id===box.dataset.globalAddTargetSkill);if(!skill||skill.global)return;
          const chosen=draftTargets(skill);chosen.has(box.dataset.targetKey)?chosen.delete(box.dataset.targetKey):chosen.add(box.dataset.targetKey);renderAdd();
          addBody.querySelectorAll('[data-global-add-target-skill]').forEach(b=>{if(b.dataset.globalAddTargetSkill===skill.id&&b.dataset.targetKey===box.dataset.targetKey)b.focus();});
          return;
        }
        const button=event.target.closest('[data-global-add-id]');if(!button||button.disabled||busy||addSubmitting)return;
        const id=button.dataset.globalAddId;addSelected.has(id)?addSelected.delete(id):addSelected.add(id);renderAdd();
        addBody.querySelectorAll('[data-global-add-id]').forEach(input=>{if(input.dataset.globalAddId===id)input.focus();});
      };
    }
    addBody.querySelector('[data-global-add-search]').value='';addError='';addGroup='';addState='all';addTargets.clear();addSelected.clear();renderAdd();addDialog.show();
  }
  async function setAndApply(skill,enabled,targets) {
    if(busy||enabled&&(!hasTargets()||!targets?.length))return false;
    const updating=enabled&&skill.global;
    busy=true;addError='';render();
    let saved=false,error='';
    try {
      await api.globalSelect({skill:skill.id,enabled,...(enabled?{targets}:{}),base_revision:data.revision});saved=true;
      const result=await api.globalApply(skill.id);
      if(result.skipped_conflicts?.length)error=`“${skill.name}”已保存，${result.skipped_conflicts.length} 个工具有同名冲突，尚未全部开启。请在全局列表处理。`;
    }catch(e){error=(saved?enabled?updating?'工具选择已保存，但应用失败：':'已添加，但开启失败：':'已标记移除，但应用失败：':'')+e.message;}
    busy=false;
    const refreshed=await load(error);
    if(!alive)return false;
    if(!refreshed&&!error)error='操作已提交，但列表刷新失败。请刷新后核对状态。';
    if(!error&&enabled&&targets.some(t=>!targetDeployed(skill.id,t)))error='工具选择已保存，但尚未全部部署。请检查技能适配、目录或来源状态。';
    if(error){addError=error;message(error);renderAdd();return false;}
    notify(enabled?`${updating?'已更新':'已开启'}“${skill.name}”的工具选择，新开 AI 会话后生效。`:`已从全局移除“${skill.name}”。`);
    renderAdd();return true;
  }

  async function act(fn, done) {
    if (busy) return false;
    busy = true; render();
    let error = '';
    try { await fn(); if (done) notify(done); } catch (e) { error = e.message; }
    busy = false;
    await load(error);
    return !error;
  }

  async function apply() {
    if (busy) return;
    let plan;
    try { plan = await api.globalPlan(); } catch (e) { message(e.message); return; }
    const lines = (plan.actions || []).filter((a) => a.action !== 'noop').map((a) => `${ACTION_LABEL[a.action] || a.action}  ~/${a.path}`);
    if (!lines.length) { message('已是最新'); return; }
    const ok = await confirmAction(lines.join('\n'), { title: '应用全局 Skill', confirmLabel: '应用' });
    if (!ok) return;
    busy = true; render();
    let error = '';
    try {
      const r = await api.globalApply();
      const skipped = (r.skipped_conflicts || []).length;
      notify(`已应用${skipped ? `，${skipped} 个同名的已跳过` : ''}。新开会话后生效。`);
    } catch (e) { error = e.message; }
    busy = false;
    await load(error);
  }

  async function load(error = '') {
    const n = ++version;
    message('正在读取…');
    try {
      const v = await api.globalSkills();
      if (!alive || n !== version) return false;
      data = v;
      q('[data-refresh]').hidden = true;
      render();
      message(error || (v.notes || []).join('；') || (!hasTargets()?'没有可用的全局存储目录。':''));
      return true;
    } catch (e) {
      if (alive && n === version) {
        // A failed read offers its own retry next to the message; there is no standing refresh button.
        q('[data-refresh]').hidden = false;
        if (data) render();
        message([error, '读取失败：' + e.message].filter(Boolean).join('；'));
      }
      return false;
    }
  }

  q('[data-refresh]').onclick = () => load();
  q('[data-add-skill]').onclick=showAdd;
  q('[data-apply]').onclick = apply;
  q('[data-search]').oninput = (e) => { search = e.target.value; if (data) render(); };
  q('[data-source-filter]').onchange=e=>{sourceFilter=e.target.value;if(data)render();};
  q('[data-status-filter]').onchange=e=>{statusFilter=e.target.value;if(data)render();};
  load();
  return { destroy() { alive = false;++version;addDialog?.destroy();detailDialog?.destroy();root.remove(); } };
}
