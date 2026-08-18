#!/usr/bin/env bash
set -euo pipefail

# This inert block exists only because the original failed workflow has one
# compatibility step that rewrites this import spelling before invoking us.
: <<'IMPORT_FIX_SENTINEL'
use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{
    HistoryKind, SourceAuthority, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};
IMPORT_FIX_SENTINEL

python3 - <<'PY'
from pathlib import Path

path = Path('scripts/tmp_finish_explain_history_body.sh')
text = path.read_text()

old = '''use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{
    HistoryKind, SourceAuthority, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};'''
new = '''use aikit_cli::app::Service;
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{HistoryKind, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF};'''
if old not in text:
    raise SystemExit('generated acceptance import block not found in patch body')
text = text.replace(old, new, 1)

old = '''                });
            }
        }

        if let Some(store) = backend.familiarity()? {
'''
new = '''                });
            }

            if let Some(search) = backend.knowledge_search(resource.as_str(), 256)? {
                if let Some(ranking) = search
                    .hits
                    .into_iter()
                    .find(|hit| hit.resource == *resource)
                    .and_then(|hit| hit.ranking)
                {
                    if let Some(route) = ranking.route.filter(|assessment| !assessment.is_empty()) {
                        let mut canonical_refs = vec![resource.clone()];
                        if let Some(route_ref) = route.route.clone() {
                            canonical_refs.push(route_ref);
                        }
                        evidence.push(ExplainFact {
                            relation: "learned-route-accessibility".into(),
                            authority: Some(SourceAuthority::Learned),
                            summary: format!(
                                "{} observed route use{}; contextual frecency {:.4}",
                                route.observations,
                                if route.observations == 1 { "" } else { "s" },
                                route.contextual_frecency
                            ),
                            canonical_refs,
                            provenance: route
                                .evidence_ids
                                .iter()
                                .map(|id| EvidenceProvenance {
                                    source: ResourceRef::parse(&format!(
                                        "familiarity-observation/{id}"
                                    ))
                                    .ok(),
                                    ..EvidenceProvenance::default()
                                })
                                .collect(),
                        });
                    }
                }
            }
        }

        if let Some(store) = backend.familiarity()? {
'''
if old not in text:
    raise SystemExit('Knowledge Explain insertion point not found in patch body')
text = text.replace(old, new, 1)

old = '''    assert!(explain
        .facts
        .iter()
        .any(|fact| fact.relation == "learned-accessibility" && fact.authority == Some(SourceAuthority::Learned)));
'''
new = '''    let learned_route = explain
        .facts
        .iter()
        .find(|fact| fact.relation == "learned-route-accessibility")
        .expect("route use remains distinct learned accessibility evidence");
    assert_eq!(learned_route.authority, Some(SourceAuthority::Learned));
    assert!(learned_route
        .canonical_refs
        .iter()
        .any(|resource| resource.as_str().starts_with("knowledge-route/")));
    assert!(!explain
        .facts
        .iter()
        .any(|fact| fact.relation == "learned-accessibility"));
'''
if old not in text:
    raise SystemExit('old learned accessibility assertion not found in patch body')
text = text.replace(old, new, 1)

old = 'rm -f .github/workflows/tmp-finish-explain-history.yml scripts/tmp_finish_explain_history.sh'
new = 'rm -f .github/workflows/tmp-finish-explain-history.yml scripts/tmp_finish_explain_history.sh scripts/tmp_finish_explain_history_body.sh'
if old not in text:
    raise SystemExit('helper cleanup line not found in patch body')
text = text.replace(old, new, 1)

path.write_text(text)
PY

bash scripts/tmp_finish_explain_history_body.sh
