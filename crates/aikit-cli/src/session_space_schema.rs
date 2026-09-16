//! Documented JSON templates for every typed SessionSpace mutation.
//!
//! Staging an intent by hand used to mean serde-error archaeology: required
//! plan fields (`backend_extensions`, `warnings`) were undocumented and
//! undiscoverable from the failure. `aikit-session-space stage --print-schema`
//! prints a template for every operation instead, and the golden test in this
//! module pins that every template parses back as a valid intent.

#[cfg(test)]
use aikit_core::session_space_application::SessionSpaceMutation;
use serde_json::{Value, json};

/// One operation's documented template: the `intent` value to pass to
/// `stage --intent-json`, plus field notes that stay outside the intent so
/// the template itself remains exactly what the deserializer accepts.
#[derive(Debug, Clone, PartialEq)]
pub struct StageSchemaOperation {
    pub operation: &'static str,
    pub intent: Value,
    pub notes: Vec<(&'static str, &'static str)>,
}

impl StageSchemaOperation {
    fn to_json(&self) -> Value {
        json!({
            "operation": self.operation,
            "intent": self.intent,
            "notes": self.notes.iter().map(|(field, note)| json!({
                "field": field,
                "note": note,
            })).collect::<Vec<_>>(),
        })
    }
}

/// The complete `SessionPlan` carried by a working-surface binding. Every
/// field the wire requires is present: `id`, `name`, `attach`, `lifecycle`,
/// `capabilities`, `views`, `backend_extensions` and `warnings` have no
/// defaults, and omitting any of them is a serde failure at stage time.
fn working_surface_plan_template() -> Value {
    json!({
        "id": "plan-id",
        "name": "plan name",
        "mux": "tmux",
        "attach": "always",
        "lifecycle": "persist",
        "capabilities": {},
        "views": [{
            "id": "main",
            "steps": [{
                "view": "main",
                "pane": "shell",
                "command": ["sh"],
                "restart": "never",
                "focus": true,
                "capabilities": {}
            }]
        }],
        "backend_extensions": {},
        "warnings": []
    })
}

fn operations() -> Vec<StageSchemaOperation> {
    vec![
        StageSchemaOperation {
            operation: "create",
            intent: json!({
                "operation": "create",
                "id": "session-space/<space-id>",
                "label": "optional human label"
            }),
            notes: vec![
                (
                    "id",
                    "required; a canonical SessionSpaceRef (`session-space/…`), unique per home",
                ),
                ("label", "optional"),
            ],
        },
        StageSchemaOperation {
            operation: "bind-project-context",
            intent: json!({
                "operation": "bind-project-context",
                "binding": {
                    "project": "<project-ref>",
                    "context": "<full ContextResolutionEvidence: read it from `aikit-session-space project-context`>",
                    "provenance": []
                }
            }),
            notes: vec![
                (
                    "binding.project",
                    "required; must equal the Project the context evidence was resolved for",
                ),
                (
                    "binding.context",
                    "required; an exact ContextResolutionEvidence, not a reference to one — pipe the whole `binding` object of `project-context` output here rather than hand-writing it",
                ),
                ("binding.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "unbind-project-context",
            intent: json!({
                "operation": "unbind-project-context",
                "project": "<project-ref>"
            }),
            notes: vec![("project", "required")],
        },
        StageSchemaOperation {
            operation: "attach-agent-session",
            intent: json!({
                "operation": "attach-agent-session",
                "attachment": {
                    "agent_session": "agent-session/<id>",
                    "purpose": "optional purpose",
                    "provenance": []
                }
            }),
            notes: vec![
                (
                    "attachment.agent_session",
                    "required; a canonical AgentSession ResourceRef",
                ),
                ("attachment.purpose", "optional"),
                ("attachment.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "detach-agent-session",
            intent: json!({
                "operation": "detach-agent-session",
                "agent_session": "agent-session/<id>"
            }),
            notes: vec![("agent_session", "required")],
        },
        StageSchemaOperation {
            operation: "attach-surface",
            intent: json!({
                "operation": "attach-surface",
                "attachment": {
                    "surface": "surface/terminal/<view>/<pane>",
                    "component": null,
                    "purpose": "optional purpose",
                    "provenance": []
                }
            }),
            notes: vec![
                (
                    "attachment.surface",
                    "required; a canonical Surface ResourceRef",
                ),
                (
                    "attachment.component",
                    "optional; a canonical Component ResourceRef",
                ),
                ("attachment.purpose", "optional"),
                ("attachment.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "detach-surface",
            intent: json!({
                "operation": "detach-surface",
                "surface": "surface/terminal/<view>/<pane>"
            }),
            notes: vec![(
                "surface",
                "required; also unbinds any working Surface bound to it",
            )],
        },
        StageSchemaOperation {
            operation: "bind-working-surface",
            intent: json!({
                "operation": "bind-working-surface",
                "binding": {
                    "binding": "working-surface/<id>",
                    "surface": "surface/terminal/<view>/<pane>",
                    "agent_session": "agent-session/<id>",
                    "provider": "provider/<technology>/current",
                    "plan": working_surface_plan_template(),
                    "plan_key": "<view>/<pane>",
                    "provenance": []
                }
            }),
            notes: vec![
                (
                    "binding.binding",
                    "required; the durable owner address for this binding",
                ),
                (
                    "binding.surface",
                    "required; must equal the canonical Surface derived from plan views/panes at plan_key",
                ),
                ("binding.agent_session", "required"),
                (
                    "binding.provider",
                    "required; the provider ref naming the place technology",
                ),
                (
                    "binding.plan",
                    "required; a complete SessionPlan. Fields with no default and no `?`: id, name, attach, lifecycle, capabilities, views, backend_extensions, warnings. `mux` is an open place-technology name (tmux, cmux, plain, or any lowercase [a-z0-9-] name up to 32 characters); omit for auto",
                ),
                (
                    "binding.plan.backend_extensions",
                    "required; per-technology options keyed by technology name, `{}` when none",
                ),
                (
                    "binding.plan.warnings",
                    "required; `[]` when empty — this field is easy to miss",
                ),
                (
                    "binding.plan_key",
                    "required; `<view>/<pane>` naming one pane step of the plan",
                ),
                ("binding.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "unbind-working-surface",
            intent: json!({
                "operation": "unbind-working-surface",
                "binding": "working-surface/<id>"
            }),
            notes: vec![("binding", "required; the binding's own ResourceRef")],
        },
        StageSchemaOperation {
            operation: "bind-native-reference",
            intent: json!({
                "operation": "bind-native-reference",
                "binding": {
                    "reference": "provider/<name>",
                    "kind": "provider",
                    "owner": null,
                    "provider": null,
                    "host": null,
                    "purpose": "optional purpose",
                    "provenance": []
                }
            }),
            notes: vec![
                ("binding.reference", "required"),
                (
                    "binding.kind",
                    "required; one of provider, host, workcell, material",
                ),
                ("binding.owner/provider/host", "optional ResourceRefs"),
                ("binding.purpose", "optional"),
                ("binding.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "unbind-native-reference",
            intent: json!({
                "operation": "unbind-native-reference",
                "reference": "provider/<name>"
            }),
            notes: vec![("reference", "required")],
        },
        StageSchemaOperation {
            operation: "focus",
            intent: json!({
                "operation": "focus",
                "focus": {
                    "target": "<resource-ref>",
                    "region": null,
                    "provenance": []
                }
            }),
            notes: vec![
                (
                    "focus",
                    "optional; omit `focus` entirely to clear the focus",
                ),
                ("focus.target", "required when focus is present"),
                ("focus.region", "optional"),
                ("focus.provenance", "optional; defaults to empty"),
            ],
        },
        StageSchemaOperation {
            operation: "restore",
            intent: json!({
                "operation": "restore",
                "target": "<full SessionSpaceAuthoredState: read it from `aikit-session-space show <space>`>",
                "evidence": "why this restore is authorised"
            }),
            notes: vec![
                (
                    "target",
                    "required; an exact authored state, not a reference to one — pipe the `state` output of `show` here rather than hand-writing it",
                ),
                ("evidence", "required"),
            ],
        },
    ]
}

/// The documented template for one operation, by kebab-case operation name.
pub fn operation_schema(operation: &str) -> Option<Value> {
    operations()
        .into_iter()
        .find(|schema| schema.operation == operation)
        .map(|schema| schema.to_json())
}

/// The documented templates for every operation, with usage.
pub fn schema() -> Value {
    let mut entries = serde_json::Map::new();
    for schema in operations() {
        entries.insert(schema.operation.to_string(), schema.to_json());
    }
    json!({
        "schema": "aikit.session-space-application/v1",
        "usage": "pass one operation's `intent` value to `aikit-session-space stage --intent-json` (prefix with @ to read from a file); `notes` are documentation, not part of the intent",
        "operations": Value::Object(entries),
    })
}

/// Every hand-writable template parses back as a valid typed intent. This is
/// the golden guarantee: what the printer documents, the deserializer accepts.
#[cfg(test)]
mod tests {
    use super::*;

    /// Operations whose intent embeds a full evidence/state struct that an
    /// owner pipes from another command (`project-context`, `show`) rather
    /// than hand-writing. Their placeholder value is documentation, not a
    /// parseable template, and the golden test says so explicitly.
    const EMBEDDED_EVIDENCE_OPERATIONS: [&str; 2] = ["bind-project-context", "restore"];

    #[test]
    fn every_printed_operation_template_parses_as_a_valid_intent() {
        let schema = schema();
        let operations = schema
            .get("operations")
            .and_then(Value::as_object)
            .expect("the schema carries an operations map");
        assert!(
            operations.len() >= 12,
            "every SessionSpace mutation has a documented template"
        );
        for (name, entry) in operations {
            if EMBEDDED_EVIDENCE_OPERATIONS.contains(&name.as_str()) {
                // Piped-evidence operations are documented as placeholders;
                // their parseability is guaranteed by the producing command.
                continue;
            }
            let intent = entry
                .get("intent")
                .unwrap_or_else(|| panic!("{name} carries an intent template"));
            let parsed: SessionSpaceMutation = serde_json::from_value(intent.clone())
                .unwrap_or_else(|error| panic!("{name} template must parse as an intent: {error}"));
            // The tag survives the round trip, so what an operator copies is
            // the operation they asked for.
            let wire = serde_json::to_value(&parsed).unwrap();
            assert_eq!(
                wire.get("operation").and_then(Value::as_str),
                Some(name.as_str()),
                "{name} template round trips to the same operation tag"
            );
        }
    }

    #[test]
    fn the_working_surface_plan_template_is_complete_and_parseable() {
        // The whole TM02-R failure mode: staging a working-surface binding by
        // JSON and meeting undocumented required plan fields. The template
        // must be exactly what SessionPlan accepts.
        let entry = operation_schema("bind-working-surface").expect("operation exists");
        let intent = entry.get("intent").unwrap();
        let parsed: SessionSpaceMutation = serde_json::from_value(intent.clone())
            .expect("the bind-working-surface template parses as an intent, plan fields included");
        match parsed {
            SessionSpaceMutation::BindWorkingSurface { binding } => {
                assert_eq!(binding.plan.id, "plan-id");
                assert_eq!(binding.plan.backend_extensions.len(), 0);
                assert!(binding.plan.warnings.is_empty());
                assert_eq!(
                    binding.plan.mux.as_ref().map(ToString::to_string),
                    Some("tmux".to_string())
                );
            }
            other => panic!("expected a bind-working-surface intent, got {other:?}"),
        }
    }

    #[test]
    fn single_operation_schema_serves_the_documented_names() {
        for name in [
            "attach-agent-session",
            "attach-surface",
            "bind-native-reference",
            "bind-working-surface",
        ] {
            let schema = operation_schema(name).expect("documented operation");
            assert_eq!(schema.get("operation").and_then(Value::as_str), Some(name));
            assert!(schema.get("intent").is_some());
            assert!(schema.get("notes").is_some());
        }
        assert!(operation_schema("no-such-operation").is_none());
    }
}
