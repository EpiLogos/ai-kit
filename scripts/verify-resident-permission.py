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
 other='agent-session/permission-other'
 apply(cli('stage','--space',space,'--intent-json',json.dumps({'operation':'attach-agent-session','attachment':{'agent_session':other,'purpose':'Wrong-session refusal','provenance':['isolated native acceptance']}})))
 opened=request('open',space=space,agent_session=session,provider='acceptance-acp',cwd=str(cwd))
 (cwd/'consent-input.txt').write_text('OI_REAL_TOOL_CONSENT')
 draft=request('draft',agent_session=session,basis=0,text='Use a shell tool to write exactly OI_APPROVED_TOOL_WRITE into consent-output.txt in the current directory. This is an explicitly authorized isolated acceptance file. Request native permission before writing, then read the file and respond with its exact contents.')
 request('prompt',agent_session=session,draft_revision=draft['revision'])
 deadline=time.monotonic()+120;permissions=[];cursor=0;events=[];answered=False;ended=False
 while time.monotonic()<deadline and not ended:
  view=request('view',agent_session=session)
  for pending in view['permissions']:
   if answered:continue
   assert pending['tool_call']==pending['raw']['toolCall']
   assert pending['choices']
   unknown=cli('encounter','--socket',str(sock),'--request-json',json.dumps({'action':'permission','agent_session':session,'request_id':pending['native_request_id'],'decision':{'outcome':'selected','option_id':'not-offered'}}))
   assert not unknown['ok']
   wrong=cli('encounter','--socket',str(sock),'--request-json',json.dumps({'action':'permission','agent_session':other,'request_id':pending['native_request_id'],'decision':{'outcome':'cancelled'}}))
   assert not wrong['ok']
   option=next(c for c in pending['choices'] if c.get('kind')=='allow_once')
   response=request('permission',agent_session=session,request_id=pending['native_request_id'],decision={'outcome':'selected','option_id':option['option_id']})
   assert response['sent']
   replay=cli('encounter','--socket',str(sock),'--request-json',json.dumps({'action':'permission','agent_session':session,'request_id':pending['native_request_id'],'decision':{'outcome':'selected','option_id':option['option_id']}}))
   assert not replay['ok']
   permissions.append(pending);answered=True
  page=request('read',agent_session=session,after=cursor,limit=73)
  events+=page['events'];cursor=page['next_cursor']
  for item in page['events']:
   host=item['event'].get('event',{})
   if 'TurnEnded' in host:ended=True
  time.sleep(.1)
 assert ended and answered,{'view':view,'events':events}
 assert any(e['event']['kind']=='provider-permission-response-sent' for e in events)
 tool_updates=[e['event'].get('event',{}).get('Signal',{}).get('kind',{}) for e in events]
 tool_updates=[e['payload'] for e in tool_updates if e.get('kind')=='tool-call']
 assert tool_updates
 if os.environ.get('AIKIT_EXPECT_TOOL_WRITE')=='1':
  assert (cwd/'consent-output.txt').read_text()=='OI_APPROVED_TOOL_WRITE'
  assert any(t.get('status')=='completed' for t in tool_updates)
 else:
  assert not (cwd/'consent-output.txt').exists()
  assert any(t.get('status')=='failed' for t in tool_updates)

 receipt={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'root':str(root),'permissions':permissions,'events':events,'view':request('view',agent_session=session)}
 (base/'permission-acceptance.json').write_text(json.dumps(receipt,indent=2))
 print(json.dumps(receipt,indent=2))
finally:
 import signal
 os.killpg(server.pid,signal.SIGTERM)
 try:server.wait(timeout=10)
 except subprocess.TimeoutExpired:os.killpg(server.pid,signal.SIGKILL);server.wait()
 log.close()
