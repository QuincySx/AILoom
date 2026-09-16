import tempfile,pathlib,subprocess,os,json,socket,time,http.client,re
B=str(pathlib.Path('target/debug/ailoom').resolve()); T=pathlib.Path(tempfile.mkdtemp(prefix='ailoom-standards-20260916-')); W=T/'repo';W.mkdir(); H=T/'home';H.mkdir();D=T/'data';
E=dict(os.environ,HOME=str(H),XDG_STATE_HOME='',XDG_DATA_HOME='',AILOOM_DATA_ROOT=str(D),AILOOM_STORE_ROOT=str(T/'store'),GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null')
def git(*a):return subprocess.run(['git',*a],cwd=W,env=E,check=True,capture_output=True)
def cli(*a):
 p=subprocess.run([B,'--json',*a],cwd=W,env=E,capture_output=True,text=True); return p.returncode,json.loads(p.stdout)['result'] if p.stdout else p.stderr
git('init','-q'); (W/'AGENTS.md').write_text('Company baseline\n');git('add','AGENTS.md')
F=T/'instructions.md';F.write_text('personal first\n');print('setup',cli('personal','--action','instructions','--file',str(F)),flush=True);print('host',cli('personal','--action','select','--host','codex','--state','enable'),flush=True)
s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close(); log=open(T/'server.log','w');p=subprocess.Popen([B,'console','--port',str(port),'--no-open'],cwd=W,env=E,stdout=log,stderr=log)
for _ in range(100):
 txt=(T/'server.log').read_text();m=re.search(r'token=([a-zA-Z0-9-]+)',txt)
 if m:break
 time.sleep(.05)
token=m[1]
def api(path,body=None):
 c=http.client.HTTPConnection('127.0.0.1',port,timeout=15); c.request('GET' if body is None else 'POST',path,body=None if body is None else json.dumps(body),headers={'X-AILoom-Session':token,'Content-Type':'application/json'});r=c.getresponse();v=json.loads(r.read());c.close();return r.status,v
def wait(j):
 for _ in range(200):
  v=api('/api/jobs/'+j)[1]
  if v.get('status') not in ['queued','running']:return v
  time.sleep(.05)
 raise RuntimeError('timeout')
def apply():
 plan=wait(api('/api/jobs/plan',{'root':str(W)})[1]['job_id']);job=api('/api/jobs/apply',{'plan_job_id':plan['id']})[1]['job_id'];return wait(job)
try:
 print('approve',api('/api/fs/approve',{'path':str(W)}),flush=True)
 a=apply();print('apply1',a['status'],a['result'],flush=True)
 target=W/'AGENTS.override.md';target.write_text('USER POST APPLY EDIT\n');print('undo user edit',api('/api/jobs/undo',{'id':a['id']}),'target_exists',target.exists(),flush=True)
 # repair stale manifest using ordinary personal sync (Restore)
 print('sync',cli('personal','--action','sync'),flush=True)
 git('add','-f','AGENTS.override.md');before=(W/'.git/index').read_bytes();print('tracked plan',cli('personal','--action','plan'),flush=True);print('tracked sync',cli('personal','--action','sync'),'file_exists',target.exists(),'index_unchanged',before==(W/'.git/index').read_bytes(),flush=True)
 # Workflow export escaping approved repository
 w=api('/api/workflows/new',{'name':'test'})[1];print('workflow',w,flush=True);art=api('/api/workflows/artifact',{'id':w['id'],'stage':'spec','title':'Spec','content':'EXPORTED OUTSIDE ROOT'})[1];print('artifact',art,flush=True)
 outside=T/'not-approved'/'victim.txt';outside.parent.mkdir();outside.write_text('USER FILE');print('export outside',api('/api/workflows/export',{'id':w['id'],'artifact_id':art['id'],'target':str(outside),'execute':True}),'content',outside.read_text(),flush=True)
finally:
 api('/api/shutdown',{});p.wait(timeout=10);log.close();print('fixture',T,flush=True)
