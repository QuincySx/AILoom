// Browser-only fixtures: cleanup and refresh never write production API data.
import {writeFile} from 'node:fs/promises';

export async function verifyWorktreeCleanup({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(String.raw`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const root='/fixture/projects/a-long-project-name-for-worktree-cleanup';
    const repos=[
      {repo_id:'cleanup-mixed',common_dir:root+'/.git',project:{name:'工作目录清理验收',category:'开发'},worktrees:{
        main:{path:root,status:'active',branch:'refs/heads/main'},
        first:{path:root+'-removed-feature-with-a-long-directory-name',status:'missing',branch:'refs/heads/feature/removed-one'},
        second:{path:root+'-removed-two',status:'missing',branch:'refs/heads/feature/removed-two'},
      }},
      {repo_id:'cleanup-all',common_dir:'/fixture/all-missing/.git',project:{name:'全部目录失联'},worktrees:{
        first:{path:'/fixture/all-missing',status:'missing',branch:'refs/heads/main'},
        second:{path:'/fixture/all-missing-feature',status:'missing',branch:'refs/heads/feature'},
      }},
      {repo_id:'cleanup-healthy',common_dir:'/fixture/healthy/.git',project:{name:'全部目录正常'},worktrees:{main:{path:'/fixture/healthy',status:'active',branch:'refs/heads/main'}}},
      {repo_id:'cleanup-restored',common_dir:'/fixture/restored/.git',project:{name:'目录已恢复'},worktrees:{main:{path:'/fixture/restored',status:'missing',branch:'refs/heads/main'}}},
      {repo_id:'nongit-cleanup',common_dir:'/fixture/notes',project:{name:'普通文件夹'},worktrees:{}},
    ];
    const fixture=window.worktreeCleanup={state:{repos},calls:{approve:[],dirs:[],source:[],prune:[],discover:[]},holdPrune:false,holdDiscover:false,failPrune:false};
    api.state=async()=>structuredClone(fixture.state);
    api.approveDir=async path=>{fixture.calls.approve.push(path);return {};};
    api.projectDirs=async path=>{fixture.calls.dirs.push(path);return {dirs:[]};};
    api.projectSource=async body=>{fixture.calls.source.push(body.root);throw Error('项目尚未绑定团队来源');};
    api.repoPrune=async id=>{
      fixture.calls.prune.push(id);
      if(fixture.holdPrune)await new Promise(resolve=>fixture.releasePrune=resolve);
      if(fixture.failPrune){fixture.failPrune=false;throw Error('清理请求失败，请重试（浏览器验收）');}
      const repo=repos.find(r=>r.repo_id===id);
      const removed=Object.entries(repo.worktrees).filter(([,w])=>w.status==='missing').map(([id])=>id);
      repo.worktrees=Object.fromEntries(Object.entries(repo.worktrees).filter(([,w])=>w.status!=='missing'));
      return structuredClone({repo_id:id,removed,worktrees:repo.worktrees});
    };
    api.repoDiscover=async path=>{
      fixture.calls.discover.push(path);
      if(fixture.holdDiscover)await new Promise(resolve=>fixture.releaseDiscover=resolve);
      return {repo_id:repos.find(r=>r.common_dir===path+'/.git')?.repo_id};
    };
    sessionStorage.removeItem('ailoom-directory-view');
    sessionStorage.setItem('ailoom-return-after-settings',JSON.stringify({repoId:'cleanup-mixed',wt:'first',dir:null,node:'worktree',tab:0}));
    location.hash='#/projects/cleanup-mixed/settings';
  })()`);

  async function assert(expression,message) {
    if(!await evaluate(expression))throw Error(message);
  }
  async function open(id) {
    await evaluate(`location.hash='#/projects/${id}/settings'`);
    await waitFor(`document.querySelector('.project-settings-profile input[name=name]')?.value===window.worktreeCleanup.state.repos.find(r=>r.repo_id==='${id}').project.name && !document.querySelector('[data-directories]')?.textContent.includes('正在读取')`);
  }
  const layouts=[];
  async function capture(name,width) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:width<=700?844:1000,deviceScaleFactor:1,mobile:false});
    await evaluate('window.scrollTo(0,0)');
    // Let fonts, wrapping, and the path-text custom element finish layout.
    await new Promise(resolve=>setTimeout(resolve,120));
    const layout=await evaluate(`(()=>{
      const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
      return {width:innerWidth,scroll:document.documentElement.scrollWidth,buttons:[...document.querySelectorAll('[data-prune],[data-refresh]')].map(e=>({text:e.textContent,disabled:e.disabled,...rect(e)})),rows:[...document.querySelectorAll('[data-directories] .ui-item')].map(rect)};
    })()`);
    if(layout.scroll>width+1)throw Error(name+' overflows '+width+': '+JSON.stringify(layout));
    if(layout.buttons.some(b=>b.x<0||b.right>width||b.width<1||b.height<(width<=700?44:32)))throw Error(name+' has clipped or undersized directory controls: '+JSON.stringify(layout));
    const [prune,refresh]=layout.buttons;
    if(prune&&refresh&&prune.x<refresh.right&&prune.right>refresh.x&&prune.y<refresh.bottom&&prune.bottom>refresh.y)throw Error(name+' directory controls overlap');
    await screenshot(output+'-'+name+'-'+width+'.png');
    layouts.push({name,...layout});
  }

  await waitFor("document.querySelectorAll('[data-directories] .badge.warn').length===2 && !document.querySelector('[data-prune]').disabled");
  await assert("document.querySelector('[data-prune]').textContent==='清理失联目录' && !document.querySelector('[data-prune-hint]').hidden",'Missing rows do not expose the cleanup action and explanation');
  await assert("document.querySelector('.project-settings-profile .path').textContent==='/fixture/projects/a-long-project-name-for-worktree-cleanup'",'Remembered missing worktree was chosen instead of the healthy root');
  await assert("window.worktreeCleanup.calls.source.at(-1)==='/fixture/projects/a-long-project-name-for-worktree-cleanup' && !window.worktreeCleanup.calls.dirs.some(p=>p.includes('removed'))",'Settings read directory configuration from a missing path');
  for(const width of [1440,390,320])await capture('missing',width);

  // A rejected request leaves both missing rows intact and permits a retry.
  await evaluate("window.worktreeCleanup.failPrune=true;document.querySelector('[data-prune]').click()");
  await waitFor("document.querySelector('[data-message]').textContent.includes('清理请求失败') && !document.querySelector('[data-prune]').disabled");
  await assert("document.querySelectorAll('[data-directories] .badge.warn').length===2 && !document.querySelector('[data-refresh]').disabled",'Failed cleanup changed rows or left actions disabled');
  await capture('failed',390);

  // Keep the retry pending so native double clicks and direct handler calls both exercise the busy guard.
  await evaluate("{const f=window.worktreeCleanup;f.holdPrune=true;const p=document.querySelector('[data-prune]'),r=document.querySelector('[data-refresh]');p.click();p.click();void p.onclick();r.click();void r.onclick();}");
  await waitFor("window.worktreeCleanup.calls.prune.length===2 && document.querySelector('[data-prune]').disabled && document.querySelector('[data-refresh]').disabled");
  await assert("window.worktreeCleanup.calls.discover.length===0 && document.querySelectorAll('[data-directories] .badge.warn').length===2",'Cleanup overlaps refresh or changes rows before completion');
  await evaluate('window.worktreeCleanup.holdPrune=false;window.worktreeCleanup.releasePrune()');
  await waitFor("document.querySelector('[data-message]').textContent.includes('已清理 2 个') && document.querySelector('[data-prune]').disabled && !document.querySelector('[data-refresh]').disabled");
  await assert("document.querySelectorAll('[data-directories] .ui-item').length===1 && !document.querySelector('[data-directories] .badge.warn') && document.querySelector('[data-prune-hint]').hidden",'Successful cleanup did not replace the list with the remaining healthy worktree');
  await assert("document.querySelector('[data-message]').textContent.includes('原有配置已保留')",'Cleanup success omits configuration preservation');
  for(const width of [1440,390,320])await capture('cleaned',width);

  // Healthy and empty Git projects keep refresh available and cleanup unavailable.
  await open('cleanup-healthy');
  await assert("document.querySelector('[data-prune]').disabled && !document.querySelector('[data-refresh]').disabled && document.querySelector('[data-prune-hint]').hidden",'Healthy project offers cleanup');
  await evaluate("{const f=window.worktreeCleanup;f.holdDiscover=true;const r=document.querySelector('[data-refresh]'),p=document.querySelector('[data-prune]');r.click();r.click();void r.onclick();p.click();void p.onclick();}");
  await waitFor("window.worktreeCleanup.calls.discover.length===1 && document.querySelector('[data-refresh]').disabled && document.querySelector('[data-prune]').disabled");
  await assert("window.worktreeCleanup.calls.prune.length===2",'Refresh allowed an overlapping cleanup');
  await evaluate('window.worktreeCleanup.holdDiscover=false;window.worktreeCleanup.releaseDiscover()');
  await waitFor("!document.querySelector('[data-refresh]').disabled && document.querySelector('[data-prune]').disabled");

  await open('cleanup-all');
  await assert("document.querySelectorAll('[data-directories] .badge.warn').length===2 && !document.querySelector('[data-prune]').disabled && document.querySelector('[data-team-source]').hidden",'All-missing project cannot be cleaned');
  await evaluate("document.querySelector('[data-prune]').click()");
  await waitFor("document.querySelector('[data-directories]').textContent.includes('没有已登记的工作目录') && document.querySelector('[data-message]').textContent.includes('已清理 2 个')");
  await assert("document.querySelector('[data-prune]').disabled && !document.querySelector('[data-refresh]').disabled",'Empty project has unusable directory controls');
  await capture('empty',320);
  await open('cleanup-healthy');
  await open('cleanup-all');
  await assert("document.querySelector('[data-directories]').textContent.includes('没有已登记的工作目录') && document.querySelector('[data-prune]').disabled && !document.querySelector('[data-refresh]').disabled && document.querySelector('.project-settings-profile .path').textContent==='/fixture/all-missing'",'Empty Git project cannot remount its settings');
  await evaluate("document.querySelector('[data-refresh]').click()");
  await waitFor("window.worktreeCleanup.calls.discover.length===2 && !document.querySelector('[data-refresh]').disabled");
  await assert("window.worktreeCleanup.calls.discover.at(-1)==='/fixture/all-missing'",'Empty project refresh uses an incorrect fallback path');

  // A directory restored after status was read is reported without claiming removal.
  await open('cleanup-restored');
  await evaluate("window.worktreeCleanup.state.repos.find(r=>r.repo_id==='cleanup-restored').worktrees.main.status='active';document.querySelector('[data-prune]').click()");
  await waitFor("document.querySelector('[data-message]').textContent.includes('没有可清理的失联目录') && document.querySelector('[data-prune]').disabled");
  await assert("document.querySelectorAll('[data-directories] [data-directory]').length===1 && !document.querySelector('[data-directories] .badge.warn')",'Restored directory was removed from the list');

  await open('nongit-cleanup');
  await assert("!document.querySelector('[data-prune],[data-refresh],[data-prune-hint]') && document.querySelectorAll('[data-directories] [data-directory]').length===1",'Non-Git folder offers Worktree cleanup');
  const calls=await evaluate('window.worktreeCleanup.calls');
  await writeFile(output+'-layout.json',JSON.stringify(layouts,null,2)+'\n');
  console.log(JSON.stringify({checks:'cleanup success, failure/retry, busy guards, healthy/all-missing/empty/remount, remembered missing root, restored directory, non-Git',pruneRequests:calls.prune.length,refreshRequests:calls.discover.length,layouts:layouts.length}));
}
