//! The managed tools-layer projection: resolved `tool-protocol` capability
//! sources planned into a harness's native MCP-server configuration through
//! the one layer merge engine ([`crate::layers`]).
//!
//! The invariant this module owns is *posture truth at plan time*. MCP server
//! records are planned only where the harness profile records a `managed`
//! tools layer naming an `mcp-servers-record` seam; a profile without one
//! yields [`ToolsProjectionOutcome::NotProjected`] with the reason in plain
//! words, never a silently empty plan — a caller that cannot tell "wrote
//! nothing on purpose" from "wrote nothing by mistake" cannot disclose what
//! it did. The existing target is read through an injected reader so the
//! planner never touches the filesystem itself; a missing file is a fresh
//! document and an unreadable one is a refusal naming the path, because AIKit
//! will not overwrite what it cannot read. Record values — `env` among them,
//! they belong in the projected file — reach the write's contents and nothing
//! else: errors and the merge report carry names, paths and problems only,
//! because native config files carry secrets. Ownership inside the foreign
//! document is the merge engine's law; this module contributes the marker
//! every AIKit tool record carries ([`TOOLS_PROJECTION_OWNERSHIP`]).

use std::collections::BTreeMap;

use aikit_core::harness_profile::{
    ActivationEffectName, HarnessProfile, LayerPosture, MergeGrammar,
};
use aikit_core::projection::ProjectionItem;

pub use aikit_core::capsule::ToolServerRecord;

use crate::layers::{apply_merge, LayerMergeError, MergeArgs, MergeReport};

/// The ownership marker every AIKit-projected tool record carries. The
/// record-map grammar matches it in a record's `command`, `args` or `url`
/// fields to tell AIKit's servers apart from the harness's own, so a stale
/// AIKit record is swept while a foreign one is preserved untouched.
pub const TOOLS_PROJECTION_OWNERSHIP: &str = "aikit tool-protocol";

/// One resolved `tool-protocol` capability source, ready to be projected: the
/// name the server is exported under and the server record itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSourceEntry {
    pub export_name: String,
    pub server: ToolServerRecord,
}

/// What projecting a harness's tools layer would do. `Projected` carries the
/// plan; `NotProjected` says plainly why nothing would be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsProjectionOutcome {
    Projected(ToolsProjectionPlan),
    NotProjected { reason: String },
}

/// One managed tools-layer projection: write the merged native config, plus
/// what the merge did and when the harness picks the change up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsProjectionPlan {
    /// The native config file the profile's project declaration names.
    pub path: String,
    /// The write itself: the merged document, pretty JSON with a trailing
    /// newline.
    pub item: ProjectionItem,
    /// What the merge did — added, replaced, removed, kept foreign.
    pub report: MergeReport,
    /// When the harness sees the change, as the profile declares it.
    pub activation: Option<ActivationEffectName>,
}

/// A tools-layer projection refusal. `code` is stable machine surface in the
/// `projection.*` namespace; the message names the slug or the path and what
/// would fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSourceError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
}

impl ToolSourceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> &BTreeMap<String, String> {
        &self.details
    }
}

impl std::fmt::Display for ToolSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.details.is_empty() {
            let rendered: Vec<String> = self
                .details
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            write!(f, " ({})", rendered.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for ToolSourceError {}

impl From<LayerMergeError> for ToolSourceError {
    fn from(error: LayerMergeError) -> Self {
        let mut tool_error = ToolSourceError::new(error.code(), error.message());
        for (key, value) in error.details() {
            tool_error = tool_error.with(key.clone(), value.clone());
        }
        tool_error
    }
}

impl From<aikit_core::AikitError> for ToolSourceError {
    fn from(error: aikit_core::AikitError) -> Self {
        let mut tool_error = ToolSourceError::new(error.code(), error.message());
        for (key, value) in error.details() {
            tool_error = tool_error.with(key.clone(), value.clone());
        }
        tool_error
    }
}

/// Plan the managed tools-layer projection for one harness profile: merge the
/// resolved tool sources into the existing native config the profile's
/// project declaration names, and return the write plus what the merge did.
///
/// A profile with no tools layer, or a tools layer whose posture is not
/// `managed`, is [`ToolsProjectionOutcome::NotProjected`] with the reason —
/// planning "nothing to do here" is a result, not a failure.
pub fn plan_tools_projection(
    entries: impl IntoIterator<Item = ToolSourceEntry>,
    profile: &HarnessProfile,
    read_existing: impl Fn(&str) -> std::io::Result<Option<String>>,
) -> Result<ToolsProjectionOutcome, ToolSourceError> {
    let Some(tools) = profile.tools.as_ref() else {
        return Ok(ToolsProjectionOutcome::NotProjected {
            reason: format!(
                "the {} profile declares no tools layer; there is no seam AIKit may project \
                 MCP server records into",
                profile.slug
            ),
        });
    };
    if tools.posture != LayerPosture::Managed {
        return Ok(ToolsProjectionOutcome::NotProjected {
            reason: format!(
                "the {} profile's tools layer posture is {}; AIKit projects MCP server \
                 records only into a managed tools layer",
                profile.slug, tools.posture
            ),
        });
    }
    // Schema validation forbids a managed layer without a project, so this is
    // an encountered-impossible state; it refuses naming the slug all the same.
    let Some(project) = tools.project.as_ref() else {
        return Err(ToolSourceError::new(
            "projection.tools_managed_without_project",
            format!(
                "the {} profile's tools layer posture is managed but declares no project; \
                 a managed layer must name the seam it projects into — add the profile's \
                 `project` declaration (file, key, format, merge) or change the posture",
                profile.slug
            ),
        )
        .with("slug", profile.slug.clone())
        .with("layer", "tools"));
    };
    if project.format != MergeGrammar::McpServersRecord {
        return Err(ToolSourceError::new(
            "projection.tools_grammar_unsupported",
            format!(
                "the {} profile's tools layer projects through the {:?} grammar, which the \
                 tools-layer projection does not implement; set the project format to \
                 \"mcp-servers-record\" or add the grammar to the layer merge engine",
                profile.slug, project.format
            ),
        )
        .with("slug", profile.slug.clone())
        .with("format", format!("{:?}", project.format)));
    }

    let existing = read_existing(&project.file).map_err(|error| {
        ToolSourceError::new(
            "projection.tools_target_unreadable",
            format!(
                "the {} profile's tool configuration at {} could not be read: {error}; fix \
                 the read before projecting — AIKit will not overwrite a file it cannot read",
                profile.slug, project.file
            ),
        )
        .with("path", project.file.clone())
    })?;
    let document = match existing {
        None => serde_json::json!({}),
        Some(raw) if raw.trim().is_empty() => serde_json::json!({}),
        Some(raw) => serde_json::from_str(&raw).map_err(|error| {
            ToolSourceError::new(
                "projection.tools_document_unreadable",
                format!(
                    "the existing tool configuration at {} is not valid JSON: {error}; AIKit \
                     will not overwrite a file it cannot read",
                    project.file
                ),
            )
            .with("path", project.file.clone())
        })?,
    };

    let managed: BTreeMap<String, serde_json::Value> = entries
        .into_iter()
        .map(|entry| (entry.export_name, server_record_value(&entry.server)))
        .collect();
    let (merged, report) = apply_merge(
        MergeGrammar::McpServersRecord,
        &document,
        MergeArgs::McpServersRecord {
            key_path: project.key.split('.').map(str::to_string).collect(),
            managed,
            ownership: TOOLS_PROJECTION_OWNERSHIP.to_string(),
        },
    )?;

    let mut contents = serde_json::to_string_pretty(&merged).map_err(|error| {
        ToolSourceError::new(
            "projection.tools_unrenderable",
            format!(
                "the merged {} tool configuration could not be rendered: {error}",
                project.file
            ),
        )
        .with("path", project.file.clone())
    })?;
    contents.push('\n');
    let item = ProjectionItem::write(&project.file, contents)?;

    Ok(ToolsProjectionOutcome::Projected(ToolsProjectionPlan {
        path: project.file.clone(),
        item,
        report,
        activation: tools.activation,
    }))
}

/// Render one server record as the JSON object the native config carries,
/// omitting absent and empty fields. Values are carried whole — they belong
/// in the projected file and nowhere else.
fn server_record_value(record: &ToolServerRecord) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    if let Some(command) = &record.command {
        object.insert("command".to_string(), serde_json::json!(command));
    }
    if !record.args.is_empty() {
        object.insert("args".to_string(), serde_json::json!(record.args));
    }
    if !record.env.is_empty() {
        object.insert("env".to_string(), serde_json::json!(record.env));
    }
    if let Some(cwd) = &record.cwd {
        object.insert("cwd".to_string(), serde_json::json!(cwd));
    }
    if let Some(url) = &record.url {
        object.insert("url".to_string(), serde_json::json!(url));
    }
    serde_json::Value::Object(object)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn bimba_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "bimba".to_string(),
            server: ToolServerRecord {
                command: Some(
                    "/Users/admin/Central/Work/epi/bimba-portable/bimba-mcp.sh".to_string(),
                ),
                args: vec!["--port".to_string(), "8080".to_string()],
                env: BTreeMap::from([("BIMBA_TOKEN".to_string(), "sk-test-value".to_string())]),
                cwd: Some("/Users/admin/Central/Work/epi".to_string()),
                url: None,
                headers: BTreeMap::new(),
            },
        }
    }

    fn fs_docs_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "fs-docs".to_string(),
            server: ToolServerRecord {
                command: Some("npx".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    "/tmp".to_string(),
                ],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                headers: BTreeMap::new(),
            },
        }
    }

    fn openclaw() -> &'static HarnessProfile {
        crate::profiles::for_slug("openclaw").expect("openclaw carries an embedded profile")
    }

    fn bimba_record_json() -> serde_json::Value {
        serde_json::json!({
            "command": "/Users/admin/Central/Work/epi/bimba-portable/bimba-mcp.sh",
            "args": ["--port", "8080"],
            "env": { "BIMBA_TOKEN": "sk-test-value" },
            "cwd": "/Users/admin/Central/Work/epi"
        })
    }

    fn fs_docs_record_json() -> serde_json::Value {
        serde_json::json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        })
    }

    /// Seed a real config file on disk and plan against a reader that reads
    /// it from the filesystem, recording every path the projection asked for.
    /// `None` seeds nothing, so the reader answers "no such file".
    fn seed_and_plan(
        existing: Option<&str>,
        entries: &[ToolSourceEntry],
        profile: &HarnessProfile,
    ) -> (ToolsProjectionOutcome, Vec<String>) {
        let dir = tempdir().expect("tempdir");
        let seeded = dir.path().join("config.json");
        if let Some(contents) = existing {
            fs::write(&seeded, contents).expect("seed the existing config on disk");
        }
        let asked = RefCell::new(Vec::new());
        let outcome = plan_tools_projection(entries.iter().cloned(), profile, |asked_path| {
            asked.borrow_mut().push(asked_path.to_string());
            if seeded.is_file() {
                fs::read_to_string(&seeded).map(Some)
            } else {
                Ok(None)
            }
        })
        .expect("the projection plans against the seeded file");
        (outcome, asked.into_inner())
    }

    fn written_contents(outcome: &ToolsProjectionOutcome) -> &str {
        let ToolsProjectionOutcome::Projected(plan) = outcome else {
            panic!("the tools layer projects: {outcome:?}");
        };
        let ProjectionItem::Write { contents, .. } = &plan.item else {
            panic!("the plan is a write of the merged config: {:?}", plan.item);
        };
        contents
    }

    #[test]
    fn an_openclaw_projection_merges_managed_records_around_a_foreign_server_and_unrelated_keys() {
        let existing = r#"{
  "mcpServers": {
    "linear-server": { "url": "https://mcp.linear.app/sse" }
  },
  "editor": { "font": "iosevka" },
  "telemetry": false
}"#;
        let (outcome, asked) = seed_and_plan(
            Some(existing),
            &[bimba_entry(), fs_docs_entry()],
            openclaw(),
        );

        let ToolsProjectionOutcome::Projected(plan) = &outcome else {
            panic!("a managed openclaw tools layer projects: {outcome:?}");
        };
        assert_eq!(
            asked,
            vec!["~/.openclaw/mcp.json".to_string()],
            "the projection reads exactly the path the profile's project declaration names"
        );
        assert_eq!(plan.path, "~/.openclaw/mcp.json");
        let ProjectionItem::Write { path, contents } = &plan.item else {
            panic!("the plan is a write of the merged config: {:?}", plan.item);
        };
        assert_eq!(path.as_path(), Path::new("~/.openclaw/mcp.json"));
        assert!(contents.ends_with('\n'), "the document ends with a newline");

        let merged: serde_json::Value =
            serde_json::from_str(contents).expect("the write contents are valid JSON");
        assert_eq!(merged["mcpServers"]["bimba"], bimba_record_json());
        assert_eq!(merged["mcpServers"]["fs-docs"], fs_docs_record_json());
        assert_eq!(
            merged["mcpServers"]["linear-server"],
            serde_json::json!({ "url": "https://mcp.linear.app/sse" }),
            "the foreign server is untouched"
        );
        assert_eq!(
            merged["editor"],
            serde_json::json!({ "font": "iosevka" }),
            "unrelated keys are untouched"
        );
        assert_eq!(merged["telemetry"], serde_json::json!(false));

        assert_eq!(
            plan.report,
            MergeReport {
                added: vec!["bimba".to_string(), "fs-docs".to_string()],
                replaced: vec![],
                removed: vec![],
                kept_foreign: vec!["linear-server".to_string()],
            }
        );
        assert_eq!(plan.activation, Some(ActivationEffectName::RestartClient));
    }

    #[test]
    fn a_previous_akit_owned_entry_not_among_the_entries_is_swept_and_reported() {
        let existing = r#"{
  "mcpServers": {
    "retired-source": { "command": "aikit tool-protocol serve retired" },
    "linear-server": { "url": "https://mcp.linear.app/sse" }
  }
}"#;
        let (outcome, _) = seed_and_plan(Some(existing), &[bimba_entry()], openclaw());

        let merged: serde_json::Value =
            serde_json::from_str(written_contents(&outcome)).expect("contents parse");
        assert!(
            merged["mcpServers"].get("retired-source").is_none(),
            "the stale owned record leaves the document: {merged}"
        );
        assert_eq!(
            merged["mcpServers"]["linear-server"],
            serde_json::json!({ "url": "https://mcp.linear.app/sse" }),
            "the foreign server survives the sweep"
        );

        let ToolsProjectionOutcome::Projected(plan) = &outcome else {
            panic!("a managed openclaw tools layer projects: {outcome:?}");
        };
        assert_eq!(plan.report.removed, vec!["retired-source".to_string()]);
        assert_eq!(plan.report.added, vec!["bimba".to_string()]);
        assert_eq!(plan.report.kept_foreign, vec!["linear-server".to_string()]);
    }

    #[test]
    fn a_brokered_tools_layer_is_not_projected_and_the_reason_names_the_posture() {
        let pi = crate::profiles::for_slug("pi").expect("pi carries an embedded profile");
        let (outcome, _) = seed_and_plan(Some(r#"{"mcpServers":{}}"#), &[bimba_entry()], pi);

        let ToolsProjectionOutcome::NotProjected { reason } = &outcome else {
            panic!("a brokered tools layer must not project: {outcome:?}");
        };
        assert!(reason.contains("pi"), "the reason names the slug: {reason}");
        assert!(
            reason.contains("brokered"),
            "the reason names the posture: {reason}"
        );
    }

    #[test]
    fn a_profile_without_a_tools_layer_is_not_projected_and_says_so() {
        let mut profile = openclaw().clone();
        profile.tools = None;
        let (outcome, _) = seed_and_plan(None, &[bimba_entry()], &profile);

        let ToolsProjectionOutcome::NotProjected { reason } = &outcome else {
            panic!("a profile without a tools layer must not project: {outcome:?}");
        };
        assert!(
            reason.contains("openclaw"),
            "the reason names the slug: {reason}"
        );
        assert!(
            reason.contains("no tools layer"),
            "the reason says there is no seam: {reason}"
        );
    }

    #[test]
    fn a_missing_existing_file_projects_a_fresh_document_of_only_the_managed_records() {
        let (outcome, asked) = seed_and_plan(None, &[bimba_entry(), fs_docs_entry()], openclaw());

        let ToolsProjectionOutcome::Projected(plan) = &outcome else {
            panic!("a missing file is a fresh document, not a refusal: {outcome:?}");
        };
        assert_eq!(
            asked,
            vec!["~/.openclaw/mcp.json".to_string()],
            "the missing path is still the one the profile names"
        );
        let merged: serde_json::Value =
            serde_json::from_str(written_contents(&outcome)).expect("contents parse");
        assert_eq!(
            merged,
            serde_json::json!({
                "mcpServers": {
                    "bimba": bimba_record_json(),
                    "fs-docs": fs_docs_record_json()
                }
            })
        );
        assert_eq!(
            plan.report.added,
            vec!["bimba".to_string(), "fs-docs".to_string()]
        );
        assert!(plan.report.kept_foreign.is_empty());
        assert!(plan.report.removed.is_empty());
    }

    #[test]
    fn the_projected_document_parses_and_preserves_every_preexisting_top_level_key() {
        let existing = r#"{
  "theme": "dark",
  "editor": { "font": "iosevka", "size": 13 },
  "flags": [true, false],
  "nested": { "deep": { "value": 1 } },
  "mcpServers": {
    "linear-server": { "url": "https://mcp.linear.app/sse" }
  }
}"#;
        let (outcome, _) = seed_and_plan(Some(existing), &[bimba_entry()], openclaw());

        let merged: serde_json::Value =
            serde_json::from_str(written_contents(&outcome)).expect("contents parse");
        for (key, expected) in [
            ("theme", serde_json::json!("dark")),
            (
                "editor",
                serde_json::json!({ "font": "iosevka", "size": 13 }),
            ),
            ("flags", serde_json::json!([true, false])),
            ("nested", serde_json::json!({ "deep": { "value": 1 } })),
            (
                "mcpServers",
                serde_json::json!({
                    "bimba": bimba_record_json(),
                    "linear-server": { "url": "https://mcp.linear.app/sse" }
                }),
            ),
        ] {
            assert_eq!(
                merged.get(key),
                Some(&expected),
                "the pre-existing `{key}` key survives the projection"
            );
        }
    }

    #[test]
    fn a_managed_tools_layer_without_a_project_declaration_refuses_naming_the_slug() {
        let mut profile = openclaw().clone();
        profile.tools.as_mut().unwrap().project = None;

        let error = plan_tools_projection([bimba_entry()], &profile, |_| Ok(None)).unwrap_err();

        assert_eq!(error.code(), "projection.tools_managed_without_project");
        assert!(
            error.to_string().contains("openclaw"),
            "the refusal names the slug: {error}"
        );
    }

    #[test]
    fn a_managed_tools_layer_with_a_non_record_grammar_refuses_rather_than_guessing() {
        let mut profile = openclaw().clone();
        profile
            .tools
            .as_mut()
            .unwrap()
            .project
            .as_mut()
            .unwrap()
            .format = MergeGrammar::ClaudeHookMap;

        let error = plan_tools_projection([bimba_entry()], &profile, |_| Ok(None)).unwrap_err();

        assert_eq!(error.code(), "projection.tools_grammar_unsupported");
        assert!(
            error.to_string().contains("openclaw"),
            "the refusal names the slug: {error}"
        );
    }

    #[test]
    fn an_unreadable_existing_file_refuses_naming_the_path() {
        let error = plan_tools_projection([bimba_entry()], openclaw(), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();

        assert_eq!(error.code(), "projection.tools_target_unreadable");
        assert!(
            error.to_string().contains("~/.openclaw/mcp.json"),
            "the refusal names the path it could not read: {error}"
        );
    }

    #[test]
    fn an_existing_file_that_is_not_json_refuses_naming_the_path_rather_than_overwriting() {
        let error = plan_tools_projection([bimba_entry()], openclaw(), |_| {
            Ok(Some("{ this is not json".to_string()))
        })
        .unwrap_err();

        assert_eq!(error.code(), "projection.tools_document_unreadable");
        assert!(
            error.to_string().contains("~/.openclaw/mcp.json"),
            "the refusal names the path: {error}"
        );
    }
}
