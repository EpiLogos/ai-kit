#!/usr/bin/env python3
from pathlib import Path

# verification trigger: KnowledgeRoute + TUI parity


def patch(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text()
    if old not in source:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    target.write_text(source.replace(old, new, 1))


patch(
    "crates/aikit-tui/src/application_service.rs",
    '''                            FamiliarityUse::Route { route, steps } => {\n                                format!(\n                                    "route {route} · {} step{}",\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n''',
    '''                            FamiliarityUse::Route { route, steps } => {\n                                format!(\n                                    "route {route} · {} step{}",\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n                            FamiliarityUse::ResolvePath {\n                                route,\n                                steps,\n                                operative,\n                            } => {\n                                format!(\n                                    "resolve {} · route {route} · {} step{}",\n                                    operative.path_identity,\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n''',
)

patch(
    "crates/aikit-core/src/knowledge.rs",
    '''            crate::FamiliarityUse::Destination => panic!("route evidence must remain a route"),\n''',
    '''            crate::FamiliarityUse::ResolvePath { route, steps, .. } => {\n                assert_eq!(route, r("knowledge-route/auth-to-code"));\n                assert_eq!(steps.len(), 2);\n            }\n            crate::FamiliarityUse::Destination => panic!("route evidence must remain a route"),\n''',
)
