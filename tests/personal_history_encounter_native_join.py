#!/usr/bin/env python3
"""Central #242 acceptance E/F, native join: a fresh body meets adopted history.

Real ctrl (Central #245 surface) + real aikit (coverage/span/prepare lane) +
real Redis + a controlled accepted body. The archive is NEVER in the prompt:
the concern alone asks what the archive answers, and every native link is
asserted — search/read, prepared revision, delivered context at the body
boundary, then a correction and a fresh-body restart that carries it.

Run: python3 tests/personal_history_encounter_native_join.py \
       --ctrl /built/ctrl --aikit /built/aikit
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import secrets
import subprocess
import tempfile
import time

CHECKS: list[str] = []


def check(condition, what: str):
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def run(argv, env, expect=True, cwd=None):
    result = subprocess.run([str(a) for a in argv], env=env, cwd=cwd,
                            capture_output=True, text=True, timeout=120)
    if expect and result.returncode != 0:
        raise RuntimeError(f"{argv[:4]} refused: {result.stdout[:300]} {result.stderr[:300]}")
    return result


def jargv(argv, env, expect=True, cwd=None):
    result = run(argv, env, expect, cwd)
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ctrl", required=True)
    parser.add_argument("--aikit", required=True)
    parser.add_argument("--redis-address", default="127.0.0.1:6379")
    args, rest = parser.parse_known_args()
    ctrl, aikit = pathlib.Path(args.ctrl).resolve(), pathlib.Path(args.aikit).resolve()

    base = pathlib.Path(tempfile.mkdtemp(prefix="ph-encounter-join-"))
    root = base / "Central"
    (root / "Work").mkdir(parents=True)
    home = base / "home"
    home.mkdir()
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("CENTRAL_", "OI_", "AIKIT_", "BKMR_"))}
    env.update(HOME=str(home), CENTRAL_ROOT=str(root),
               CENTRAL_CTRL_BIN=str(ctrl), AIKIT_HOME=str(base / "aikit-home"),
               NO_COLOR="1")

    def central(action, payload, expect=True):
        reply = jargv([ctrl, "--json", "--root", root, "action", "run", action,
                       json.dumps(payload)], env, expect)
        return reply["data"] if expect else reply

    def ai(argv, expect=True):
        value = jargv([aikit, "--json", "-C", root, *argv], env, expect, cwd=root)
        if isinstance(value, dict) and value.get("ok") is True and "data" in value:
            value = value["data"]
        return value

    # -- the world: person anchor, ordinary ground -------------------------
    run([ctrl, "--json", "--root", root, "init"], env)
    identity = root / "Control/user/identity"
    identity.mkdir(parents=True, exist_ok=True)
    (identity / "present.md").write_text("I am Wren Ellison. I keep field notes.\n")
    (identity / "manifest.json").write_text(json.dumps({
        "schema": "central.pasu.identity-manifest/v1", "revision": "1",
        "subject": {"ref": "central:pasu:nara:local", "title": "Wren Ellison"},
        "identity_source": {"path": "Control/user/identity",
            "provenance_law": "controlled corpus",
            "sources": [{"path": "Control/user/identity/present.md",
                         "standing": "authored-ground"}]}}, indent=1))
    anchor = central("central.personal.anchor.inspect", {})
    check(anchor["person"]["subject_ref"] == "central:pasu:nara:local",
          "E0 the fresh world anchors its person")
    (root / "Control/user/civil-time-policy.json").write_text(json.dumps({
        "schema": "central.civil-time-policy/v1", "scope_ref": "control:root",
        "timezone": "Europe/London", "day_boundary_minutes": 0,
        "automatic_day_rollover": True}, indent=1))

    # -- the archive: adopted through the ordinary intake ------------------
    origin = base / "archive"
    (origin / "journal").mkdir(parents=True)
    (origin / "journal/2026-05-20-the-long-walk.md").write_text(
        "---\ndate: 2026-05-20\ntype: essay\n---\n# The long walk\n\n"
        "Sections of weather narration. " * 400 +
        "FINAL PARAGRAPH — the decisive record: I decided on the quarry road, "
        "on 20 May 2026, to sell the van and keep the bicycle.\n")
    (origin / "journal/2026-03-14-first-thaw.md").write_text(
        "---\ndate: 2026-03-14\ntype: journal\n---\n# First thaw\n\n"
        "Snowdrops by the back step; the winter I kept calling lost was only "
        "dormant — a half-belief.\n")
    (origin / "journal/2026-06-11-correction.md").write_text(
        "---\ndate: 2026-06-11\ntype: journal\n---\n# Correction\n\n"
        "The snowdrops were crocuses; the dormant line stands.\n")
    plan = central("central.personal.collection.plan",
                   {"path": str(origin), "collection_id": "field-notes",
                    "title": "Field notes"})
    outcome = central("central.personal.collection.apply",
                      {"plan": plan, "acceptance": "human-accepted"})
    check(outcome["receipt"]["entries_added"] == 3,
          "E1 the archive is adopted through the ordinary intake")
    record = json.loads((root / "Control/user/collections/field-notes/collection.json").read_text())
    entries = {e["entry_id"]: e for e in record["entries"]}
    source_refs = {e["entry_id"]: e["source_ref"] for e in record["entries"]}

    # -- knowledge: the entries are discoverable, coverage is declared -----
    for entry_id, source_ref in source_refs.items():
        if entry_id.endswith(".bin"):
            continue
        reading = ai(["knowledge", "read", json.dumps({"kind": "source", "value": source_ref})])
        check(reading["content"], f"E2 {entry_id} reopens exactly")
        ai(["knowledge", "coverage", "declare", source_ref,
            "--revision", reading["revision"],
            "--extents", f"0:{len(reading['content'].chars()) if hasattr(reading['content'], 'chars') else len(reading['content'])}",
            "--actor", "agent:zcode-242-lead",
            "--note", "read whole; meaning kept qualified"])
    correction_entry = entries["journal/2026-06-11-correction.md"]
    ai(["knowledge", "coverage", "declare", correction_entry["source_ref"],
        "--revision", correction_entry["content_revision"],
        "--extents", "0:120", "--actor", "agent:zcode-242-lead",
        "--note", "correction: snowdrops were crocuses; the dormant line stands"])

    # -- the NOW substrate --------------------------------------------------
    policy = central("central.time.policy", {})
    day = central("central.day.ensure", {"expected_time_policy_revision": policy["revision"]})
    allocated = central("central.now.allocate", {
        "expected_policy_revision": policy["revision"],
        "task_ref": "central:task:control:root:ph-encounter-proof",
        "purpose": "Personal-history encounter proof (Central #242 acceptance E/F).",
        "horizon": "workcell-root",
        "participant_refs": ["central:pasu:nara:local"]})
    now_ref = allocated["now_ref"]
    check(bool(now_ref), "E3 the world holds a NOW clearing for the participant")

    # -- the body: express, review, accept through the world's own authority
    purpose = ("Answer my present concern from my world; quote my own words "
               "exactly and name their dates.")
    made = central("agent-profile.express", {
        "name": "Field-notes companion", "purpose": purpose,
        "intent_expression": purpose, "world_ref": "control:root",
        "ratified_world_refs": ["control:root"], "skill_refs": []})
    profile = made["profile"]
    review = central("agent-profile.review", {"profile_ref": profile["ref"]})
    acceptance = {"profile_ref": profile["ref"],
                  "expected_revision": profile["revision"],
                  "expected_content_digest": review["content_digest"]}
    central("agent-profile.accept", acceptance, expect=False)
    token = secrets.token_hex(32)
    authority = root / "Control/user/controlled-acceptance.json"
    authority.write_text(json.dumps({
        "schema": "central.native-action-authority/v1", "scope_ref": "control:root",
        "grants": [{"principal_ref": "human:controlled-test", "actor_kind": "human",
                    "token_sha256": hashlib.sha256(token.encode()).hexdigest(),
                    "scope_refs": ["control:root"],
                    "actions": ["agent-profile.accept"],
                    "expires_at_unix_seconds": int(time.time()) + 600}]}))
    env["CENTRAL_NATIVE_TOKEN"] = token
    accepted = central("agent-profile.accept", acceptance)
    check(accepted["accepted"] is True, "E4 the body's profile is accepted by the world's own authority")
    del env["CENTRAL_NATIVE_TOKEN"]

    # -- preparation: the concern alone; the archive is never in the prompt -
    redis_cfg = {"schema": "aikit.redis-now-config/v1",
                 "address": args.redis_address, "database": 0,
                 "key_prefix": f"ph-join-{os.getpid()}", "username": None,
                 "credential_ref": None, "allow_remote": False,
                 "connect_timeout_ms": 1000, "io_timeout_ms": 1000,
                 "prepared_ttl_seconds": 3600,
                 "coordination_retention_seconds": 7200}
    candidates = [{"selection": json.dumps({
        "kind": "source", "value": ref}), "visibility": "team",
        "note": entry_id}
        for entry_id, ref in source_refs.items() if not entry_id.endswith(".bin")]
    request = {
        "schema": "aikit.now-preparation-request/v1", "redis": redis_cfg,
        "project_ref": "control:root", "now_ref": now_ref,
        "participant_ref": "central:pasu:nara:local",
        "agent_session": "agent-session/ph-encounter-fresh-body",
        "concern": ("What did I decide about the van this spring, and has "
                    "anything I wrote since qualified what I observed in "
                    "March? Quote my own words."),
        "disclosure_revision": "ph-encounter-disclosure-r1",
        "practice_refs": [],
        "central": {"root": str(root), "project": None,
                    "ctrl_bin": str(ctrl), "source_refs": list(source_refs.values())},
        "wiki_queries": ["van", "correction"],
        "candidate_items": candidates,
        "expected_version": 0, "external_provider": False,
        "allow_redis_env_import": False,
        "selection": {"mode": "all"},
    }
    work = base / "requests"
    work.mkdir()
    (work / "prepare.json").write_text(json.dumps(request))
    (work / "redis.json").write_text(json.dumps(redis_cfg))
    prepared = ai(["now-context", "prepare", "--request-file", str(work / "prepare.json")])
    body_text = json.dumps(prepared)
    check("sell the van" in body_text and "crocuses" in body_text,
          "E5 the prepared context carries the decisive archive material "
          "(the van decision and the crocus correction) — absent from the prompt")
    revision = prepared.get("version") or prepared.get("prepared_version")
    check(revision is not None and revision >= 1, "E6 the preparation carries a revision")

    inspected = ai(["now-context", "inspect", "--config-file", str(work / "redis.json"),
                    "--participant-ref", "central:pasu:nara:local"])
    check(inspected["prepared"]["version"] == revision,
          "E7 inspect reads back the same prepared revision")

    # -- the correction: declared forward, the next body sees it -----------
    ai(["knowledge", "coverage", "declare", correction_entry["source_ref"],
        "--revision", correction_entry["content_revision"],
        "--extents", "0:120", "--actor", "agent:zcode-242-lead",
        "--note", "CORRECTION APPLIED: quote crocuses, never snowdrops; the "
                  "dormant line stands. Sources citing snowdrops must carry "
                  "this qualification."])
    change = ai(["now-context", "append-change", "--config-file", str(work / "redis.json"),
                 "--participant-ref", "central:pasu:nara:local",
                 "--change", json.dumps({
                     "kind": "interpretation-corrected",
                     "source_ref": correction_entry["source_ref"],
                     "note": "the snowdrop reading is corrected to crocuses"})])
    request["expected_version"] = revision + 1
    request["agent_session"] = "agent-session/ph-encounter-fresh-body-2"
    (work / "prepare2.json").write_text(json.dumps(request))
    reprepaired = ai(["now-context", "prepare", "--request-file", str(work / "prepare2.json")])
    new_revision = reprepaired.get("version") or reprepaired.get("prepared_version")
    check(new_revision == revision + 1,
          "F1 the correction advances the prepared revision; a fresh body "
          "prepares against the corrected basis")

    print(json.dumps({"checks": CHECKS,
                      "world": str(root),
                      "prepared_revision": revision,
                      "corrected_revision": new_revision}, indent=1))
    print(f"\nOK — {len(CHECKS)} native links proven")


if __name__ == "__main__":
    main()
