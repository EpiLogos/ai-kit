#!/usr/bin/env python3
"""Explicit actual ACP acceptance; requires configured native argv and binary.
No provider subprocesses or protocol responses are simulated.
"""
import os,json,subprocess,tempfile,time,socket,hashlib
from pathlib import Path
base=Path(os.environ.get('AIKIT_ACCEPTANCE_OUTPUT', tempfile.mkdtemp(prefix='aikit-acp-receipt-')));base.mkdir(parents=True,exist_ok=True)
binary=Path(os.environ['AIKIT_SESSION_SPACE_BINARY']).resolve()
argv=json.loads(os.environ['AIKIT_ACP_NATIVE_ARGV'])
assert argv and all(isinstance(arg,str) for arg in argv)
root=Path(tempfile.mkdtemp(prefix='oi-acp-resident-',dir='/tmp'));cwd=root/'work';cwd.mkdir();home=root/'home'
env=dict(os.environ,AIKIT_HOME=str(home))
space='session-space/resident-acceptance';session='agent-session/resident-acceptance'
def cli(*args):
 p=subprocess.run([str(binary),'-C',str(cwd),*args],env=env,text=True,capture_output=True,timeout=150)
 if p.returncode:raise RuntimeError(p.stderr)
 return json.loads(p.stdout)
def apply(preview):
 f=root/'preview.json';f.write_text(json.dumps(preview));return cli('apply','--preview-json','@'+str(f))
apply(cli('create',space,'--label','Real resident ACP acceptance'))
apply(cli('stage','--space',space,'--intent-json',json.dumps({'operation':'attach-agent-session','attachment':{'agent_session':session,'purpose':'Native streaming acceptance','provenance':['explicit isolated native acceptance']}})))
cli('encounter-configure','--provider-json',json.dumps({'id':'acceptance-acp','label':'Explicit native acceptance ACP','argv':argv}))
log=(base/'resident-server.log').open('w');server=subprocess.Popen([str(binary),'-C',str(cwd),'encounter-serve','--socket',str(root/'ipc'/'owner.sock')],env=env,stdout=log,stderr=log,start_new_session=True)
sock=root/'ipc'/'owner.sock'
def request(action,**fields):
 r=cli('encounter','--socket',str(sock),'--request-json',json.dumps({'action':action,**fields}))
 if not r.get('ok'):raise RuntimeError(r)
 return r['data']
try:
 deadline=time.monotonic()+20
 while not sock.exists():
  if server.poll() is not None:raise RuntimeError('Resident server exited')
  if time.monotonic()>deadline:raise TimeoutError('Resident socket')
  time.sleep(.05)
 other='agent-session/concurrent-second'
 apply(cli('stage','--space',space,'--intent-json',json.dumps({'operation':'attach-agent-session','attachment':{'agent_session':other,'purpose':'Concurrent native session acceptance','provenance':['explicit isolated native acceptance']}})))
 from concurrent.futures import ThreadPoolExecutor
 def turn(identity,expected):
  opened=request('open',space=space,agent_session=identity,provider='acceptance-acp',cwd=str(cwd))
  time.sleep(1)
  cursor=request('read',agent_session=identity,after=0,limit=256)['next_cursor']
  draft=request('draft',agent_session=identity,basis=0,text='Reply with exactly '+expected+'. Do not use tools.')
  request('prompt',agent_session=identity,draft_revision=draft['revision'])
  text='';end=False;deadline=time.monotonic()+150
  while time.monotonic()<deadline and not end:
   page=request('read',agent_session=identity,after=cursor,limit=31)
   for item in page['events']:
    event=item['event'];host=event.get('event',{})
    signal=host.get('Signal',{}).get('kind',{})
    if signal.get('kind')=='agent-message-chunk':text+=signal['text']
    if 'TurnEnded' in host:
     assert 'Completed' in host['TurnEnded']['stop'],host
     end=True
   cursor=page['next_cursor'];time.sleep(.05)
  assert end and text.strip()==expected,text
  view=request('view',agent_session=identity)
  assert view['schema']=='aikit.encounter-view/v1'
  assert view['connection']['native_session_id']==opened['native_session_id']
  assert any(a['ref']=='aikit.encounter.prompt' and a['enabled'] for a in view['actions'])
  assert expected in ''.join(b['text'] for b in view['blocks'])
  return {'canonical':identity,'native':opened['native_session_id'],'reply':text,'view':view}
 with ThreadPoolExecutor(max_workers=2) as pool:
  results=list(pool.map(lambda pair:turn(*pair),[(session,'OI_FIRST_CONCURRENT'),(other,'OI_SECOND_CONCURRENT')]))
 assert results[0]['native']!=results[1]['native']
 assert 'OI_SECOND_CONCURRENT' not in ''.join(b['text'] for b in results[0]['view']['blocks'])
 receipt={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'results':results,'root':str(root)}
 (base/'concurrent-acceptance.json').write_text(json.dumps(receipt,indent=2))
 print(json.dumps(receipt,indent=2))
finally:
 import signal
 os.killpg(server.pid,signal.SIGTERM)
 try:server.wait(timeout=10)
 except subprocess.TimeoutExpired:os.killpg(server.pid,signal.SIGKILL);server.wait()
 log.close()
