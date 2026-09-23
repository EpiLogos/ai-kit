//! The native-MCP fallback's execution half: when a negotiated session cannot
//! carry the composed tool surface on its wire, this module decides whether
//! the harness's own configuration is a place those capsules may actually
//! land — and when it is, puts them there through the one receipt pipeline.
//!
//! The invariant this module owns is *no write outside a declared seam*. A
//! harness profile records its ownership postures as data; MCP server records
//! are written only where the profile declares a **managed** tools layer
//! naming an `mcp-servers-record` project seam. An observed or brokered tools
//! layer, a provider with no profile at all, a seam in another grammar — each
//! is a [`NativeToolsProjection::Boundary`]: nothing is written, and the
//! reason says why in plain words, so the encounter's fallback record stands
//! as a named boundary rather than a promise the write never kept. Where the
//! seam IS declared, the projection runs through
//! [`crate::client::apply_managed_tools_projection`] — the same plan →
//! WorldEdit + Inverse → Procedure implementation `aikit apply` uses — never
//! a bare file write.
//!
//! A failed write is a disclosure, not a session precondition: the encounter
//! journals it as an error event and the open proceeds, because the session's
//! wire is unchanged by this whole route either way.

use std::path::{Path, PathBuf};

use aikit_adapters::tool_sources::ToolSourceEntry;
use aikit_core::harness_profile::{LayerPosture, MergeGrammar};
use aikit_store::AikitHome;
use serde_json::{json, Value};

use crate::client::ToolsLayerOutcome;

/// What executing the native-MCP fallback for one open did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeToolsProjection {
    /// The harness's profile declares a managed tools layer with a writable
    /// `mcp-servers-record` seam, and the write went through the one
    /// Procedure pipeline. `state` inside is `written`, or `satisfied` when
    /// the merge output was already exactly in place.
    Executed(Box<ToolsLayerOutcome>),
    /// The harness declares no writable managed MCP seam. Nothing was
    /// written and nothing will be; `reason` says why in plain words.
    Boundary { reason: String },
    /// A declared seam existed but the write itself failed — an unreadable
    /// existing config, an unrenderable merge, a refused procedure. Never
    /// swallowed: the caller journals this as an error event.
    Failed { reason: String },
}

impl NativeToolsProjection {
    /// The journal event for one executed fallback: a written projection
    /// carries its procedure receipt, a boundary names why nothing may be
    /// written, a failure is the error event for a declared seam whose write
    /// went wrong.
    pub fn journal_event(&self, provider: &str, slug: Option<&str>) -> Value {
        let mut event = match self {
            NativeToolsProjection::Executed(outcome) => {
                let mut event = json!({
                    "kind":"native-mcp-native-projection-written",
                    "provider":provider,
                    "harness_profile":slug,
                    "state":outcome.state,
                    "path":outcome.path,
                    "procedure":outcome.procedure,
                    "undo":outcome.undo,
                    "entries_written":outcome.added.map(|added| added + outcome.replaced.unwrap_or(0)),
                    "entries_swept":outcome.removed,
                    "kept_foreign":outcome.kept_foreign,
                    "written_entries":outcome.written_entries,
                    "swept_entries":outcome.swept_entries,
                    "activation":outcome.activation,
                    "standing":"projected through the one managed tools-layer Procedure \
                     pipeline, never a bare file write",
                });
                if let Some(reason) = &outcome.reason {
                    event["reason"] = json!(reason);
                }
                event
            }
            NativeToolsProjection::Boundary { reason } => json!({
                "kind":"native-mcp-native-projection-boundary",
                "provider":provider,
                "harness_profile":slug,
                "reason":reason,
                "standing":"no managed tools seam declared; nothing written, nothing to write",
            }),
            NativeToolsProjection::Failed { reason } => json!({
                "kind":"native-mcp-native-projection-failed",
                "provider":provider,
                "harness_profile":slug,
                "error":reason,
                "standing":"the session may still open — the projection is a disclosed route, \
                 not a session precondition — but the write failure is journaled, never \
                 swallowed",
            }),
        };
        event["composed_tools_route"] =
            json!("harness-native-mcp-config-seam-not-the-session-wire");
        event
    }

    /// The fragment the open receipt records under `composed_tools.execution`:
    /// what ran and what it touched, or why nothing was written. Present
    /// whenever the fallback route was declared, so a receipt can never
    /// promise the native seam while staying silent about whether the write
    /// happened.
    pub fn execution_receipt(&self, slug: Option<&str>) -> Value {
        match self {
            NativeToolsProjection::Executed(outcome) => {
                let mut receipt = json!({
                    "state":outcome.state,
                    "harness_profile":slug,
                    "path":outcome.path,
                    "procedure":outcome.procedure,
                    "undo":outcome.undo,
                    "entries_written":outcome.added.map(|added| added + outcome.replaced.unwrap_or(0)),
                    "entries_swept":outcome.removed,
                    "kept_foreign":outcome.kept_foreign,
                    "written_entries":outcome.written_entries,
                    "swept_entries":outcome.swept_entries,
                    "activation":outcome.activation,
                });
                if let Some(reason) = &outcome.reason {
                    receipt["reason"] = json!(reason);
                }
                receipt
            }
            NativeToolsProjection::Boundary { reason } => json!({
                "state":"not-writable",
                "harness_profile":slug,
                "reason":reason,
            }),
            NativeToolsProjection::Failed { reason } => json!({
                "state":"failed",
                "harness_profile":slug,
                "reason":reason,
            }),
        }
    }
}

/// Execute the native-MCP fallback for one open: project the composed trusted
/// `tool-protocol` entries into the target harness's native MCP configuration
/// when — and only when — its profile declares a managed tools layer naming an
/// `mcp-servers-record` seam.
///
/// `slug` is the provider's `from_profile` harness profile; `None` is a
/// freeform provider with no profile, and is a boundary, never a guess. `cwd`
/// is the encounter's working directory: a relative seam resolves against the
/// AIKit project root discovered from it, exactly as the apply path resolves
/// one. `machine_home` roots the profile's `~/`-anchored seam paths.
///
/// The entries are already the one trusted+enabled resolution
/// ([`crate::encounter_mcp`]); this module never re-resolves them and never
/// writes capsule material that did not compose. The write itself rides
/// [`crate::client::apply_managed_tools_projection`], so it is planned,
/// diffable and reversible exactly like `aikit apply`'s tools tail.
pub fn project_composed_tools_to_native_seam(
    home: &AikitHome,
    cwd: &Path,
    machine_home: &Path,
    slug: Option<&str>,
    entries: &[ToolSourceEntry],
) -> NativeToolsProjection {
    let boundary = |reason: String| NativeToolsProjection::Boundary { reason };
    let Some(slug) = slug else {
        return boundary(
            "the provider carries no harness profile, so no managed tools seam is declared \
             for it; AIKit never writes outside a declared seam"
                .to_string(),
        );
    };
    let Some((static_slug, profile)) =
        aikit_adapters::profiles::all().find(|(key, _)| *key == slug)
    else {
        return boundary(format!(
            "no embedded harness profile is named {slug}, so no managed tools seam can be \
             declared for this open; AIKit never writes outside a declared seam"
        ));
    };
    let Some(tools) = profile.tools.as_ref() else {
        return boundary(format!(
            "the {slug} profile declares no tools layer, so there is no managed MCP seam \
             for the composed capsules; AIKit never writes outside a declared seam"
        ));
    };
    if tools.posture != LayerPosture::Managed {
        return boundary(format!(
            "the {slug} profile's tools layer posture is {}; AIKit projects MCP server \
             records only into a managed tools layer, so the composed capsules have no \
             declared seam to write",
            tools.posture
        ));
    }
    let Some(project) = tools.project.as_ref() else {
        return boundary(format!(
            "the {slug} profile's tools layer posture is managed but declares no project \
             seam; profile validation should have refused the document, and writing nothing \
             is the safe reading"
        ));
    };
    if project.format != MergeGrammar::McpServersRecord {
        return boundary(format!(
            "the {slug} profile's tools layer projects through the {:?} grammar, which \
             carries no MCP server record seam; AIKit writes no MCP records there",
            project.format
        ));
    }

    // The project basis a relative seam would resolve against, discovered the
    // same way the apply path discovers it. Every managed tools seam declared
    // today is `~/`-anchored, so this is the inert half of the expansion —
    // resolved honestly all the same rather than assumed away.
    let service = crate::app::Service::open(home.clone(), cwd, |key| std::env::var(key).ok()).ok();
    let tree = service
        .as_ref()
        .and_then(|service| service.descriptor().project_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    // With nothing composed there is nothing a fresh document would carry, so
    // a missing seam stays missing — the apply path's own no-composition law,
    // on the same reason implementation.
    let uncomposed = if entries.is_empty() {
        Some(match &service {
            Some(service) => crate::client::uncomposed_tools_reason(service),
            None => String::from(
                "the AIKit application context could not be resolved and no tool-protocol \
                 capsule is composed, so there are no MCP server records to project",
            ),
        })
    } else {
        None
    };

    let outcome = crate::client::apply_managed_tools_projection(
        home,
        &tree,
        machine_home,
        crate::client::client_for_slug(static_slug).unwrap_or(static_slug),
        static_slug,
        entries,
        uncomposed.as_deref(),
    );
    match outcome.state {
        "written" | "satisfied" => NativeToolsProjection::Executed(Box::new(outcome)),
        "not-projected" => NativeToolsProjection::Boundary {
            reason: outcome.reason.unwrap_or_else(|| {
                "the tools layer's own posture says there is nothing to project".to_string()
            }),
        },
        _ => NativeToolsProjection::Failed {
            reason: outcome.reason.unwrap_or_else(|| {
                "the managed tools-layer write failed without naming a reason".to_string()
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use aikit_adapters::tool_sources::ToolSourceEntry;
    use aikit_core::catalog::Catalog as _;
    use aikit_core::{CapsuleId, TrustKey, TrustState};
    use aikit_store::trust::TrustStore;
    use aikit_store::AikitHome;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    use super::*;

    const SOURCE: &str = "test";

    /// A tool-protocol capsule whose record is one AIKit can later recognise
    /// as its own on a sweep: the dispatch-style command carries the
    /// ownership marker, the same shape the drift disclosure reads for.
    fn bimba_manifest() -> &'static str {
        "schema = 1\n\
         id = \"tool-protocol/test/bimba\"\n\
         kind = \"tool-protocol\"\n\
         name = \"Bimba test server\"\n\
         description = \"A real tool-protocol capsule for the native projection test\"\n\
         \n\
         [tool-protocol]\n\
         export_name = \"bimba\"\n\
         \n\
         [tool-protocol.server]\n\
         command = \"aikit tool-protocol serve bimba\"\n\
         args = [\"--port\", \"8080\"]\n\
         \n\
         [tool-protocol.server.env]\n\
         BIMBA_TOKEN = \"sk-test\"\n"
    }

    /// A second capsule, remote-shaped, whose record is owned by managed name
    /// while its capsule stays composed.
    fn linear_manifest() -> &'static str {
        "schema = 1\n\
         id = \"tool-protocol/test/linear\"\n\
         kind = \"tool-protocol\"\n\
         name = \"Linear test server\"\n\
         description = \"A remote tool-protocol capsule for the native projection test\"\n\
         \n\
         [tool-protocol.server]\n\
         url = \"https://mcp.example/sse\"\n"
    }

    /// A real AIKit home on disk: the capsule manifests under a real registry,
    /// a real scope profile enabling `enable`, and — when `trust` — a real
    /// trusted trust record. This is the same resolution floor the encounter's
    /// ACP composition stands on ([`crate::encounter_mcp`]): enabled plus
    /// trusted, or nothing.
    fn home_with_capsules(
        tmp: &TempDir,
        manifests: &[&str],
        id_paths: &[&str],
        enable: &[&str],
        trust: bool,
    ) -> AikitHome {
        let home = AikitHome::at(tmp.path().join("home"));
        home.ensure_layout().unwrap();
        for (manifest, id_path) in manifests.iter().zip(id_paths) {
            let dir = home.registry(SOURCE).join("capsules").join(id_path);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("manifest.toml"), manifest).unwrap();
        }
        let mut profile = String::from("schema = 1\n");
        if !enable.is_empty() {
            profile.push_str(&format!(
                "enable = [{}]\n",
                enable
                    .iter()
                    .map(|id| format!("\"{id}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        fs::create_dir_all(home.root().join("scopes/global")).unwrap();
        fs::write(home.global_profile(), profile).unwrap();

        let world = tmp.path().join("world");
        fs::create_dir_all(&world).unwrap();
        let mut service = crate::app::Service::open(home.clone(), &world, |_: &str| None).unwrap();
        if trust {
            {
                let store = TrustStore::new(service.index());
                for id_path in id_paths {
                    let id = CapsuleId::parse(id_path).unwrap();
                    let capsule = service
                        .snapshot()
                        .get(&id)
                        .unwrap_or_else(|| panic!("the registry loaded {id_path}"))
                        .clone();
                    store
                        .record(
                            &TrustKey::new(
                                capsule.source.clone().unwrap(),
                                id,
                                capsule.revision.clone().unwrap(),
                            ),
                            TrustState::Trusted,
                            Some("native projection test review"),
                        )
                        .unwrap();
                }
            }
            service.refresh().unwrap();
        }
        home
    }

    /// The composed entries the encounter would carry, resolved through the
    /// one application engine exactly as the open path resolves them.
    fn composed_entries(home: &AikitHome, tmp: &TempDir) -> Vec<ToolSourceEntry> {
        let world = tmp.path().join("world");
        let service = crate::app::Service::open(home.clone(), &world, |_: &str| None).unwrap();
        crate::encounter_mcp::tool_source_entries_from_service(&service).unwrap()
    }

    /// The temp-rooted native seam, expanded the way the profile's
    /// `~/.openclaw/mcp.json` declaration expands against `machine_home`.
    fn seeded_seam(machine_home: &Path, contents: &str) -> PathBuf {
        let seam = machine_home.join(".openclaw/mcp.json");
        fs::create_dir_all(seam.parent().unwrap()).unwrap();
        fs::write(&seam, contents).unwrap();
        seam
    }

    fn executed(projection: NativeToolsProjection) -> ToolsLayerOutcome {
        let NativeToolsProjection::Executed(outcome) = projection else {
            panic!("the declared seam took the projection: {projection:?}");
        };
        *outcome
    }

    fn boundary_reason(projection: NativeToolsProjection) -> String {
        let NativeToolsProjection::Boundary { reason } = projection else {
            panic!("the projection must stay at the boundary: {projection:?}");
        };
        reason
    }

    fn read_seam(seam: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(seam).unwrap()).expect("the seam is valid JSON")
    }

    // -- the golden write ----------------------------------------------------

    #[test]
    fn a_managed_seam_takes_the_composed_records_and_preserves_the_foreign_entries() {
        let tmp = TempDir::new().unwrap();
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest(), linear_manifest()],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            true,
        );
        let entries = composed_entries(&home, &tmp);
        assert_eq!(entries.len(), 2, "both trusted capsules compose");

        let machine_home = tmp.path().join("machine");
        let seam = seeded_seam(
            &machine_home,
            r#"{
  "mcpServers": {
    "linear-server": { "url": "https://mcp.linear.app/sse" }
  },
  "editor": { "font": "iosevka" }
}"#,
        );
        let world = tmp.path().join("world");
        let projection = project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &entries,
        );
        let outcome = executed(projection);

        assert_eq!(outcome.state, "written", "the merge wrote the seam");
        assert_eq!(outcome.path.as_deref(), Some(seam.to_str().unwrap()));
        assert!(
            outcome.procedure.is_some() && outcome.undo.is_some(),
            "the write went through the Procedure pipeline, never a bare file write: {outcome:?}"
        );
        assert_eq!(outcome.added, Some(2));
        assert_eq!(outcome.removed, Some(0));
        assert_eq!(outcome.kept_foreign, Some(1));
        let written = outcome.written_entries.clone().unwrap();
        assert!(written.contains(&"bimba".to_string()) && written.contains(&"linear".to_string()));
        assert_eq!(
            serde_json::to_value(outcome.activation).unwrap(),
            json!("restart-client"),
            "the activation is the profile's own tools-layer truth"
        );

        let merged = read_seam(&seam);
        assert_eq!(
            merged["mcpServers"]["bimba"],
            json!({
                "command": "aikit tool-protocol serve bimba",
                "args": ["--port", "8080"],
                "env": { "BIMBA_TOKEN": "sk-test" }
            }),
            "the composed capsule record lands whole"
        );
        assert_eq!(
            merged["mcpServers"]["linear"],
            json!({ "url": "https://mcp.example/sse" })
        );
        assert_eq!(
            merged["mcpServers"]["linear-server"],
            json!({ "url": "https://mcp.linear.app/sse" }),
            "the foreign server is untouched"
        );
        assert_eq!(
            merged["editor"],
            json!({ "font": "iosevka" }),
            "unrelated keys are untouched"
        );

        // The journal and receipt fragments disclose the same execution.
        let outcome = NativeToolsProjection::Executed(Box::new(outcome));
        let event = outcome.journal_event("openclaw-acp", Some("openclaw"));
        assert_eq!(event["kind"], "native-mcp-native-projection-written");
        assert_eq!(event["entries_written"], 2);
        let receipt = outcome.execution_receipt(Some("openclaw"));
        assert_eq!(receipt["state"], "written");
        assert_eq!(receipt["activation"], "restart-client");
    }

    #[test]
    fn a_second_identical_application_is_satisfied_and_claims_no_new_procedure() {
        let tmp = TempDir::new().unwrap();
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &["tool-protocol/test/bimba"],
            true,
        );
        let entries = composed_entries(&home, &tmp);
        let machine_home = tmp.path().join("machine");
        let world = tmp.path().join("world");
        let first = executed(project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &entries,
        ));
        let second = executed(project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &entries,
        ));

        assert_eq!(second.state, "satisfied");
        assert!(
            second.procedure.is_none(),
            "nothing was written, so no procedure runs and no receipt is claimed: {second:?}"
        );
        assert_eq!(second.path, first.path);
    }

    // -- the sweep -----------------------------------------------------------

    #[test]
    fn a_second_application_with_a_removed_capsule_sweeps_the_stale_owned_entry() {
        let tmp = TempDir::new().unwrap();
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest(), linear_manifest()],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            true,
        );
        let machine_home = tmp.path().join("machine");
        let seam = seeded_seam(
            &machine_home,
            r#"{
  "mcpServers": {
    "linear-server": { "url": "https://mcp.linear.app/sse" }
  }
}"#,
        );
        let world = tmp.path().join("world");
        let all_entries = composed_entries(&home, &tmp);
        let first = executed(project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &all_entries,
        ));
        assert_eq!(first.state, "written");

        // The bimba capsule left the composed set: the same home now enables
        // only the linear capsule, so the one application engine resolves
        // only it.
        let reduced_home = home_with_capsules(
            &tmp,
            &[linear_manifest()],
            &["tool-protocol/test/linear"],
            &["tool-protocol/test/linear"],
            true,
        );
        let reduced_entries = composed_entries(&reduced_home, &tmp);
        assert_eq!(
            reduced_entries.len(),
            1,
            "only the linear capsule composes now"
        );

        let second = executed(project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &reduced_entries,
        ));

        assert_eq!(second.state, "written", "the sweep is real retraction work");
        assert_eq!(second.removed, Some(1));
        assert_eq!(
            second.swept_entries.as_deref(),
            Some(["bimba".to_string()].as_slice()),
            "the stale AIKit-owned record leaves the document"
        );
        let merged = read_seam(&seam);
        assert!(
            merged["mcpServers"].get("bimba").is_none(),
            "the swept record is gone: {merged}"
        );
        assert_eq!(
            merged["mcpServers"]["linear"],
            json!({ "url": "https://mcp.example/sse" }),
            "the still-composed record stays"
        );
        assert_eq!(
            merged["mcpServers"]["linear-server"],
            json!({ "url": "https://mcp.linear.app/sse" }),
            "the foreign record survives the sweep"
        );
    }

    // -- the boundary --------------------------------------------------------

    #[test]
    fn a_brokered_tools_layer_never_writes_and_the_boundary_says_why() {
        let tmp = TempDir::new().unwrap();
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &["tool-protocol/test/bimba"],
            true,
        );
        let entries = composed_entries(&home, &tmp);
        assert_eq!(entries.len(), 1);

        let machine_home = tmp.path().join("machine");
        let seam = seeded_seam(
            &machine_home,
            r#"{ "mcpServers": { "own": { "command": "own-server" } } }"#,
        );
        let before = fs::read_to_string(&seam).unwrap();
        let world = tmp.path().join("world");

        // pi's tools layer is brokered: the harness owns its native state.
        let projection = project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("pi"),
            &entries,
        );
        let reason = boundary_reason(projection);

        assert!(
            reason.contains("pi") && reason.contains("brokered"),
            "the boundary names the profile and the posture: {reason}"
        );
        assert_eq!(
            fs::read_to_string(&seam).unwrap(),
            before,
            "an observed or brokered tools layer is never written"
        );

        let event = NativeToolsProjection::Boundary {
            reason: reason.clone(),
        }
        .journal_event("pi-rpc", Some("pi"));
        assert_eq!(event["kind"], "native-mcp-native-projection-boundary");
        assert_eq!(event["reason"], reason.clone());
        let receipt = NativeToolsProjection::Boundary { reason }.execution_receipt(Some("pi"));
        assert_eq!(receipt["state"], "not-writable");
    }

    #[test]
    fn a_provider_without_a_profile_is_a_boundary_never_a_guess() {
        let tmp = TempDir::new().unwrap();
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &["tool-protocol/test/bimba"],
            true,
        );
        let entries = composed_entries(&home, &tmp);
        let machine_home = tmp.path().join("machine");
        let world = tmp.path().join("world");

        let projection =
            project_composed_tools_to_native_seam(&home, &world, &machine_home, None, &entries);
        let reason = boundary_reason(projection);
        assert!(
            reason.contains("no harness profile") && reason.contains("declared seam"),
            "the boundary says no seam is declared: {reason}"
        );
        assert!(
            !machine_home.join(".openclaw").exists(),
            "nothing is written anywhere"
        );
    }

    // -- the trust law -------------------------------------------------------

    #[test]
    fn an_untrusted_capsule_never_reaches_the_file() {
        let tmp = TempDir::new().unwrap();
        // Enabled but never reviewed: the composition resolves nothing.
        let home = home_with_capsules(
            &tmp,
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &["tool-protocol/test/bimba"],
            false,
        );
        let entries = composed_entries(&home, &tmp);
        assert!(
            entries.is_empty(),
            "trust is never self-declared; an unreviewed capsule does not compose: {entries:?}"
        );

        let machine_home = tmp.path().join("machine");
        let world = tmp.path().join("world");
        let projection = project_composed_tools_to_native_seam(
            &home,
            &world,
            &machine_home,
            Some("openclaw"),
            &entries,
        );
        let reason = boundary_reason(projection);
        assert!(
            reason.contains("not trusted") || reason.contains("no tool-protocol capsule"),
            "the boundary carries the no-composition truth: {reason}"
        );
        assert!(
            !machine_home.join(".openclaw/mcp.json").exists(),
            "a missing seam stays missing when nothing composed — the capsule's record \
             never reaches any file"
        );
    }

    // -- journal + receipt shapes --------------------------------------------

    #[test]
    fn a_failed_write_journals_an_error_event_that_names_the_failure() {
        let failure = NativeToolsProjection::Failed {
            reason: "the existing tool configuration at ~/.openclaw/mcp.json is not valid \
                     JSON"
                .to_string(),
        };
        let event = failure.journal_event("gemini-acp", Some("gemini"));
        assert_eq!(event["kind"], "native-mcp-native-projection-failed");
        assert!(
            event["standing"]
                .as_str()
                .is_some_and(|s| s.contains("the session may still open")),
            "the failure says the open is not held hostage by the projection: {event}"
        );
        let receipt = failure.execution_receipt(Some("gemini"));
        assert_eq!(receipt["state"], "failed");
        assert_eq!(receipt["harness_profile"], "gemini");
    }

    #[test]
    fn every_journal_event_names_the_native_seam_route() {
        for projection in [
            NativeToolsProjection::Failed {
                reason: "x".to_string(),
            },
            NativeToolsProjection::Boundary {
                reason: "y".to_string(),
            },
        ] {
            assert_eq!(
                projection.journal_event("p", None)["composed_tools_route"],
                "harness-native-mcp-config-seam-not-the-session-wire",
                "every event keeps the route the fallback record declared"
            );
        }
    }
}
