#!/usr/bin/env python3
"""Controlled real ctrl + aikit + ACP process join. No installed/live-model claim.

Only a new temporary World/home is changed. The private native human authority
below is a test fixture, never an input credential or production configuration.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import threading
import time


def peer(base: Path) -> None:
    """Test-only fragmented ACP peer. It performs no inference."""
    lock = threading.Lock()
    cancel = threading.Event()
    permission = threading.Event()
    decision: list[dict] = []
    native = "native-controlled-source-session"

    def emit(value: dict) -> None:
        encoded = (json.dumps(value) + "\n").encode()
        with lock:
            sys.stdout.buffer.write(encoded[:7]); sys.stdout.buffer.flush()
            time.sleep(0.002)
            sys.stdout.buffer.write(encoded[7:]); sys.stdout.buffer.flush()

    def effect(value: dict) -> None:
        with lock:
            with (base / "peer-effects.jsonl").open("a") as out:
                out.write(json.dumps(value) + "\n")

    def chunk(text: str) -> None:
        emit({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":native,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}})

    def turn(request: dict) -> None:
        prompt = json.dumps(request["params"]["prompt"])
        effect({"kind":"prompt", "digest":hashlib.sha256(prompt.encode()).hexdigest()})
        if "CONTROLLED-DISCONNECT" in prompt:
            chunk("partial-before-disconnect")
            os._exit(0)
        if "CONTROLLED-DENIAL" in prompt:
            emit({"jsonrpc":"2.0","id":"permission-controlled","method":"session/request_permission","params":{"sessionId":native,"toolCall":{"toolCallId":"controlled-tool","title":"Controlled denied operation"},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"reject","name":"Reject once","kind":"reject_once"}]}})
            if not permission.wait(10):
                os._exit(4)
            assert decision[0]["outcome"] == {"outcome":"selected","optionId":"reject"}, decision
            effect({"kind":"denied","operation_performed":False})
            chunk("controlled permission denied; operation not performed")
        elif "CONTROLLED-CANCEL" in prompt:
            chunk("partial-before-cancel")
            assert cancel.wait(10), "cancel was not forwarded"
            emit({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"cancelled"}})
            return
        else:
            # The unpredictable source is not in the prompt. This peer uses
            # its explicit test-only bound path, not a mocked provider result.
            body = (base / "source.txt").read_text()
            assert body not in prompt
            chunk(body[:9]); chunk(body[9:])
        emit({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}})

    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        if method == "initialize":
            assert not os.getenv("CENTRAL_NATIVE_TOKEN"), "human credential reached provider"
            effect({"kind":"initialize","human_token_absent":True})
            emit({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True}}})
        elif method in ("session/new","session/load"):
            effect({"kind":method,"requested":request.get("params",{}).get("sessionId")})
            returned = "different-native-session" if method == "session/load" and (base / "wrong-load").exists() else native
            emit({"jsonrpc":"2.0","id":request["id"],"result":{"sessionId":returned}})
        elif method == "session/prompt":
            cancel.clear(); permission.clear(); decision.clear()
            threading.Thread(target=turn,args=(request,),daemon=True).start()
        elif method == "session/cancel":
            effect({"kind":"cancel"}); cancel.set()
        elif request.get("id") == "permission-controlled":
            decision.append(request["result"]); permission.set()


def exercise(ctrl: Path, aikit: Path) -> dict:
    checks: list[str] = []
    with tempfile.TemporaryDirectory(prefix="native-agent-join-") as temp:
        base = Path(temp); root = base / "Central"
        (base / "home").mkdir()
        env = {k:v for k,v in os.environ.items() if not (k.startswith("CENTRAL_") or k.startswith("AIKIT_"))}
        env.update(HOME=str(base/"home"),AIKIT_HOME=str(base/"aikit"),CENTRAL_CTRL_BIN=str(ctrl),CENTRAL_ROOT=str(root))
        processes: list[subprocess.Popen] = []
        logs = (base/"owner.log").open("w+")
        def command(argv: list[str], expect: bool = True) -> dict | list:
            run = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=25)
            try: value = json.loads(run.stdout)
            except json.JSONDecodeError:
                value = {"ok":False,"error":run.stderr[-2000:]}
            ok = run.returncode == 0 and (not isinstance(value,dict) or value.get("ok",True) is not False)
            assert ok == expect, f"{argv[1:4]} rc={run.returncode}: {value} {run.stderr[-500:]}"
            if expect and isinstance(value,dict) and value.get("ok") is True and "data" in value: return value["data"]
            return value
        def central(action: str, data: dict, expect: bool = True):
            return command([str(ctrl),"--json","--root",str(root),"action","run",action,json.dumps({"scope":"root",**data})],expect)
        def ai(*args: str, expect: bool = True):
            return command([str(aikit),"session-space","-C",str(root),*args],expect)
        ipc = base/"ipc"; ipc.mkdir(mode=0o700); sock = ipc/"owner.sock"
        def request(action: str, expect: bool = True, **data):
            with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as stream:
                stream.settimeout(20); stream.connect(str(sock))
                stream.sendall((json.dumps({"action":action,**data})+"\n").encode())
                with stream.makefile("rb") as inp: value=json.loads(inp.readline(2*1024*1024))
            assert value.get("ok") is expect, value
            return value.get("data",value) if expect else value
        def start():
            process = subprocess.Popen([str(aikit),"session-space","-C",str(root),"encounter-serve","--socket",str(sock)],env=env,stdout=logs,stderr=logs)
            processes.append(process)
            deadline=time.monotonic()+15
            while time.monotonic()<deadline:
                if process.poll() is not None:
                    logs.flush(); logs.seek(0); raise AssertionError(logs.read())
                try: return request("health")["pid"]
                except (FileNotFoundError,ConnectionRefusedError): time.sleep(0.02)
            raise AssertionError("Native IPC did not start")
        def events(session: str): return request("read",agent_session=session,after=0,limit=1000)["events"]
        def effects():
            path=base/"peer-effects.jsonl"
            return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        def until(fn, why: str):
            deadline=time.monotonic()+15
            while time.monotonic()<deadline:
                value=fn()
                if value: return value
                time.sleep(0.03)
            raise AssertionError(why)
        def settle(session: str):
            return until(lambda: (v if (v:=request("status",agent_session=session))["state"]=="Idle" and v.get("error") is None else None),"native turn did not reach Idle")
        def send(session: str, text: str):
            page=request("read",agent_session=session,after=0,limit=1)
            draft=request("draft",agent_session=session,basis=page["draft"]["revision"],text=text)
            ack=request("prompt",agent_session=session,draft_revision=draft["revision"])
            assert ack["accepted"] is True
        try:
            command([str(ctrl),"--json","--root",str(root),"init"])
            scope=ai("agent-session-scope")
            assert scope["project_ref"]=="control:root" and scope["world_readiness"]["ready"] is False
            assert scope["world_readiness"]["action"]=="central.world-relations.save"
            assert scope["execution_authority_granted"] is False
            skills=ai("agent-session-skills")
            assert skills["schema"]=="aikit.direct-agent-skills/v1" and skills["activation_performed"] is False
            checks.append("Real native prerequisite and Skill discovery perform no proposal, grant or provider launch")
            central("central.world-relations.save",{"record":{"schema":"central.world-relations/v1","ref":"control:root","revision":"controlled-r1","parent":None,"sources":[]}})
            assert ai("agent-session-scope")["world_readiness"]["world_ref"]=="control:root"
            purpose="Read only my selected source and report its exact content."
            made=central("agent-profile.express",{"name":"Controlled source reader","purpose":purpose,"intent_expression":purpose,"world_ref":"control:root","ratified_world_refs":["control:root"],"skill_refs":[]})
            profile=made["profile"]
            review=central("agent-profile.review",{"profile_ref":profile["ref"]})
            assert review["accepted"] is False
            acceptance={"profile_ref":profile["ref"],"expected_revision":profile["revision"],"expected_content_digest":review["content_digest"]}
            central("agent-profile.accept",acceptance,False)
            token=secrets.token_hex(32)
            authority=root/"Control/user/controlled-acceptance.json"
            authority.write_text(json.dumps({"schema":"central.native-action-authority/v1","scope_ref":"control:root","grants":[{"principal_ref":"human:controlled-test","actor_kind":"human","token_sha256":hashlib.sha256(token.encode()).hexdigest(),"scope_refs":["control:root"],"actions":["agent-profile.accept"],"expires_at_unix_seconds":int(time.time())+600}]}))
            (root/"Control/relations/source-relations.json").write_text(json.dumps({"schema":"central.control.ground-relations/v1","project_id":"control:root","relations":[{"ref":"central:root/source/Control/user/controlled-acceptance.json","path":"Control/user/controlled-acceptance.json","roles":["native-action-authority"],"provenance":"human-adopted","standing":"architecture-contract","treatment":"projectcentral-user","recognition":"controlled-test-only","recorded_at_unix_seconds":1}]}))
            env["CENTRAL_NATIVE_TOKEN"]=token
            accepted=central("agent-profile.accept",acceptance)
            roster=central("agent-profile.roster",{})
            assert len(roster["profiles"])==1 and roster["profiles"][0]["accepted"] is True
            assert accepted["execution_authority_granted"] is False
            checks.append("Real Central proposal, unauthenticated denial, exact-source human acceptance and fresh-process roster readback")
            preparation={"request_id":"controlled-native-join-12345678",**acceptance,"expected_acceptance_ref":accepted["acceptance"]["acceptance_ref"]}
            prepared=ai("agent-session-prepare","--request-json",json.dumps(preparation))
            repeated=ai("agent-session-prepare","--request-json",json.dumps(preparation))
            assert prepared==repeated and prepared["prepared"] is True and prepared["provider_started"] is False
            session=prepared["agent_session"]; space=prepared["space"]
            assert ai("agent-session-find","--request-id",preparation["request_id"])["agent_session"]==session
            assert not effects()
            checks.append("AIKit prepares one native attached identity and readback/repeated correlation cannot duplicate it or start a provider")
            provider={"id":"controlled-acp-test-only","label":"Controlled ACP peer — not a model","argv":[sys.executable,str(Path(__file__).resolve()),"--peer",str(base)],"protocol":"acp"}
            ai("encounter-configure","--provider-json",json.dumps(provider))
            pid=start()
            opening={"space":space,"agent_session":session,"provider":provider["id"],"cwd":str(root)}
            opened=request("open",**opening); native=opened["native_session_id"]
            assert opened["inference_observed"] is False
            nonce=secrets.token_hex(24); (base/"source.txt").write_text(nonce)
            send(session,"CONTROLLED-SOURCE: return the explicitly bound test source, not these prompt bytes.")
            settle(session)
            history=events(session)
            assert nonce[:9] in json.dumps(history) and nonce[9:] in json.dumps(history)
            assert any(e.get("event",{}).get("kind")=="direct-agent-context-submitted" for e in history), history
            checks.append("Real production handshake, fragmented stream and source-dependent result reach the native journal; human token is absent in the peer")
            send(session,"CONTROLLED-DENIAL")
            pending=until(lambda:request("status",agent_session=session)["permissions"],"permission never reached native owner")
            request("permission",agent_session=session,request_id=pending[0]["native_request_id"],decision={"outcome":"selected","option_id":"reject"})
            settle(session)
            assert {"kind":"denied","operation_performed":False} in effects()
            send(session,"CONTROLLED-CANCEL")
            until(lambda:"partial-before-cancel" in json.dumps(events(session)),"partial cancel stream absent")
            request("cancel",agent_session=session,reason="controlled explicit cancellation")
            settle(session)
            assert any(e["kind"]=="cancel" for e in effects())
            checks.append("Native permission denial and mid-stream cancellation travel bidirectionally through the actual production handlers")
            send(session,"CONTROLLED-DISCONNECT")
            until(lambda:request("status",agent_session=session).get("error"),"disconnect not recorded")
            before=sum(e["kind"]=="prompt" for e in effects())
            resumed=request("reconnect",**opening)
            assert resumed["native_session_id"]==native
            assert sum(e["kind"]=="prompt" for e in effects())==before
            assert any(e["kind"]=="session/load" and e["requested"]==native for e in effects())
            (base/"source.txt").write_text(secrets.token_hex(24))
            send(session,"CONTROLLED-SOURCE after explicit repair")
            settle(session)
            checks.append("Failed-body cleanup and same-identity session/load recover after disconnect without replaying the uncertain turn")
            request("shutdown",expected_pid=pid)
            processes[-1].wait(timeout=10)
            pid=start(); reopened=request("reconnect",**opening)
            assert reopened["native_session_id"]==native
            checks.append("A new owner process reopens the same persisted native session rather than manufacturing a replacement")
            send(session,"CONTROLLED-DISCONNECT second")
            until(lambda:request("status",agent_session=session).get("error"),"second disconnect not recorded")
            (base/"wrong-load").touch()
            refused=request("reconnect",False,**opening)
            assert refused["error"]["code"]=="encounter.native_identity_changed",refused
            assert "native-reconnect-identity-refused" in json.dumps(events(session))
            checks.append("A provider returning a different load identity is refused and never counted as continuation")
            (base/"wrong-load").unlink()
            request("shutdown",expected_pid=pid); processes[-1].wait(timeout=10)
            profile_file=next((root/"Control/agents/profiles").glob("*.json"))
            original=json.loads(profile_file.read_text()); original["purpose"]="Changed outside accepted basis"
            profile_file.write_text(json.dumps(original))
            assert central("agent-profile.review",{"profile_ref":profile["ref"]})["accepted"] is False
            ai("agent-session-prepare","--request-json",json.dumps(preparation),expect=False)
            checks.append("Same-revision edited native source invalidates acceptance and prevents session preparation")
            return {"standing":"controlled-native-process-proof-not-live-model","passed":len(checks),"checks":checks}
        finally:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
                    try: process.wait(timeout=5)
                    except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
            logs.close()


def main() -> None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ctrl",type=Path); parser.add_argument("--aikit",type=Path)
    parser.add_argument("--peer",type=Path)
    args=parser.parse_args()
    if args.peer: peer(args.peer); return
    if not args.ctrl or not args.aikit: parser.error("--ctrl and --aikit are required")
    print(json.dumps(exercise(args.ctrl.resolve(),args.aikit.resolve()),indent=2))

if __name__=="__main__": main()
