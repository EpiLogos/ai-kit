#!/usr/bin/env python3
from pathlib import Path

# acceptance trigger: preserve zero-query navigation while explicit @ remains open, 2026-09-01

path = Path("crates/aikit-tui/src/application_service.rs")
source = path.read_text()

old = '''        let path = resolve_expression(&expression, &index, 256);\n        let resources = path\n            .candidates\n            .iter()\n            .filter_map(|candidate| {\n'''
new = '''        let path = resolve_expression(&expression, &index, 256);\n        // Empty human Search is the existing zero-query navigation state, not an\n        // explicit `@` aperture. Preserve its evidence-only presentation while\n        // the typed expression/path remains inspectable. Explicit `@` is non-empty\n        // input and therefore discloses the full addressable field.\n        let zero_query_hits = query\n            .trim()\n            .is_empty()\n            .then(|| index.search("", 256));\n        let resources = path\n            .candidates\n            .iter()\n            .filter(|candidate| {\n                zero_query_hits.as_ref().is_none_or(|hits| {\n                    hits.iter().any(|hit| hit.resource == candidate.resource)\n                })\n            })\n            .filter_map(|candidate| {\n'''
if old not in source:
    raise SystemExit("Resolve Search candidate projection anchor changed; re-inspect")
source = source.replace(old, new, 1)

old = '''        let revision = format!(\n            "aikit.resolve-search/v1:{}:{}:{}",\n            self.backend.view().catalog_revision,\n            self.backend.view().hash,\n            path.identity\n        );\n'''
new = '''        let revision = format!(\n            "aikit.resolve-search/v1:{}:{}:{}:{}",\n            self.backend.view().catalog_revision,\n            self.backend.view().hash,\n            query,\n            path.identity\n        );\n'''
if old not in source:
    raise SystemExit("Resolve Search revision anchor changed; re-inspect")
source = source.replace(old, new, 1)

path.write_text(source)
