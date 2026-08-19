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

trigger = by_id["trigger-positive-negative"]
if not trigger["positive_select"] or trigger["negative_select"]:
    raise SystemExit("Skill trigger fixture does not discriminate authoring from ordinary Skill use")

extraction = by_id["process-heavy-governance-extracts"]
if not extraction["contains_long_reusable_procedure"]:
    raise SystemExit("governance extraction fixture lacks process-heavy source")
if extraction["proposed_destination"] != "skill-or-context-pointer" or extraction["automatic_source_mutation"]:
    raise SystemExit("process-heavy governance is not proposed behind a Skill/reference with human source preserved")

deletion = by_id["no-op-deletion-test"]
if not deletion["no_op_instruction_removed"] or deletion["no_op_conformance_changed"]:
    raise SystemExit("no-op deletion fixture does not prove sediment removal")
if not deletion["load_bearing_instruction_removed"] or not deletion["load_bearing_conformance_changed"]:
    raise SystemExit("deletion fixture fails to distinguish load-bearing instruction")

contrast = by_id["contrast-targeted-no-leak"]
if not contrast["contrast_example_present"] or not contrast["target_behaviour_improves"] or contrast["unrelated_output_changes"]:
    raise SystemExit("contrast example is absent, ineffective or leaks into unrelated output")

disclosure = by_id["progressive-disclosure-common-path"]
if disclosure["condition_present"] or disclosure["rare_reference_loaded"]:
    raise SystemExit("progressive disclosure loads a rare reference on the common path")

phase = by_id["phase-separation-hides-future"]
if phase["closure_condition_met"] or phase["later_artifact_visible"]:
    raise SystemExit("phase-separated investigation exposes the later artifact before closure")

planning = by_id["planning-retrieves-available-fact"]
if not planning["available_fact"] or not planning["fact_retrieved"] or planning["human_request"]:
    raise SystemExit("planning fixture turns an available fact into a human request")

authorial = by_id["authorial-fork-reuses-human-authority"]
if authorial["source_or_evidence_can_resolve"] or not authorial["materially_different_product_futures"]:
    raise SystemExit("authorial fork fixture is not genuinely unresolved")
if not authorial["human_request"] or authorial["question_level"] != "experienced-product-consequence":
    raise SystemExit("genuine authorial fork is not escalated at the right altitude")

history = by_id["history-audit-proposes-governance"]
if not history["repeated_failure_evidence"] or not history["cause_classified"]:
    raise SystemExit("history audit does not classify evidence before instruction change")
if not history["governance_change_proposed"] or not history["human_review_required"] or history["automatic_governance_mutation"]:
    raise SystemExit("history audit self-modifies governance or omits human adoption")

vertical = by_id["vertical-slice-has-evidence-reason"]
if not vertical["vertical_slice_recommended"] or not vertical["risk_named"] or not vertical["layers_crossed"] or not vertical["earlier_evidence_named"]:
    raise SystemExit("vertical-slice guidance lacks a concrete evidence reason")
if vertical["phrase_is_mandatory_dogma"]:
    raise SystemExit("vertical-slice phrase has become mandatory engineering dogma")

tradeoff = by_id["disclosure-tradeoff-explicit"]
if not tradeoff["context_load_considered"] or not tradeoff["human_invocation_load_considered"]:
    raise SystemExit("disclosure-mode choice omits one of the two load dimensions")
if tradeoff["universal_threshold_claimed"]:
    raise SystemExit("instruction craft invents a universal disclosure threshold")

manifest_description = manifest["description"]
for fragment in ("creating, reviewing or simplifying", "trigger", "disclosure"):
    if fragment not in manifest_description:
        raise SystemExit(f"skill-authoring manifest description lacks routing signal: {fragment}")

for required in (
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
    "instruction-precedence algorithm",
):
    if required not in skill:
        raise SystemExit(f"skill-authoring missing instruction architecture relation: {required}")

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

if "phenomenal consciousness" not in skill:
    raise SystemExit("instruction craft does not bound the language-conditioned behaviour claim")

print("AIKit instruction architecture craft: OK")
