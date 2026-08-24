#!/usr/bin/env python3
from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "skills/fixtures/human-authority/cases.toml"
PRODUCT_SKILL = ROOT / "skills/registry/capsules/skill/aikit/product-understanding/payload/SKILL.md"
GUIDANCE = ROOT / "skills/registry/capsules/guidance/aikit/living-project-collaboration/payload/guidance.md"

cases = tomllib.loads(FIXTURE.read_text(encoding="utf-8"))["case"]
by_id = {case["id"]: case for case in cases}
expected = {
    "existing-intent-resolves",
    "reversible-engineering-proceeds",
    "prototype-before-abstract-question",
    "genuine-authorial-fork-escalates",
    "right-altitude-question",
    "returned-discovery-proposes",
    "small-task-keeps-stop-rule",
}
if set(by_id) != expected:
    raise SystemExit(f"human-authority fixture set mismatch: {set(by_id) ^ expected}")

if by_id["existing-intent-resolves"]["human_request"]:
    raise SystemExit("existing authored intent incorrectly requires a HumanRequest")
if not by_id["reversible-engineering-proceeds"]["return_evidence"]:
    raise SystemExit("reversible engineering does not return evidence")
if not by_id["prototype-before-abstract-question"]["prototype_before_human_request"]:
    raise SystemExit("bounded evidence is not preferred before an abstract human question")
if not by_id["genuine-authorial-fork-escalates"]["human_request"]:
    raise SystemExit("genuine authorial fork does not escalate")
if by_id["right-altitude-question"]["question_level"] != "experienced-product-consequence":
    raise SystemExit("human question is not asked at consequential product altitude")
if by_id["returned-discovery-proposes"]["human_source_mutated"]:
    raise SystemExit("returned discovery silently mutates human-authored source")
if by_id["small-task-keeps-stop-rule"]["loads_product_corpus"]:
    raise SystemExit("human-authority guidance defeats the smallest-sufficient-context stop rule")

skill = PRODUCT_SKILL.read_text(encoding="utf-8")
guidance = GUIDANCE.read_text(encoding="utf-8")
for required in (
    "Human authority — resolve before escalating",
    "What determination does only the human need to make here?",
    "COMMISSION",
    "RETURNED REALITY",
    "RECOGNITION",
    "Prototype before abstract escalation",
):
    if required not in skill:
        raise SystemExit(f"product-understanding missing human-authority relation: {required}")

for required in (
    "preserve human attention",
    "redundant confirmation",
    "reversible",
    "authorship",
    "Recognition",
):
    if required not in guidance:
        raise SystemExit(f"living-project-collaboration missing human-authority relation: {required}")

print("AIKit human-authority collaboration: OK")
