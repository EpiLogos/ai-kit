//! Herdr working-environment provider for O:I composable inhabitation.
//!
//! Herdr is consumed through its public CLI/API surface. Workspace, tab, pane and
//! recognised-agent ids remain provider-native evidence; canonical AIKit refs are
//! supplied only by explicit bindings.

use std::collections::BTreeMap;

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runner::CommandRunner;
use crate::working_environment::{
    NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
    WorkingEnvironmentHealth, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};

pub const HERDR_UPSTREAM_REVISION: &str = "facf0aafca011d147e798ad37e83799bdd29b75e";
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrAgentObservation {
    pub native_id: String,
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

pub fn parse_herdr_snapshot(raw: &str) -> Result<HerdrSnapshot> {
    let envelope: Value = serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            "herdr.invalid_json",
            format!("could not parse Herdr API response: {error}"),
        )
    })?;
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
    let version = string_field(snapshot, "version").ok_or_else(|| {
        AikitError::new("herdr.missing_version", "Herdr snapshot has no version")
    })?;
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| AikitError::new("herdr.missing_protocol", "Herdr snapshot has no protocol"))?;
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
            let status = agent
                .get("status")
                .and_then(Value::as_str)
                .map(parse_agent_status)
                .transpose()?
                .unwrap_or(HerdrAgentStatus::Unknown);
            Ok(HerdrAgentObservation {
                native_id,
                pane_id,
                name: string_field(agent, "name"),
                status,
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
    })
}

pub struct HerdrWorkingEnvironment<R> {
    runner: R,
    provider: ResourceRef,
    workspace_id: Option<String>,
    create_cwd: Option<String>,
    create_label: Option<String>,
    surface_bindings: BTreeMap<ResourceRef, String>,
    project_bindings: BTreeMap<ResourceRef, String>,
    agent_session_bindings: BTreeMap<ResourceRef, String>,
}

impl<R> HerdrWorkingEnvironment<R> {
    pub fn new(runner: R, provider: ResourceRef) -> Self {
        Self {
            runner,
            provider,
            workspace_id: None,
            create_cwd: None,
            create_label: None,
            surface_bindings: BTreeMap::new(),
            project_bindings: BTreeMap::new(),
            agent_session_bindings: BTreeMap::new(),
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

    pub fn snapshot(&self) -> Result<HerdrSnapshot> {
        parse_herdr_snapshot(&self.run(&["api", "snapshot"])? )
    }

    fn create_workspace(&mut self) -> Result<()> {
        let cwd = self.create_cwd.clone().ok_or_else(|| {
            AikitError::new(
                "herdr.workspace_absent",
                "configured Herdr workspace is absent and no create cwd was supplied",
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
        let output = self.runner.run(&argv)?;
        let output = output.require(&argv, "herdr.workspace_create_failed")?;
        let envelope: Value = serde_json::from_str(&output.stdout).map_err(|error| {
            AikitError::new(
                "herdr.invalid_create_response",
                format!("could not parse Herdr workspace create response: {error}"),
            )
        })?;
        self.workspace_id = envelope
            .pointer("/result/workspace/workspace_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if self.workspace_id.is_none() {
            return Err(AikitError::new(
                "herdr.create_missing_workspace",
                "Herdr workspace create response has no workspace_id",
            ));
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
        bindings.extend(self.agent_session_bindings.iter().map(|(canonical, native)| {
            ProviderNativeBinding {
                kind: NativeBindingKind::AgentSession,
                native_id: native.clone(),
                canonical_ref: Some(canonical.clone()),
                provenance: vec!["explicit AgentSessionRef -> Herdr agent/pane binding".into()],
            }
        }));
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: self.provider.clone(),
            provider_version: Some(snapshot.version.clone()),
            health,
            capabilities: self.capabilities(),
            bindings,
            focused_native_id: snapshot.focused_pane_id.clone(),
            provenance: vec![
                format!("Herdr public API snapshot protocol={}", snapshot.protocol),
                format!("herdrdev/herdr@{HERDR_UPSTREAM_REVISION}"),
                HERDR_PROVIDER_VERSION.into(),
            ],
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
        let snapshot = self.snapshot()?;
        if self
            .workspace_id
            .as_ref()
            .is_some_and(|id| !snapshot.workspace_ids.contains(id))
        {
            self.create_workspace()?;
        }
        self.observe()
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let pane = self.surface_bindings.get(surface).ok_or_else(|| {
            AikitError::new(
                "herdr.surface_unbound",
                format!("Surface {surface} has no explicit Herdr pane binding"),
            )
        })?;
        self.run(&["pane", "focus", pane])?;
        Ok(())
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
    use super::*;

    #[test]
    fn parses_public_session_snapshot_without_collapsing_ids() {
        let raw = r#"{
          "id":"cli:api:snapshot",
          "result":{"type":"session_snapshot","snapshot":{
            "version":"0.9.0","protocol":7,
            "focused_workspace_id":"w1","focused_tab_id":"w1:t1","focused_pane_id":"w1:p2",
            "workspaces":[{"workspace_id":"w1"}],
            "tabs":[{"tab_id":"w1:t1"}],
            "panes":[{"pane_id":"w1:p1"},{"pane_id":"w1:p2"}],
            "layouts":[],
            "agents":[{"terminal_id":"term-2","name":"reviewer","pane_id":"w1:p2","status":"blocked"}]
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
    fn rejects_unknown_agent_state_instead_of_inventing_completion() {
        let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p","status":"finished"}]}}}"#;
        let error = parse_herdr_snapshot(raw).unwrap_err();
        assert_eq!(error.code(), "herdr.unknown_agent_status");
    }
}
