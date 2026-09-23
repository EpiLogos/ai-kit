"""Controlled peer for real EncounterService/ACP handlers. Never a production provider."""
import json
import os
import sys
import time

native = "controlled-native"
model = "test/a"
effort = "low"
permission_mode = "default"
waiting = None
mode = sys.argv[1] if len(sys.argv) > 1 else "normal"

def emit(value):
    # Deliberately split wire bytes, not just semantically complete updates.
    wire = (json.dumps(value) + "\n").encode()
    cut = max(1, len(wire) // 3)
    os.write(1, wire[:cut])
    time.sleep(0.002)
    os.write(1, wire[cut:])

def result(ident, value):
    emit({"jsonrpc": "2.0", "id": ident, "result": value})

def config():
    return [
        {"id": "model", "category": "model", "type": "select", "name": "Model", "currentValue": model,
         "options": [{"value": "test/a", "name": "A"}, {"value": "test/b", "name": "B"}]},
        {"id": "reasoning_effort", "type": "select", "name": "Reasoning", "currentValue": effort,
         "options": [{"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]},
    ]

def modes():
    return {"currentModeId": permission_mode, "availableModes": [
        {"id": "default", "name": "Default", "description": "Ask before edits."},
        {"id": "accept_edits", "name": "Accept Edits"},
        {"id": "plan", "name": "Plan"},
    ]}

def update(value):
    emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": native, "update": value}})

def text(value):
    update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": value}})

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    ident = message.get("id")
    if method == "initialize":
        # Resume advertised: the encounter reconnect rides the capability-gated
        # session/resume (no history replay), not session/load.
        result(ident, {"protocolVersion": 1, "agentCapabilities": {"loadSession": True,
                        "sessionCapabilities": {"resume": True}}})
    elif method in ("session/new", "session/load", "session/resume"):
        native = message.get("params", {}).get("sessionId", native)
        result(ident, {"sessionId": native, "configOptions": config(), "modes": modes()})
    elif method == "session/set_mode":
        requested = message["params"]["modeId"]
        if mode == "lost-mode-ack":
            os._exit(0)
        if requested not in [m["id"] for m in modes()["availableModes"]]:
            emit({"jsonrpc": "2.0", "id": ident, "error": {"code": -32602, "message": "unknown mode"}})
            continue
        permission_mode = requested
        # Like real agents, report the change as a session update, then answer.
        update({"sessionUpdate": "current_mode_update", "currentModeId": permission_mode})
        result(ident, {})
    elif method == "session/set_config_option":
        params = message["params"]
        if params["configId"] == "model":
            model = params["value"]
        elif params["configId"] == "reasoning_effort":
            effort = params["value"]
        if mode == "lost-model-ack":
            os._exit(0)
        result(ident, {"configOptions": config()})
    elif method == "session/prompt":
        prompt = "".join(item.get("text", "") for item in message["params"]["prompt"])
        if mode == "echo-prompt":
            text(prompt)
            result(ident, {"stopReason": "end_turn"})
        elif prompt == "deny":
            waiting = ident
            update({"sessionUpdate": "tool_call", "toolCallId": "read-1", "title": "Read a source", "kind": "read", "status": "pending"})
            emit({"jsonrpc": "2.0", "id": "permission-1", "method": "session/request_permission", "params": {
                "sessionId": native, "toolCall": {"toolCallId": "read-1", "title": "Read a source", "kind": "read", "status": "pending"},
                "options": [{"optionId": "deny", "name": "Deny", "kind": "reject_once"}, {"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}})
        elif prompt == "cancel":
            waiting = ident
            text("Started; waiting for cancellation.")
        elif prompt == "plan-yourself":
            permission_mode = "plan"
            update({"sessionUpdate": "current_mode_update", "modeId": permission_mode})
            text("Switched myself to plan.")
            result(ident, {"stopReason": "end_turn"})
        elif prompt == "disconnect":
            text("Partial answer before disconnect.")
            os._exit(0)
        else:
            text("First partial ")
            text("then final.")
            result(ident, {"stopReason": "end_turn"})
    elif method == "session/cancel" and waiting is not None:
        result(waiting, {"stopReason": "cancelled"})
        waiting = None
    elif ident == "permission-1" and waiting is not None:
        outcome = message["result"]["outcome"]
        denied = outcome.get("optionId") == "deny" or outcome.get("outcome") == "cancelled"
        update({"sessionUpdate": "tool_call_update", "toolCallId": "read-1", "status": "failed" if denied else "completed"})
        text("Permission denied; no source was read." if denied else "Permission allowed.")
        result(waiting, {"stopReason": "end_turn"})
        waiting = None
