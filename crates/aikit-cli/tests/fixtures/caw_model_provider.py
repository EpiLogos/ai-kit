"""Controlled Pi RPC process. Identity/credential assertions are not model quality proof."""
import argparse
import json
import os
from pathlib import Path
import sys

parser = argparse.ArgumentParser()
parser.add_argument('mode')
parser.add_argument('audit', type=Path)
parser.add_argument('--provider', required=True)
parser.add_argument('--model', required=True)
args = parser.parse_args()
turns = 0

def emit(value):
    print(json.dumps(value), flush=True)

def reply(m, data=None):
    emit({'type': 'response', 'id': m['id'], 'command': m['type'], 'success': True, 'data': data or {}})

for line in sys.stdin:
    m = json.loads(line)
    with args.audit.open('a') as f:
        f.write(json.dumps(m) + '\n')
    if m['type'] == 'get_state':
        native = 'unselected-other-model' if args.mode == 'wrong-state' or (args.mode == 'drift' and turns) else args.model
        reply(m, {'sessionId': 'model-native-stable', 'isStreaming': False,
                  'isCompacting': False, 'pendingMessageCount': 0,
                  'model': {'provider': args.provider, 'id': native, 'name': 'Controlled native model'}})
    elif m['type'] == 'prompt':
        turns += 1
        facts = {'standing': 'CONTROLLED_NATIVE', 'native_provider': args.provider, 'native_id': args.model,
                 'selected_context': 'SELECTED_CONTEXT_root' in m['message'], 'cwd': os.getcwd(),
                 'credential_delivered': os.environ.get('CAW_NATIVE_API_KEY') == 'CONTROLLED_MODEL_SECRET_NOT_USER_DATA',
                 'source_credential_leaked': 'CAW_SOURCE_API_KEY' in os.environ,
                 'unrelated_credential_leaked': 'UNRELATED_API_KEY' in os.environ,
                 'central_token_leaked': 'CENTRAL_NATIVE_TOKEN' in os.environ,
                 'workcell_token_leaked': 'WORKCELL_CONTROL_TOKEN' in os.environ}
        args.audit.with_suffix('.facts.json').write_text(json.dumps(facts))
        reply(m)
        emit({'type': 'message_update', 'assistantMessageEvent': {'type': 'text_delta', 'delta': 'CONTROLLED_NATIVE_MODEL_REPLY'}})
        emit({'type': 'message_end', 'message': {'role': 'assistant', 'provider': args.provider,
              'model': 'contradictory-model' if args.mode == 'wrong-response' else args.model, 'stopReason': 'stop'}})
        emit({'type': 'agent_settled'})
    elif m['type'] in ('abort', 'clear_queue'):
        reply(m)
