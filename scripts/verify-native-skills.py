#!/usr/bin/env python3
from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "skills/registry/capsules"
SETS = ROOT / "skills/skillsets"
EXPECTED_SKILLS = {
    "skill/aikit/operation",
    "skill/aikit/profile-skillset",
    "skill/aikit/skill-authoring",
    "skill/aikit/knowledge-navigation",
    "skill/aikit/product-understanding",
    "skill/aikit/runtime-operation",
    "skill/aikit/session-space",
    "skill/aikit/provider-authoring",
    "skill/aikit/component-surface-authoring",
    "skill/aikit/verification",
}
EXPECTED_GUIDANCE = {
    "guidance/aikit/living-project-collaboration",
}

seen_skills: set[str] = set()
seen_guidance: set[str] = set()
for manifest in REGISTRY.glob("**/manifest.toml"):
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    capsule_id = data["id"]
    relative = manifest.parent.relative_to(REGISTRY).as_posix()
    if relative != capsule_id:
        raise SystemExit(f"{manifest}: path {relative!r} != id {capsule_id!r}")

    kind = data["kind"]
    if kind == "skill":
        if data["skill"]["root"] != "payload":
            raise SystemExit(f"{manifest}: not an existing-shape Skill capsule")
        body = manifest.parent / "payload/SKILL.md"
        text = body.read_text(encoding="utf-8")
        if not text.startswith("---\n") or "\ndescription:" not in text:
            raise SystemExit(f"{body}: invalid Agent Skill frontmatter")
        seen_skills.add(capsule_id)
    elif kind == "guidance":
        guidance = data.get("guidance", {})
        entry = guidance.get("entry")
        if not entry or "SessionStart" not in guidance.get("inject", []):
            raise SystemExit(f"{manifest}: project guidance must be an explicit SessionStart fragment")
        body = manifest.parent / entry
        if not body.is_file() or not body.read_text(encoding="utf-8").strip():
            raise SystemExit(f"{manifest}: guidance entry is missing or empty")
        seen_guidance.add(capsule_id)
    else:
        raise SystemExit(f"{manifest}: unexpected first-party capsule kind {kind!r}")

if seen_skills != EXPECTED_SKILLS:
    raise SystemExit(f"first-party Skill corpus mismatch: {seen_skills ^ EXPECTED_SKILLS}")
if seen_guidance != EXPECTED_GUIDANCE:
    raise SystemExit(f"first-party guidance corpus mismatch: {seen_guidance ^ EXPECTED_GUIDANCE}")

index = tomllib.loads((SETS / "index.toml").read_text(encoding="utf-8"))
refs = {entry["semantic_ref"]: entry["directory"] for entry in index["skillset"]}
if set(refs) != {"aikit:operator", "aikit:project-author", "aikit:extension-developer"}:
    raise SystemExit("native SkillSet semantic refs mismatch")
for semantic_ref, directory in refs.items():
    members = [line.strip() for line in (SETS / directory / "members").read_text().splitlines() if line.strip()]
    if not members or len(members) != len(set(members)):
        raise SystemExit(f"{semantic_ref}: empty/duplicate member list")
    unknown = set(members) - EXPECTED_SKILLS
    if unknown:
        raise SystemExit(f"{semantic_ref}: unknown members {unknown}")

for semantic_ref in ("aikit:project-author", "aikit:extension-developer"):
    members = {
        line.strip()
        for line in (SETS / refs[semantic_ref] / "members").read_text().splitlines()
        if line.strip()
    }
    if "skill/aikit/product-understanding" not in members:
        raise SystemExit(f"{semantic_ref}: product-understanding Skill missing")

fixture = ROOT / "skills/fixtures/minimal-authored-skill"
fdata = tomllib.loads((fixture / "manifest.toml").read_text(encoding="utf-8"))
if fdata["id"] != "skill/aikit-fixture/inspect-source" or fdata["kind"] != "skill":
    raise SystemExit("authored Skill fixture manifest invalid")
if not (fixture / "payload/SKILL.md").read_text().startswith("---\n"):
    raise SystemExit("authored Skill fixture body invalid")

cases = tomllib.loads((ROOT / "skills/fixtures/product-understanding/cases.toml").read_text(encoding="utf-8"))["case"]
by_id = {case["id"]: case for case in cases}
if set(by_id) != {
    "straight-retrieval-stops",
    "product-design-traverses-provenance",
    "current-capability-descends-to-reality",
    "returned-reality-proposes-pressure",
}:
    raise SystemExit("product-understanding fixture set mismatch")
if "philosophical-corpus" not in by_id["straight-retrieval-stops"]["avoid"]:
    raise SystemExit("retrieval fixture does not prove the shallow stop rule")
design_visits = set(by_id["product-design-traverses-provenance"]["visit"])
if not {
    "authored-human-position",
    "product-constitutional-intent",
    "design-decision",
    "architectural-contract",
    "implementation-fact",
    "current-development-state",
}.issubset(design_visits) or not by_id["product-design-traverses-provenance"]["report_classes"]:
    raise SystemExit("product-design fixture does not traverse/report provenance classes")
capability = by_id["current-capability-descends-to-reality"]
if "implementation-fact" not in capability["visit"] or "product-constitutional-intent" not in capability["forbid_claim_authority"]:
    raise SystemExit("current-capability fixture permits vision to masquerade as implementation")
pressure = by_id["returned-reality-proposes-pressure"]
if not pressure["proposal_only"] or pressure["authored_source_mutated"]:
    raise SystemExit("returned-reality fixture permits silent authored-source mutation")

product_understanding = (REGISTRY / "skill/aikit/product-understanding/payload/SKILL.md").read_text()
for required in (
    "Provenance determines authority for the question being asked",
    "smallest sufficient depth",
    "AUTHORED HUMAN POSITION",
    "IMPLEMENTATION FACT",
    "CURRENT DEVELOPMENT STATE",
    "INFERENCE / INTERPRETATION",
    "Do not silently rewrite Central Control",
):
    if required not in product_understanding:
        raise SystemExit(f"product-understanding Skill missing required distinction: {required}")

lean_guidance = (REGISTRY / "guidance/aikit/living-project-collaboration/payload/guidance.md").read_text()
for required in (
    "You and I communicate directly as collaborators",
    "Current code tells us what is real now; it does not retroactively define why the project exists.",
    "Vision tells us what is meant; it does not prove what currently works.",
    "the smallest sufficient context for straightforward retrieval or coding",
):
    if required not in lean_guidance:
        raise SystemExit(f"lean project guidance missing required voice/orientation: {required}")
if len(lean_guidance.split()) > 330:
    raise SystemExit("lean project guidance has become procedural rather than orienting")

operator = (REGISTRY / "skill/aikit/operation/payload/SKILL.md").read_text()
if "SkillSet selected != Root position" not in (ROOT / "skills/README.md").read_text():
    raise SystemExit("suite authority distinction missing")
if "projected Skill" not in operator:
    raise SystemExit("operator source/projection distinction missing")

print("AIKit first-party native Skills, guidance and SkillSets: OK")
