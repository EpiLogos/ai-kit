#!/usr/bin/env python3
"""Controlled ACP fixture for conversation requests. Its replies are protocol
FIXTURES, never model output: each reply is derived from the recipient's own name
(from the prompt's "You are X,") and the ASKED entry it was actually given, so a
test can tell whose reply is whose and which question it answers. A prompt that
contains SLOW_<n> delays its answer n tenths of a second, letting two recipients'
turns cross."""
import json, os, re, sys, threading, time
mode, log = sys.argv[1:3]
write_lock = threading.Lock()
def emit(value):
    with write_lock:
        print(json.dumps(value), flush=True)
def reply(message, result=None):
    emit(dict(jsonrpc='2.0', id=message['id'], result=result))
def answer(text):
    me = re.search(r'You are ([^,]+),', text)
    asked = re.search(r'ASKED \[\d+\] [^:]*: (.*)', text, re.S)
    return (me.group(1) if me else 'someone'), (asked.group(1).strip() if asked else text.strip())
def turn(message, text):
    slow = re.search(r'SLOW_(\d+)', text)
    if slow:
        time.sleep(int(slow.group(1)) / 10.0)
    me, asked = answer(text)
    native = message['params']['sessionId']
    for piece in (f'{me} considers: ', asked.split('\n')[0]):
        emit({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':native,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':piece}}}})
        time.sleep(0.05)
    reply(message, {'stopReason':'end_turn'})
for line in sys.stdin:
    m = json.loads(line)
    with open(log, 'a', encoding='utf-8') as f: f.write(json.dumps(m) + '\n')
    method = m.get('method')
    if method == 'initialize': reply(m, {'protocolVersion':1,'agentCapabilities':{'loadSession':True,'sessionCapabilities':{'resume':True}}})
    elif method == 'session/new': reply(m, {'sessionId':'fixture-native-stable'})
    elif method == 'session/resume': reply(m, {'sessionId':m['params']['sessionId']})
    elif method == 'session/prompt':
        text = ''.join(p.get('text','') for p in m['params']['prompt'])
        threading.Thread(target=turn, args=(m, text), daemon=True).start()
