#!/usr/bin/env python3
"""Assemble a cross-document contemplation state from registered sources.

This is the designed assembly for the Jev/Redis NOW loop: the contemplation
state is built automatically from the documents the world already keeps —
the capability matrix (manifest + CSV), the UX spine trace (stories,
practices, capability-coverage cells) and the NOW reference — never
hand-authored per question.

Forward pass  (prospective): planning/development — what binds, what next.
Returning pass (retrospective): review/analysis — what changed, what returns.

THIN CALLER, not a second arithmetic path: the matrix/spine/practice-binding
numbers below are read straight out of the native `aikit now-context field`
assembly (`aikit.contemplation-field/v1`), never recomputed here. This
script's own job is unchanged — build the same `jev_request` shape this
loop has always emitted, from source documents that are registered once,
native and revision-bound — but the counting now lives in one place: AIKit's
`contemplation_field.rs`, which also understands `native_skill_ref`
(`ai-kit:...`) and carries `classification`, which this script's own CSV/JSON
parsing never did.

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
import json
import pathlib
import subprocess
from collections import Counter


def run_native_field(args) -> dict:
    """Delegate the whole document read + arithmetic to `aikit now-context
    field`. Returns the `aikit.contemplation-field/v1` payload (the `data`
    member of the CLI's JSON envelope)."""
    argv = [args.aikit_bin, "--json", "now-context", "field",
            "--spine-trace", args.spine_trace]
    if args.projectcentral:
        argv += ["--projectcentral", args.projectcentral]
    else:
        argv += ["--matrix-manifest", args.matrix_manifest, "--matrix-csv", args.matrix_csv]
    if args.spine_repo_root:
        argv += ["--spine-repo-root", args.spine_repo_root]
    if args.ai_kit_repo_root:
        argv += ["--ai-kit-repo-root", args.ai_kit_repo_root]
    if args.telos_goal_dir:
        argv += ["--telos-goal-dir", args.telos_goal_dir]
    if args.serving_track:
        argv += ["--serving-track", args.serving_track]
    argv += ["--pass", "retrospective" if args.retrospective else "prospective"]

    completed = subprocess.run(argv, capture_output=True, text=True)
    if completed.returncode != 0:
        raise SystemExit(
            f"`{' '.join(argv)}` failed (exit {completed.returncode}):\n"
            f"{completed.stderr.strip() or completed.stdout.strip()}"
        )
    try:
        envelope = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"`{' '.join(argv)}` did not return JSON: {error}\n{completed.stdout[:2000]}")
    if not envelope.get("ok", True) and "data" not in envelope:
        raise SystemExit(f"`{' '.join(argv)}` refused: {envelope}")
    return envelope.get("data", envelope)


def implemented_field_from_capabilities(capabilities: list[dict]) -> dict:
    implemented = [c for c in capabilities if (c.get("implementation_status") or "").startswith("Implemented")]
    open_rows = [c for c in capabilities if c not in implemented]
    return {
        "capability_rows": len(capabilities),
        "implemented": len(implemented),
        "open_rows": [
            {"id": c["id"], "status": c.get("implementation_status")} for c in open_rows
        ],
    }


def grid_relations_summary(grid_relations: list[dict]) -> dict:
    relations = Counter(r.get("relation") for r in grid_relations if r.get("relation"))
    return {"relation_records": sum(relations.values()), "types": dict(relations)}


def spine_practices_from_field(practices: list[dict]) -> list[dict]:
    """Reshape the native field's practice entries into this script's
    long-standing per-practice record. `canonical_skill` here means "the
    resolved skill path if this practice is actually bound" — bound now
    covers both `canonical_skill` and `native_skill_ref` (the native field
    resolves both); `binding_status`/`classification` are new, additive
    fields the native field carries that this script's old CSV/JSON reads
    never could."""
    out = []
    for practice in practices:
        binding = practice.get("binding") or {}
        out.append({
            "id": practice["id"],
            "purpose": practice.get("purpose"),
            "stories_served": practice.get("stories_served") or [],
            "implementation_owner": practice.get("implementation_owner"),
            "canonical_skill": binding.get("skill_ref") if binding.get("status") == "bound" else None,
            "capability_coverage_cells": practice.get("capability_coverage_cells") or [],
            "binding_status": binding.get("status"),
            "binding_kind": binding.get("kind"),
            "classification": binding.get("classification"),
        })
    return out


def contemplation_request(args, field: dict) -> dict:
    matrix = field.get("matrix") or {}
    capabilities = matrix.get("capabilities") or []
    matrix_field = implemented_field_from_capabilities(capabilities)
    relations = grid_relations_summary(matrix.get("grid_relations") or [])
    practices = spine_practices_from_field(field.get("spine", {}).get("practices") or [])
    # "Skill-less" now means genuinely unresolved — neither a canonical_skill
    # nor a native_skill_ref actually bound to a real file — not merely
    # "canonical_skill is null", since a practice can be bound entirely
    # through native_skill_ref (e.g. an AIKit-owned capsule).
    skill_less = [p for p in practices if p.get("binding_status") != "bound"]
    served_by = {p["id"]: p["stories_served"] for p in practices}

    def criterion(practice: dict) -> dict:
        return {
            "purpose": practice["purpose"],
            "stories_served": practice["stories_served"],
            "implementation_owner": practice["implementation_owner"],
            "canonical_skill": practice["canonical_skill"],
            "capability_coverage_cells": practice["capability_coverage_cells"],
            "binding_status": practice["binding_status"],
            "classification": practice["classification"],
        }

    spine = field.get("spine") or {}
    state = {
        "matrix_field": (
            f"ql-capability-matrix/1: {matrix_field['capability_rows']} capability rows, "
            f"{matrix_field['implemented']} implemented with linked evidence; open rows: "
            f"{json.dumps(matrix_field['open_rows'])}"
        ),
        "grid_relations": (
            f"{relations['relation_records']} relation records across "
            f"{json.dumps(relations['types'])}"
        ),
        "ux_spine": (
            f"ql.ux-spine-trace/1: {len(spine.get('stories') or [])} stories, "
            f"{len(practices)} practices, {len(spine.get('coverage_cells') or [])} "
            "capability-coverage cells. Skill-less practices are the open frontier; "
            "AIKit owns discovery/projection on the practices with a bound Skill."
        ),
        "coverage_note": (
            "Each choice criterion carries the capability-coverage cells touching its "
            "stories, so the contemplation traverses practice -> stories -> capabilities "
            "across both documents. Assembled by the native `aikit now-context field` "
            "(scripts/jev-redis/assemble_contemplation.py is now a thin caller)."
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
    return request, practices


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--matrix-manifest", required=False)
    ap.add_argument("--matrix-csv", required=False)
    ap.add_argument("--projectcentral", required=False,
                    help="ProjectCentral dir; discovers capability-matrix carriers "
                         "(telos folder first, then user/) — same discovery the native "
                         "field verb performs")
    ap.add_argument("--spine-trace", required=True)
    ap.add_argument("--spine-repo-root", default=None,
                    help="Git repo root canonical_skill resolves against; defaults to the "
                         "spine trace's own containing Git repository")
    ap.add_argument("--ai-kit-repo-root", default=None,
                    help="Git repo root native_skill_ref `ai-kit:` refs resolve against; "
                         "defaults to this invocation's own repo root")
    ap.add_argument("--telos-goal-dir", required=False,
                    help="telos goal folder (goal.md + tracks/); anchors the state "
                         "in the long horizon per the integrated-field chain of custody")
    ap.add_argument("--serving-track", default=None)
    ap.add_argument("--now-ref", required=True)
    ap.add_argument("--participant-ref", default="participant/contemplation/forward")
    ap.add_argument("--agent-session", default=None)
    ap.add_argument("--redis-config", default=None, help="aikit.redis-now-config/v1 JSON file")
    ap.add_argument("--jev-model", default="jev-latest")
    ap.add_argument("--retrospective", action="store_true",
                    help="Assemble the returning (retrospective) pass instead of the forward pass")
    ap.add_argument("--aikit-bin", default="aikit",
                    help="aikit executable that answers `now-context field` "
                         "(default: aikit on PATH)")
    ap.add_argument("--out-request", required=True)
    args = ap.parse_args()

    if not args.projectcentral and not (args.matrix_manifest and args.matrix_csv):
        raise SystemExit("provide --matrix-manifest/--matrix-csv or --projectcentral")

    field = run_native_field(args)
    request, practices = contemplation_request(args, field)

    served_by = request["questions"].pop("_served_by")
    matrix = field.get("matrix") or {}
    out = {"schema": "aikit.contemplation-assembly/v1", "now_ref": args.now_ref,
           "pass": "retrospective" if args.retrospective else "prospective",
           "participant_ref": args.participant_ref,
           "stories_served_by_practice": served_by,
           "matrix_carriers": {
               "manifest": matrix.get("source_manifest"),
               "csv": matrix.get("source_csv"),
           },
           "jev_request": request}
    if field.get("telos"):
        out["telos"] = field["telos"]
        request["state"]["telos_anchor"] = (
            f"Long horizon: goal '{out['telos'].get('goal')}' with tracks "
            f"{out['telos'].get('tracks')}"
            + (f"; this contemplation serves the '{args.serving_track}' track"
               if args.serving_track else "")
            + ". Recognitions returned from this contemplation must name the "
              "goal/track they serve (integrated-field chain of custody: "
              "telos -> task -> now -> sessions)."
        )
    if args.redis_config:
        # This script's own carried config, not the native field's live
        # (optional, participant-scoped) Redis read: preserves the existing
        # flag's behaviour exactly rather than silently changing it.
        out["redis"] = json.loads(pathlib.Path(args.redis_config).read_text())
    pathlib.Path(args.out_request).write_text(json.dumps(out, indent=1))
    skill_less = [p["id"] for p in practices if p.get("binding_status") != "bound"]
    print(f"assembled {out['pass']} contemplation: "
          f"{len(skill_less)} skill-less practices {skill_less}, "
          f"matrix from {out['matrix_carriers']['csv']}"
          + (f", telos anchor: {out['telos'].get('goal')}" if field.get("telos") else ""))


if __name__ == "__main__":
    main()
