#!/usr/bin/env python3
"""Fixture owners for World-inhabitation contact tests.

One script answers as three owners, chosen by the name it is invoked under
(`ctrl`, `actuation`, `factory`), over one JSON world file named by
$FIXTURE_WORLD. It speaks exactly the CLI shapes the pinned contract
(O-I docs/contracts/WORLD-INHABITATION-V1.md) and the owners' in-flight
implementations publish:

  ctrl --json [--root R] action run central.position.list|central.position.read|central.world.here JSON
  actuation occupancy list|read|verify|claim|release ... --json
  factory development current-work|custody assign ... --json

Every invocation is appended to $FIXTURE_WORLD.calls so a test can prove which
owner verbs ran. Anything else answers the way a not-yet-upgraded owner does.

Occupancy is Actuation's, and Actuation keeps one ledger per Workcell. When
$FIXTURE_OCCUPANCY names a file, `actuation` reads and writes occupancy there
instead of in the world file, so two AIKit homes standing for two Workcells
share Central's Position definitions but each keeps its own occupancy ledger.
"""
import json
import os
import sys
import time
import uuid

WORLD = os.environ["FIXTURE_WORLD"]
LEDGER = os.environ.get("FIXTURE_OCCUPANCY") or None


def load(path=WORLD):
    with open(path) as handle:
        return json.load(handle)


def save(world, path=WORLD):
    tmp = path + ".tmp"
    with open(tmp, "w") as handle:
        json.dump(world, handle, indent=2)
    os.replace(tmp, path)


def load_ledger():
    """This Workcell's occupancy ledger: its own file, or the world file."""
    if LEDGER is None:
        return load()
    if not os.path.exists(LEDGER):
        return {"occupancy": {}, "presence": {}}
    return load(LEDGER)


def save_ledger(ledger):
    if LEDGER is None:
        save(ledger)
    else:
        save(ledger, LEDGER)


def emit(value, status=0):
    print(json.dumps(value))
    sys.exit(status)


def flag(args, name):
    if name in args:
        index = args.index(name)
        if index + 1 < len(args):
            return args[index + 1]
    return None


def refusal(code, fact, consequence, action, status=1):
    emit({"ok": False, "error": {"code": code, "fact": fact, "consequence": consequence, "action": action}}, status)


def tenures(world, position):
    return world.setdefault("occupancy", {}).setdefault(position, [])


def current(world, position):
    open_ = [t for t in tenures(world, position) if t.get("ended_at_unix_ms") is None]
    return open_[0] if open_ else None


def reading(world, position):
    gens = tenures(world, position)
    now = current(world, position)
    value = {
        "schema": "actuation.position-occupancy/v1",
        "position_ref": position,
        "state": "occupied" if now else "vacant",
        "generations": gens,
    }
    if now:
        value["current"] = now
        presence = world.get("presence", {}).get(position)
        if presence and presence.get("generation_ref") == now["generation_ref"]:
            value["presence"] = presence
    return value


def ctrl(args):
    if "--json" not in args or "action" not in args:
        emit({"ok": False, "error": {"code": "invalid_input", "message": "fixture ctrl answers only action run"}}, 2)
    action = args[args.index("run") + 1]
    payload = json.loads(args[args.index("run") + 2])
    world = load()
    if action == "central.position.list":
        world_ref = world.get("world_ref", "control:root")
        positions = [{"record": p, "source": {"ref": "fixture", "revision": "r1"}}
                     for p in world.get("positions", []) if p["enclosing_world_ref"] == world_ref]
        inherited = [{"record": p, "source": {"ref": "fixture", "revision": "r1"}}
                     for p in world.get("positions", []) if p["enclosing_world_ref"] != world_ref]
        emit({"ok": True, "action": action, "data": {
            "schema": "central.position-listing/v1", "world_ref": world_ref,
            "positions": positions, "inherited": inherited, "invalid": []}})
    if action == "central.position.read":
        for p in world.get("positions", []):
            if p["ref"] == payload["position_ref"]:
                emit({"ok": True, "action": action, "data": {"record": p, "source": {"ref": "fixture", "revision": "r1"}}})
        emit({"ok": False, "action": action,
              "error": {"code": "central.position_not_found", "message": "not found"},
              "data": {"fact": "No Position definition exists at %s." % payload["position_ref"],
                       "consequence": "Nothing was read.",
                       "action": "ctrl --json action run central.position.list '{}'"}}, 2)
    if action == "central.world.here":
        emit({"ok": True, "action": action, "data": {
            "schema": "central.world-here/v1",
            "local_world": {"ref": "control:root", "root": "/fixture"},
            "project_world": {"state": "absent"},
            "workcells": [],
        }})
    emit({"ok": False, "action": action, "error": {"code": "invalid_input", "message": "Unknown Action: %s" % action}}, 2)


def actuation(args):
    if args[:1] != ["occupancy"]:
        sys.stderr.write("actuation: unknown command %s; run actuation help\n" % (args[:1] or [""])[0])
        sys.exit(2)
    verb = args[1]
    world = load_ledger()
    position = flag(args, "--position")
    if verb == "list":
        rows = []
        for ref in sorted(world.get("occupancy", {})):
            now = current(world, ref)
            row = {"position_ref": ref, "state": "occupied" if now else "vacant",
                   "generation_count": len(tenures(world, ref))}
            if now:
                row["current"] = now
                presence = world.get("presence", {}).get(ref)
                if presence:
                    row["presence"] = presence
            rows.append(row)
        emit({"schema": "actuation.position-occupancy-listing/v1", "store": "fixture", "positions": rows, "invalid": []})
    if verb == "read":
        emit(reading(world, position))
    if verb == "verify":
        generation = flag(args, "--generation")
        now = current(world, position)
        if now and now["generation_ref"] == generation:
            emit({"ok": True, "verb": "verify", "position_ref": position, "generation": generation, "current": now})
        known = [t for t in tenures(world, position) if t["generation_ref"] == generation]
        if known:
            refusal("occupancy.superseded",
                    "%s no longer holds %s; it was %s." % (generation, position, known[0].get("end_kind", "ended")),
                    "Nothing was verified; this generation grants no authority.",
                    "actuation occupancy read --position %s" % position)
        refusal("occupancy.unknown_generation", "%s never held %s." % (generation, position),
                "Nothing was verified.", "actuation occupancy read --position %s" % position)
    if verb == "claim":
        gens = tenures(world, position)
        now = current(world, position)
        stamp = int(time.time() * 1000)
        new = {
            "position_ref": position,
            "generation_ref": "actuation:generation:%s" % (flag(args, "--generation-id") or uuid.uuid4()),
            "generation_ordinal": len(gens) + 1,
            "kind": flag(args, "--kind") or ("handover" if now else "initial"),
            "agent_ref": flag(args, "--agent") or "agent/fixture",
            "agency_ref": flag(args, "--agency") or "agency/fixture",
            "workcell_ref": flag(args, "--workcell"),
            "began_at_unix_ms": stamp,
            "reason": flag(args, "--reason") or "fixture claim",
        }
        if now:
            now["ended_at_unix_ms"] = stamp
            now["end_kind"] = "superseded"
            new["predecessor_generation_ref"] = now["generation_ref"]
        gens.append(new)
        save_ledger(world)
        emit({"ok": True, "verb": "claim", "tenure": new})
    if verb == "release":
        now = current(world, position)
        if not now:
            refusal("occupancy.vacant", "%s has no current occupant to release." % position,
                    "Nothing was released.", "actuation occupancy read --position %s" % position)
        now["ended_at_unix_ms"] = int(time.time() * 1000)
        now["end_kind"] = "released"
        save_ledger(world)
        emit({"ok": True, "verb": "release", "tenure": now})
    sys.stderr.write("actuation occupancy: unknown verb %s\n" % verb)
    sys.exit(2)


def factory(args):
    if args[:1] != ["development"]:
        sys.exit(2)
    world = load()
    if args[1] == "current-work":
        position = flag(args, "--position")
        live = [c for c in world.get("custody", []) if c["position_ref"] == position and c["state"] == "in-progress"]
        nodes = sorted({c["work_ref"] for c in live})
        outcome = "none" if not nodes else ("one" if len(nodes) == 1 else "ambiguous")
        current_ = None
        if outcome == "one":
            current_ = {"node_ref": nodes[0], "work_ref": nodes[0], "run_ref": live[0].get("run_ref")}
        emit({"schema": "factory.current-work/v1", "position_ref": position, "outcome": outcome,
              "current": current_, "candidates": [{"work_ref": n} for n in nodes] if outcome == "ambiguous" else [],
              "considered": len(live), "basis": "fixture"})
    if args[1:3] == ["custody", "assign"]:
        work = flag(args, "--work")
        if work == "work:refused":
            emit({"schema": "factory.refusal/v1", "code": "factory.custody.run_not_found",
                  "fact": "No Run holds work:refused.", "consequence": "No custody was created.",
                  "action": "factory development custody list"}, 1)
        custody = {
            "schema": "factory.work-custody/v1",
            "custody_ref": "factory:custody:%s" % uuid.uuid4(),
            "position_ref": flag(args, "--position"),
            "work_ref": work,
            "run_ref": flag(args, "--run"),
            "state": "in-progress",
            "reason": flag(args, "--reason"),
            "origin": {"communique_ref": flag(args, "--origin-communique")},
        }
        world.setdefault("custody", []).append(custody)
        save(world)
        emit({"schema": "factory.work-custody-receipt/v1", "result": "applied", "custody": custody})
    sys.exit(2)


def main():
    with open(WORLD + ".calls", "a") as calls:
        calls.write(json.dumps([os.path.basename(sys.argv[0])] + sys.argv[1:]) + "\n")
    name = os.path.basename(sys.argv[0])
    args = sys.argv[1:]
    if name == "ctrl":
        ctrl(args)
    if name == "actuation":
        actuation(args)
    if name == "factory":
        factory(args)
    sys.exit(3)


main()
