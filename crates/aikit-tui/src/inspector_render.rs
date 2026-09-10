//! The wide-shell Inspector column (`docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md`
//! §2.1): "attached to selected subject/state", always visible in a wide
//! shell rather than reached through the modal `Overlay::Explain`.
//!
//! This module owns no resolver, retrieval or mutation state and performs no
//! backend calls of its own. It formats an already-fetched
//! [`InspectorSnapshot`] (computed once per dispatch by
//! `ApplicationSurfaceController::refresh_inspector`, mirroring how
//! `refresh_relation` already caches `RelationReadModel`) alongside the same
//! `TuiState`/`ProjectWorldReadModel` `project_workspace_render::explain_lines`
//! already reads. Nothing here is reached only through this column: every
//! section is either identical to, or a superset of, what `Overlay::Explain`
//! already renders, so retiring the modal in a wide shell drops no
//! capability (constraint 2). A field the snapshot does not carry renders as
//! an explicit, worded absence — never a guessed value (constraint 6),
//! following this codebase's own established idiom
//! (`project_workspace_render::context_lines`'s `Scopes` row).

use serde_json::Value;

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::{ExplainEvidence, ProjectWorldReadModel};

use crate::application::TuiState;
use crate::layout::Glyphs;
use crate::project_workspace_render::{authority_label, explain_lines};

/// Everything the Inspector column shows about the current selection beyond
/// `TuiState`/`ProjectWorldReadModel` themselves: the two read-only Explain
/// projections `application_service.rs`'s `invoke_action` already computes
/// for the `EXPLAIN_ACTION_REF` / `action/capability/explain` contextual
/// Actions, fetched proactively here so the column never waits on the user
/// pressing `:` first.
///
/// Both fields are independently optional: a lookup failure (the subject is
/// outside the navigation index, or the backend call itself errors) is a
/// legitimate, disclosable outcome, not a bug — `inspector_lines` renders the
/// absence honestly rather than treating an `Err` as an empty-but-successful
/// read.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorSnapshot {
    pub subject: ResourceRef,
    /// `TuiApplicationService::explain` — the same JSON `action/capability/
    /// explain` renders for a Capability, and (for any other resource with a
    /// Knowledge address) a smaller knowledge-only projection. Works for any
    /// resource the navigation index or Knowledge backend recognises, not
    /// only Capabilities: `self.explain`'s own body branches on the
    /// resource, not on a caller-supplied kind.
    pub explain: Option<Value>,
    /// `ExplainHistoryApplicationService::explain_evidence` — the same
    /// evidence `EXPLAIN_ACTION_REF` renders, carrying per-fact authority and
    /// provenance the plain `explain` JSON does not break out individually.
    pub evidence: Option<ExplainEvidence>,
}

/// The Inspector column's content, as plain lines — the same `Vec<String>`
/// convention `project_workspace_render`'s own line-builders use, so the
/// surface controller renders it exactly the way it already renders those
/// (one `Line::raw` per entry).
///
/// `glyphs` is resolved once at `ApplicationSurfaceController` construction
/// (see its own doc comment) and threaded in here rather than read from the
/// environment, so a rendered frame stays a pure function of already-resolved
/// inputs. Every mark this function draws comes from `glyphs`; nothing here
/// hardcodes a Unicode/Nerd Font literal (constraint 4).
pub fn inspector_lines(
    state: &TuiState,
    world: Option<&ProjectWorldReadModel>,
    snapshot: Option<&InspectorSnapshot>,
    glyphs: &Glyphs,
) -> Vec<String> {
    let mut lines = vec!["INSPECTOR".to_string(), String::new()];

    let Some(selected) = state.selected.as_ref() else {
        lines.push("nothing selected".into());
        lines.push("Select a Resource in the list to inspect it here.".into());
        return lines;
    };

    lines.push(format!("subject   {selected}"));
    match resource_item_kind(state, selected) {
        Some((kind, label, summary)) => {
            lines.push(format!("kind      {}", kind.as_str()));
            if !label.is_empty() && label != selected.as_str() {
                lines.push(format!("label     {label}"));
            }
            if !summary.is_empty() {
                lines.push(format!("summary   {summary}"));
            }
        }
        None => lines.push("kind      not supplied by the current read model".into()),
    }

    lines.push(String::new());
    match world {
        Some(world) => lines.extend(explain_lines(state, world, *glyphs)),
        None => lines.push(
            "Effective state   not exposed - no Project world resolved for this session".into(),
        ),
    }

    lines.push(String::new());
    lines.push("Evidence".into());
    match snapshot.and_then(|snapshot| snapshot.evidence.as_ref()) {
        Some(evidence) if !evidence.facts.is_empty() => {
            for fact in &evidence.facts {
                lines.push(format!(
                    "  {} ({}): {}",
                    fact.relation,
                    fact.authority
                        .map(authority_label)
                        .unwrap_or("not supplied"),
                    fact.summary,
                ));
                if !fact.canonical_refs.is_empty() {
                    lines.push(format!(
                        "    refs: {}",
                        fact.canonical_refs
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                for provenance in &fact.provenance {
                    lines.push(format!(
                        "    provider: {}  source: {}  lens: {}  revision: {}",
                        provenance
                            .provider
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "not supplied".into()),
                        provenance
                            .source
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "not supplied".into()),
                        provenance.lens.as_deref().unwrap_or("not supplied"),
                        provenance.revision.as_deref().unwrap_or("not supplied"),
                    ));
                }
            }
        }
        Some(_) => lines.push("  no provider Explain evidence recorded for this selection".into()),
        None => lines.push("  not exposed - this selection is outside the navigation index".into()),
    }

    if let Some(explain) = snapshot.and_then(|snapshot| snapshot.explain.as_ref()) {
        let mut extra_lines = Vec::new();
        if let Some(state_obj) = non_null_object(explain, "packageCapabilityState") {
            extra_lines.push(String::new());
            extra_lines.push("Capability state".into());
            extra_lines.push(format!(
                "  active {} - declared {} - runnable {}",
                json_yes_no(state_obj.get("active")),
                json_yes_no(state_obj.get("declaredEnabled")),
                json_yes_no(state_obj.get("runnable")),
            ));
            if let Some(unavailable) = state_obj.get("unavailable").and_then(Value::as_str) {
                extra_lines.push(format!("  unavailable: {unavailable}"));
            }
            let related = state_obj
                .get("related")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|joined| !joined.is_empty());
            if let Some(related) = related {
                extra_lines.push(format!("  related: {related}"));
            }
        } else if matches!(
            resource_item_kind(state, selected),
            Some((ResourceKind::Capability, _, _))
        ) {
            extra_lines.push(String::new());
            extra_lines.push("Capability state   not resolved for this Capability".into());
        }

        // Fields the two Explain paths surface that the structured sections
        // above do not already carry, in the fixed order below (never a
        // `serde_json::Map`'s own hash order — constraint 5). Printed as
        // compact JSON rather than hand-formatted: this is a completeness
        // backstop so nothing `self.explain` returns can go missing from the
        // column, not the primary reading surface.
        for (label, key) in [
            ("Knowledge address", "knowledgeAddress"),
            ("Learned accessibility", "learnedAccessibility"),
            ("Annotations", "annotations"),
            ("Ranking", "ranking"),
            ("Navigation evidence", "navigationEvidence"),
        ] {
            if let Some(value) = explain.get(key).filter(|value| !is_empty_json(value)) {
                extra_lines.push(String::new());
                extra_lines.push(format!("{label}   {value}"));
            }
        }
        lines.extend(extra_lines);
    }

    lines.push(String::new());
    let action_count = state.contextual_actions.len();
    if action_count == 0 {
        lines.push("Actions   none available for this selection".into());
    } else {
        lines.push(format!(
            "Actions   {action_count} available {} press : to act",
            glyphs.selected(),
        ));
    }

    lines
}

fn resource_item_kind<'a>(
    state: &'a TuiState,
    selected: &ResourceRef,
) -> Option<(ResourceKind, &'a str, &'a str)> {
    state
        .read_model
        .resources
        .iter()
        .find(|item| &item.resource == selected)
        .map(|item| (item.kind, item.label.as_str(), item.summary.as_str()))
}

fn non_null_object<'a>(value: &'a Value, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    value.get(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_object()
        }
    })
}

fn json_yes_no(value: Option<&Value>) -> &'static str {
    match value.and_then(Value::as_bool) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "not supplied",
    }
}

fn is_empty_json(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::String(text) => text.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{ResourceListItem, ResourceListReadModel, TuiState};
    use aikit_core::resource::ResourceKind;

    fn state_with_selection(resource: &str, kind: ResourceKind) -> TuiState {
        let resource = ResourceRef::parse(resource).unwrap();
        let mut state = TuiState {
            selected: Some(resource.clone()),
            ..TuiState::default()
        };
        state.read_model = ResourceListReadModel {
            revision: "r1".into(),
            resources: vec![ResourceListItem {
                resource,
                kind,
                label: "Alpha".into(),
                summary: "a test resource".into(),
            }],
        };
        state
    }

    #[test]
    fn nothing_selected_renders_an_honest_empty_state_not_a_blank_column() {
        let state = TuiState::default();
        let lines = inspector_lines(&state, None, None, &Glyphs::unicode());
        assert!(lines.iter().any(|line| line == "nothing selected"));
        assert!(
            lines.len() > 2,
            "the empty state must say something, not just the heading"
        );
    }

    #[test]
    fn a_selection_outside_every_evidence_source_discloses_absence_not_a_guess() {
        let state = state_with_selection("skill/alpha", ResourceKind::Capability);
        let lines = inspector_lines(&state, None, None, &Glyphs::unicode());
        assert!(lines
            .iter()
            .any(|line| line.contains("subject   skill/alpha")));
        assert!(lines
            .iter()
            .any(|line| line
                .contains("not exposed - this selection is outside the navigation index")));
        assert!(lines
            .iter()
            .any(|line| line.contains("not exposed - no Project world resolved for this session")));
    }

    #[test]
    fn evidence_facts_render_with_authority_and_summary() {
        use aikit_core::{ExplainEvidence, ExplainFact};
        let state = state_with_selection("skill/alpha", ResourceKind::Capability);
        let snapshot = InspectorSnapshot {
            subject: ResourceRef::parse("skill/alpha").unwrap(),
            explain: None,
            evidence: Some(ExplainEvidence {
                schema: "aikit.explain-history/v1".into(),
                subject: ResourceRef::parse("skill/alpha").unwrap(),
                facts: vec![ExplainFact {
                    relation: "owner".into(),
                    authority: Some(aikit_core::SourceAuthority::Authored),
                    summary: "owned by team:ops".into(),
                    canonical_refs: Vec::new(),
                    provenance: Vec::new(),
                }],
            }),
        };
        let lines = inspector_lines(&state, None, Some(&snapshot), &Glyphs::unicode());
        assert!(lines
            .iter()
            .any(|line| line.contains("owner (authored): owned by team:ops")));
    }

    #[test]
    fn capability_state_renders_when_present_and_is_disclosed_absent_when_not() {
        use aikit_core::{ExplainEvidence, SourceAuthority};
        let state = state_with_selection("cap/alpha", ResourceKind::Capability);
        let subject = ResourceRef::parse("cap/alpha").unwrap();

        let present = InspectorSnapshot {
            subject: subject.clone(),
            explain: Some(serde_json::json!({
                "packageCapabilityState": {
                    "active": true,
                    "declaredEnabled": true,
                    "runnable": false,
                    "unavailable": "trust required",
                    "related": ["cap/beta"],
                }
            })),
            evidence: Some(ExplainEvidence {
                schema: "aikit.explain-history/v1".into(),
                subject: subject.clone(),
                facts: Vec::new(),
            }),
        };
        let lines = inspector_lines(&state, None, Some(&present), &Glyphs::unicode());
        assert!(lines
            .iter()
            .any(|line| line.contains("active yes - declared yes - runnable no")));
        assert!(lines
            .iter()
            .any(|line| line.contains("unavailable: trust required")));
        assert!(lines.iter().any(|line| line.contains("related: cap/beta")));

        let absent = InspectorSnapshot {
            subject: subject.clone(),
            explain: Some(serde_json::json!({ "resource": subject.as_str() })),
            evidence: Some(ExplainEvidence {
                schema: "aikit.explain-history/v1".into(),
                subject,
                facts: Vec::new(),
            }),
        };
        let lines = inspector_lines(&state, None, Some(&absent), &Glyphs::unicode());
        assert!(lines
            .iter()
            .any(|line| line.contains("Capability state   not resolved for this Capability")));
        let _ = SourceAuthority::Authored; // keep import used across cfg configurations
    }

    #[test]
    fn nothing_in_an_ascii_rendering_is_non_ascii() {
        use aikit_core::resource::{ActionStageability, ContextualActionDescriptor};
        use aikit_core::{EvidenceProvenance, ExplainEvidence, ExplainFact, SourceAuthority};
        let mut state = state_with_selection("skill/alpha", ResourceKind::Capability);
        let subject = ResourceRef::parse("skill/alpha").unwrap();
        // Exercise the Actions line's glyph marker too, not just the text
        // paths above it.
        state.contextual_actions = vec![ContextualActionDescriptor::new(
            ResourceRef::parse("action/aikit/explain").unwrap(),
            subject.clone(),
            "Explain",
            "explain this resource",
            ActionStageability::NotStageable,
        )];
        let snapshot = InspectorSnapshot {
            subject: subject.clone(),
            explain: Some(serde_json::json!({
                "packageCapabilityState": {
                    "active": true,
                    "declaredEnabled": true,
                    "runnable": true,
                    "unavailable": Value::Null,
                    "related": ["skill/beta"],
                },
                "learnedAccessibility": {"observations": 3},
                "annotations": {"aikit.search-tags": "rust,review"},
            })),
            evidence: Some(ExplainEvidence {
                schema: "aikit.explain-history/v1".into(),
                subject,
                facts: vec![ExplainFact {
                    relation: "source".into(),
                    authority: Some(SourceAuthority::Observed),
                    summary: "source config/skill.toml is Available".into(),
                    canonical_refs: vec![
                        ResourceRef::parse("source/aikit/resolved-catalogue").unwrap()
                    ],
                    provenance: vec![EvidenceProvenance {
                        provider: Some(ResourceRef::parse("provider/aikit/catalog").unwrap()),
                        source: Some(
                            ResourceRef::parse("source/aikit/resolved-catalogue").unwrap(),
                        ),
                        lens: Some("catalog".into()),
                        revision: Some("3".into()),
                        native_id: None,
                    }],
                }],
            }),
        };
        for line in inspector_lines(&state, None, Some(&snapshot), &Glyphs::ascii()) {
            assert!(line.is_ascii(), "`{line}` is not ASCII");
        }
    }
}
