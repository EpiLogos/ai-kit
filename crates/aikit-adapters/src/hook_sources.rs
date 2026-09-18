//! The managed hooks-layer projection for pi: the resolved extension-carrier
//! capability planned into pi's native `extensions` registration through the
//! one layer merge engine ([`crate::layers`]).
//!
//! Pi has no shell-command hook seam, so the managed hooks layer does not
//! write dispatcher entries the way the Claude/zcode grammars do. What it
//! projects is the *carrier*: one first-party TypeScript extension whose
//! handlers translate pi's native events into
//! `aikit hook dispatch pi <AIKitEvent>`. The carrier file is
//! content-addressed (`aikit-hook-carrier-<sha256-12>.ts`) so a re-projected
//! revision is a fresh file pi cannot serve from a stale module cache, and
//! the settings `extensions` array carries exactly one owned entry — the
//! current carrier — swept and replaced by ownership marker on re-projection.
//!
//! The invariant this module owns is *posture truth at plan time*, joined to
//! the trust gate. A carrier is planned only where the harness profile
//! records a `managed` hooks layer naming a `pi-extensions-record` seam AND
//! the caller resolved the carrier capsule as active — which the resolver
//! only yields for a trust-recorded revision (`Kind::Hook` requires trust to
//! activate). A profile without the seam is [`HooksProjectionOutcome::
//! NotProjected`]; an inactive carrier — untrusted, blocked, or simply not
//! enabled — is [`HooksProjectionOutcome::Swept`]: every owned entry leaves
//! the settings array and every owned carrier file leaves the projection
//! directory, so pi never loads a revision the owner has not reviewed.
//! Foreign extension entries are never touched.
//!
//! Like [`crate::tool_sources`], the planner never touches the filesystem:
//! the existing settings document arrives through an injected reader and the
//! projection directory's contents through an injected lister, so a missing
//! file is a fresh document and an unreadable one is a refusal naming the
//! path. Reports and errors carry names, paths and problems only, because
//! native config files carry secrets.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use aikit_core::harness_profile::{
    ActivationEffectName, HarnessProfile, LayerPosture, MergeGrammar,
};
use aikit_core::projection::ProjectionItem;

use crate::layers::{apply_merge, LayerMergeError, MergeArgs, MergeReport};

/// The ownership marker every AIKit-projected carrier entry and file carries.
/// The merge grammar matches it in an extension entry's file name, and the
/// file name itself is `{marker}-{content hash}.ts`, so the marker is both
/// the sweep identity and the file naming scheme.
pub const HOOKS_PROJECTION_OWNERSHIP: &str = "aikit-hook-carrier";

/// The first-party capsule the carrier projection ships as. A `hook` kind,
/// so the resolver's trust gate binds it: only a trust-recorded revision is
/// active, and only an active carrier is ever projected.
pub const CARRIER_CAPSULE_ID: &str = "hook/aikit/pi-extension-carrier";

/// How many hex characters of the payload digest name the carrier file.
const HASH_LEN: usize = 12;

/// One resolved, active extension-carrier capability, ready to be projected:
/// the TypeScript payload exactly as the capsule ships it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCarrierSource {
    pub payload: String,
}

impl HookCarrierSource {
    /// The content-addressed file name for this payload: the ownership
    /// marker plus the leading digest characters, so a new revision is a
    /// new file and a re-projection is observable in a directory listing.
    pub fn file_name(&self) -> String {
        let digest = Sha256::digest(self.payload.as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        format!(
            "{HOOKS_PROJECTION_OWNERSHIP}-{hex}.ts",
            hex = &hex[..HASH_LEN]
        )
    }
}

/// The directory AIKit's carrier files live in, in its two honest spellings.
///
/// `absolute` is the form pi's `extensions` entry must carry: pi resolves a
/// relative settings path against its cwd, which would make the carrier
/// load depend on where the session started. `declared` is the same
/// directory home-relative (`~/.aikit/...`), the spelling every profile
/// declaration uses so plans survive machines; the carrier write item uses
/// it, because a plan's write destinations are relative or home-relative by
/// law and the applying caller expands `~` against its own home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionDir {
    pub absolute: PathBuf,
    pub declared: String,
}

impl ProjectionDir {
    pub fn new(absolute: impl Into<PathBuf>, declared: impl Into<String>) -> Self {
        Self {
            absolute: absolute.into(),
            declared: declared.into(),
        }
    }
}

/// What projecting a harness's hooks layer would do. `Projected` carries the
/// carrier plan; `Swept` carries the deactivation plan; `NotProjected` says
/// plainly why nothing would be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HooksProjectionOutcome {
    Projected(HooksProjectionPlan),
    Swept(HooksSweepPlan),
    NotProjected { reason: String },
}

/// One managed hooks-layer projection: write the content-addressed carrier
/// file, merge the registration into the settings document, and name the
/// stale owned files the caller should delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksProjectionPlan {
    /// The native settings file the profile's project declaration names.
    pub settings_path: String,
    /// The projected carrier's absolute path, content-addressed.
    pub carrier_path: PathBuf,
    /// The write that places the carrier payload at `carrier_path`.
    pub carrier_item: ProjectionItem,
    /// Owned carrier files already in the projection directory that this
    /// plan replaces; the caller deletes them when it applies the plan.
    pub stale_carrier_files: Vec<PathBuf>,
    /// The write that updates the settings registration.
    pub settings_item: ProjectionItem,
    /// What the settings merge did — added, replaced, removed, kept foreign.
    pub report: MergeReport,
    /// When pi sees the change, as the profile declares it.
    pub activation: Option<ActivationEffectName>,
}

/// The deactivation plan for an inactive carrier: sweep the registration and
/// name the owned files to delete. pi then loads no AIKit extension at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksSweepPlan {
    pub settings_path: String,
    pub settings_item: ProjectionItem,
    pub stale_carrier_files: Vec<PathBuf>,
    pub report: MergeReport,
}

/// A hooks-layer projection refusal. `code` is stable machine surface in the
/// `projection.*` namespace; the message names the slug or the path and what
/// would fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSourceError {
    code: &'static str,
    message: String,
    details: std::collections::BTreeMap<String, String>,
}

impl HookSourceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: std::collections::BTreeMap::new(),
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

    pub fn details(&self) -> &std::collections::BTreeMap<String, String> {
        &self.details
    }
}

impl std::fmt::Display for HookSourceError {
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

impl std::error::Error for HookSourceError {}

impl From<HookSourceError> for aikit_core::AikitError {
    fn from(error: HookSourceError) -> Self {
        let mut aikit_error = aikit_core::AikitError::new(error.code, error.message);
        for (key, value) in error.details {
            aikit_error = aikit_error.with(key, value);
        }
        aikit_error
    }
}

impl From<LayerMergeError> for HookSourceError {
    fn from(error: LayerMergeError) -> Self {
        let mut hook_error = HookSourceError::new(error.code(), error.message());
        for (key, value) in error.details() {
            hook_error = hook_error.with(key.clone(), value.clone());
        }
        hook_error
    }
}

impl From<aikit_core::AikitError> for HookSourceError {
    fn from(error: aikit_core::AikitError) -> Self {
        let mut hook_error = HookSourceError::new(error.code(), error.message());
        for (key, value) in error.details() {
            hook_error = hook_error.with(key.clone(), value.clone());
        }
        hook_error
    }
}

/// Plan the managed hooks-layer projection for one harness profile.
///
/// `carrier` is `Some` exactly when the resolver yielded the carrier capsule
/// as an active capability — the trust gate lives in that resolution, not
/// here. `projection_dir` names the AIKit-owned directory the
/// content-addressed carrier files live in, in both spellings (see
/// [`ProjectionDir`]); `read_existing` reads the settings document the
/// profile's project declaration names; `list_projected` lists the carrier
/// directory's file names so the plan can name the stale files to delete.
pub fn plan_hooks_projection(
    carrier: Option<HookCarrierSource>,
    profile: &HarnessProfile,
    projection_dir: &ProjectionDir,
    read_existing: impl Fn(&str) -> std::io::Result<Option<String>>,
    list_projected: impl Fn(&Path) -> std::io::Result<Vec<String>>,
) -> Result<HooksProjectionOutcome, HookSourceError> {
    let Some(hooks) = profile.hooks.as_ref() else {
        return Ok(HooksProjectionOutcome::NotProjected {
            reason: format!(
                "the {} profile declares no hooks layer; there is no seam AIKit may \
                 project the extension carrier into",
                profile.slug
            ),
        });
    };
    if hooks.posture != LayerPosture::Managed {
        return Ok(HooksProjectionOutcome::NotProjected {
            reason: format!(
                "the {} profile's hooks layer posture is {}; AIKit projects the \
                 extension carrier only into a managed hooks layer",
                profile.slug, hooks.posture
            ),
        });
    }
    // Schema validation forbids a managed layer without a project, so this is
    // an encountered-impossible state; it refuses naming the slug all the same.
    let Some(project) = hooks.project.as_ref() else {
        return Err(HookSourceError::new(
            "projection.hooks_managed_without_project",
            format!(
                "the {} profile's hooks layer posture is managed but declares no \
                 project; a managed layer must name the seam it projects into — add \
                 the profile's `project` declaration (file, format, \
                 ownership-identity) or change the posture",
                profile.slug
            ),
        )
        .with("slug", profile.slug.clone())
        .with("layer", "hooks"));
    };
    if project.format != MergeGrammar::PiExtensionsRecord {
        return Err(HookSourceError::new(
            "projection.hooks_grammar_unsupported",
            format!(
                "the {} profile's hooks layer projects through the {:?} grammar, which \
                 the hooks-layer projection does not implement; set the project format \
                 to \"pi-extensions-record\" or add the grammar to the layer merge engine",
                profile.slug, project.format
            ),
        )
        .with("slug", profile.slug.clone())
        .with("format", format!("{:?}", project.format)));
    }

    let existing = read_existing(&project.file).map_err(|error| {
        HookSourceError::new(
            "projection.hooks_target_unreadable",
            format!(
                "the {} profile's hook registration at {} could not be read: {error}; \
                 fix the read before projecting — AIKit will not overwrite a file it \
                 cannot read",
                profile.slug, project.file
            ),
        )
        .with("path", project.file.clone())
    })?;
    let document = match existing {
        None => serde_json::json!({}),
        Some(raw) if raw.trim().is_empty() => serde_json::json!({}),
        Some(raw) => serde_json::from_str(&raw).map_err(|error| {
            HookSourceError::new(
                "projection.hooks_document_unreadable",
                format!(
                    "the existing hook registration at {} is not valid JSON: {error}; \
                     AIKit will not overwrite a file it cannot read",
                    project.file
                ),
            )
            .with("path", project.file.clone())
        })?,
    };

    let projection_dir_abs = &projection_dir.absolute;
    let projected_files = list_projected(projection_dir_abs).map_err(|error| {
        HookSourceError::new(
            "projection.hooks_projection_dir_unreadable",
            format!(
                "the carrier projection directory {} could not be listed: {error}; fix \
                 the read before projecting — AIKit will not sweep files it cannot see",
                projection_dir_abs.display()
            ),
        )
        .with("path", projection_dir_abs.display().to_string())
    })?;

    let managed = carrier
        .as_ref()
        .map(|source| projection_dir_abs.join(source.file_name()))
        .map(|path| path.to_string_lossy().into_owned());
    let (merged, report) = apply_merge(
        MergeGrammar::PiExtensionsRecord,
        &document,
        MergeArgs::PiExtensionsRecord {
            key_path: vec!["extensions".to_string()],
            managed: managed.clone(),
            ownership: project.ownership_identity.clone(),
        },
    )?;

    let current_name = carrier.as_ref().map(HookCarrierSource::file_name);
    let stale = |owned_only: bool| -> Vec<PathBuf> {
        projected_files
            .iter()
            .filter(|name| name.starts_with(&project.ownership_identity) && name.ends_with(".ts"))
            .filter(|name| !owned_only || Some(name.as_str()) != current_name.as_deref())
            .map(|name| projection_dir_abs.join(name))
            .collect()
    };

    let mut contents = serde_json::to_string_pretty(&merged).map_err(|error| {
        HookSourceError::new(
            "projection.hooks_unrenderable",
            format!(
                "the merged {} hook registration could not be rendered: {error}",
                project.file
            ),
        )
        .with("path", project.file.clone())
    })?;
    contents.push('\n');
    let settings_item = ProjectionItem::write(&project.file, contents)?;

    match carrier {
        Some(source) => {
            let carrier_path = projection_dir_abs.join(source.file_name());
            let carrier_item = ProjectionItem::write(
                Path::new(&projection_dir.declared).join(source.file_name()),
                source.payload,
            )?;
            Ok(HooksProjectionOutcome::Projected(HooksProjectionPlan {
                settings_path: project.file.clone(),
                carrier_path,
                carrier_item,
                stale_carrier_files: stale(true),
                settings_item,
                report,
                activation: hooks.activation,
            }))
        }
        None => Ok(HooksProjectionOutcome::Swept(HooksSweepPlan {
            settings_path: project.file.clone(),
            settings_item,
            stale_carrier_files: stale(false),
            report,
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    const CARRIER_V1: &str = "// aikit carrier revision 1\nexport default () => {};\n";
    const CARRIER_V2: &str = "// aikit carrier revision 2\nexport default () => {};\n";

    fn pi_profile() -> &'static HarnessProfile {
        crate::profiles::for_slug("pi").expect("pi carries an embedded profile")
    }

    fn carrier(payload: &str) -> Option<HookCarrierSource> {
        Some(HookCarrierSource {
            payload: payload.to_string(),
        })
    }

    /// Plan against a real temp home: the settings file seeded at
    /// `<home>/.pi/agent/settings.json`, the projection directory at
    /// `<home>/.aikit/ctx/projections/pi` pre-populated with `seeded`
    /// carrier files. Returns the outcome, the seeded paths, and every
    /// path the reader and lister were asked for.
    fn plan_in_temp_home(
        existing: Option<&str>,
        seeded: &[&str],
        payload: Option<&str>,
        profile: &HarnessProfile,
    ) -> (HooksProjectionOutcome, Vec<PathBuf>, Vec<String>) {
        let home = tempdir().expect("tempdir");
        let settings = home.path().join(".pi/agent/settings.json");
        if let Some(contents) = existing {
            fs::create_dir_all(settings.parent().unwrap()).expect("settings parent");
            fs::write(&settings, contents).expect("seed the existing settings");
        }
        let projection_dir = home.path().join(".aikit/ctx/projections/pi");
        fs::create_dir_all(&projection_dir).expect("projection dir");
        for name in seeded {
            fs::write(projection_dir.join(name), "stale").expect("seed carrier file");
        }
        let asked = RefCell::new(Vec::new());
        let projection = ProjectionDir::new(&projection_dir, "~/.aikit/ctx/projections/pi");
        let outcome = plan_hooks_projection(
            payload.map(|p| HookCarrierSource {
                payload: p.to_string(),
            }),
            profile,
            &projection,
            |asked_path| {
                asked.borrow_mut().push(asked_path.to_string());
                if settings.is_file() {
                    fs::read_to_string(&settings).map(Some)
                } else {
                    Ok(None)
                }
            },
            |dir| {
                fs::read_dir(dir).map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .collect()
                })
            },
        )
        .expect("the projection plans against the seeded home");
        (outcome, vec![settings], asked.into_inner())
    }

    fn settings_contents(outcome: &HooksProjectionOutcome) -> String {
        let item = match outcome {
            HooksProjectionOutcome::Projected(plan) => &plan.settings_item,
            HooksProjectionOutcome::Swept(plan) => &plan.settings_item,
            HooksProjectionOutcome::NotProjected { reason } => {
                panic!("the hooks layer projects or sweeps: {reason}")
            }
        };
        let ProjectionItem::Write { contents, .. } = item else {
            panic!("the settings plan is a write: {item:?}");
        };
        contents.clone()
    }

    fn carrier_file_name(payload: &str) -> String {
        HookCarrierSource {
            payload: payload.to_string(),
        }
        .file_name()
    }

    #[test]
    fn the_pi_profile_declares_a_managed_hooks_layer_through_the_carrier_grammar() {
        let hooks = pi_profile()
            .hooks
            .as_ref()
            .expect("the 2026-09-18 census gives pi a hooks layer");
        assert_eq!(hooks.posture, LayerPosture::Managed);
        let project = hooks.project.as_ref().expect("managed names its seam");
        assert_eq!(project.file, "~/.pi/agent/settings.json");
        assert_eq!(project.format, MergeGrammar::PiExtensionsRecord);
        assert_eq!(project.ownership_identity, HOOKS_PROJECTION_OWNERSHIP);
        assert_eq!(
            hooks.activation,
            Some(ActivationEffectName::NextSessionOnly)
        );
        pi_profile()
            .validate()
            .expect("the managed pi hooks layer satisfies posture truth");
    }

    #[test]
    fn an_active_carrier_is_placed_content_addressed_and_registered_once() {
        let (outcome, _settings, asked) = plan_in_temp_home(
            Some("{\n  \"theme\": \"dark\"\n}"),
            &[],
            Some(CARRIER_V1),
            pi_profile(),
        );

        let HooksProjectionOutcome::Projected(plan) = &outcome else {
            panic!("an active carrier projects: {outcome:?}");
        };
        assert_eq!(
            asked,
            vec!["~/.pi/agent/settings.json".to_string()],
            "the planner reads exactly the path the profile's project declaration names"
        );
        assert_eq!(plan.settings_path, "~/.pi/agent/settings.json");
        assert_eq!(
            plan.carrier_path.file_name().and_then(|name| name.to_str()),
            Some(carrier_file_name(CARRIER_V1).as_str()),
        );
        assert!(plan.stale_carrier_files.is_empty());
        assert_eq!(
            plan.report.added,
            vec![carrier_file_name(CARRIER_V1)],
            "the registration is added exactly once"
        );

        let ProjectionItem::Write { path, contents } = &plan.carrier_item else {
            panic!("the carrier plan is a write: {:?}", plan.carrier_item);
        };
        assert_eq!(contents, CARRIER_V1);
        assert_eq!(
            path.as_path(),
            Path::new("~/.aikit/ctx/projections/pi")
                .join(carrier_file_name(CARRIER_V1))
                .as_path(),
            "the write destination is home-relative; the registration carries the \
             absolute path"
        );
        assert!(
            plan.carrier_path.is_absolute(),
            "the registration path is the absolute spelling: {:?}",
            plan.carrier_path
        );

        let merged: serde_json::Value =
            serde_json::from_str(&settings_contents(&outcome)).expect("contents parse");
        assert_eq!(
            merged["extensions"],
            serde_json::json!([plan.carrier_path.to_string_lossy().into_owned()]),
            "the settings array carries exactly the current carrier path"
        );
        assert_eq!(merged["theme"], serde_json::json!("dark"));
    }

    #[test]
    fn a_reprojected_revision_sweeps_the_old_file_and_registration_not_accumulates() {
        let old_name = carrier_file_name(CARRIER_V1);
        let (outcome, _settings, _) = plan_in_temp_home(
            Some(&format!(
                "{{\"extensions\": [\"/home/x/.aikit/ctx/projections/pi/{old_name}\"]}}"
            )),
            &[old_name.as_str()],
            Some(CARRIER_V2),
            pi_profile(),
        );

        let HooksProjectionOutcome::Projected(plan) = &outcome else {
            panic!("a re-projection projects: {outcome:?}");
        };
        assert_eq!(
            plan.stale_carrier_files,
            vec![plan.carrier_path.parent().unwrap().join(old_name.as_str())],
            "the old revision's file is named for deletion"
        );
        assert_eq!(
            plan.report.removed,
            vec![old_name],
            "the old registration entry is swept"
        );
        assert_eq!(
            plan.report.replaced,
            vec![carrier_file_name(CARRIER_V2)],
            "the fresh revision replaces, never accumulates"
        );

        let merged: serde_json::Value =
            serde_json::from_str(&settings_contents(&outcome)).expect("contents parse");
        let entries = merged["extensions"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "exactly one carrier entry: {merged}");
    }

    #[test]
    fn a_swept_carrier_leaves_no_owned_entry_and_names_every_owned_file() {
        let (outcome, _settings, _) = plan_in_temp_home(
            None,
            &[carrier_file_name(CARRIER_V1).as_str()],
            None,
            pi_profile(),
        );

        let HooksProjectionOutcome::Swept(plan) = &outcome else {
            panic!("an absent carrier sweeps: {outcome:?}");
        };
        let merged: serde_json::Value =
            serde_json::from_str(&settings_contents(&outcome)).expect("contents parse");
        assert!(
            merged
                .get("extensions")
                .is_none_or(|e| e.as_array().is_none_or(Vec::is_empty)),
            "no owned entry survives the sweep: {merged}"
        );
        assert_eq!(
            plan.stale_carrier_files.len(),
            1,
            "the owned file is named for deletion: {:?}",
            plan.stale_carrier_files
        );
        assert!(plan.report.kept_foreign.is_empty());
    }

    #[test]
    fn foreign_extensions_survive_projection_and_sweep_alike() {
        let existing = r#"{
  "theme": "dark",
  "extensions": ["/Users/admin/my-extensions/grammars.ts"]
}"#;
        let (projected, _, _) =
            plan_in_temp_home(Some(existing), &[], Some(CARRIER_V1), pi_profile());
        let merged: serde_json::Value =
            serde_json::from_str(&settings_contents(&projected)).expect("contents parse");
        assert_eq!(
            merged["extensions"][0],
            serde_json::json!("/Users/admin/my-extensions/grammars.ts"),
            "the foreign extension rides through projection untouched"
        );

        let (swept, _, _) = plan_in_temp_home(Some(existing), &[], None, pi_profile());
        let merged: serde_json::Value =
            serde_json::from_str(&settings_contents(&swept)).expect("contents parse");
        assert_eq!(
            merged["extensions"],
            serde_json::json!(["/Users/admin/my-extensions/grammars.ts"]),
            "the foreign extension rides through sweep untouched"
        );
    }

    #[test]
    fn an_unmanaged_hooks_layer_is_not_projected_and_the_reason_names_the_posture() {
        let mut profile = pi_profile().clone();
        profile.hooks.as_mut().unwrap().posture = LayerPosture::Observed;
        // Posture truth: an observed layer must not declare a project.
        profile.hooks.as_mut().unwrap().project = None;

        let (outcome, _, _) = plan_in_temp_home(None, &[], Some(CARRIER_V1), &profile);

        let HooksProjectionOutcome::NotProjected { reason } = &outcome else {
            panic!("an observed hooks layer must not project: {outcome:?}");
        };
        assert!(
            reason.contains("observed"),
            "the reason names the posture: {reason}"
        );
    }

    #[test]
    fn a_profile_without_a_hooks_layer_is_not_projected_and_says_so() {
        let mut profile = pi_profile().clone();
        profile.hooks = None;

        let (outcome, _, _) = plan_in_temp_home(None, &[], Some(CARRIER_V1), &profile);

        let HooksProjectionOutcome::NotProjected { reason } = &outcome else {
            panic!("a profile without a hooks layer must not project: {outcome:?}");
        };
        assert!(
            reason.contains("no hooks layer"),
            "the reason says there is no seam: {reason}"
        );
    }

    #[test]
    fn a_managed_hooks_layer_with_a_non_carrier_grammar_refuses_rather_than_guessing() {
        let mut profile = pi_profile().clone();
        profile
            .hooks
            .as_mut()
            .unwrap()
            .project
            .as_mut()
            .unwrap()
            .format = MergeGrammar::ClaudeHookMap;

        let error = plan_hooks_projection(
            carrier(CARRIER_V1),
            &profile,
            &ProjectionDir::new("/tmp/projections", "~/.aikit/ctx/projections/pi"),
            |_| Ok(None),
            |_| Ok(vec![]),
        )
        .unwrap_err();

        assert_eq!(error.code(), "projection.hooks_grammar_unsupported");
        assert!(
            error.to_string().contains("pi"),
            "the refusal names the slug: {error}"
        );
    }

    #[test]
    fn an_unreadable_settings_file_refuses_naming_the_path() {
        let error = plan_hooks_projection(
            carrier(CARRIER_V1),
            pi_profile(),
            &ProjectionDir::new("/tmp/projections", "~/.aikit/ctx/projections/pi"),
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            |_| Ok(vec![]),
        )
        .unwrap_err();

        assert_eq!(error.code(), "projection.hooks_target_unreadable");
        assert!(
            error.to_string().contains("~/.pi/agent/settings.json"),
            "the refusal names the path it could not read: {error}"
        );
    }

    #[test]
    fn an_existing_file_that_is_not_json_refuses_naming_the_path_rather_than_overwriting() {
        let error = plan_hooks_projection(
            carrier(CARRIER_V1),
            pi_profile(),
            &ProjectionDir::new("/tmp/projections", "~/.aikit/ctx/projections/pi"),
            |_| Ok(Some("{ this is not json".to_string())),
            |_| Ok(vec![]),
        )
        .unwrap_err();

        assert_eq!(error.code(), "projection.hooks_document_unreadable");
        assert!(
            error.to_string().contains("~/.pi/agent/settings.json"),
            "the refusal names the path: {error}"
        );
    }

    #[test]
    fn an_unlistable_projection_directory_refuses_rather_than_sweeping_blind() {
        let error = plan_hooks_projection(
            carrier(CARRIER_V1),
            pi_profile(),
            &ProjectionDir::new("/tmp/projections", "~/.aikit/ctx/projections/pi"),
            |_| Ok(None),
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        )
        .unwrap_err();

        assert_eq!(error.code(), "projection.hooks_projection_dir_unreadable");
    }

    #[test]
    fn a_carrier_file_name_is_a_pure_function_of_the_payload_and_never_a_directory() {
        let name = carrier_file_name(CARRIER_V1);
        assert!(name.starts_with(HOOKS_PROJECTION_OWNERSHIP));
        assert!(name.ends_with(".ts"));
        assert!(
            !name.contains('/'),
            "the file name is a leaf, not a path: {name}"
        );
        assert_eq!(
            name,
            carrier_file_name(CARRIER_V1),
            "same payload, same name"
        );
        assert_ne!(
            name,
            carrier_file_name(CARRIER_V2),
            "a new revision is a new file"
        );
    }
}
