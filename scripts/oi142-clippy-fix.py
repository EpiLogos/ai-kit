#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/aikit-core/src/familiarity.rs")
source = path.read_text()
old = '''    ResolvePath {\n        /// Present only when an actual canonical KnowledgeRoute participated in\n        /// the traversal. A general ResolvePath never manufactures route identity.\n        #[serde(default, skip_serializing_if = "Option::is_none")]\n        knowledge_route: Option<ResourceRef>,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n    },\n'''
new = '''    ResolvePath {\n        /// Present only when an actual canonical KnowledgeRoute participated in\n        /// the traversal. A general ResolvePath never manufactures route identity.\n        #[serde(default, skip_serializing_if = "Option::is_none")]\n        knowledge_route: Option<ResourceRef>,\n        steps: Vec<RouteStepEvidence>,\n        operative: Box<OperativePathEvidence>,\n    },\n'''
if old not in source:
    raise SystemExit("ResolvePath enum anchor changed; re-inspect")
source = source.replace(old, new, 1)
old = '''            use_kind: FamiliarityUse::ResolvePath {\n                knowledge_route,\n                steps,\n                operative,\n            },\n'''
new = '''            use_kind: FamiliarityUse::ResolvePath {\n                knowledge_route,\n                steps,\n                operative: Box::new(operative),\n            },\n'''
if old not in source:
    raise SystemExit("ResolvePath constructor anchor changed; re-inspect")
path.write_text(source.replace(old, new, 1))
