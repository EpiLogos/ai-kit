#!/usr/bin/env python3
from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "skills/registry/capsules"
SETS = ROOT / "skills/skillsets"
EXPECTED = {
    "skill/aikit/operation",
    "skill/aikit/profile-skillset",
    "skill/aikit/skill-authoring",
    "skill/aikit/knowledge-navigation",
    "skill/aikit/runtime-operation",
    "skill/aikit/provider-authoring",
    "skill/aikit/component-surface-authoring",
    "skill/aikit/verification",
}

seen: set[str] = set()
for manifest in REGISTRY.glob("**/manifest.toml"):
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    capsule_id = data["id"]
    relative = manifest.parent.relative_to(REGISTRY).as_posix()
    if relative != capsule_id:
        raise SystemExit(f"{manifest}: path {relative!r} != id {capsule_id!r}")
    if data["kind"] != "skill" or data["skill"]["root"] != "payload":
        raise SystemExit(f"{manifest}: not an existing-shape Skill capsule")
    body = manifest.parent / "payload/SKILL.md"
    text = body.read_text(encoding="utf-8")
    if not text.startswith("---\n") or "\ndescription:" not in text:
        raise SystemExit(f"{body}: invalid Agent Skill frontmatter")
    seen.add(capsule_id)

if seen != EXPECTED:
    raise SystemExit(f"first-party Skill corpus mismatch: {seen ^ EXPECTED}")

index = tomllib.loads((SETS / "index.toml").read_text(encoding="utf-8"))
refs = {entry["semantic_ref"]: entry["directory"] for entry in index["skillset"]}
if set(refs) != {"aikit:operator", "aikit:project-author", "aikit:extension-developer"}:
    raise SystemExit("native SkillSet semantic refs mismatch")
for semantic_ref, directory in refs.items():
    members = [line.strip() for line in (SETS / directory / "members").read_text().splitlines() if line.strip()]
    if not members or len(members) != len(set(members)):
        raise SystemExit(f"{semantic_ref}: empty/duplicate member list")
    unknown = set(members) - EXPECTED
    if unknown:
        raise SystemExit(f"{semantic_ref}: unknown members {unknown}")

fixture = ROOT / "skills/fixtures/minimal-authored-skill"
fdata = tomllib.loads((fixture / "manifest.toml").read_text(encoding="utf-8"))
if fdata["id"] != "skill/aikit-fixture/inspect-source" or fdata["kind"] != "skill":
    raise SystemExit("authored Skill fixture manifest invalid")
if not (fixture / "payload/SKILL.md").read_text().startswith("---\n"):
    raise SystemExit("authored Skill fixture body invalid")

operator = (REGISTRY / "skill/aikit/operation/payload/SKILL.md").read_text()
if "SkillSet selected != Root position" not in (ROOT / "skills/README.md").read_text():
    raise SystemExit("suite authority distinction missing")
if "projected Skill" not in operator:
    raise SystemExit("operator source/projection distinction missing")

print("AIKit first-party native Skills and SkillSets: OK")
