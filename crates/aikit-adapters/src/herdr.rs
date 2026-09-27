//! Herdr working-environment provider for O:I composable inhabitation.
//!
//! Herdr is consumed through its public CLI/API surface. Workspace, tab, pane and
//! recognised-agent ids remain provider-native evidence; canonical AIKit refs are
//! supplied only by explicit bindings.

use std::collections::BTreeMap;

use aikit_core::resource::ResourceRef;
use aikit_core::session::{Direction, SessionPlan, Split};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runner::CommandRunner;
use crate::working_environment::{
    NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
    WorkingEnvironmentHealth, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};

/// The upstream herdr source revision whose wire shapes this adapter was
/// first written against (fixtures captured at its API protocol 7). The live
/// herdr 0.8.2 server speaks API protocol 20 and proved field-compatible on
/// 2026-09-18; the observation provenance below still records this pin so
/// drift between the pin and a live daemon stays discoverable.
pub const HERDR_UPSTREAM_REVISION: &str = "94f6d9c0d9bb9cf9ffae99d8bbfb09e9bf2fc9e0";
pub const HERDR_PROVIDER_VERSION: &str = "aikit.herdr-working-environment/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrAgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrSplitDirection {
    Right,
    Down,
}

impl HerdrSplitDirection {
    fn as_cli(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrAgentObservation {
    pub native_id: String,
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub status: HerdrAgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrWorkspaceCreation {
    pub workspace_id: String,
    pub tab_id: String,
    pub root_pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrStartedAgent {
    pub terminal_id: String,
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub status: HerdrAgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrSnapshot {
    pub version: String,
    pub protocol: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<String>,
    pub workspace_ids: Vec<String>,
    pub pane_ids: Vec<String>,
    pub agents: Vec<HerdrAgentObservation>,
    /// Each tab layout's workspace and its own focused pane, as Herdr reports
    /// them (`layouts[]`). Lets a surface bound to a workspace's focused pane
    /// be focused exactly through `workspace focus`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layouts: Vec<HerdrLayoutFocus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrLayoutFocus {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<String>,
    pub pane_ids: Vec<String>,
    /// Each pane of the layout with its reported rect, when Herdr publishes
    /// one. The multi-surface reconcile reads the parent pane's rect before
    /// splitting so a pane is never minted invisibly small.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panes: Vec<HerdrLayoutPane>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrLayoutPane {
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<HerdrPaneRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrPaneRect {
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub y: i64,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn object_ids(snapshot: &Value, collection: &str, key: &str) -> Vec<String> {
    snapshot
        .get(collection)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| string_field(entry, key))
        .collect()
}

fn parse_agent_status(raw: &str) -> Result<HerdrAgentStatus> {
    match raw {
        "idle" => Ok(HerdrAgentStatus::Idle),
        "working" => Ok(HerdrAgentStatus::Working),
        "blocked" => Ok(HerdrAgentStatus::Blocked),
        "done" => Ok(HerdrAgentStatus::Done),
        "unknown" => Ok(HerdrAgentStatus::Unknown),
        other => Err(AikitError::new(
            "herdr.unknown_agent_status",
            format!("Herdr returned unknown agent status {other:?}"),
        )),
    }
}

fn agent_status_field(agent: &Value) -> Result<HerdrAgentStatus> {
    agent
        .get("agent_status")
        .or_else(|| agent.get("status"))
        .and_then(Value::as_str)
        .map(parse_agent_status)
        .transpose()
        .map(|status| status.unwrap_or(HerdrAgentStatus::Unknown))
}

fn parse_json(raw: &str, code: &'static str, subject: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            code,
            format!("could not parse Herdr {subject} response: {error}"),
        )
    })
}

pub fn parse_herdr_snapshot(raw: &str) -> Result<HerdrSnapshot> {
    let envelope = parse_json(raw, "herdr.invalid_json", "API")?;
    if let Some(error) = envelope.get("error") {
        return Err(AikitError::new(
            "herdr.api_error",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Herdr API returned an error"),
        ));
    }
    let result = envelope.get("result").ok_or_else(|| {
        AikitError::new("herdr.missing_result", "Herdr API response has no result")
    })?;
    if result.get("type").and_then(Value::as_str) != Some("session_snapshot") {
        return Err(AikitError::new(
            "herdr.unexpected_result",
            "Herdr API response was not a session_snapshot",
        ));
    }
    let snapshot = result.get("snapshot").ok_or_else(|| {
        AikitError::new(
            "herdr.missing_snapshot",
            "Herdr session_snapshot result has no snapshot",
        )
    })?;
    let version = string_field(snapshot, "version")
        .ok_or_else(|| AikitError::new("herdr.missing_version", "Herdr snapshot has no version"))?;
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AikitError::new("herdr.missing_protocol", "Herdr snapshot has no protocol")
        })?;
    let agents = snapshot
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|agent| {
            let pane_id = string_field(agent, "pane_id").ok_or_else(|| {
                AikitError::new("herdr.agent_missing_pane", "Herdr AgentInfo has no pane_id")
            })?;
            let native_id = string_field(agent, "terminal_id")
                .or_else(|| string_field(agent, "name"))
                .unwrap_or_else(|| pane_id.clone());
            Ok(HerdrAgentObservation {
                native_id,
                pane_id,
                name: string_field(agent, "name"),
                status: agent_status_field(agent)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(HerdrSnapshot {
        version,
        protocol,
        focused_workspace_id: string_field(snapshot, "focused_workspace_id"),
        focused_tab_id: string_field(snapshot, "focused_tab_id"),
        focused_pane_id: string_field(snapshot, "focused_pane_id"),
        workspace_ids: object_ids(snapshot, "workspaces", "workspace_id"),
        pane_ids: object_ids(snapshot, "panes", "pane_id"),
        agents,
        layouts: snapshot
            .get("layouts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|layout| {
                Some(HerdrLayoutFocus {
                    workspace_id: string_field(layout, "workspace_id")?,
                    focused_pane_id: string_field(layout, "focused_pane_id"),
                    pane_ids: object_ids(layout, "panes", "pane_id"),
                    panes: layout
                        .get("panes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|pane| {
                            Some(HerdrLayoutPane {
                                pane_id: string_field(pane, "pane_id")?,
                                rect: pane.get("rect").and_then(|rect| {
                                    Some(HerdrPaneRect {
                                        x: rect.get("x").and_then(Value::as_i64)?,
                                        y: rect.get("y").and_then(Value::as_i64)?,
                                        width: rect.get("width").and_then(Value::as_i64)?,
                                        height: rect.get("height").and_then(Value::as_i64)?,
                                    })
                                }),
                            })
                        })
                        .collect(),
                })
            })
            .collect(),
    })
}

/// The canonical provider Ref this build drives herdr through, as a string.
///
/// Kept beside the plan-route code so the created-evidence derivation and the
/// CLI's `provider_ref(PlaceTechnology::herdr())` answer identically without
/// either layer importing the other.
pub fn herdr_provider_ref_uri() -> &'static str {
    "provider/herdr/current"
}

/// The Herdr workspace id recorded in a plan's `backend_extensions.herdr`
/// table, if the plan carries persisted provider-native evidence.
///
/// A workspace id enters the plan only through an explicit open whose created
/// evidence the caller persisted, so a recorded id is the attach identity of
/// the plan route. A workspace label is never read back as identity: Herdr
/// labels are display metadata and provably not unique, so a name match can
/// never stand in for the recorded place.
pub fn herdr_recorded_workspace(plan: &SessionPlan) -> Option<String> {
    plan.backend_extensions
        .get("herdr")?
        .get("workspace-id")
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}

/// The recorded Herdr pane ids for a plan, as (logical plan key, pane id)
/// pairs.
///
/// Only logical keys with an explicitly recorded pane id are returned. A pane
/// id is never guessed from a name: provider-native ids are minted by Herdr
/// and enter the plan only through an explicit open whose created evidence
/// the caller persisted.
pub fn herdr_recorded_surface_keys(plan: &SessionPlan) -> Vec<(String, String)> {
    let Some(herdr) = plan.backend_extensions.get("herdr") else {
        return Vec::new();
    };
    let Some(surfaces) = herdr.get("surfaces").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    surfaces
        .iter()
        .filter_map(|(logical, pane)| pane.as_str().map(|pane| (logical.clone(), pane.to_owned())))
        .collect()
}

/// The provider-native bindings an explicit open created, when the fresh
/// observation proves a Herdr place the plan does not yet record.
///
/// This is the travel permit for created evidence: per binding, not
/// all-or-nothing. A reconciling open on an already-recorded place mints
/// only the panes the plan declares but does not yet record — those travel,
/// and the workspace evidence travels with them; every surface the plan
/// already records at the same native id stays home, so a repeated open
/// never re-issues evidence the caller already persists.
pub fn created_place_bindings(
    plan: &SessionPlan,
    observation: &WorkingEnvironmentObservation,
) -> Option<Vec<ProviderNativeBinding>> {
    if observation.provider.as_str() != herdr_provider_ref_uri() {
        return None;
    }
    let has_session = observation.bindings.iter().any(|binding| {
        binding.kind == NativeBindingKind::Session && binding.canonical_ref.is_none()
    });
    if !has_session {
        return None;
    }
    let recorded = herdr_recorded_surface_keys(plan);
    let mut fresh: Vec<ProviderNativeBinding> = Vec::new();
    let mut any_new_surface = false;
    for binding in &observation.bindings {
        let keep = match (&binding.kind, &binding.canonical_ref) {
            (NativeBindingKind::Surface, Some(canonical)) => {
                match canonical.as_str().strip_prefix("surface/terminal/") {
                    Some(logical) => {
                        let already_recorded = recorded
                            .iter()
                            .any(|(known, pane)| known == logical && pane == &binding.native_id);
                        if !already_recorded {
                            any_new_surface = true;
                        }
                        !already_recorded
                    }
                    None => {
                        any_new_surface = true;
                        true
                    }
                }
            }
            _ => true,
        };
        if keep {
            fresh.push(binding.clone());
        }
    }
    if !any_new_surface {
        return None;
    }
    Some(fresh)
}

/// The smallest pane a split may mint, in layout cells: herdr mints
/// degenerate 0-width panes silently, so the reconcile refuses to create one.
const MIN_SPLIT_COLS: i64 = 8;
const MIN_SPLIT_ROWS: i64 = 3;

fn geometry_allows(
    snapshot: &HerdrSnapshot,
    source_pane: &str,
    direction: HerdrSplitDirection,
) -> bool {
    let rect = snapshot
        .layouts
        .iter()
        .flat_map(|layout| layout.panes.iter())
        .find(|pane| pane.pane_id == source_pane)
        .and_then(|pane| pane.rect);
    let Some(rect) = rect else {
        // No geometry published: herdr accepted every observed split where a
        // rect existed, and refusing without evidence would make the rich
        // provider less usable than the floor. Allow it; the fresh snapshot
        // after the split discloses the outcome either way.
        return true;
    };
    // The split halves the source pane; the new pane must keep a minimal
    // visible area.
    match direction {
        HerdrSplitDirection::Right => rect.width - rect.width / 2 >= MIN_SPLIT_COLS,
        HerdrSplitDirection::Down => rect.height - rect.height / 2 >= MIN_SPLIT_ROWS,
    }
}

pub struct HerdrWorkingEnvironment<R> {
    runner: R,
    provider: ResourceRef,
    workspace_id: Option<String>,
    create_cwd: Option<String>,
    create_label: Option<String>,
    open_subject: Option<ResourceRef>,
    surface_bindings: BTreeMap<ResourceRef, String>,
    project_bindings: BTreeMap<ResourceRef, String>,
    agent_session_bindings: BTreeMap<ResourceRef, String>,
    /// The whole plan surface list with its logical keys — not only the
    /// recorded subset — so a reconciling open can materialise the panes the
    /// plan declares but no open has created yet.
    plan_surfaces: Vec<(ResourceRef, String)>,
    /// Logical pane key → the plan's declared split (source + direction).
    plan_splits: BTreeMap<String, Split>,
    /// Logical pane key → the plan's declared command, honoured after the
    /// pane materialises: herdr has no command slot at create or split time,
    /// so `herdr pane run` is the post-hoc route.
    plan_commands: BTreeMap<String, Vec<String>>,
    /// Why a declared pane was not materialised by the last open: recorded
    /// into the observation's provenance, never silently dropped.
    reconcile_warnings: Vec<String>,
}

impl<R> HerdrWorkingEnvironment<R> {
    pub fn new(runner: R, provider: ResourceRef) -> Self {
        Self {
            runner,
            provider,
            workspace_id: None,
            create_cwd: None,
            create_label: None,
            open_subject: None,
            surface_bindings: BTreeMap::new(),
            project_bindings: BTreeMap::new(),
            agent_session_bindings: BTreeMap::new(),
            plan_surfaces: Vec::new(),
            plan_splits: BTreeMap::new(),
            plan_commands: BTreeMap::new(),
            reconcile_warnings: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace_id: impl Into<String>) -> Self {
        self.workspace_id = Some(workspace_id.into());
        self
    }

    #[must_use]
    pub fn with_create(mut self, cwd: impl Into<String>, label: Option<String>) -> Self {
        self.create_cwd = Some(cwd.into());
        self.create_label = label;
        self
    }

    /// The plan-routed environment: the place this plan names is the Herdr
    /// workspace created under the plan's own name as its label, and the
    /// provider-native evidence recorded in `backend_extensions.herdr` is
    /// the only attach identity.
    ///
    /// `surfaces` is the caller-owned canonical Surface -> logical plan key
    /// list; recorded evidence binds exactly those canonical Surfaces whose
    /// logical key carries a pane id. `subject`, when given, is the canonical
    /// Surface a following open addresses: a first open binds it to the root
    /// pane Herdr mints, so the created evidence can travel back to the
    /// caller for persistence.
    #[must_use]
    pub fn for_plan(
        runner: R,
        plan: &SessionPlan,
        provider: ResourceRef,
        surfaces: &[(ResourceRef, String)],
        subject: Option<&ResourceRef>,
    ) -> Self {
        let mut environment = Self::new(runner, provider);
        environment.create_label = Some(plan.name.clone());
        environment.create_cwd = plan.root.as_ref().map(|root| root.display().to_string());
        environment.open_subject = subject.cloned();
        if let Some(workspace_id) = herdr_recorded_workspace(plan) {
            environment.workspace_id = Some(workspace_id);
        }
        environment.plan_surfaces = surfaces.to_vec();
        for view in &plan.views {
            for step in &view.steps {
                let logical = format!("{}/{}", view.id, step.pane);
                if let Some(split) = &step.split {
                    environment
                        .plan_splits
                        .insert(logical.clone(), split.clone());
                }
                if !step.command.is_empty() {
                    environment
                        .plan_commands
                        .insert(logical, step.command.clone());
                }
            }
        }
        let recorded = herdr_recorded_surface_keys(plan);
        for (surface, logical) in surfaces {
            if let Some((_, pane)) = recorded.iter().find(|(key, _)| key == logical) {
                environment = environment.bind_surface(surface.clone(), pane.clone());
            }
        }
        environment
    }

    #[must_use]
    pub fn bind_surface(mut self, surface: ResourceRef, pane_id: impl Into<String>) -> Self {
        self.surface_bindings.insert(surface, pane_id.into());
        self
    }

    #[must_use]
    pub fn bind_project(mut self, project: ResourceRef, workspace_id: impl Into<String>) -> Self {
        self.project_bindings.insert(project, workspace_id.into());
        self
    }

    #[must_use]
    pub fn bind_agent_session(
        mut self,
        agent_session: ResourceRef,
        agent_or_pane_id: impl Into<String>,
    ) -> Self {
        self.agent_session_bindings
            .insert(agent_session, agent_or_pane_id.into());
        self
    }
}

impl<R: CommandRunner> HerdrWorkingEnvironment<R> {
    fn run(&self, args: &[&str]) -> Result<String> {
        let mut argv = vec!["herdr".to_string()];
        argv.extend(args.iter().map(|arg| (*arg).to_string()));
        let output = self.runner.run(&argv)?;
        let output = output.require(&argv, "herdr.command_failed")?;
        Ok(output.stdout)
    }

    fn run_argv(&self, argv: Vec<String>, code: &'static str) -> Result<String> {
        let output = self.runner.run(&argv)?;
        let output = output.require(&argv, code)?;
        Ok(output.stdout)
    }

    pub fn snapshot(&self) -> Result<HerdrSnapshot> {
        parse_herdr_snapshot(&self.run(&["api", "snapshot"])?)
    }

    /// Read only the pane bound by this provider's persisted plan and fresh
    /// snapshot. No workspace creation, focus or input occurs here.
    pub fn capture_surface(&self, subject: &ResourceRef, lines: u16) -> Result<(String, String)> {
        if !(1..=2000).contains(&lines) {
            return Err(AikitError::new(
                "herdr.capture_limit",
                "Capture lines must be between 1 and 2000",
            ));
        }
        let observation = self.observation(self.snapshot()?);
        let native = observation
            .canonical_native_id(subject)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.surface_not_live",
                    "The persisted working Surface has no currently observed Herdr pane",
                )
            })?
            .to_string();
        let text = self.run(&[
            "pane",
            "read",
            &native,
            "--source",
            "visible",
            "--format",
            "ansi",
            "--lines",
            &lines.to_string(),
            "--raw",
        ])?;
        let after = self.observation(self.snapshot()?);
        if after.canonical_native_id(subject) != Some(native.as_str()) {
            return Err(AikitError::new(
                "herdr.surface_changed",
                "The working pane changed during capture; retry its current binding",
            ));
        }
        Ok((native, text))
    }

    pub fn create_workspace(&mut self) -> Result<HerdrWorkspaceCreation> {
        let cwd = self.create_cwd.clone().ok_or_else(|| {
            AikitError::new(
                "herdr.workspace_absent",
                "configured Herdr workspace is absent and no create cwd was supplied: set the working-surface plan's `root` (the directory Herdr creates the workspace in) and re-bind it",
            )
        })?;
        let mut argv = vec![
            "herdr".to_string(),
            "workspace".to_string(),
            "create".to_string(),
            "--cwd".to_string(),
            cwd,
            "--no-focus".to_string(),
        ];
        if let Some(label) = &self.create_label {
            argv.extend(["--label".to_string(), label.clone()]);
        }
        let raw = self.run_argv(argv, "herdr.workspace_create_failed")?;
        let envelope = parse_json(&raw, "herdr.invalid_create_response", "workspace create")?;
        let result = envelope.get("result").ok_or_else(|| {
            AikitError::new(
                "herdr.create_missing_result",
                "Herdr workspace create response has no result",
            )
        })?;
        let workspace_id = result
            .pointer("/workspace/workspace_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.create_missing_workspace",
                    "Herdr workspace create response has no workspace_id",
                )
            })?
            .to_string();
        let tab_id = result
            .pointer("/tab/tab_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.create_missing_tab",
                    "Herdr workspace create response has no initial tab_id",
                )
            })?
            .to_string();
        let root_pane_id = result
            .pointer("/root_pane/pane_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.create_missing_root_pane",
                    "Herdr workspace create response has no root_pane pane_id",
                )
            })?
            .to_string();
        self.workspace_id = Some(workspace_id.clone());
        Ok(HerdrWorkspaceCreation {
            workspace_id,
            tab_id,
            root_pane_id,
        })
    }

    pub fn create_workspace_and_bind_root(
        &mut self,
        root_surface: ResourceRef,
    ) -> Result<HerdrWorkspaceCreation> {
        let created = self.create_workspace()?;
        self.surface_bindings
            .insert(root_surface, created.root_pane_id.clone());
        Ok(created)
    }

    pub fn focus_workspace(&self) -> Result<()> {
        let workspace = self.workspace_id.as_deref().ok_or_else(|| {
            AikitError::new(
                "herdr.workspace_unbound",
                "Herdr provider has no current workspace binding",
            )
        })?;
        self.run(&["workspace", "focus", workspace])?;
        Ok(())
    }

    pub fn split_surface(
        &mut self,
        source_surface: &ResourceRef,
        new_surface: ResourceRef,
        direction: HerdrSplitDirection,
    ) -> Result<String> {
        let source_pane = self
            .surface_bindings
            .get(source_surface)
            .cloned()
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.surface_unbound",
                    format!("Surface {source_surface} has no explicit Herdr pane binding"),
                )
            })?;
        let raw = self.run(&[
            "pane",
            "split",
            &source_pane,
            "--direction",
            direction.as_cli(),
            "--no-focus",
        ])?;
        let envelope = parse_json(&raw, "herdr.invalid_split_response", "pane split")?;
        let pane_id = envelope
            .pointer("/result/pane/pane_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.split_missing_pane",
                    "Herdr pane split response has no pane_id",
                )
            })?
            .to_string();
        self.surface_bindings.insert(new_surface, pane_id.clone());
        Ok(pane_id)
    }

    pub fn start_agent_session(
        &mut self,
        agent_session: ResourceRef,
        surface: &ResourceRef,
        name: &str,
        kind: &str,
        timeout_ms: Option<u64>,
        agent_args: &[String],
    ) -> Result<HerdrStartedAgent> {
        let pane_id = self.surface_bindings.get(surface).cloned().ok_or_else(|| {
            AikitError::new(
                "herdr.surface_unbound",
                format!("Surface {surface} has no explicit Herdr pane binding"),
            )
        })?;
        let mut argv = vec![
            "herdr".to_string(),
            "agent".to_string(),
            "start".to_string(),
            name.to_string(),
            "--kind".to_string(),
            kind.to_string(),
            "--pane".to_string(),
            pane_id.clone(),
        ];
        if let Some(timeout_ms) = timeout_ms {
            argv.extend(["--timeout".to_string(), timeout_ms.to_string()]);
        }
        if !agent_args.is_empty() {
            argv.push("--".to_string());
            argv.extend(agent_args.iter().cloned());
        }
        let raw = self.run_argv(argv, "herdr.agent_start_failed")?;
        let envelope = parse_json(&raw, "herdr.invalid_agent_start_response", "agent start")?;
        let agent = envelope.pointer("/result/agent").ok_or_else(|| {
            AikitError::new(
                "herdr.agent_start_missing_agent",
                "Herdr agent start response has no agent",
            )
        })?;
        let returned_pane = string_field(agent, "pane_id").ok_or_else(|| {
            AikitError::new(
                "herdr.agent_start_missing_pane",
                "Herdr agent start response has no pane_id",
            )
        })?;
        if returned_pane != pane_id {
            return Err(AikitError::new(
                "herdr.agent_pane_drift",
                format!(
                    "Herdr started agent in pane {returned_pane}, expected explicitly bound pane {pane_id}"
                ),
            ));
        }
        let terminal_id = string_field(agent, "terminal_id").ok_or_else(|| {
            AikitError::new(
                "herdr.agent_start_missing_terminal",
                "Herdr agent start response has no terminal_id",
            )
        })?;
        let returned_name = string_field(agent, "name");
        if returned_name
            .as_deref()
            .is_some_and(|returned| returned != name)
        {
            return Err(AikitError::new(
                "herdr.agent_name_drift",
                format!("Herdr returned agent name {returned_name:?}, expected {name:?}"),
            ));
        }
        let status = agent_status_field(agent)?;
        self.agent_session_bindings.insert(
            agent_session,
            returned_name
                .clone()
                .unwrap_or_else(|| returned_pane.clone()),
        );
        Ok(HerdrStartedAgent {
            terminal_id,
            pane_id: returned_pane,
            name: returned_name,
            status,
        })
    }

    pub fn focus_agent_session(&self, agent_session: &ResourceRef) -> Result<()> {
        let native = self
            .agent_session_bindings
            .get(agent_session)
            .ok_or_else(|| {
                AikitError::new(
                    "herdr.agent_session_unbound",
                    format!(
                        "AgentSession {agent_session} has no explicit Herdr agent/pane binding"
                    ),
                )
            })?;
        self.run(&["agent", "focus", native])?;
        Ok(())
    }

    pub fn reconcile_after_reconnect(&mut self) -> Result<WorkingEnvironmentObservation> {
        self.observe()
    }

    /// Reconcile the plan's declared panes that no open has materialised yet
    /// into the live workspace — the create-or-attach contract the mux floor
    /// gives operators, which the rich provider now honours too.
    ///
    /// Each unbound plan pane splits off the pane its own step declares as
    /// its source (falling back to the plan's root pane), with `--no-focus`
    /// always: herdr has no absolute pane focus, and focus is a global
    /// desktop fact the plan must never be keyed to. A pane that cannot be
    /// honoured honestly — an unhonourable direction, an unbound source, a
    /// split that would mint an invisibly small pane — is skipped with a
    /// provenance line naming it, never silently.
    /// Run one materialised pane's declared command in it. `herdr pane run`
    /// is the only route — workspace create and pane split take no command —
    /// and it is best-effort by contract: a command that does not start is a
    /// named provenance line, never a failed open.
    fn run_declared_command(&mut self, logical: &str, pane_id: &str) {
        let Some(command) = self.plan_commands.get(logical).cloned() else {
            return;
        };
        let mut argv = vec![
            "herdr".to_string(),
            "pane".to_string(),
            "run".to_string(),
            pane_id.to_string(),
        ];
        argv.extend(command);
        match self.runner.run(&argv) {
            Ok(output) if output.status == 0 => {}
            Ok(output) => self.reconcile_warnings.push(format!(
                "plan pane {logical}: its command did not start in {pane_id} (exit {}): {}",
                output.status,
                output.stderr.trim()
            )),
            Err(error) => self.reconcile_warnings.push(format!(
                "plan pane {logical}: its command did not start in {pane_id}: {}",
                error.message()
            )),
        }
    }

    fn reconcile_plan_surfaces(&mut self) -> Result<()> {
        let snapshot = self.snapshot()?;
        for (surface, logical) in self.plan_surfaces.clone() {
            if self.surface_bindings.contains_key(&surface) {
                continue;
            }
            let split = match self.plan_splits.get(&logical).cloned() {
                Some(split) => split,
                None => {
                    self.reconcile_warnings.push(format!(
                        "plan pane {logical} has no declared split source; only opens that address it directly can materialise it"
                    ));
                    continue;
                }
            };
            let direction = match split.direction {
                Direction::Right => HerdrSplitDirection::Right,
                Direction::Down => HerdrSplitDirection::Down,
                other => {
                    self.reconcile_warnings.push(format!(
                        "plan pane {logical} declares direction {} which herdr cannot honour; skipped without transposing",
                        other.as_str()
                    ));
                    continue;
                }
            };
            let Some((source_surface, _)) = self
                .plan_surfaces
                .iter()
                .find(|(_, key)| *key == split.from)
                .cloned()
            else {
                self.reconcile_warnings.push(format!(
                    "plan pane {logical} splits from {} which names no pane of this plan; skipped",
                    split.from
                ));
                continue;
            };
            let Some(source_pane) = self.surface_bindings.get(&source_surface).cloned() else {
                self.reconcile_warnings.push(format!(
                    "plan pane {logical} splits from {} whose pane is not bound yet; skipped",
                    split.from
                ));
                continue;
            };
            if !snapshot.pane_ids.contains(&source_pane) {
                self.reconcile_warnings.push(format!(
                    "plan pane {logical} splits from bound pane {source_pane} that the fresh snapshot does not show live; skipped"
                ));
                continue;
            }
            if !geometry_allows(&snapshot, &source_pane, direction) {
                self.reconcile_warnings.push(format!(
                    "plan pane {logical} skipped: splitting {source_pane} {} would mint an invisibly small pane",
                    direction.as_cli()
                ));
                continue;
            }
            let pane_id = self.split_surface(&source_surface, surface, direction)?;
            self.run_declared_command(&logical, &pane_id);
        }
        Ok(())
    }

    fn observation(&self, snapshot: HerdrSnapshot) -> WorkingEnvironmentObservation {
        let workspace_present = self
            .workspace_id
            .as_ref()
            .is_none_or(|id| snapshot.workspace_ids.contains(id));
        let surfaces_present = self
            .surface_bindings
            .values()
            .all(|pane| snapshot.pane_ids.contains(pane));
        let health = if !workspace_present || !surfaces_present {
            WorkingEnvironmentHealth::Degraded
        } else {
            WorkingEnvironmentHealth::Healthy
        };
        let mut bindings = Vec::new();
        // Recorded panes that are no longer live are churn, not identity:
        // Herdr never reuses pane ids, so a missing id is a closed pane, and
        // the observation discloses it by name instead of silently rebinding.
        let churned: Vec<&str> = self
            .surface_bindings
            .values()
            .filter(|pane| !snapshot.pane_ids.contains(pane))
            .map(String::as_str)
            .collect();
        if let Some(workspace_id) = &self.workspace_id {
            bindings.push(ProviderNativeBinding {
                kind: NativeBindingKind::Session,
                native_id: workspace_id.clone(),
                canonical_ref: None,
                provenance: vec!["Herdr Workspace is provider-native SessionSpace evidence".into()],
            });
        }
        bindings.extend(self.surface_bindings.iter().map(|(canonical, pane)| {
            ProviderNativeBinding {
                kind: NativeBindingKind::Surface,
                native_id: pane.clone(),
                canonical_ref: Some(canonical.clone()),
                provenance: vec!["explicit SurfaceRef -> Herdr pane binding".into()],
            }
        }));
        bindings.extend(self.project_bindings.iter().map(|(canonical, workspace)| {
            ProviderNativeBinding {
                kind: NativeBindingKind::Project,
                native_id: workspace.clone(),
                canonical_ref: Some(canonical.clone()),
                provenance: vec!["explicit ProjectRef -> Herdr workspace binding".into()],
            }
        }));
        bindings.extend(
            self.agent_session_bindings
                .iter()
                .map(|(canonical, native)| ProviderNativeBinding {
                    kind: NativeBindingKind::AgentSession,
                    native_id: native.clone(),
                    canonical_ref: Some(canonical.clone()),
                    provenance: vec![
                        "explicit AgentSessionRef -> Herdr live Agent/pane binding".into()
                    ],
                }),
        );
        let mut provenance = vec![
            format!("Herdr public API snapshot protocol={}", snapshot.protocol),
            format!("herdrdev/herdr@{HERDR_UPSTREAM_REVISION}"),
            HERDR_PROVIDER_VERSION.into(),
        ];
        for warning in &self.reconcile_warnings {
            provenance.push(format!("reconcile: {warning}"));
        }
        if !churned.is_empty() {
            provenance.push(format!(
                "recorded Herdr pane(s) {} no longer live; herdr never reuses pane ids",
                churned.join(", ")
            ));
        }
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: self.provider.clone(),
            provider_version: Some(snapshot.version.clone()),
            health,
            capabilities: self.capabilities(),
            bindings,
            focused_native_id: snapshot.focused_pane_id.clone(),
            provenance,
        }
    }

    pub fn agent_observations(&self) -> Result<Vec<HerdrAgentObservation>> {
        Ok(self.snapshot()?.agents)
    }
}

impl<R: CommandRunner> WorkingEnvironmentProvider for HerdrWorkingEnvironment<R> {
    fn provider_ref(&self) -> &ResourceRef {
        &self.provider
    }

    fn capabilities(&self) -> WorkingEnvironmentCapabilities {
        WorkingEnvironmentCapabilities {
            discover: true,
            open: true,
            focus: true,
            select: true,
            multi_project: true,
            editor_surface: false,
            terminal_surface: true,
            conversation_surface: true,
            diff_surface: false,
            preview_surface: false,
            test_surface: false,
            surface_attach_detach: false,
            agent_session_attach_detach: false,
            reconstruct: true,
        }
    }

    fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
        self.snapshot().map(|snapshot| self.observation(snapshot))
    }

    fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
        self.reconcile_warnings.clear();
        let snapshot = self.snapshot()?;
        let recorded_live = self
            .workspace_id
            .as_ref()
            .is_some_and(|id| snapshot.workspace_ids.contains(id));
        if !recorded_live {
            // Either no place was ever recorded for this plan, or the recorded
            // place is gone. Herdr never reuses workspace ids, so an id
            // missing from a fresh snapshot is a closed place, not a stale
            // read: creating anew is the create half of create-or-attach, and
            // nothing pretends the old place came back. Its pane bindings
            // belong to that gone place and are dropped with it.
            self.surface_bindings.clear();
            let created = self.create_workspace()?;
            if let Some(subject) = self.open_subject.clone() {
                let subject_logical = self
                    .plan_surfaces
                    .iter()
                    .find(|(surface, _)| *surface == subject)
                    .map(|(_, key)| key.clone());
                self.surface_bindings
                    .insert(subject.clone(), created.root_pane_id.clone());
                if let Some(logical) = subject_logical {
                    self.run_declared_command(&logical, &created.root_pane_id);
                }
            }
        }
        // Both halves of create-or-attach reconcile: a plan that declares
        // panes no open has materialised gets them now, split off the pane
        // their own step names. The mux floor has always worked this way;
        // the rich provider matches it instead of under-reconciling it.
        self.reconcile_plan_surfaces()?;
        // Whatever was decided, the returned observation is fresh provider
        // proof: the workspace this open now stands on, and every bound pane
        // seen live in the same snapshot.
        let proof = self.snapshot()?;
        Ok(self.observation(proof))
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let pane = self.surface_bindings.get(surface).cloned().ok_or_else(|| {
            AikitError::new(
                "herdr.surface_unbound",
                format!("Surface {surface} has no explicit Herdr pane binding"),
            )
        })?;
        // Installed Herdr has no absolute pane focus: `herdr pane focus` is
        // neighbour-relative navigation (`--direction left|right|up|down`), so
        // the only available command would silently move the operator to a
        // neighbour of the bound pane. When the bound pane is its workspace's
        // own focused pane, `workspace focus` lands exactly on it; otherwise
        // withhold the surface-level operation rather than focus something else.
        // Observing is not acting: an unreadable snapshot proves nothing and
        // falls through to the withheld refusal.
        let exact_workspace = self.snapshot().ok().and_then(|snapshot| {
            snapshot
                .layouts
                .into_iter()
                .find(|layout| layout.focused_pane_id.as_deref() == Some(pane.as_str()))
        });
        if let Some(layout) = exact_workspace {
            self.run(&["workspace", "focus", &layout.workspace_id])?;
            return Ok(());
        }
        Err(AikitError::new(
            "herdr.surface_focus_unsupported",
            format!(
                "Herdr cannot focus pane {pane} directly: `herdr pane focus` is \
                 neighbour-relative and `herdr workspace focus` is the coarser supported route"
            ),
        ))
    }

    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        Err(AikitError::new(
            "herdr.detach_requires_explicit_operation",
            format!(
                "Surface {surface} is bound to Herdr, but generic detach is intentionally unsupported; closing a pane is destructive provider-local lifecycle"
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::runner::ScriptedRunner;

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn parses_current_public_session_snapshot_without_collapsing_ids() {
        let raw = r#"{
          "id":"cli:api:snapshot",
          "result":{"type":"session_snapshot","snapshot":{
            "version":"0.9.0","protocol":7,
            "focused_workspace_id":"w1","focused_tab_id":"w1:t1","focused_pane_id":"w1:p2",
            "workspaces":[{"workspace_id":"w1"}],
            "tabs":[{"tab_id":"w1:t1"}],
            "panes":[{"pane_id":"w1:p1"},{"pane_id":"w1:p2"}],
            "layouts":[],
            "agents":[{"terminal_id":"term-2","name":"reviewer","pane_id":"w1:p2","agent_status":"blocked"}]
          }}
        }"#;
        let snapshot = parse_herdr_snapshot(raw).unwrap();
        assert_eq!(snapshot.workspace_ids, vec!["w1"]);
        assert_eq!(snapshot.pane_ids, vec!["w1:p1", "w1:p2"]);
        assert_eq!(snapshot.focused_pane_id.as_deref(), Some("w1:p2"));
        assert_eq!(snapshot.agents[0].status, HerdrAgentStatus::Blocked);
        assert_eq!(snapshot.agents[0].pane_id, "w1:p2");
    }

    #[test]
    fn a_surface_that_is_its_workspaces_focused_pane_is_focused_through_the_workspace() {
        let snapshot = |focused: &str| {
            format!(
                r#"{{"id":"s","result":{{"type":"session_snapshot","snapshot":{{
                  "version":"0.9.1","protocol":8,"focused_workspace_id":"w2","focused_pane_id":"w2:p1",
                  "workspaces":[{{"workspace_id":"w2"}},{{"workspace_id":"w9"}}],"tabs":[],
                  "panes":[{{"pane_id":"w2:p1"}},{{"pane_id":"w9:p1"}},{{"pane_id":"w9:p2"}}],
                  "layouts":[
                    {{"workspace_id":"w2","tab_id":"w2:t1","focused_pane_id":"w2:p1","panes":[{{"pane_id":"w2:p1"}}]}},
                    {{"workspace_id":"w9","tab_id":"w9:t1","focused_pane_id":"{focused}","panes":[{{"pane_id":"w9:p1"}},{{"pane_id":"w9:p2"}}]}}
                  ],"agents":[]}}}}}}"#
            )
        };
        let surface = r("surface/terminal/guardian/shell");

        // Bound pane is w9's focused pane: focusing w9 lands exactly on it.
        let runner = Arc::new(
            ScriptedRunner::new()
                .on("api snapshot", &snapshot("w9:p1"))
                .on("workspace focus w9", "{}"),
        );
        let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"));
        provider
            .surface_bindings
            .insert(surface.clone(), "w9:p1".into());
        provider.focus_surface(&surface).unwrap();
        assert!(runner
            .call_lines()
            .iter()
            .any(|line| line.contains("workspace focus w9")));

        // Bound pane is not its workspace's focused pane: refused, nothing moved.
        let runner = Arc::new(ScriptedRunner::new().on("api snapshot", &snapshot("w9:p2")));
        let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"));
        provider
            .surface_bindings
            .insert(surface.clone(), "w9:p1".into());
        let error = provider.focus_surface(&surface).unwrap_err();
        assert_eq!(error.code(), "herdr.surface_focus_unsupported");
        assert!(!runner
            .call_lines()
            .iter()
            .any(|line| line.contains("focus")));
    }

    #[test]
    fn keeps_legacy_status_field_compatible() {
        let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p","status":"done"}]}}}"#;
        let snapshot = parse_herdr_snapshot(raw).unwrap();
        assert_eq!(snapshot.agents[0].status, HerdrAgentStatus::Done);
    }

    #[test]
    fn rejects_unknown_agent_state_instead_of_inventing_completion() {
        let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p","agent_status":"finished"}]}}}"#;
        let error = parse_herdr_snapshot(raw).unwrap_err();
        assert_eq!(error.code(), "herdr.unknown_agent_status");
    }

    #[test]
    fn current_public_automation_binds_only_returned_provider_ids() {
        let snapshot = r#"{
          "id":"cli:api:snapshot",
          "result":{"type":"session_snapshot","snapshot":{
            "version":"0.9.1","protocol":8,
            "focused_workspace_id":"w7","focused_tab_id":"w7:t1","focused_pane_id":"w7:p2",
            "workspaces":[{"workspace_id":"w7"}],
            "tabs":[{"tab_id":"w7:t1"}],
            "panes":[{"pane_id":"w7:p1"},{"pane_id":"w7:p2"}],
            "layouts":[],
            "agents":[{"terminal_id":"term-2","name":"reviewer","pane_id":"w7:p2","agent_status":"idle"}]
          }}
        }"#;
        let runner = Arc::new(
            ScriptedRunner::new()
                .on(
                    "workspace create --cwd /repo --no-focus --label reference",
                    r#"{"id":"create","result":{"type":"workspace_created","workspace":{"workspace_id":"w7"},"tab":{"tab_id":"w7:t1"},"root_pane":{"pane_id":"w7:p1"}}}"#,
                )
                .on(
                    "pane split w7:p1 --direction right --no-focus",
                    r#"{"id":"split","result":{"type":"pane_info","pane":{"pane_id":"w7:p2"}}}"#,
                )
                .on(
                    "agent start reviewer --kind codex --pane w7:p2 -- -m gpt-5.4",
                    r#"{"id":"start","result":{"type":"agent_info","agent":{"terminal_id":"term-2","name":"reviewer","pane_id":"w7:p2","agent_status":"idle"}}}"#,
                )
                .on("workspace focus w7", "{}")
                .on("agent focus reviewer", "{}")
                .on("api snapshot", snapshot),
        );
        let root_surface = r("surface/reference/root");
        let review_surface = r("surface/reference/review");
        let agent_session = r("agent-session/reference/reviewer");
        let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
            .with_create("/repo", Some("reference".into()));

        let created = provider
            .create_workspace_and_bind_root(root_surface.clone())
            .unwrap();
        assert_eq!(created.workspace_id, "w7");
        assert_eq!(created.tab_id, "w7:t1");
        assert_eq!(created.root_pane_id, "w7:p1");

        let split = provider
            .split_surface(
                &root_surface,
                review_surface.clone(),
                HerdrSplitDirection::Right,
            )
            .unwrap();
        assert_eq!(split, "w7:p2");

        let started = provider
            .start_agent_session(
                agent_session.clone(),
                &review_surface,
                "reviewer",
                "codex",
                None,
                &["-m".into(), "gpt-5.4".into()],
            )
            .unwrap();
        assert_eq!(started.pane_id, "w7:p2");
        assert_eq!(started.name.as_deref(), Some("reviewer"));
        assert_eq!(started.status, HerdrAgentStatus::Idle);

        provider.focus_workspace().unwrap();
        provider.focus_agent_session(&agent_session).unwrap();
        let observation = provider.reconcile_after_reconnect().unwrap();
        assert_eq!(observation.health, WorkingEnvironmentHealth::Healthy);
        assert_eq!(
            observation.canonical_native_id(&root_surface),
            Some("w7:p1")
        );
        assert_eq!(
            observation.canonical_native_id(&review_surface),
            Some("w7:p2")
        );
        assert_eq!(
            observation.canonical_native_id(&agent_session),
            Some("reviewer")
        );

        let calls = runner.call_lines();
        assert!(calls.iter().any(|call| {
            call == "herdr workspace create --cwd /repo --no-focus --label reference"
        }));
        assert!(calls
            .iter()
            .any(|call| call == "herdr pane split w7:p1 --direction right --no-focus"));
        assert!(calls.iter().any(|call| {
            call == "herdr agent start reviewer --kind codex --pane w7:p2 -- -m gpt-5.4"
        }));
        assert!(calls.iter().any(|call| call == "herdr api snapshot"));
    }

    #[test]
    fn agent_start_refuses_provider_pane_drift() {
        let runner = ScriptedRunner::new().on(
            "agent start reviewer --kind codex --pane w1:p2",
            r#"{"id":"start","result":{"agent":{"terminal_id":"term-x","name":"reviewer","pane_id":"w9:p9","agent_status":"idle"}}}"#,
        );
        let surface = r("surface/reference/review");
        let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
            .bind_surface(surface.clone(), "w1:p2");
        let error = provider
            .start_agent_session(
                r("agent-session/reference/reviewer"),
                &surface,
                "reviewer",
                "codex",
                None,
                &[],
            )
            .unwrap_err();
        assert_eq!(error.code(), "herdr.agent_pane_drift");
    }
}
