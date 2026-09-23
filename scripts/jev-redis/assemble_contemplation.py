#!/usr/bin/env python3
"""Assemble a cross-document contemplation state from registered sources.

This is the designed assembly for the Jev/Redis NOW loop: the contemplation
state is built automatically from the documents the world already keeps —
the capability matrix (manifest + CSV), the UX spine trace (stories,
practices, capability-coverage cells) and the NOW reference — never
hand-authored per question.

Forward pass  (prospective): planning/development — what binds, what next.
Returning pass (retrospective): review/analysis — what changed, what returns.

Usage:
  python3 assemble_contemplation.py \
    --matrix-manifest ProjectCentral/user/capability-matrix.json \
    --matrix-csv ProjectCentral/user/capability-matrix.csv \
    --spine-trace <path>/ux-spine-trace.json \
    --now-ref central:now:control:root:<id> \
    --participant-ref participant/contemplation/forward \
    --out-request contemplation-request.json
"""

from __future__ import annotations

import argparse
import csv
import json
import pathlib
from collections import Counter


def load_matrix_rows(csv_path: pathlib.Path) -> list[dict]:
    with csv_path.open(newline="") as handle:
        return list(csv.DictReader(handle))


def implemented_field(rows: list[dict]) -> dict:
    caps = [r for r in rows if r.get("record_type") == "capability"]
    implemented = [c for c in caps if (c.get("implementation_status") or "").startswith("Implemented")]
    open_rows = [c for c in caps if c not in implemented]
    return {
        "capability_rows": len(caps),
        "implemented": len(implemented),
        "open_rows": [
            {"id": c["id"], "status": c.get("implementation_status")} for c in open_rows
        ],
    }


def grid_relations(rows: list[dict]) -> dict:
    relations = Counter(
        r.get("relation") for r in rows if r.get("record_type") == "relation"
        and r.get("relation") and ":" in r["relation"]
    )
    return {"relation_records": sum(relations.values()), "types": dict(relations)}


def spine_practices(trace: dict) -> list[dict]:
    stories = {s["id"]: s for s in trace.get("stories", [])}
    coverage = trace.get("m_capability_coverage", [])
    out = []
    for practice in trace.get("practices", []):
        pid = practice["id"]
        served = sorted(sid for sid, s in stories.items() if pid in (s.get("practices") or []))
        cells = [
            {"capability_ref": c.get("capability_ref"), "stories": c.get("stories")}
            for c in coverage
            if set(c.get("stories") or []) & set(served)
        ]
        out.append({
            "id": pid,
            "purpose": practice.get("purpose"),
            "stories_served": served,
            "implementation_owner": practice.get("implementation_owner"),
            "canonical_skill": practice.get("canonical_skill"),
            "capability_coverage_cells": cells,
        })
    return out


def contemplation_request(args, matrix_rows: list[dict], trace: dict) -> dict:
    field = implemented_field(matrix_rows)
    relations = grid_relations(matrix_rows)
    practices = spine_practices(trace)
    skill_less = [p for p in practices if not p.get("canonical_skill")]
    served_by = {p["id"]: p["stories_served"] for p in practices}

    def criterion(practice: dict) -> dict:
        body = {
            "purpose": practice["purpose"],
            "stories_served": practice["stories_served"],
            "implementation_owner": practice["implementation_owner"],
            "canonical_skill": practice["canonical_skill"],
            "capability_coverage_cells": practice["capability_coverage_cells"],
        }
        return body

    state = {
        "matrix_field": (
            f"ql-capability-matrix/1: {field['capability_rows']} capability rows, "
            f"{field['implemented']} implemented with linked evidence; open rows: "
            f"{json.dumps(field['open_rows'])}"
        ),
        "grid_relations": (
            f"{relations['relation_records']} relation records across "
            f"{json.dumps(relations['types'])}"
        ),
        "ux_spine": (
            f"ql.ux-spine-trace/1: {len(trace.get('stories', []))} stories, "
            f"{len(practices)} practices, {len(trace.get('m_capability_coverage', []))} "
            "capability-coverage cells. Skill-less practices are the open frontier; "
            "AIKit owns discovery/projection on the practices with canonical skills."
        ),
        "coverage_note": (
            "Each choice criterion carries the capability-coverage cells touching its "
            "stories, so the contemplation traverses practice -> stories -> capabilities "
            "across both documents. Assembled automatically from registered sources by "
            "scripts/jev-redis/assemble_contemplation.py."
        ),
    }
    request = {
        "model": args.jev_model,
        "state": state,
        "questions": {
            "binding_practice": {
                "type": "choice",
                "instructions": (
                    "Forward pass: across the skill-less UX-spine practices, which single "
                    "practice is the binding constraint for making agent development "
                    "autonomous relative to the UX spine, given the implemented capability "
                    "field? Weigh the stories each practice serves, the capability-coverage "
                    "cells touching those stories, and whether implemented capabilities "
                    "already carry the practice's substrate."
                ),
                "criteria": {p["id"]: criterion(p) for p in skill_less},
            },
            "spine_readiness": {
                "type": "score",
                "instructions": (
                    "Rate the field's current readiness for autonomous spine-relative agent "
                    "development: 0 low (contemplation cannot traverse the relations), "
                    "1 medium (traversal works but open practices gate autonomy), "
                    "2 high (the field carries development autonomously)."
                ),
                "criteria": ["low", "medium", "high"],
            },
        },
    }
    if args.retrospective:
        request["questions"]["binding_practice"]["instructions"] = (
            "Returning pass: across the skill-less UX-spine practices, which practice "
            "most needs its recognition returned into NOW and agent knowledge after this "
            "development episode — which changed, what was learned, and where does the "
            "improved field most need a fresh participant?"
        )
    request["questions"]["_served_by"] = served_by  # verification aid, stripped by caller if undesired
    return request


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--matrix-manifest", required=True)
    ap.add_argument("--matrix-csv", required=True)
    ap.add_argument("--spine-trace", required=True)
    ap.add_argument("--now-ref", required=True)
    ap.add_argument("--participant-ref", default="participant/contemplation/forward")
    ap.add_argument("--agent-session", default=None)
    ap.add_argument("--redis-config", default=None, help="aikit.redis-now-config/v1 JSON file")
    ap.add_argument("--jev-model", default="jev-latest")
    ap.add_argument("--retrospective", action="store_true",
                    help="Assemble the returning (retrospective) pass instead of the forward pass")
    ap.add_argument("--out-request", required=True)
    args = ap.parse_args()

    matrix_rows = load_matrix_rows(pathlib.Path(args.matrix_csv))
    trace = json.loads(pathlib.Path(args.spine_trace).read_text())
    request = contemplation_request(args, matrix_rows, trace)

    served_by = request["questions"].pop("_served_by")
    out = {"schema": "aikit.contemplation-assembly/v1", "now_ref": args.now_ref,
           "pass": "retrospective" if args.retrospective else "prospective",
           "participant_ref": args.participant_ref,
           "stories_served_by_practice": served_by,
           "jev_request": request}
    if args.redis_config:
        out["redis"] = json.loads(pathlib.Path(args.redis_config).read_text())
    pathlib.Path(args.out_request).write_text(json.dumps(out, indent=1))
    skill_less = [p["id"] for p in spine_practices(trace) if not p.get("canonical_skill")]
    print(f"assembled {out['pass']} contemplation: "
          f"{len(skill_less)} skill-less practices {skill_less}, "
          f"state fields: {list(request['state'].keys())}")


if __name__ == "__main__":
    main()
