"""Actual protocol child; its response remains CONTROLLED_NATIVE, not a model."""
import json
import os
from pathlib import Path
import sys

log, human, loose = map(Path, sys.argv[1:4])

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    with log.open('a') as stream:
        stream.write(json.dumps(message) + '\n')
    method = message.get('method')
    result = {}
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentCapabilities': {'loadSession': True}}
    elif method in ('session/new', 'session/load'):
        result = {'sessionId': 'task-native-stable'}
    elif method == 'session/prompt':
        text = ''.join(item.get('text', '') for item in message['params']['prompt'])
        denied = []
        for target in (human, loose):
            try:
                target.write_text('FORBIDDEN')
                denied.append(False)
            except PermissionError:
                denied.append(True)
        now = next(line.removeprefix('Task output directory: ') for line in text.splitlines()
                   if line.startswith('Task output directory: '))
        evidence = {'standing': 'CONTROLLED_NATIVE', 'denied': denied,
                    'cwd': os.getcwd(), 'central_token_present': 'CENTRAL_NATIVE_TOKEN' in os.environ,
                    'selected_context': 'SELECTED_CONTEXT' in text}
        (log.parent / 'result.json').write_text(json.dumps(evidence))
        (Path(now) / 'return.txt').write_text('ACTUAL_TASK_RETURN')
        emit({'jsonrpc': '2.0', 'method': 'session/update', 'params': {
            'sessionId': message['params']['sessionId'], 'update': {
                'sessionUpdate': 'agent_message_chunk',
                'content': {'type': 'text', 'text': 'CONTROLLED_NATIVE_RETURN'}}}})
        result = {'stopReason': 'end_turn'}
    if 'id' in message:
        emit({'jsonrpc': '2.0', 'id': message['id'], 'result': result})
