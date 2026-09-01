#!/usr/bin/env python3
from pathlib import Path

# acceptance trigger: exact compiler-discovered Method consumers, 2026-09-01


def patch(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text()
    if old not in source:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    target.write_text(source.replace(old, new, 1))


# Flow has an authored Method composition, but no source-authored operative path
# yet. Preserve that distinction rather than manufacturing a typed expectation.
patch(
    "crates/aikit-core/src/flow.rs",
    '''        verification: vec![],\n        expected_return_forms: vec![\n''',
    '''        verification: vec![],\n        expected_resolve: None,\n        expected_return_forms: vec![\n''',
)

# The Method source supports more than one Explain fact; keep its provenance
# reusable rather than moving the Option into the first fact.
patch(
    "crates/aikit-core/src/praxis.rs",
    '''                        source: source_ref,\n                        revision: resolution.revision.as_ref().map(ToString::to_string),\n''',
    '''                        source: source_ref.clone(),\n                        revision: resolution.revision.as_ref().map(ToString::to_string),\n''',
)
