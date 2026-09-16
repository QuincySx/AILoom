const fs=require('fs'),vm=require('vm');
let src=fs.readFileSync('src/console/web.rs','utf8').split('<script>')[1].split('</script>')[0].replace('{token:?}',JSON.stringify('review-token')).replaceAll('{{','{').replaceAll('}}','}');
src=src.slice(0,src.indexOf('// ---------- 启动：'));
const nodes={'#h-claude':{checked:false},'#h-codex':{checked:false},'#capMsg':{},'#dirInput':{value:'/repo-B'}};
const calls=[];
const ctx=vm.createContext({window:{},document:{querySelector:s=>nodes[s]||(nodes[s]={}),querySelectorAll:()=>[]},console});
vm.runInContext(src,ctx);
vm.runInContext(`render=()=>{}; api=async(verb,path,body)=>{ calls.push({verb,path,body:JSON.parse(JSON.stringify(body||{}))});return {revision:1}; };`,Object.assign(ctx,{calls}));
(async()=>{
 await vm.runInContext('window.saveCapabilities()',ctx);
 console.log('UNCHECK_BOTH',JSON.stringify(calls));calls.length=0;
 vm.runInContext(`draft.repo={repo_root:'/repo-A'};draft.planJob='plan-A';draft.applyJob='apply-A';draft.capSaved=true;`,ctx);
 await vm.runInContext('window.approveDir()',ctx);
 console.log('SWITCH_DIRECTORY',vm.runInContext('JSON.stringify(draft)',ctx));
 calls.length=0;
 vm.runInContext(`api=async(verb,path,body)=>{calls.push({verb,path,body:JSON.parse(JSON.stringify(body||{}))});if(calls.length===1) throw Object.assign(new Error('conflict'),{status:409,data:{current_revision:7}});return {revision:8};}`,ctx);
 await vm.runInContext('saveDraft()',ctx);await vm.runInContext('saveDraft()',ctx);
 console.log('CONFLICT_RETRY',JSON.stringify(calls));
})().catch(e=>{console.error(e);process.exitCode=1});
