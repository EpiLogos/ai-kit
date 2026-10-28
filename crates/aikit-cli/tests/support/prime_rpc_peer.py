"""Controlled Prime Agent 0.9.4 RPC peer for real EncounterService handlers.

Never a production provider and never a model. It answers get_state and
switch_session on stdio JSONL in the pinned wire's shapes, keeps one native
session file in its sessions directory the way Prime's SessionManager does,
and records each launch's argv and each received command for the test.
"""
import json
import os
import sys
import uuid

record, sessions = sys.argv[1], sys.argv[2]


def note(value):
    with open(record, "a", encoding="utf-8") as out:
        out.write(json.dumps(value) + "\n")


note({"argv": sys.argv[1:]})

# A launched Prime process starts on a fresh native session of its own.
current = str(uuid.uuid4())
current_file = os.path.join(sessions, current + ".jsonl")
os.makedirs(sessions, exist_ok=True)
with open(current_file, "w", encoding="utf-8") as header:
    header.write(json.dumps({"type": "session", "id": current, "cwd": os.getcwd()}) + "\n")


def emit(value):
    sys.stdout.write(json.dumps(value) + "\n")
    sys.stdout.flush()


def state():
    return {
        "sessionId": current,
        "sessionFile": current_file,
        "isStreaming": False,
        "isCompacting": False,
        "sessionActions": {"queuedCount": 0},
        "model": {"provider": "zai", "id": "glm-5.3-flash", "name": "GLM-5.3-Flash"},
    }


for line in sys.stdin:
    if not line.strip():
        continue
    command = json.loads(line)
    note({"command": command.get("type"), "payload": command})
    kind, ident = command.get("type"), command.get("id")
    if kind == "get_state":
        emit({"type": "response", "id": ident, "command": kind, "success": True, "data": state()})
    elif kind == "switch_session":
        path = command["sessionPath"]
        with open(path, encoding="utf-8") as existing:
            head = json.loads(existing.readline())
        current, current_file = head["id"], path
        emit({"type": "response", "id": ident, "command": kind, "success": True,
              "data": {"cancelled": False}})
    else:
        emit({"type": "response", "id": ident, "command": kind, "success": False,
              "error": "controlled peer answers state and session switching only"})
