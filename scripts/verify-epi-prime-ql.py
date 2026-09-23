#!/usr/bin/env python3
"""Installed Epi-Logos Prime-QL acceptance over the real AIKit/Prime/QL/Actuation join.

No provider, Prime, QL, child, model or faculty response is simulated.  This
runner is intentionally local/installed acceptance: it consumes the person's
existing AIKit model catalogue/credentials and one explicit Project ground.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time


def required_path(name: str, *, directory: bool = False) -> Path:
    raw = os.environ.get(name, "").strip()
    if not raw:
        raise RuntimeError(f"{name} is required")
    path = Path(raw).expanduser().resolve()
    present = path.is_dir() if directory else path.is_file()
    if not present:
        kind = "directory" if directory else "file"
        raise RuntimeError(f"{name} must name an existing {kind}")
    return path


def required_revision(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if len(value) != 40 or any(ch not in "0123456789abcdef" for ch in value):
        raise RuntimeError(f"{name} must be a lowercase 40-hex revision")
    return value


aikit = required_path("EPI_AIKIT_BINARY")
launcher = required_path("EPI_ACTUATION_PRIME_BINARY")
prime = required_path("EPI_PRIME_AGENT_BINARY")
ql = required_path("EPI_QL_BINARY")
research = required_path("EPI_ACTUATION_RESEARCH_BINARY")
skill = required_path("EPI_QL_SKILL_PATH", directory=True)
faculty_config = required_path("EPI_FACULTY_CONFIG")
project = required_path("EPI_ACCEPTANCE_PROJECT_DIR", directory=True)
ql_revision = required_revision("EPI_QL_REVISION")
actuation_revision = required_revision("EPI_ACTUATION_REVISION")
ql_root_raw = os.environ.get("EPI_QL_SOURCE_ROOT", "").strip()
ql_root = Path(ql_root_raw).expanduser().resolve() if ql_root_raw else None
if ql_root is not None and not ql_root.is_dir():
    raise RuntimeError("EPI_QL_SOURCE_ROOT must be an existing directory when supplied")

central_raw = os.environ.get("EPI_CENTRAL_CTRL_BINARY", "").strip()
central_root_raw = os.environ.get("EPI_CENTRAL_ROOT", "").strip()
central_project = os.environ.get("EPI_CENTRAL_PROJECT", "").strip()
central_enabled = bool(central_raw or central_root_raw or central_project)
central_ctrl = None
central_root = None
lane_repo = os.environ.get("EPI_ACCEPTANCE_REPO", "").strip()
lane_branch = os.environ.get("EPI_ACCEPTANCE_BRANCH", "").strip()
lane_worktree = os.environ.get("EPI_ACCEPTANCE_WORKTREE", "").strip()
if central_enabled:
    if not (central_raw and central_root_raw and central_project):
        raise RuntimeError(
            "EPI_CENTRAL_CTRL_BINARY, EPI_CENTRAL_ROOT and EPI_CENTRAL_PROJECT "
            "must be supplied together"
        )
    central_ctrl = Path(central_raw).expanduser().resolve()
    central_root = Path(central_root_raw).expanduser().resolve()
    if not central_ctrl.is_file() or not central_root.is_dir():
        raise RuntimeError("Central acceptance paths are unavailable")
    if not (lane_repo and lane_branch and lane_worktree):
        raise RuntimeError(
            "EPI_ACCEPTANCE_REPO, EPI_ACCEPTANCE_BRANCH and "
            "EPI_ACCEPTANCE_WORKTREE are required for native continuation proof"
        )
    if not Path(lane_worktree).expanduser().resolve().is_dir():
        raise RuntimeError("EPI_ACCEPTANCE_WORKTREE must be an existing directory")

faculty = json.loads(faculty_config.read_text())
evidence_root_raw = faculty.get("evidence_root")
if not isinstance(evidence_root_raw, str) or not evidence_root_raw.strip():
    raise RuntimeError("EPI_FACULTY_CONFIG must provide evidence_root for acceptance")
evidence_root = Path(evidence_root_raw).expanduser().resolve()
evidence_root.mkdir(parents=True, exist_ok=True)

stamp = f"{int(time.time())}-{os.getpid()}"
provider_id = f"epi-prime-ql-acceptance-{stamp}"
space = f"session-space/epi-prime-ql-acceptance-{stamp}"
session = f"agent-session/epi-prime-ql-acceptance-{stamp}"
handoff_id = f"epi-prime-ql-continuation-{stamp}"
out_dir = Path(
    os.environ.get(
        "EPI_ACCEPTANCE_OUTPUT",
        tempfile.mkdtemp(prefix="epi-prime-ql-acceptance-"),
    )
).expanduser().resolve()
out_dir.mkdir(parents=True, exist_ok=True)
socket_path = out_dir / "encounter.sock"
server_log = (out_dir / "encounter-owner.log").open("w")

env = dict(os.environ)


def run(*args: str, timeout: int = 180) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(aikit), *args],
        cwd=project,
        env=env,
        text=True,
        capture_output=True,
        timeout=timeout,
    )


def cli(*args: str):
    proc = run(*args)
    if proc.returncode:
        raise RuntimeError(proc.stderr.strip() or proc.stdout.strip())
    text = proc.stdout.strip()
    return json.loads(text) if text else {}


def ss(*args: str):
    return cli("session-space", *args)


def apply(preview):
    path = out_dir / "preview.json"
    path.write_text(json.dumps(preview))
    return ss("apply", "--preview-json", "@" + str(path))


def request(action: str, **fields):
    reply = ss(
        "encounter",
        "--socket",
        str(socket_path),
        "--request-json",
        json.dumps({"action": action, **fields}),
    )
    if reply.get("ok") is not True:
        raise RuntimeError(json.dumps(reply, sort_keys=True))
    return reply["data"]


def paced_admission(session, text, cursor, expected_marker, *, timeout: int = 1500):
    """Draft, prompt and observe one admission turn, waiting out parent busyness.

    A completed child keeps delivering result messages to the parent, and
    Prime 0.9.4 refuses a prompt that lands while the parent still processes
    one - sometimes as a request refusal, sometimes as a turn-level failure
    after the prompt was forwarded. Retrying that specific refusal is pacing;
    the marker assertion is unchanged.
    """
    deadline = time.monotonic() + timeout
    while True:
        page = event_page(cursor)
        draft = request(
            "draft",
            agent_session=session,
            basis=page["draft"]["revision"],
            text=text,
        )
        prompt_when_idle(session, draft["revision"])
        cursor, root_text, terminal, _ = wait_turn(cursor)
        if "already processing" in json.dumps(terminal):
            if time.monotonic() >= deadline:
                raise RuntimeError(
                    f"{expected_marker} admission kept colliding with parent processing: {terminal}"
                )
            time.sleep(20.0)
            continue
        if "Completed" not in terminal or root_text.strip() != expected_marker:
            raise RuntimeError(
                f"{expected_marker} admission failed: {terminal} / {root_text!r}"
            )
        return cursor, root_text, terminal


def wait_socket(server):
    deadline = time.monotonic() + 30
    while not socket_path.exists():
        if server.poll() is not None:
            raise RuntimeError("AIKit Encounter owner exited before socket readiness")
        if time.monotonic() > deadline:
            raise TimeoutError("AIKit Encounter socket did not appear")
        time.sleep(0.05)


def stop_owner(server):
    if server.poll() is not None:
        return {"standing": "already-exited", "exit_code": server.returncode}
    try:
        health = request("health")
        ack = request("shutdown", expected_pid=health["pid"])
        server.wait(timeout=20)
        return {
            "standing": "native-shutdown-acknowledged",
            "pid": health["pid"],
            "ack": ack,
            "exit_code": server.returncode,
        }
    except Exception as error:
        os.killpg(server.pid, signal.SIGTERM)
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(server.pid, signal.SIGKILL)
            server.wait(timeout=10)
        return {
            "standing": "fallback-process-group-termination",
            "error": str(error),
            "exit_code": server.returncode,
        }


def event_page(cursor: int, limit: int = 128):
    return request("read", agent_session=session, after=cursor, limit=limit)


def wait_turn(cursor: int, *, timeout: int = 600):
    text = ""
    terminal = None
    captured = []
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline and terminal is None:
        page = event_page(cursor)
        for item in page["events"]:
            cursor = max(cursor, item["cursor"])
            event = item["event"]
            captured.append(event)
            host = event.get("event", {})
            signal_row = host.get("Signal")
            if signal_row:
                kind = signal_row.get("kind", {})
                if kind.get("kind") == "agent-message-chunk":
                    text += kind.get("text", "")
            if "TurnEnded" in host:
                terminal = host["TurnEnded"]["stop"]
        cursor = page["next_cursor"]
        if terminal is None:
            time.sleep(0.05)
    if terminal is None:
        raise TimeoutError("Prime turn did not reach a terminal Encounter state")
    return cursor, text, terminal, captured


def new_receipts(before: set[str]):
    rows = []
    for path in evidence_root.glob("*.json"):
        if path.name in before:
            continue
        value = json.loads(path.read_text())
        if value.get("schema") == "actuation.prime-ql-operation/v1":
            rows.append((path.name, value))
    return rows


# Current Project ground and canonical SessionSpace attachment.
project_context = ss("project-context")
preview = ss("create", space, "--label", "Epi Prime-QL installed acceptance")
apply(preview)
intent = {"operation": "bind-project-context", "binding": project_context}
intent_path = out_dir / "project-context.json"
intent_path.write_text(json.dumps(intent))
apply(ss("stage", "--space", space, "--intent-json", "@" + str(intent_path)))
intent = {
    "operation": "attach-agent-session",
    "attachment": {
        "agent_session": session,
        "purpose": "Installed Epi Prime-QL acceptance",
        "provenance": ["local acceptance; no cloud-installed claim"],
    },
}
intent_path.write_text(json.dumps(intent))
apply(ss("stage", "--space", space, "--intent-json", "@" + str(intent_path)))

# Configure the actual candidate body. This is configuration only.
configure = [
    "session-space",
    "encounter-epi-prime-configure",
    "--provider-id",
    provider_id,
    "--launcher",
    str(launcher),
    "--prime-bin",
    str(prime),
    "--ql-bin",
    str(ql),
    "--ql-revision",
    ql_revision,
    "--body-revision",
    actuation_revision,
    "--skill-path",
    str(skill),
    "--research-bin",
    str(research),
    "--faculty-config",
    str(faculty_config),
]
if ql_root is not None:
    configure.extend(["--ql-root", str(ql_root)])
if central_enabled:
    configure.extend([
        "--central-ctrl-bin",
        str(central_ctrl),
        "--central-root",
        str(central_root),
        "--central-project",
        central_project,
    ])
configured = cli(*configure)
if configured.get("body_ref") != "agent-body/epi-prime-ql":
    raise RuntimeError(f"unexpected configured body: {configured}")

server = subprocess.Popen(
    [
        str(aikit),
        "session-space",
        "-C",
        str(project),
        "encounter-serve",
        "--socket",
        str(socket_path),
    ],
    cwd=project,
    env=env,
    stdout=server_log,
    stderr=server_log,
    start_new_session=True,
)
shutdown = None

try:
    wait_socket(server)
    before_receipts = {path.name for path in evidence_root.glob("*.json")}
    opened = request(
        "open",
        space=space,
        agent_session=session,
        provider=provider_id,
        cwd=str(project),
    )
    if opened.get("body_ref") != "agent-body/epi-prime-ql":
        raise RuntimeError(f"opened body mismatch: {opened}")
    if opened.get("body_revision") != actuation_revision:
        raise RuntimeError(f"opened Actuation revision mismatch: {opened}")
    native_session = opened.get("native_session_id")
    if not isinstance(native_session, str) or not native_session:
        raise RuntimeError("Prime open returned no native session identity")

    status = request("status", agent_session=session)
    if status.get("native_session_id") != native_session:
        raise RuntimeError("Prime status changed native identity")
    provider_reading = status.get("provider", {})
    if provider_reading.get("body_ref") != "agent-body/epi-prime-ql":
        raise RuntimeError(f"status body mismatch: {status}")
    if provider_reading.get("body_revision") != actuation_revision:
        raise RuntimeError(f"status body revision mismatch: {status}")

    # Resolve the child route independently through AIKit's native roster.
    resolution = cli(
        "--json",
        "model-resolve",
        "--use-type",
        "agent-child",
        "--ranking-policy",
        "CHEAPEST_ELIGIBLE",
    )
    if resolution.get("ok") is not True:
        raise RuntimeError(f"AIKit cheapest-eligible resolution failed: {resolution}")
    selected = resolution["data"]["selected"]
    expected_child_selector = (
        selected["provider"].removeprefix("provider:")
        + "/"
        + selected["provider_native_id"]
    )

    cursor = 0
    page = event_page(cursor)
    cursor = page["next_cursor"]

    child_task = (
        "Import ql_relational. Call await ql_relational.anuttara_read("
        "'M0-2-9', max_relations=8). Refuse unless the returned language "
        "coordinate is M0-2-9 and declared relations are present. "
    )
    if central_enabled:
        child_task += (
            "Then call await ql_relational.central_now_handover("
            + json.dumps("Prime child continuation")
            + ", "
            + json.dumps("M0-2-9 was read with declared relations; continue from the named source/evidence and lane refs.")
            + ", actor="
            + json.dumps("prime-child/ql-child-proof")
            + ", project="
            + json.dumps(central_project)
            + ", handoff_id="
            + json.dumps(handoff_id)
            + ", source_refs=["
            + json.dumps(f"ql-mef:{ql_revision}:M0-2-9")
            + "], evidence_refs=["
            + json.dumps(f"actuation:{actuation_revision}:faculty:#0")
            + "], work_refs=[{'repo':"
            + json.dumps(lane_repo)
            + ",'branch':"
            + json.dumps(lane_branch)
            + ",'worktree_path':"
            + json.dumps(str(Path(lane_worktree).expanduser().resolve()))
            + "}]). Refuse unless the returned handoff id is "
            + json.dumps(handoff_id)
            + ". "
        )
    child_task += (
        "Then use the available agent_message capability to send exactly CHILD_QL_OK "
        "to your parent with receiver_role='parent'. Do not edit files, "
        "create a worktree/clone, spawn another child, or claim live Bimba mutation."
    )
    prompt = (
        "Use the installed ql_relational Skill. Call "
        "await ql_relational.spawn_child_cheapest(" + json.dumps(child_task)
        + ", name='ql-child-proof', use_type='agent-child'). "
        "After the native child is admitted, reply exactly ROOT_CHILD_ADMITTED."
    )
    draft = request("draft", agent_session=session, basis=0, text=prompt)
    request("prompt", agent_session=session, draft_revision=draft["revision"])
    cursor, root_text, terminal, captured = wait_turn(cursor)
    if "Completed" not in terminal or root_text.strip() != "ROOT_CHILD_ADMITTED":
        raise RuntimeError(f"root child-admission turn failed: {terminal} / {root_text!r}")

    child_updates = []
    saw_child_message = False
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        page = event_page(cursor)
        for item in page["events"]:
            cursor = max(cursor, item["cursor"])
            event = item["event"]
            if "CHILD_QL_OK" in json.dumps(event, sort_keys=True):
                saw_child_message = True
            host = event.get("event", {})
            row = host.get("Signal", {}).get("kind", {})
            if row.get("kind") == "status":
                message = row.get("message", "")
                if message.startswith("prime-rlm-child:"):
                    child = json.loads(message[len("prime-rlm-child:"):]).get("child")
                    if isinstance(child, dict) and child.get("sessionName") == "ql-child-proof":
                        child_updates.append(child)
        cursor = page["next_cursor"]
        completed = [row for row in child_updates if row.get("status") in {"done", "completed"}]
        if completed and saw_child_message:
            break
        time.sleep(0.1)
    if not child_updates:
        raise RuntimeError("no native Prime child lifecycle was observed")
    child = child_updates[-1]
    if child.get("model") != expected_child_selector:
        raise RuntimeError(
            f"child model mismatch: observed {child.get('model')!r}, "
            f"AIKit cheapest-eligible expected {expected_child_selector!r}"
        )
    session_dir = child.get("sessionDir")
    if not isinstance(session_dir, str) or not session_dir:
        raise RuntimeError("Prime child event disclosed no sessionDir")
    child_locus_digest = hashlib.sha256(session_dir.encode()).hexdigest()

    receipts = new_receipts(before_receipts)
    child_receipts = [
        (name, row)
        for name, row in receipts
        if row.get("trace_ref") == session
        and row.get("operation") == "anuttara-read"
        and row.get("success") is True
        and row.get("ql_mef_revision") == ql_revision
        and isinstance(row.get("declared_locus_ref"), str)
        and row["declared_locus_ref"].startswith(
            f"prime-rlm-session-sha256:{child_locus_digest}:depth:"
        )
    ]
    if not child_receipts:
        raise RuntimeError(
            "no native child-owned #0 faculty receipt correlated to the Prime child locus"
        )
    if not saw_child_message:
        raise RuntimeError("child did not return the explicit CHILD_QL_OK message")

    replacement = None
    replacement_message_observed = False
    if central_enabled:
        replacement_task = (
            "Import ql_relational. Read the exact Central NOW handoff with "
            "await ql_relational.central_now_handoff_read("
            + json.dumps(handoff_id)
            + ", project="
            + json.dumps(central_project)
            + "). Refuse unless its id, repo, branch and worktree_path equal "
            + json.dumps({
                "id": handoff_id,
                "repo": lane_repo,
                "branch": lane_branch,
                "worktree_path": str(Path(lane_worktree).expanduser().resolve()),
            }, sort_keys=True)
            + ". Continue only from that handoff's source/evidence/work refs; "
            "do not reconstruct the parent investigation. Then send exactly "
            "REPLACEMENT_NOW_OK to the parent with receiver_role='parent'."
        )
        replacement_prompt = (
            "Use the installed ql_relational Skill. Call "
            "await ql_relational.spawn_child_cheapest("
            + json.dumps(replacement_task)
            + ", name='ql-child-replacement', use_type='agent-child'). "
            "After admission reply exactly ROOT_REPLACEMENT_ADMITTED."
        )
        cursor, root_text, terminal = paced_admission(
            session, replacement_prompt, cursor, "ROOT_REPLACEMENT_ADMITTED"
        )

        replacement_updates = []
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            page = event_page(cursor)
            for item in page["events"]:
                cursor = max(cursor, item["cursor"])
                event = item["event"]
                if "REPLACEMENT_NOW_OK" in json.dumps(event, sort_keys=True):
                    replacement_message_observed = True
                host = event.get("event", {})
                row = host.get("Signal", {}).get("kind", {})
                if row.get("kind") == "status":
                    message = row.get("message", "")
                    if message.startswith("prime-rlm-child:"):
                        candidate = json.loads(
                            message[len("prime-rlm-child:"):]
                        ).get("child")
                        if (
                            isinstance(candidate, dict)
                            and candidate.get("sessionName") == "ql-child-replacement"
                        ):
                            replacement_updates.append(candidate)
            cursor = page["next_cursor"]
            completed = [
                row
                for row in replacement_updates
                if row.get("status") in {"done", "completed"}
            ]
            if completed and replacement_message_observed:
                break
            time.sleep(0.1)
        if not replacement_updates or not replacement_message_observed:
            raise RuntimeError(
                "replacement child did not continue from the native NOW handoff"
            )
        replacement = replacement_updates[-1]
        if replacement.get("model") != expected_child_selector:
            raise RuntimeError(
                f"replacement child model mismatch: {replacement.get('model')!r}"
            )

    # Cancellation must stop the live effect without destroying the session.
    page = event_page(cursor)
    basis = page["draft"]["revision"]
    draft = request(
        "draft",
        agent_session=session,
        basis=basis,
        text="Write the integers 1 through 50000 separated by spaces. Begin immediately. Do not use tools.",
    )
    request("prompt", agent_session=session, draft_revision=draft["revision"])
    time.sleep(1.0)
    request("cancel", agent_session=session, reason="Prime-QL installed acceptance cancellation")
    cursor, _, cancelled, _ = wait_turn(cursor)
    if "Cancelled" not in cancelled:
        raise RuntimeError(f"Prime cancellation did not terminate as cancelled: {cancelled}")

    cursor, continuation, terminal = paced_admission(
        session,
        "Reply exactly EPI_PRIME_CONTINUED_OK. Do not use tools.",
        cursor,
        "EPI_PRIME_CONTINUED_OK",
    )
    if request("status", agent_session=session)["native_session_id"] != native_session:
        raise RuntimeError("continuation reminted the native Prime session")

    result = {
        "schema": "aikit.epi-prime-ql-installed-acceptance/v1",
        "result": "passed",
        "standing": "installed/live-provider acceptance; human two-mode walkthrough remains separate",
        "revisions": {
            "ql_mef": ql_revision,
            "actuation": actuation_revision,
        },
        "body": {
            "ref": "agent-body/epi-prime-ql",
            "revision": actuation_revision,
            "provider_id": provider_id,
            "native_session_id": native_session,
        },
        "model": {
            "root_observation": opened.get("model_observation"),
            "child_policy": "CHEAPEST_ELIGIBLE",
            "child_model_ref": selected["model"],
            "child_provider": selected["provider"],
            "child_provider_native_id": selected["provider_native_id"],
            "child_observed_selector": child.get("model"),
        },
        "child": {
            "id": child.get("id"),
            "name": child.get("sessionName"),
            "status": child.get("status"),
            "session_dir_sha256": child_locus_digest,
            "returned_message_observed": saw_child_message,
            "faculty_receipts": [name for name, _ in child_receipts],
        },
        "continuation": {
            "cancelled": True,
            "same_native_session": True,
            "marker": continuation.strip(),
            "native_now_handover": (
                {
                    "id": handoff_id,
                    "project": central_project,
                    "repo": lane_repo,
                    "branch": lane_branch,
                    "worktree_path_sha256": hashlib.sha256(
                        str(Path(lane_worktree).expanduser().resolve()).encode()
                    ).hexdigest(),
                    "replacement_child_id": replacement.get("id") if replacement else None,
                    "replacement_message_observed": replacement_message_observed,
                }
                if central_enabled
                else {"standing": "not-exercised; Central inputs not supplied"}
            ),
        },
        "binary_sha256": {
            "aikit": hashlib.sha256(aikit.read_bytes()).hexdigest(),
            "actuation_epi_prime": hashlib.sha256(launcher.read_bytes()).hexdigest(),
            "prime_agent": hashlib.sha256(prime.read_bytes()).hexdigest(),
            "ql": hashlib.sha256(ql.read_bytes()).hexdigest(),
            "actuation_research": hashlib.sha256(research.read_bytes()).hexdigest(),
        },
        "not_claimed": [
            "human Expressions/Techne experience acceptance",
            "live Bimba mutation",
            "EBM or Jev result unless independently enabled and observed",
            "OS/worktree confinement without a Workcell/sandbox boundary",
        ],
    }
    (out_dir / "epi-prime-ql-installed-acceptance.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print(json.dumps(result, indent=2))
finally:
    shutdown = stop_owner(server)
    (out_dir / "owner-shutdown.json").write_text(json.dumps(shutdown, indent=2) + "\n")
    server_log.close()
