#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
KNOWLEDGE = ROOT / "skills/registry/capsules/skill/aikit/knowledge-navigation/payload/SKILL.md"
VERIFY = ROOT / "skills/registry/capsules/skill/aikit/verification/payload/SKILL.md"

knowledge = KNOWLEDGE.read_text(encoding="utf-8")
verification = VERIFY.read_text(encoding="utf-8")

for required in (
    "Whole-relative adequacy: proof over plausibility",
    "Plausibility does not manufacture epistemic standing.",
    "whole-relative disclosure ledger",
    "aggregate-under-structure",
    "anchor, twofold, threefold, fourfold, `4+1`, sixfold",
    "Current temporal and Run-cognitive aggregates",
    "Central `NOW` / `DAY`, Factory `RunThoughtField`, Flow",
    "Contemplate may generate a better Claim, candidate interpretation or integrative reading; it does not thereby verify the whole.",
    "plausible determination != verified whole",
):
    if required not in knowledge:
        raise SystemExit(f"knowledge-navigation missing whole-relative proof law: {required}")

for required in (
    "Whole-relative verification law",
    "Verification begins from the **Claim and the operative Whole it purports to determine**",
    "A green check proves the condition it actually checks.",
    "Do not silently shrink the operative Whole",
    "`n-1` satisfied obligations are not full Closure",
    "Reconcile the evidence against the original Whole and Claim.",
):
    if required not in verification:
        raise SystemExit(f"verification Skill missing whole-relative proof law: {required}")

print("AIKit whole-relative proof / plausibility barrier: OK")
