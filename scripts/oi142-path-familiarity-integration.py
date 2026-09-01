#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text()
    if old not in source:
        raise SystemExit(f"missing integration anchor in {path}: {old[:120]!r}")
    target.write_text(source.replace(old, new, 1))


# A KnowledgeRoute familiarity observation remains exactly a KnowledgeRoute. The
# ResolvePath variant exists beside it; this test must not imply conversion.
patch(
    "crates/aikit-core/src/knowledge.rs",
    '''            crate::FamiliarityUse::ResolvePath { route, steps, .. } => {\n                assert_eq!(route, r("knowledge-route/auth-to-code"));\n                assert_eq!(steps.len(), 2);\n            }\n''',
    '''            crate::FamiliarityUse::ResolvePath { .. } => {\n                panic!("KnowledgeRoute familiarity must remain route evidence")\n            }\n''',
)

# The existing test already carries its attribute; the migration inserts a new
# test immediately before it and must not duplicate that marker.
patch(
    "crates/aikit-core/src/resource/search.rs",
    '''    #[test]\n    #[test]\n    fn learned_resolve_path_breaks_an_otherwise_equal_resolution_tie() {\n''',
    '''    #[test]\n    fn learned_resolve_path_breaks_an_otherwise_equal_resolution_tie() {\n''',
)
