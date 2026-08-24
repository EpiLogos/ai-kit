#!/usr/bin/env python3
from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "skills/registry/capsules/skill/aikit/skill-authoring/payload/SKILL.md"
MANIFEST = ROOT / "skills/registry/capsules/skill/aikit/skill-authoring/manifest.toml"
REFERENCE = ROOT / "skills/registry/capsules/skill/aikit/skill-authoring/payload/references/instruction-architecture-review.md"
FIXTURE = ROOT / "skills/fixtures/instruction-craft/cases.toml"

skill = SKILL.read_text(encoding="utf-8")
manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
reference = REFERENCE.read_text(encoding="utf-8")
cases = tomllib.loads(FIXTURE.read_text(encoding="utf-8"))["case"]
by_id = {case["id"]: case for case in cases}

required_cases = {
    "trigger-positive-negative",
    "process-heavy-governance-extracts",
    "no-op-deletion-test",
    "contrast-targeted-no-leak",
    "progressive-disclosure-common-path",
    "phase-separation-hides-future",
    "planning-retrieves-available-fact",
    "authorial-fork-reuses-human-authority",
    "history-audit-proposes-governance",
    "vertical-slice-has-evidence-reason",
    "disclosure-tradeoff-explicit",
}
if set(by_id) != required_cases:
    raise SystemExit(f"instruction-craft fixture set mismatch: {set(by_id) ^ required_cases}")

if not by_id["trigger-positive-negative"]["positive_select"] or by_id["trigger-positive-negative"]["negative_select"]:
    raise SystemExit("Skill trigger fixture does not discriminate authoring from ordinary use")
if by_id["process-heavy-governance-extracts"]["automatic_source_mutation"]:
    raise SystemExit("instruction craft silently mutates human governance")
if by_id["no-op-deletion-test"]["no_op_conformance_changed"]:
    raise SystemExit("sediment deletion incorrectly changes conformance")
if not by_id["no-op-deletion-test"]["load_bearing_conformance_changed"]:
    raise SystemExit("load-bearing deletion is not distinguished from sediment")
if by_id["contrast-targeted-no-leak"]["unrelated_output_changes"]:
    raise SystemExit("contrast specimen leaks into unrelated output")
if by_id["progressive-disclosure-common-path"]["rare_reference_loaded"]:
    raise SystemExit("rare reference loads on the common path")
if by_id["phase-separation-hides-future"]["later_artifact_visible"]:
    raise SystemExit("future phase artifact is exposed before closure")
if by_id["planning-retrieves-available-fact"]["human_request"]:
    raise SystemExit("available fact became a human question")
if by_id["authorial-fork-reuses-human-authority"]["question_level"] != "experienced-product-consequence":
    raise SystemExit("authorial fork is not escalated at product/experience altitude")
if by_id["history-audit-proposes-governance"]["automatic_governance_mutation"]:
    raise SystemExit("historical behaviour self-promotes into governance")
if by_id["vertical-slice-has-evidence-reason"]["phrase_is_mandatory_dogma"]:
    raise SystemExit("vertical-slice language became mandatory dogma")
if by_id["disclosure-tradeoff-explicit"]["universal_threshold_claimed"]:
    raise SystemExit("instruction craft invents a universal disclosure threshold")

for fragment in ("creating, reviewing or simplifying", "trigger", "disclosure"):
    if fragment not in manifest["description"]:
        raise SystemExit(f"skill-authoring manifest lacks routing signal: {fragment}")

for required in (
    "## Instruction architecture",
    "Source ownership first",
    "routing affordance",
    "model/context load",
    "human invocation load",
    "progressive disclosure",
    "positive operational specification",
    "Human authority is inherited, not reimplemented",
    "Phase separation",
    "Vertical slices and tracer bullets",
    "Regression before sediment",
    "Historical behaviour is evidence, not authorship",
    "Communication is part of capability",
    "references/instruction-architecture-review.md",
    "central.agent-governance-relations/v1",
    "phenomenal consciousness",
):
    if required not in skill:
        raise SystemExit(f"skill-authoring missing current instruction-craft relation: {required}")

for required in (
    "Trigger audit",
    "Guidance versus procedure audit",
    "Invocation/disclosure trade-off",
    "Vocabulary audit",
    "Positive specification audit",
    "Progressive disclosure audit",
    "Phase-separation audit",
    "Vertical-slice audit",
    "Historical evidence audit",
    "No-op / deletion test",
):
    if required not in reference:
        raise SystemExit(f"deep review reference missing section: {required}")

print("AIKit instruction architecture craft: OK")
