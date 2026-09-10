#!/usr/bin/env python3
"""Controlled protocol fixture. FIXTURE_REPLY is not a model/harness response."""
import json, os, sys, threading, time
mode, log = sys.argv[1:3]
write_lock = threading.Lock()
pending = {}
def emit(value):
    with write_lock:
        print(json.dumps(value), flush=True)
def reply(message, result=None, error=None):
    if mode == 'pi':
        emit(dict(type='response', id=message['id'], command=message['type'], success=error is None, **({'error':str(error)} if error else {'data':result or {}})))
    else:
        emit(dict(jsonrpc='2.0', id=message['id'], **({'error':{'code':-32000,'message':str(error)}} if error else {'result':result})))
def turn(message, text):
    if 'CONTROLLED_DISCONNECT' in text:
        os._exit(7)
    if 'CONTROLLED_DENIAL' in text:
        reply(message,error='CONTROLLED_DENIAL'); return
    if 'CONTROLLED_SLOW' in text:
        time.sleep(0.25)
    if mode == 'pi':
        reply(message)
        emit({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':'FIXTURE_REPLY'}})
        emit({'type':'message_end','message':{'role':'assistant','stopReason':'stop'}})
        emit({'type':'agent_settled'})
    else:
        native=message['params']['sessionId']
        emit({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':native,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'FIXTURE_REPLY'}}}})
        reply(message, {'stopReason':'end_turn'})
for line in sys.stdin:
    m=json.loads(line)
    with open(log,'a',encoding='utf-8') as f: f.write(json.dumps(m)+'\n')
    method=m.get('method',m.get('type'))
    if method=='initialize': reply(m,{'protocolVersion':1,'agentCapabilities':{'loadSession':True}})
    elif method=='session/new': reply(m,{'sessionId':'fixture-native-stable'})
    elif method=='session/load':
        emit({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':m['params']['sessionId'],'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'FIXTURE_REPLAY_BEFORE_LOAD'}}}})
        reply(m,None)
    elif method=='get_state': reply(m,{'sessionId':'fixture-pi-stable','isStreaming':False,'isCompacting':False,'pendingMessageCount':0})
    elif method in ('session/prompt','prompt'):
        text=m['message'] if mode=='pi' else ''.join(p.get('text','') for p in m['params']['prompt'])
        threading.Thread(target=turn,args=(m,text),daemon=True).start()
    elif method in ('abort','clear_queue'): reply(m)
