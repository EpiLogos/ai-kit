#!/usr/bin/env python3
"""Disposable joined Jev + Redis NOW proof for O:I #65 / AIKit #388.

Uses real AIKit, Central/ctrl, bkmr, Factory and Redis. In controlled mode only
Jev is replaced by a loopback protocol actor; its receipt is explicitly marked
controlled by the production transport. Live mode uses the same requests against
the official provider with a native credential reference and finite budget.
"""
from __future__ import annotations

import argparse, json, os, pathlib, subprocess, sys, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CONTROLLED_KEY = "aikit-controlled-protocol-only"
MODEL = "jev-1.13.0"

def run(cmd, *, env=None, cwd=None, ok=True, input_text=None):
    started = time.perf_counter()
    p = subprocess.run([str(x) for x in cmd], env=env, cwd=cwd, text=True,
                       input=input_text, capture_output=True, timeout=120)
    elapsed = (time.perf_counter() - started) * 1000.0
    if ok and p.returncode != 0:
        raise RuntimeError(f"command failed {cmd}\nstdout={p.stdout}\nstderr={p.stderr}")
    if not ok and p.returncode == 0:
        raise RuntimeError(f"command unexpectedly succeeded {cmd}\n{p.stdout}")
    return p, elapsed

def parse_json(p):
    try:
        return json.loads(p.stdout)
    except Exception as exc:
        raise RuntimeError(f"invalid JSON: {p.stdout}\n{p.stderr}") from exc

def ctrl_action(ctrl, root, action, payload, env, *, ok=True):
    p, ms = run([ctrl, "--json", "--root", root, "action", "run", action,
                 json.dumps(payload, separators=(",", ":"))], env=env, ok=ok)
    value = parse_json(p)
    if bool(value.get("ok")) != ok:
        raise RuntimeError(f"Central action standing mismatch: {value}")
    if not ok:
        return value, ms
    data = value["data"]
    if data.get("schema") == "central.file-map/v1" and "result" in data:
        return data["result"], ms
    return data, ms

def factory_json(factory, args, env):
    p, ms = run([factory, *args, "--json"], env=env)
    return parse_json(p), ms

def aikit_json(aikit, args, env, cwd, *, ok=True):
    p, ms = run([aikit, "--json", "-C", cwd, *args], env=env, ok=ok)
    if not ok:
        return {"returncode": p.returncode, "stdout": p.stdout, "stderr": p.stderr}, ms
    return parse_json(p), ms

def actuation_json(actuation, args, payload, env, *, ok=True):
    p, ms = run([actuation, *args, "--json"], env=env, ok=ok,
                input_text=json.dumps(payload, separators=(",", ":")))
    if not ok:
        return {"returncode": p.returncode, "stdout": p.stdout, "stderr": p.stderr}, ms
    return parse_json(p), ms

def write_json(path, value):
    path = pathlib.Path(path)
    path.write_text(json.dumps(value, indent=2) + "\n")
    return path

def string_set(value):
    return [str(x) for x in value if isinstance(x, str)]

class ControlledHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *_):
        pass
    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        request = json.loads(self.rfile.read(length))
        server = self.server
        if getattr(server, "delay_once", False):
            server.delay_once = False
            server.request_seen.set()
            server.release_response.wait(timeout=10)
        if getattr(server, "malformed_once", False):
            server.malformed_once = False
            body = b'{"model":"jev-1.13.0","answers":{},"usage":{"input_tokens":1,"output_tokens":1}}'
        else:
            answers = {}
            for key, q in request["questions"].items():
                kind = q["type"]
                if kind == "noul":
                    noul = 0.2 if key == "catalogue-sufficient" else (
                        0.95 if key == "candidate/000" else
                        0.82 if key == "candidate/001" else
                        0.10 if key.startswith("candidate/") else 0.76
                    )
                    answers[key] = {"type": "noul", "noul": noul}
                elif kind == "choice":
                    choices = list(q.get("criteria", {}).keys())
                    choice = choices[0]
                    answers[key] = {
                        "type": "choice", "choice": choice,
                        "probabilities": {name: (1.0 if name == choice else 0.0) for name in choices},
                        "confidence": 1.0,
                    }
                elif kind == "score":
                    labels = list(q.get("criteria", []))
                    answers[key] = {
                        "type": "score", "score": 0.0,
                        "legend": {str(i): label for i, label in enumerate(labels)},
                        "probabilities": {str(i): (1.0 if i == 0 else 0.0) for i in range(len(labels))},
                        "confidence": 1.0,
                    }
                else:
                    raise RuntimeError(f"unknown question kind {kind}")
            body = json.dumps({
                "model": request["model"], "answers": answers,
                "usage": {"input_tokens": 211, "output_tokens": 19}
            }, separators=(",", ":")).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

def start_controlled():
    server = ThreadingHTTPServer(("127.0.0.1", 0), ControlledHandler)
    server.delay_once = False
    server.malformed_once = False
    server.request_seen = threading.Event()
    server.release_response = threading.Event()
    server.release_response.set()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server

def redis_config(address, prefix):
    return {
        "schema": "aikit.redis-now-config/v1", "address": address, "database": 0,
        "key_prefix": prefix, "username": None, "credential_ref": None,
        "allow_remote": False, "connect_timeout_ms": 1000, "io_timeout_ms": 1000,
        "prepared_ttl_seconds": 3600, "coordination_retention_seconds": 7200,
    }

def limits(model, budget):
    return {
        "timeout_ms": 5000, "max_attempts": 1,
        "max_total_reserved_microusd": budget,
        "tariff": {
            "model_version": model,
            "source": "https://docs.typesafe.ai/models; checked 2026-09-22",
            "max_input_tokens_per_attempt": 64000,
            "max_output_tokens_per_attempt": 64000,
            "input_microusd_per_million_tokens": 42000,
            "output_microusd_per_million_tokens": 0,
        },
    }

def candidate(ref, title, excerpt, egress="allowed"):
    return {
        "source_ref": ref, "source_revision": "candidate-r1", "title": title,
        "excerpt": excerpt, "route": "controlled-capability-catalogue",
        "agent_visibility": "payload", "external_egress": egress,
    }

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--aikit", required=True)
    ap.add_argument("--ctrl", required=True)
    ap.add_argument("--bkmr", required=True)
    ap.add_argument("--factory", required=True)
    ap.add_argument("--factory-state", required=True)
    ap.add_argument("--actuation")
    ap.add_argument("--agency-ref", default="agency:oi65-controlled")
    ap.add_argument("--actuation-ref", default="actuation:oi65-controlled")
    ap.add_argument("--activity-ref", default="activity:oi65-jev")
    ap.add_argument("--actuation-stream-ref", default="stream:oi65-jev")
    ap.add_argument("--actuation-session-ref", default="session:oi65-jev")
    ap.add_argument("--redis-address", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--jev-mode", choices=["controlled", "live", "skip"], default="controlled")
    ap.add_argument("--jev-credential-ref")
    ap.add_argument("--jev-model", default=MODEL)
    ap.add_argument("--jev-budget-microusd", type=int, default=3000)
    ap.add_argument("--allow-env-import", action="store_true")
    args = ap.parse_args()

    root_tmp = tempfile.TemporaryDirectory(prefix="aikit-jev-now-joined-")
    base = pathlib.Path(root_tmp.name).resolve()
    central = base / "Central"
    (central / "Control/user").mkdir(parents=True)
    (central / "Work").mkdir()
    wiki_dir = central / "Control/agents/wiki"
    wiki_dir.mkdir(parents=True)
    wiki_file = wiki_dir / "wiki.json"
    write_json(wiki_file, {
        "objects": [{
            "object": "space",
            "profile": "okf-wiki/v1",
            "ref": "central:wiki:root",
            "revision": 1,
            "provenance": [],
            "title": "Central",
            "parent_space_refs": [],
            "child_space_refs": [],
            "node_refs": []
        }]
    })
    home = base / "home"; home.mkdir()
    aikit_home = base / "aikit-home"
    env = {k:v for k,v in os.environ.items()
           if not k.startswith(("CENTRAL_", "BKMR_", "AIKIT_"))}
    env.update({
        "HOME": str(home), "AIKIT_HOME": str(aikit_home),
        "CENTRAL_ROOT": str(central), "CENTRAL_CTRL_BIN": str(pathlib.Path(args.ctrl).resolve()),
        "CENTRAL_BKMR_BIN": str(pathlib.Path(args.bkmr).resolve()), "NO_COLOR": "1",
    })

    source_path = central / "Control/user/jev-redis-now-source.md"
    source_path.write_text(
        "# Operative source\n\nquartz operative context: preserve exact source revision, "
        "Factory capability relations and participant-specific disclosure.\n"
    )
    inspect, _ = ctrl_action(args.ctrl, central, "central.file-map.inspect", {}, env)
    registered, _ = ctrl_action(args.ctrl, central, "central.file-map.register", {
        "path": "Control/user/jev-redis-now-source.md",
        "expected_revision": inspect["revision"],
    }, env)
    source_ref = registered["source_ref"]
    policy, _ = ctrl_action(args.ctrl, central, "central.work.policy", {}, env)
    participants = ["agent/comparison-worker", "agent/related-worker", "agent/verifier"]
    allocation, _ = ctrl_action(args.ctrl, central, "central.now.allocate", {
        "task_ref": "task:oi-65-jev-redis-cloud-proof",
        "purpose": "Jev Redis NOW joined cloud proof",
        "participant_refs": participants,
        "source_refs": [source_ref],
        "expected_policy_revision": policy["revision"],
    }, env)
    now_ref = allocation["now_ref"]

    fixture = json.loads(pathlib.Path(args.factory_state).read_text())
    state = fixture.get("state", fixture)
    project_ref = state["build"]["project"]["ref"]
    run_ref = next(iter(state["build"]["runs"]["runs"]))
    unit_list, _ = factory_json(args.factory, [
        "development", "workflow-units", args.factory_state, run_ref
    ], env)
    units = unit_list["units"]
    if len(units) < 3:
        raise RuntimeError("Factory joined fixture must expose at least three WorkflowUnits")
    unit_refs = [u["workflowUnitRef"] for u in units[:3]]

    # Actual baseline owner discovery: no Redis and no Jev.
    baseline_calls = []
    t0 = time.perf_counter()
    for action, payload in [
        ("central.now.read", {"now_ref": now_ref}),
        ("central.file-map.resolve", {"source_ref": source_ref, "content": True}),
    ]:
        value, ms = ctrl_action(args.ctrl, central, action, payload, env)
        baseline_calls.append({"owner": "central", "operation": action, "elapsed_ms": ms})
    for argv in [
        ["development", "run", args.factory_state, run_ref],
        ["development", "workflow-units", args.factory_state, run_ref],
        ["development", "workflow-unit", args.factory_state, unit_refs[0], run_ref],
    ]:
        _, ms = factory_json(args.factory, argv, env)
        baseline_calls.append({"owner": "factory", "operation": argv[1], "elapsed_ms": ms})
    knowledge, ms = aikit_json(args.aikit, ["knowledge", "search", "quartz"], env, central)
    baseline_calls.append({"owner": "aikit", "operation": "knowledge.search", "elapsed_ms": ms})
    ordinary_ms = (time.perf_counter() - t0) * 1000.0

    public_candidates = [
        candidate("context-source/capability-a", "Capability A", "Provides source revision validation and BKMR location."),
        candidate("context-source/capability-b", "Capability B", "Provides participant-specific delivery and continuation."),
        candidate("context-source/irrelevant", "Irrelevant", "A capability unrelated to the present undertaking."),
    ]
    verifier_canary = candidate(
        "context-source/verifier-canary", "Verifier-only expectation",
        "VERIFIER_EXPECTATION_CANARY: independently check the returned Factory basis.", "denied"
    )

    def prep_request(config, participant, session, unit_ref, selection, candidates,
                     expected=0, continuation=None, wiki_queries=None):
        return {
            "schema": "aikit.now-preparation-request/v1", "redis": config,
            "project_ref": project_ref, "now_ref": now_ref,
            "participant_ref": participant, "agent_session": session,
            "concern": "Complete the bounded O:I #65 Jev + Redis NOW integration",
            "disclosure_revision": "cloud-proof-disclosure-r1",
            "practice_refs": ["skill/aikit/operation", "skill/aikit/knowledge-navigation"],
            "central": {
                "root": str(central), "project": None, "ctrl_bin": str(pathlib.Path(args.ctrl).resolve()),
                "source_refs": [source_ref],
            },
            "factory": {
                "state": str(pathlib.Path(args.factory_state).resolve()), "run_ref": run_ref,
                "factory_bin": str(pathlib.Path(args.factory).resolve()),
                "workflow_unit_refs": [unit_ref],
            },
            "wiki_queries": ["quartz"] if wiki_queries is None else wiki_queries,
            "candidate_items": candidates,
            "continuation": continuation, "expected_version": expected,
            "external_provider": False, "allow_redis_env_import": False,
            "selection": selection,
        }

    work = base / "requests"; work.mkdir()
    redis_cfg = redis_config(args.redis_address, f"aikit-oi65-{os.getpid()}-redis")
    jev_cfg = redis_config(args.redis_address, f"aikit-oi65-{os.getpid()}-jev")
    episode_cfg = redis_config(args.redis_address, f"aikit-oi65-{os.getpid()}-episode")
    write_json(work/"redis.json", redis_cfg)

    # Prepared Redis arm, no Jev selection.
    redis_req = prep_request(
        redis_cfg, "agent/comparison-worker", "agent-session/comparison-redis",
        unit_refs[0], {"mode": "all"}, public_candidates
    )
    write_json(work/"redis-prepare.json", redis_req)
    t0 = time.perf_counter()
    redis_result, redis_prepare_ms = aikit_json(
        args.aikit, ["now-context", "prepare", "--request-file", str(work/"redis-prepare.json")],
        env, central)
    redis_inspect, redis_inspect_ms = aikit_json(
        args.aikit, ["now-context", "inspect", "--config-file", str(work/"redis.json"),
                     "--participant-ref", "agent/comparison-worker"], env, central)
    redis_ms = (time.perf_counter() - t0) * 1000.0
    factory_payload = redis_inspect["prepared"]["factory"]
    if not factory_payload or not factory_payload["workflow_units"][0]["capability_refs"]:
        raise RuntimeError("Prepared Redis arm did not retain Factory capability meaning")

    controlled = None
    server = None
    jev_env = dict(env)
    if args.jev_mode == "controlled":
        server = start_controlled()
        controlled = f"127.0.0.1:{server.server_address[1]}"
        jev_env["AIKIT_JEV_CONTROLLED_KEY"] = CONTROLLED_KEY
        credential_ref = "env://AIKIT_JEV_CONTROLLED_KEY"
        allow_env = True
    elif args.jev_mode == "live":
        if not args.jev_credential_ref:
            raise RuntimeError("--jev-credential-ref is required for live mode")
        credential_ref = args.jev_credential_ref
        allow_env = args.allow_env_import
    else:
        credential_ref = None
        allow_env = False

    general_jev = None
    actuation_evidence = None
    jev_result = None
    jev_inspect = None
    jev_ms = None
    if args.jev_mode != "skip":
        general_request = {
            "model": args.jev_model,
            "state": {"undertaking": "Classify a general maintenance decision outside document practice"},
            "questions": {
                "defer": {"type": "noul", "instructions": "Should this action wait?"},
                "route": {"type": "choice", "instructions": "Which route is appropriate?",
                          "criteria": {"continue": "Proceed", "hold": "Wait"}},
                "risk": {"type": "score", "instructions": "Rate operational risk",
                         "criteria": ["low", "medium", "high"]},
            },
        }
        write_json(work/"jev-general.json", general_request)
        write_json(work/"jev-limits.json", limits(args.jev_model, args.jev_budget_microusd))
        general_args = [
            "jev", "invoke", "--request-file", str(work/"jev-general.json"),
            "--limits-file", str(work/"jev-limits.json"),
            "--credential-ref", credential_ref,
        ]
        if allow_env:
            general_args.append("--allow-env-import")
        if controlled:
            general_args += ["--controlled-endpoint", controlled]
        general_jev, _ = aikit_json(args.aikit, general_args, jev_env, central)
        expected_standing = "controlled-protocol" if controlled else "provider-protocol"
        if general_jev["standing"] != expected_standing:
            raise RuntimeError(f"Jev standing mismatch: {general_jev['standing']}")

        if args.actuation:
            # Actuation owns the durable Activity/usage relation. The cloud
            # defaults are explicitly controlled identities; installed live
            # episodes pass the admitted Agency/Actuation/Session refs.
            store = base / "actuation-streams"
            store.mkdir()
            observed = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            opening = {
                "stream_ref": args.actuation_stream_ref,
                "actuation_ref": args.actuation_ref,
                "agency_ref": args.agency_ref,
                "agent_session_ref": args.actuation_session_ref,
                "provenance": ["aikit.jev-invocation/v1"],
                "started_at": observed,
            }
            opened, _ = actuation_json(
                args.actuation, ["stream", "open", "--store", str(store), "-"],
                opening, jev_env)
            invocation_ref = general_jev["invocation_ref"]
            usage_ref = "model-usage:jev:" + invocation_ref.split("/")[-1]
            usage = general_jev["answer"]["usage"]
            model = general_jev["answer"]["model"]
            observation = {
                "schema": "actuation.model-usage/v1",
                "usage_ref": usage_ref,
                "actuation_ref": args.actuation_ref,
                "invocation_ref": invocation_ref,
                "correlation": {
                    "agency_ref": args.agency_ref,
                    "agent_session_ref": args.actuation_session_ref,
                    "external_refs": [run_ref, now_ref],
                },
                "provider": {
                    "standing": "normalized-from-native",
                    "name": "typesafe-systemone" if not controlled else "typesafe-systemone-controlled",
                },
                "model": {"standing": "provider-reported", "name": model},
                "tokens": {
                    "standing": "provider-reported",
                    "input": usage["input_tokens"],
                    "output": usage["output_tokens"],
                },
                "cache": {"standing": "not-reported"},
                "timing": {
                    "latency": {
                        "standing": "observed",
                        "milliseconds": general_jev["elapsed_ms"],
                    }
                },
                # AIKit retains the bounded tariff calculation. Actuation does
                # not call it observed monetary cost without an exact effective
                # pricing basis.
                "cost": {"standing": "unavailable"},
                "outcome": {
                    "state": "completed",
                    "standing": "observed",
                    "reason": "jev-completed",
                },
                "provenance": {
                    "reporter_ref": "aikit:jev",
                    "native_event_ref": invocation_ref,
                    "native_request_ref": invocation_ref,
                    "native_schema": "aikit.jev-invocation/v1",
                    "observed_at": observed,
                    "raw_evidence_refs": [invocation_ref],
                },
            }
            occurrence = {
                "adapter": "observation",
                "stream_ref": args.actuation_stream_ref,
                "event_ref": "event:jev-usage",
                "native_event": observation,
            }
            usage_receipt, _ = actuation_json(
                args.actuation, ["stream", "usage", "--store", str(store), "-"],
                occurrence, jev_env)
            replay, _ = run(
                [args.actuation, "stream", "replay", args.actuation_stream_ref,
                 "--store", str(store), "--json"], env=jev_env)
            replay_value = parse_json(replay)
            activity = {
                "schema": "actuation.activity/v1",
                "activity_ref": args.activity_ref,
                "actor": {"agency_ref": args.agency_ref},
                "agent_session_ref": args.actuation_session_ref,
                "run_ref": run_ref,
                "subject_ref": "task:oi-65-jev-redis-cloud-proof",
                "native_owner": "actuation",
                "action_ref": "action/model/decide",
                "invocation_ref": invocation_ref,
                "actuation_ref": args.actuation_ref,
                "usage_refs": [usage_ref],
                "verb": "classified",
                "object": "general-jev-question",
                "summary": "General typed Jev decision attributed through Actuation",
                "phase": "completed",
                "outcome": "succeeded",
                "salience": "normal",
                "needs_attention": False,
                "trace": {
                    "stream_ref": args.actuation_stream_ref,
                    "event_refs": ["event:jev-usage"],
                    "from_sequence": 1,
                    "through_sequence": 1,
                },
                "started_at": observed,
                "updated_at": observed,
                "completed_at": observed,
                "metadata": {
                    "standing": "controlled-actor" if controlled else "live-owner-supplied",
                    "now_ref": now_ref,
                },
            }
            activity_value, _ = actuation_json(
                args.actuation, ["activity", "-"], activity, jev_env)
            actuation_evidence = {
                "opening": opened,
                "usage": usage_receipt,
                "replay": replay_value,
                "activity": activity_value,
            }

        selection = {
            "mode": "jev", "credential_ref": credential_ref,
            "limits": limits(args.jev_model, args.jev_budget_microusd),
            "state": {
                "undertaking": "Select all complementary capabilities needed for the bounded change",
                "catalogue_question": "Can the current catalogue completely represent the need?",
            },
            "relevance_threshold": 0.5, "allow_env_import": allow_env,
            "curl": None, "controlled_endpoint": controlled,
        }
        jev_req = prep_request(
            jev_cfg, "agent/comparison-worker", "agent-session/comparison-jev",
            unit_refs[0], selection, public_candidates + [verifier_canary]
        )
        write_json(work/"jev-prepare.json", jev_req)
        write_json(work/"jev-redis.json", jev_cfg)
        t0 = time.perf_counter()
        jev_result, jev_prepare_ms = aikit_json(
            args.aikit, ["now-context", "prepare", "--request-file", str(work/"jev-prepare.json")],
            jev_env, central)
        jev_inspect, jev_inspect_ms = aikit_json(
            args.aikit, ["now-context", "inspect", "--config-file", str(work/"jev-redis.json"),
                         "--participant-ref", "agent/comparison-worker"], jev_env, central)
        jev_ms = (time.perf_counter() - t0) * 1000.0
        selected = jev_result["selection"]["selected_candidate_refs"]
        if len(selected) < 2 or jev_result["selection"]["catalogue_sufficient_noul"] >= 0.5:
            raise RuntimeError("controlled Jev arm did not expose multi-capability selection + catalogue gap")
        if "context-source/verifier-canary" not in jev_result["selection"]["withheld_from_jev"]:
            raise RuntimeError("egress-denied verifier material was sent to Jev")
        if "VERIFIER_EXPECTATION_CANARY" in json.dumps(jev_inspect):
            raise RuntimeError("verifier canary leaked into worker prepared context")

    # Shared undertaking: distinct related worker and verifier views.
    write_json(work/"episode-redis.json", episode_cfg)
    for participant, session, unit_ref, candidates in [
        ("agent/comparison-worker", "agent-session/episode-implementer", unit_refs[0], public_candidates),
        ("agent/related-worker", "agent-session/episode-related", unit_refs[1], public_candidates[:2]),
        ("agent/verifier", "agent-session/episode-verifier", unit_refs[2], [verifier_canary]),
    ]:
        request = prep_request(
            episode_cfg, participant, session, unit_ref, {"mode": "all"}, candidates
        )
        file = work / (participant.replace("/", "-") + ".json")
        write_json(file, request)
        aikit_json(args.aikit, ["now-context", "prepare", "--request-file", str(file)], env, central)

    implementer, _ = aikit_json(
        args.aikit, ["now-context", "inspect", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/comparison-worker"], env, central)
    related, _ = aikit_json(
        args.aikit, ["now-context", "inspect", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/related-worker"], env, central)
    verifier, _ = aikit_json(
        args.aikit, ["now-context", "inspect", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/verifier"], env, central)
    if "VERIFIER_EXPECTATION_CANARY" in json.dumps(implementer) or "VERIFIER_EXPECTATION_CANARY" in json.dumps(related):
        raise RuntimeError("verifier-private material crossed a participant boundary")
    if "VERIFIER_EXPECTATION_CANARY" not in json.dumps(verifier):
        raise RuntimeError("verifier did not receive its own private expectation")

    change = {
        "change_id": "related-return-r1", "kind": "factory-return",
        "source_ref": "return/related-worker", "source_revision": "return-r1",
        "detail": "Related worker returned a dependency result for the undertaking",
        "observed_at_unix_ms": int(time.time() * 1000),
    }
    write_json(work/"change.json", change)
    change_receipt, _ = aikit_json(
        args.aikit, ["now-context", "append-change", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/comparison-worker", "--change-file", str(work/"change.json")],
        env, central)

    # Fresh-session continuation updates the same participant by CAS.
    continuation = prep_request(
        episode_cfg, "agent/comparison-worker", "agent-session/episode-fresh",
        unit_refs[0], {"mode": "all"}, public_candidates, expected=1,
        continuation="fresh admitted session continues the same Factory undertaking"
    )
    write_json(work/"fresh.json", continuation)
    fresh, _ = aikit_json(
        args.aikit, ["now-context", "prepare", "--request-file", str(work/"fresh.json")], env, central)
    stale_attempt, _ = aikit_json(
        args.aikit, ["now-context", "prepare", "--request-file", str(work/"fresh.json")],
        env, central, ok=False)
    if "now_context.stale" not in stale_attempt["stderr"] + stale_attempt["stdout"]:
        raise RuntimeError("stale expected-version preparation did not expose its conflict")

    source_conflict = None
    provider_failure = None
    if server is not None:
        # Change a real Central source while Jev is in flight. Final source
        # revalidation must reject the old basis before publication.
        conflict_cfg = redis_config(args.redis_address, f"aikit-oi65-{os.getpid()}-conflict")
        conflict_selection = {
            "mode": "jev", "credential_ref": credential_ref,
            "limits": limits(args.jev_model, args.jev_budget_microusd),
            "state": {"undertaking": "stale-basis proof"},
            "relevance_threshold": 0.5, "allow_env_import": True,
            "curl": None, "controlled_endpoint": controlled,
        }
        conflict_req = prep_request(
            conflict_cfg, "agent/stale-worker", "agent-session/stale-worker",
            unit_refs[0], conflict_selection, public_candidates
        )
        write_json(work/"conflict.json", conflict_req)
        server.request_seen.clear(); server.release_response.clear(); server.delay_once = True
        proc = subprocess.Popen(
            [args.aikit, "--json", "-C", str(central), "now-context", "prepare",
             "--request-file", str(work/"conflict.json")],
            env=jev_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if not server.request_seen.wait(timeout=10):
            proc.kill(); raise RuntimeError("controlled Jev request did not reach loopback server")
        source_path.write_text(source_path.read_text() + "\nmaterial revision changed in-flight\n")
        ctrl_action(args.ctrl, central, "central.file-map.refresh", {}, env)
        server.release_response.set()
        out, err = proc.communicate(timeout=30)
        if proc.returncode == 0:
            raise RuntimeError("in-flight stale source basis replaced a newer source")
        source_conflict = {"returncode": proc.returncode, "stdout": out, "stderr": err}

        # Missing/malformed provider answer must not publish a successful view.
        server.malformed_once = True
        failure_cfg = redis_config(args.redis_address, f"aikit-oi65-{os.getpid()}-provider-failure")
        failure_req = prep_request(
            failure_cfg, "agent/provider-failure", "agent-session/provider-failure",
            unit_refs[0], conflict_selection, public_candidates
        )
        write_json(work/"provider-failure.json", failure_req)
        provider_failure, _ = aikit_json(
            args.aikit, ["now-context", "prepare", "--request-file", str(work/"provider-failure.json")],
            jev_env, central, ok=False)
        if provider_failure["returncode"] == 0:
            raise RuntimeError("malformed provider answer resembled success")

    # Revocation removes hot payload for the revoked actor.
    revoke, _ = aikit_json(
        args.aikit, ["now-context", "revoke", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/verifier", "--disclosure-revision", "cloud-proof-disclosure-r1"],
        env, central)
    revoked_inspect, _ = aikit_json(
        args.aikit, ["now-context", "inspect", "--config-file", str(work/"episode-redis.json"),
                     "--participant-ref", "agent/verifier"], env, central, ok=False)

    # Durable Return: the acting field may update agent-maintained Wiki/practice
    # knowledge, but it does not silently rewrite human-authored account/matrix
    # ground. Record the catalogue gap as explicit pressure and write the
    # authorised practice Return through AIKit's CAS-validated Wiki owner.
    wiki_return, wiki_write_ms = aikit_json(
        args.aikit, [
            "wiki", "node", "create", "wiki:node:jev-redis-now-return",
            "--file", str(wiki_file),
            "--space", "central:wiki:root",
            "--type", "practice-return",
            "--title", "Jev Redis NOW Return: revalidate source and disclosure at delivery",
            "--source", source_ref,
        ], env, central)
    wiki_validate, wiki_validate_ms = aikit_json(
        args.aikit, ["wiki", "validate", str(wiki_file)], env, central)
    if wiki_validate.get("valid") is not True:
        raise RuntimeError("durable Wiki Return failed whole-document validation")
    wiki_query, wiki_query_ms = aikit_json(
        args.aikit, [
            "wiki", "query", "search",
            "Jev Redis NOW Return",
            "--file", str(wiki_file),
        ], env, central)
    if "jev-redis-now-return" not in json.dumps(wiki_query):
        raise RuntimeError("native Wiki Return was not queryable after write")

    later_req = prep_request(
        episode_cfg, "agent/later-participant", "agent-session/later-participant",
        unit_refs[0], {"mode": "all"}, public_candidates, expected=0,
        continuation="later fresh participant enters after durable Wiki/practice Return",
        wiki_queries=["Jev Redis NOW Return"]
    )
    write_json(work/"later-participant.json", later_req)
    later_prepare, _ = aikit_json(
        args.aikit, ["now-context", "prepare", "--request-file", str(work/"later-participant.json")],
        env, central)
    later_inspect, _ = aikit_json(
        args.aikit, [
            "now-context", "inspect", "--config-file", str(work/"episode-redis.json"),
            "--participant-ref", "agent/later-participant"
        ], env, central)
    later_used_revised_wiki = "Jev Redis NOW Return" in json.dumps(later_inspect)
    if not later_used_revised_wiki:
        raise RuntimeError("later fresh participant did not receive revised Wiki knowledge")

    durable_return = {
        "wiki_write": wiki_return,
        "wiki_validate": wiki_validate,
        "wiki_query": wiki_query,
        "wiki_write_ms": wiki_write_ms,
        "wiki_validate_ms": wiki_validate_ms,
        "wiki_query_ms": wiki_query_ms,
        "later_prepare": later_prepare,
        "later_participant_used_revised_wiki": later_used_revised_wiki,
        "matrix_pressure": {
            "warranted": bool(
                jev_result is not None
                and jev_result["selection"]["catalogue_sufficient_noul"] < 0.5
            ),
            "standing": "proposal-pressure-not-silent-authorship",
            "reason": "controlled multi-capability need was judged insufficiently represented; human-authored account/matrix ground remains an explicit source decision"
        }
    }

    comparison = {
        "schema": "aikit.jev-redis-now-comparison/v1",
        "standing": "controlled-actor observation; no worker-model performance claim",
        "conditions": {
            "task_ref": "task:oi-65-jev-redis-cloud-proof", "now_ref": now_ref,
            "central_source_ref": source_ref, "factory_run_ref": run_ref,
            "factory_workflow_unit_ref": unit_refs[0], "worker_model": "not-invoked-in-controlled-comparison",
        },
        "arms": {
            "ordinary": {
                "elapsed_ms": ordinary_ms, "context_discovery_calls": len(baseline_calls),
                "calls": baseline_calls, "jev_calls": 0, "redis_prepared": False,
            },
            "redis_prepared_no_jev": {
                "elapsed_ms": redis_ms, "prepare_ms": redis_prepare_ms,
                "inspect_ms": redis_inspect_ms, "context_discovery_calls_after_entry": 0,
                "jev_calls": 0, "redis_prepared": True,
                "prepared_version": redis_result["publishedVersion"],
            },
            "jev_assisted": None if jev_result is None else {
                "elapsed_ms": jev_ms, "prepare_ms": jev_prepare_ms,
                "inspect_ms": jev_inspect_ms, "context_discovery_calls_after_entry": 0,
                "jev_calls": 1, "redis_prepared": True,
                "prepared_version": jev_result["publishedVersion"],
                "selection": jev_result["selection"],
            },
        },
        "unavailable_metrics": [
            "worker-model token/cost/latency (no live worker model in controlled cloud comparison)",
            "human corrections (requires human episode)",
        ],
    }

    result = {
        "schema": "aikit.jev-redis-now-joined-proof/v1",
        "sources": {
            "central_root": str(central), "central_source_ref": source_ref,
            "central_now_ref": now_ref, "factory_project_ref": project_ref,
            "factory_run_ref": run_ref, "workflow_unit_refs": unit_refs,
        },
        "general_jev": general_jev,
        "actuation": actuation_evidence,
        "redis_arm": redis_result,
        "jev_arm": jev_result,
        "participant_isolation": {
            "implementer_version": implementer["currentVersion"],
            "related_version": related["currentVersion"],
            "verifier_version_before_revoke": verifier["currentVersion"],
            "verifier_canary_isolated": True,
        },
        "return_change": change_receipt,
        "fresh_session": fresh,
        "stale_version_refused": True,
        "source_change_during_jev_refused": source_conflict is not None,
        "provider_malformed_refused": provider_failure is not None,
        "revocation": revoke,
        "revoked_read_refused": revoked_inspect["returncode"] != 0,
        "durable_return": durable_return,
        "comparison": comparison,
        "knowledge_search": knowledge,
        "environment_only_remaining": (
            ["live Jev provider", "installed/live worker model", "human experience judgment"]
            if args.jev_mode != "live" else
            ["installed/live worker model", "human experience judgment"]
        ),
    }
    pathlib.Path(args.output).write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if server is not None:
        server.shutdown()
    root_tmp.cleanup()

if __name__ == "__main__":
    main()
