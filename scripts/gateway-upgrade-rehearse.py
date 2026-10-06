#!/usr/bin/env python3
"""Rehearse the managed gateway upgrade against a CONTROLLED service instance under the
platform's real service manager (launchd on macOS, systemd --user on Linux).

Nothing here touches the real gateway service: the instance is named
(AIKIT_GATEWAY_SERVICE_INSTANCE), has its own AIKIT_HOME, socket and definition, and is
uninstalled at the end. A scripted `oi` stands in for the managed installer, and two byte-distinct
builds of the binary under test (its embedded revision rewritten in a copy) stand in for "the
installed build" and "the new build".

usage: AIKIT_BUILD_SOURCE_REVISION=<the revision stamped in the binary> \
       scripts/gateway-upgrade-rehearse.py AIKIT_BINARY [--keep]
Environment: REHEARSAL_TMP (a SHORT directory: unix socket paths are limited to ~104 bytes).
Scenarios: install-drain-restart-verify, already-current, installer-fails-unchanged,
installer-flips-then-fails (rolled back), broken-new-build (rolled back), and — when a
`gateway-connector-specimen` binary is found (GATEWAY_SPECIMEN, else next to AIKIT_BINARY) —
asked-through-the-gateway: `/upgrade apply` typed into a bound conversation, the upgrade worker
started FROM INSIDE the service under the platform's service manager (not as a child process), the
restart, and the receipt announced back into that conversation once. Prints a JSON evidence
document; exit status 1 if any scenario did not end as expected.
"""
import json, os, shutil, stat, subprocess, sys, tempfile, time, socket

binary = os.path.abspath(sys.argv[1])
keep = "--keep" in sys.argv
system = sys.platform
root = tempfile.mkdtemp(prefix="gw-rehearsal-", dir=os.environ.get("REHEARSAL_TMP", tempfile.gettempdir()))
home = os.path.join(root, "home"); os.makedirs(home)
managed = os.path.join(root, "managed"); tools = os.path.join(root, "tools")
for d in (managed, tools):
    os.makedirs(d)
instance = "rh" + str(os.getpid())
evidence = {"platform": system, "instance": instance, "scenarios": []}

def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)

# ---- two real builds: the binary, and a copy whose embedded revision is rewritten
raw = open(binary, "rb").read()
import re
rev = os.environ.get("AIKIT_BUILD_SOURCE_REVISION")
if not rev:
    m = re.search(rb"[0-9a-f]{40}", raw); rev = m.group(0).decode() if m else None
assert rev, "no stamped revision to rewrite"
rev_b = "".join("0" if c == "f" else "f" for c in rev)
patched = raw.replace(rev.encode(), rev_b.encode()).replace(rev[:12].encode(), rev_b[:12].encode())
def put(path, data, mode=0o755):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, "wb").write(data); os.chmod(path, mode)
put(os.path.join(managed, "bin-a/aikit"), raw)
put(os.path.join(managed, "bin-b/aikit"), patched)
if system == "darwin":
    assert sh(["codesign", "--force", "--sign", "-", os.path.join(managed, "bin-b/aikit")]).returncode == 0
put(os.path.join(managed, "bin-bad/aikit"), b"#!/bin/sh\necho broken >&2\nexit 1\n")
def flip(link, target):
    tmp = link + ".new"
    if os.path.lexists(tmp): os.remove(tmp)
    os.symlink(target, tmp); os.replace(tmp, link)
flip(os.path.join(managed, "current"), os.path.join(managed, "bin-a/aikit"))
flip(os.path.join(tools, "aikit"), os.path.join(managed, "current"))
open(os.path.join(managed, "previous"), "w").write(os.path.join(managed, "bin-a/aikit"))
open(os.path.join(managed, "next"), "w").write("")
oi = os.path.join(tools, "oi")
put(oi, f"""#!/bin/sh
ROOT='{managed}'
echo "$@" >> "$ROOT/oi.calls"
case "$*" in
  *--rollback*) ln -sfn "$(cat "$ROOT/previous")" "$ROOT/current"; exit 0 ;;
  *--apply*)
    next="$(cat "$ROOT/next")"
    case "$next" in
      FAIL) echo "cargo build failed" >&2; exit 1 ;;
      FLIP_THEN_FAIL:*) ln -sfn "${{next#FLIP_THEN_FAIL:}}" "$ROOT/current"; echo "a later product failed" >&2; exit 1 ;;
      *) readlink "$ROOT/current" > "$ROOT/previous"; ln -sfn "$next" "$ROOT/current"; exit 0 ;;
    esac ;;
esac
exit 0
""".encode())

env = dict(os.environ, PATH=f"{tools}:{os.environ['HOME']}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin", HOME=os.environ["HOME"], AIKIT_HOME=home,
           AIKIT_GATEWAY_SERVICE_INSTANCE=instance, AIKIT_WORKCELL_REF="workcell:rehearsal", AIKIT_GATEWAY_REF="agency-gateway/rehearsal")
env.pop("AIKIT_UPGRADE_WORKER_MODE", None)
AIKIT = os.path.join(tools, "aikit")

def aikit(*args, timeout=600):
    r = subprocess.run([AIKIT, *args, "--json"], capture_output=True, text=True, env=env, cwd=home, timeout=timeout)
    try:
        return json.loads(r.stdout), r
    except Exception:
        return {"raw_stdout": r.stdout, "stderr": r.stderr, "rc": r.returncode}, r

def raw(command):
    path = os.path.join(home, "state/gateway.sock")
    s = socket.socket(socket.AF_UNIX); s.settimeout(20); s.connect(path)
    s.sendall((json.dumps({"request_id": None, "command": command}) + "\n").encode())
    data = b""
    while not data.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk: break
        data += chunk
    s.close(); return json.loads(data)

def running():
    try:
        r = raw({"type": "protocol"}); return r["response"].get("build")
    except Exception:
        return None

def wait_running(timeout=60, not_pid=None):
    end = time.time() + timeout
    while time.time() < end:
        b = running()
        if b and (not_pid is None or b["pid"] != not_pid): return b
        time.sleep(0.3)
    raise SystemExit("gateway never answered")

def service_pid():
    if system == "darwin":
        uid = sh(["id", "-u"]).stdout.strip()
        out = sh(["launchctl", "print", f"gui/{uid}/ai.aikit.gateway.{instance}"]).stdout
        for line in out.splitlines():
            if line.strip().startswith("pid ="): return int(line.split("=")[1])
        return None
    out = sh(["systemctl", "--user", "show", f"aikit-gateway-{instance}.service", "-p", "MainPID", "--value"]).stdout.strip()
    return int(out) if out and out != "0" else None

specimen = os.environ.get("GATEWAY_SPECIMEN") or os.path.join(os.path.dirname(binary), "gateway-connector-specimen")
if not os.path.exists(specimen):
    specimen = None
if specimen:
    os.makedirs(os.path.join(home, "state"), exist_ok=True)
    with open(os.path.join(home, "state/gateway-connectors.json"), "w") as f:
        json.dump({"schema": "aikit.gateway-connectors/v1", "connectors": [{
            "connector_ref": "gateway-connector/specimen/main", "platform": "specimen", "implementation": "stdio",
            "program": [specimen, "--connector-ref", "gateway-connector/specimen/main"]}]}, f)
evidence["specimen_connector"] = bool(specimen)

try:
    # install the controlled service instance (unix socket only; supervised lifecycle)
    data, r = aikit("gateway", "install-service", "--workcell-ref", "workcell:rehearsal", "--gateway-ref", "agency-gateway/rehearsal")
    evidence["install"] = {k: data.get("data", {}).get(k) for k in ("platform", "label", "unit", "binary")} if isinstance(data, dict) else data
    if r.returncode != 0:
        evidence["install_error"] = data; raise SystemExit(json.dumps(evidence, indent=1))
    b0 = wait_running()
    evidence["before"] = {"pid": b0["pid"], "revision": b0["revision"][:12], "lifecycle": b0["lifecycle"], "service_pid": service_pid()}
    sent = raw({"type": "send-communique", "draft": {"communique_ref": "aikit:communique:rehearsal-before", "attribution": "unknown", "attribution_basis": "rehearsal", "to_position_ref": "central:position:control:root:keeper", "body": "before upgrade", "sent_at_unix_ms": 1, "state": "held", "state_basis": "rehearsal"}})
    assert sent["ok"], sent

    def scenario(name, next_value, previous, args, expect):
        open(os.path.join(managed, "next"), "w").write(next_value)
        open(os.path.join(managed, "previous"), "w").write(os.path.join(managed, previous))
        before = running()
        t0 = time.time()
        data, r = aikit("gateway", "upgrade", "apply", *args)
        d = data.get("data", {}) if isinstance(data, dict) else {}
        after = wait_running(timeout=90)
        entry = {"name": name, "seconds": round(time.time() - t0, 1), "status": (d.get("outcome") or {}).get("status"),
                 "summary": (d.get("outcome") or {}).get("summary"), "pid_before": before["pid"] if before else None,
                 "pid_after": after["pid"], "revision_after": after["revision"][:12], "service_pid": service_pid(),
                 "expected": expect, "ok": (d.get("outcome") or {}).get("status") == expect}
        evidence["scenarios"].append(entry); return entry

    # 1. the happy path: install succeeds, the service manager restarts onto the new image
    scenario("install-drain-restart-verify", os.path.join(managed, "bin-b/aikit"), "bin-a/aikit",
             ["--install", "--wait", "--verify-timeout-secs", "90"], "completed")
    kept = raw({"type": "read-communique", "communique_ref": "aikit:communique:rehearsal-before"})
    evidence["communique_survived"] = bool(kept.get("ok"))
    # 2. nothing to do
    scenario("already-current", "", "bin-b/aikit", ["--wait"], "no-change")
    # 3. failed install, build unchanged
    flip(os.path.join(managed, "current"), os.path.join(managed, "bin-b/aikit"))
    scenario("installer-fails-unchanged", "FAIL", "bin-b/aikit", ["--install", "--wait"], "failed-before-change")
    # 4. flip-then-fail => rolled back
    scenario("installer-flips-then-fails", f"FLIP_THEN_FAIL:{os.path.join(managed, 'bin-a/aikit')}", "bin-b/aikit", ["--install", "--wait", "--verify-timeout-secs", "60"], "rolled-back")
    # 5. the new build is broken => rolled back, old image verified running
    flip(os.path.join(managed, "current"), os.path.join(managed, "bin-b/aikit"))
    time.sleep(1)
    scenario("broken-new-build", os.path.join(managed, "bin-bad/aikit"), "bin-b/aikit", ["--install", "--wait", "--verify-timeout-secs", "25"], "rolled-back")
    # 6. asked through the gateway itself, under the real service manager
    if specimen:
        flip(os.path.join(managed, "current"), os.path.join(managed, "bin-a/aikit"))   # the installed build is now A; B runs
        before = wait_running()
        t0 = time.time()
        binding = {"type": "bind", "binding": {
            "binding_ref": "gateway-binding/specimen", "connector_ref": "gateway-connector/specimen/main",
            "address": {"platform": "specimen", "conversation_id": "main"},
            "agent_session_ref": "agent-session/specimen", "agency_ref": "agency/specimen",
            "actuation_ref": "actuation/specimen", "actuation_stream_ref": "actuation-stream/specimen",
            "ingress": {"default": "allow", "sender_overrides": {}}}}
        def try_raw(command):
            try: return raw(command)
            except Exception: return None
        end = time.time() + 180
        while time.time() < end:
            r = try_raw(binding)
            if r and r.get("ok"): break
            time.sleep(1)
        asked = try_raw({"type": "ingest", "event": {
            "event_ref": "gateway-ingress/socket/rehearsal-upgrade", "connector_ref": "gateway-connector/specimen/main",
            "address": {"platform": "specimen", "conversation_id": "main"},
            "sender": {"native_sender_id": "owner", "kind": "human"}, "kind": "message",
            "native_event_id": "rehearsal-upgrade-1", "native_message_id": "rehearsal-upgrade-msg-1", "text": "/upgrade apply"}})
        worker_seen = None
        after = None
        end = time.time() + 300
        while time.time() < end:
            if worker_seen is None:
                if system == "darwin":
                    out = sh(["launchctl", "list"]).stdout
                    names = [l.split()[-1] for l in out.splitlines() if "gateway-upgrade." in l]
                else:
                    out = sh(["systemctl", "--user", "list-units", "aikit-gateway-upgrade-*", "--no-legend", "--all"]).stdout
                    names = [l.split()[0] for l in out.splitlines() if l.strip()]
                if names: worker_seen = names[0]
            b = running()
            if b and b["pid"] != before["pid"] and b["revision"] != before["revision"]:
                after = b; break
            time.sleep(1)
        details = []
        end = time.time() + 180
        while time.time() < end:
            snap = try_raw({"type": "snapshot"})
            details = [r.get("detail", "") for r in ((snap or {}).get("response", {}).get("snapshot", {}).get("delivery_receipts") or [])]
            if any("gateway upgrade upg-" in d and "completed" in d for d in details): break
            time.sleep(1)
        told = [d for d in details if "gateway upgrade upg-" in d and "completed" in d]
        # The worker must run the executable the instance's service DEFINITION
        # names (the instance's own build), not whatever `aikit` the manager's
        # PATH resolves to. The worker records its own executable as a step of
        # the transaction; read it back and hold it against the definition.
        worker_exe = None
        definition_named = None
        if system == "darwin":
            definition = os.path.expanduser(f"~/Library/LaunchAgents/{instance}.plist")
        else:
            definition = os.path.expanduser(f"~/.config/systemd/user/{instance}.service")
        if os.path.exists(definition):
            text = open(definition).read()
            import re as _re
            if system == "darwin":
                m = _re.search(r"<string>(/[^<]+aikit[^<]*)</string>", text)
            else:
                m = _re.search(r"ExecStart=(/[^\s]+)", text)
            if m:
                definition_named = os.path.realpath(m.group(1))
        txn_dir = os.path.join(home, "state", "gateway-upgrade")
        if os.path.isdir(txn_dir):
            for txn in sorted(os.listdir(txn_dir), reverse=True):
                txn_json = os.path.join(txn_dir, txn, "transaction.json")
                if not os.path.exists(txn_json):
                    continue
                try:
                    body = open(txn_json).read()
                except OSError:
                    continue
                import re as _re
                m = _re.search(r"worker executable: (/[^\"\\n]+)", body)
                if m:
                    worker_exe = os.path.realpath(m.group(1))
                    break
        worker_matches_definition = bool(
            worker_exe and definition_named and worker_exe == definition_named
        )
        entry = {"name": "asked-through-the-gateway", "seconds": round(time.time() - t0, 1),
                 "ask_accepted": bool(asked and asked.get("ok")),
                 "worker_under_service_manager": worker_seen, "pid_before": before["pid"],
                 "pid_after": after["pid"] if after else None, "revision_after": after["revision"][:12] if after else None,
                 "worker_executable": worker_exe, "definition_names": definition_named,
                 "worker_ran_the_definitions_executable": worker_matches_definition,
                 "receipt_announced_into_the_conversation": len(told), "receipt": told[0][:200] if told else None,
                 "expected": "completed, announced once, worker under the service manager, worker running the definition's executable",
                 "ok": bool(after) and len(told) == 1 and bool(worker_seen) and worker_matches_definition}
        evidence["scenarios"].append(entry)
    status, _ = aikit("gateway", "doctor")
    evidence["doctor_verdict"] = status.get("data", {}).get("verdict") if isinstance(status, dict) else None
    findings = status.get("data", {}).get("findings", []) if isinstance(status, dict) else []
    evidence["doctor_findings"] = [{"id": f.get("id"), "severity": f.get("severity"), "what": (f.get("what") or "")[:160]}
                                   for f in findings if str(f.get("severity")).lower() not in ("ok", "info")]
    evidence["leftover_worker_definitions"] = []
    if system == "darwin":
        la = os.path.expanduser("~/Library/LaunchAgents")
        evidence["leftover_worker_definitions"] = [f for f in os.listdir(la) if f.startswith("ai.aikit.gateway-upgrade.")]
finally:
    aikit("gateway", "uninstall-service")
    time.sleep(2)
    if not keep:
        shutil.rmtree(root, ignore_errors=True)
    evidence["root"] = root
print(json.dumps(evidence, indent=1))
sys.exit(0 if evidence.get("scenarios") and all(x["ok"] for x in evidence["scenarios"]) else 1)
