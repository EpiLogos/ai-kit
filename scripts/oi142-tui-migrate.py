#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/aikit-tui/src/application_service.rs")
source = path.read_text()
old = '''                            FamiliarityUse::Route { route, steps } => {\n                                format!(\n                                    "route {route} · {} step{}",\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n'''
new = '''                            FamiliarityUse::Route { route, steps } => {\n                                format!(\n                                    "route {route} · {} step{}",\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n                            FamiliarityUse::ResolvePath {\n                                route,\n                                steps,\n                                operative,\n                            } => {\n                                format!(\n                                    "resolve {} · route {route} · {} step{}",\n                                    operative.path_identity,\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n'''
if old not in source:
    raise SystemExit("missing TUI familiarity route projection anchor")
path.write_text(source.replace(old, new, 1))
