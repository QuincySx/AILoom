import pathlib,subprocess,os,json,socket,time,http.client,re
T=pathlib.Path('/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/ailoom-standards-20260916-4fzllejs');B=str(pathlib.Path('target/debug/ailoom').resolve());E=dict(os.environ,HOME=str(T/'home'),XDG_STATE_HOME='',XDG_DATA_HOME='',AILOOM_DATA_ROOT=str(T/'data'),AILOOM_STORE_ROOT=str(T/'store'),GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null');s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close();log=open(T/'restart.log','w');p=subprocess.Popen([B,'console','--port',str(port),'--no-open'],cwd=T/'repo',env=E,stdout=log,stderr=log)
for _ in range(100):
 m=re.search(r'token=([a-zA-Z0-9-]+)',(T/'restart.log').read_text())
 if m:break
 time.sleep(.05)
def api(path,body=None):
 c=http.client.HTTPConnection('127.0.0.1',port,timeout=15);c.request('GET' if body is None else 'POST',path,body=None if body is None else json.dumps(body),headers={'X-AILoom-Session':m[1]});r=c.getresponse();v=json.loads(r.read());c.close();return r.status,v
try:
 plan=next((T/'data/console/jobs').glob('plan-*.json')).stem
 print('GET old plan',api('/api/jobs/'+plan)[0]);print('list after restart',api('/api/jobs'));print('apply old plan',api('/api/jobs/apply',{'plan_job_id':plan}))
finally:
 api('/api/shutdown',{});p.wait(timeout=10);log.close()
