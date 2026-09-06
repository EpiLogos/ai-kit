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
 opened=request('open',space=space,agent_session=session,provider='acceptance-acp',cwd=str(cwd));native=opened['native_session_id']
 # Observe startup separately from the prompted turn.
 cursor=0
 for _ in range(5):
  page=request('read',agent_session=session,after=cursor,limit=64);cursor=page['next_cursor'];time.sleep(.2)
 draft=request('draft',agent_session=session,basis=0,text='Reply with exactly OI_RESIDENT_OK. Do not use tools.')
 request('prompt',agent_session=session,draft_revision=draft['revision'])
 # The CLI client is now gone. Repeated fresh clients consume one durable stream.
 text='';thinking=0;first=None;deadline=time.monotonic()+150;events=0
 while time.monotonic()<deadline:
  page=request('read',agent_session=session,after=cursor,limit=31)
  assert len(page['events'])<=31
  for item in page['events']:
   event=item['event'];events+=1
   if first is None:first=event['kind']
   if event['kind']=='provider':
    host=event['event']
    if 'Signal' in host:
     signal=host['Signal']['kind']
     if signal['kind']=='agent-message-chunk':text+=signal['text']
     if signal['kind']=='agent-thought-chunk':
      assert signal['text']==signal['content']['content']['text'];thinking+=1
    if 'TurnEnded' in host:
     assert host['TurnEnded']['stop'].get('Completed');deadline=0
  cursor=page['next_cursor']
  if deadline==0:break
  time.sleep(.1)
 assert first=='user-message',first
 assert text.strip()=='OI_RESIDENT_OK',text
 assert request('status',agent_session=session)['native_session_id']==native
 # Cross the old ceiling with fresh IPC clients, preserve thinking and cancel.
 draft=request('draft',agent_session=session,basis=page['draft']['revision'],text='Write the integers from 1 to 20000, separated by spaces. Begin immediately. Do not use tools.')
 request('prompt',agent_session=session,draft_revision=draft['revision'])
 long_events=0;long_thoughts=0;previous=0;cancelled=False;terminal=None;deadline=time.monotonic()+150
 while time.monotonic()<deadline and terminal is None:
  page=request('read',agent_session=session,after=cursor,limit=73)
  for item in page['events']:
   assert item['cursor']>cursor
   event=item['event']
   if event['kind']=='provider':
    host=event['event']
    if 'Signal' in host:
     signal=host['Signal'];assert signal['sequence']>previous;previous=signal['sequence'];long_events+=1
     if signal['kind']['kind']=='agent-thought-chunk':
      assert signal['kind']['text']==signal['kind']['content']['content']['text'];long_thoughts+=1
    if 'TurnEnded' in host:terminal=host['TurnEnded']['stop']
  cursor=page['next_cursor']
  if long_events>512 and long_thoughts and not cancelled:
   request('cancel',agent_session=session,reason='Explicit long-stream acceptance cancellation');cancelled=True
  if not page['more']:time.sleep(.05)
 assert cancelled and terminal and 'Cancelled' in terminal,(long_events,long_thoughts,terminal)
 draft=request('draft',agent_session=session,basis=page['draft']['revision'],text='Reply with exactly OI_CONTINUED_OK. Do not use tools.')
 request('prompt',agent_session=session,draft_revision=draft['revision'])
 continuation='';terminal=None;deadline=time.monotonic()+120
 while time.monotonic()<deadline and terminal is None:
  page=request('read',agent_session=session,after=cursor,limit=73)
  for item in page['events']:
   event=item['event']
   if event['kind']=='provider':
    host=event['event']
    if 'Signal' in host and host['Signal']['kind']['kind']=='agent-message-chunk':continuation+=host['Signal']['kind']['text']
    if 'TurnEnded' in host:terminal=host['TurnEnded']['stop']
  cursor=page['next_cursor']
  if not page['more']:time.sleep(.05)
 assert terminal and 'Completed' in terminal and continuation.strip()=='OI_CONTINUED_OK',(terminal,continuation)
 assert request('status',agent_session=session)['native_session_id']==native
 draft=request('draft',agent_session=session,basis=page['draft']['revision'],text='Durable composer across processes')
 reopened=request('read',agent_session=session,after=0,limit=256)
 assert reopened['draft']['text']==draft['text']
 # Compare the encounter presentation to every durable provider thinking byte.
 raw_thinking='';raw_cursor=0
 while True:
  raw=request('read',agent_session=session,after=raw_cursor,limit=256)
  for item in raw['events']:
   event=item['event'];kind=event.get('event',{}).get('Signal',{}).get('kind',{})
   if kind.get('kind')=='agent-thought-chunk':raw_thinking+=kind['text']
  raw_cursor=raw['next_cursor']
  if not raw['more']:break
 pages=[];before=None
 while True:
  view=request('view',agent_session=session,before=before);assert len(view['blocks'])<=16
  for block in view['blocks']:assert len(block['text'].encode())<=16*1024
  pages.insert(0,view['blocks'])
  if not view['more']:break
  before=view['blocks'][0]['id']
 displayed=''.join(block['text'] for page_blocks in pages for block in page_blocks if block['kind']=='thinking')
 assert displayed==raw_thinking and displayed
 # Native process restart preserves authored history/composer, without
 # falsely claiming that the old provider connection has been recovered.
 import signal
 os.killpg(server.pid,signal.SIGTERM);server.wait(timeout=10)
 server=subprocess.Popen([str(binary),'-C',str(cwd),'encounter-serve','--socket',str(sock)],env=env,stdout=log,stderr=log,start_new_session=True)
 deadline=time.monotonic()+20
 while True:
  try:
   health=request('health');break
  except Exception:
   if time.monotonic()>deadline:raise
   time.sleep(.05)
 recovered=request('read',agent_session=session,after=0,limit=31)
 assert recovered['draft']==draft and recovered['events'],recovered
 unavailable=cli('encounter','--socket',str(sock),'--request-json',json.dumps({'action':'status','agent_session':session}))
 assert unavailable['ok']==False and 'no resident native session' in unavailable['error']['message']
 result={'result':'passed','root':str(root),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'native_session_id':native,'events':events,'long_events':long_events,'long_thinking_updates':long_thoughts,'same_session_continuation':continuation.strip(),'thinking_updates':thinking,'first_event':first,'canonical_draft_revision':draft['revision']}
 print(json.dumps(result));(base/'resident-acceptance.json').write_text(json.dumps(result,indent=2))
finally:
 import signal
 os.killpg(server.pid,signal.SIGTERM)
 try:server.wait(timeout=10)
 except subprocess.TimeoutExpired:server.kill();server.wait()
