//! Hyprland graphical-environment binding for canonical AIKit Surfaces.
//!
//! This adapter consumes `hyprctl` JSON/dispatcher interfaces. A Hyprland window
//! address is presentation evidence only; it never becomes `SurfaceRef` identity.

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

pub const HYPRLAND_UPSTREAM_REVISION: &str = "59177602ffd75d81e8995e0456057e2d086a01c8";
pub const HYPRLAND_PROVIDER_VERSION: &str = "aikit.hyprland-surface-provider/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyprlandWindowObservation {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<i64>,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub hidden: bool,
}

pub fn parse_hyprland_clients(raw: &str) -> Result<Vec<HyprlandWindowObservation>> {
    let clients: Value = serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            "hyprland.invalid_clients_json",
            format!("could not parse `hyprctl -j clients`: {error}"),
        )
    })?;
    let clients = clients.as_array().ok_or_else(|| {
        AikitError::new(
            "hyprland.clients_not_array",
            "`hyprctl -j clients` did not return an array",
        )
    })?;
    clients
        .iter()
        .map(|client| {
            let address = client
                .get("address")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AikitError::new(
                        "hyprland.client_missing_address",
                        "Hyprland client has no address",
                    )
                })?
                .to_string();
            let workspace = client
                .get("workspace")
                .and_then(|workspace| {
                    workspace
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| workspace.get("id").and_then(Value::as_i64).map(|id| id.to_string()))
                });
            Ok(HyprlandWindowObservation {
                address,
                class: client.get("class").and_then(Value::as_str).map(str::to_owned),
                title: client.get("title").and_then(Value::as_str).map(str::to_owned),
                workspace,
                monitor: client.get("monitor").and_then(Value::as_i64),
                floating: client.get("floating").and_then(Value::as_bool).unwrap_or(false),
                mapped: client.get("mapped").and_then(Value::as_bool).unwrap_or(true),
                hidden: client.get("hidden").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

pub struct HyprlandWorkingEnvironment<R> {
    runner: R,
    provider: ResourceRef,
    surface_bindings: BTreeMap<ResourceRef, String>,
}

impl<R> HyprlandWorkingEnvironment<R> {
    pub fn new(runner: R, provider: ResourceRef) -> Self {
        Self {
            runner,
            provider,
            surface_bindings: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn bind_surface(mut self, surface: ResourceRef, address: impl Into<String>) -> Self {
        self.surface_bindings.insert(surface, address.into());
        self
    }
}

impl<R: CommandRunner> HyprlandWorkingEnvironment<R> {
    fn run(&self, args: &[&str]) -> Result<String> {
        let mut argv = vec!["hyprctl".to_string()];
        argv.extend(args.iter().map(|arg| (*arg).to_string()));
        let output = self.runner.run(&argv)?;
        let output = output.require(&argv, "hyprland.command_failed")?;
        Ok(output.stdout)
    }

    pub fn clients(&self) -> Result<Vec<HyprlandWindowObservation>> {
        parse_hyprland_clients(&self.run(&["-j", "clients"])? )
    }

    fn dispatch_window(&self, dispatcher: &str, address: &str) -> Result<()> {
        let selector = format!("address:{address}");
        self.run(&["dispatch", dispatcher, &selector])?;
        Ok(())
    }

    pub fn move_surface(&self, surface: &ResourceRef, workspace: &str, silent: bool) -> Result<()> {
        let address = self.surface_bindings.get(surface).ok_or_else(|| {
            AikitError::new(
                "hyprland.surface_unbound",
                format!("Surface {surface} has no explicit Hyprland address binding"),
            )
        })?;
        let target = format!("{workspace},address:{address}");
        self.run(&[
            "dispatch",
            if silent { "movetoworkspacesilent" } else { "movetoworkspace" },
            &target,
        ])?;
        Ok(())
    }

    pub fn set_surface_floating(&self, surface: &ResourceRef, floating: bool) -> Result<()> {
        let address = self.surface_bindings.get(surface).ok_or_else(|| {
            AikitError::new(
                "hyprland.surface_unbound",
                format!("Surface {surface} has no explicit Hyprland address binding"),
            )
        })?;
        self.dispatch_window(if floating { "setfloating" } else { "settiled" }, address)
    }

    fn observation(&self, clients: Vec<HyprlandWindowObservation>) -> WorkingEnvironmentObservation {
        let present = self
            .surface_bindings
            .values()
            .filter(|address| clients.iter().any(|client| &client.address == *address))
            .count();
        let health = if self.surface_bindings.is_empty() || present == self.surface_bindings.len() {
            WorkingEnvironmentHealth::Healthy
        } else if present == 0 {
            WorkingEnvironmentHealth::Unavailable
        } else {
            WorkingEnvironmentHealth::Degraded
        };
        let bindings = self
            .surface_bindings
            .iter()
            .map(|(surface, address)| ProviderNativeBinding {
                kind: NativeBindingKind::Surface,
                native_id: address.clone(),
                canonical_ref: Some(surface.clone()),
                provenance: vec!["explicit SurfaceRef -> Hyprland window address binding".into()],
            })
            .collect();
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: self.provider.clone(),
            provider_version: None,
            health,
            capabilities: self.capabilities(),
            bindings,
            focused_native_id: None,
            provenance: vec![
                format!("hyprwm/Hyprland@{HYPRLAND_UPSTREAM_REVISION}"),
                "hyprctl JSON clients + public dispatchers".into(),
                HYPRLAND_PROVIDER_VERSION.into(),
            ],
        }
    }
}

impl<R: CommandRunner> WorkingEnvironmentProvider for HyprlandWorkingEnvironment<R> {
    fn provider_ref(&self) -> &ResourceRef {
        &self.provider
    }

    fn capabilities(&self) -> WorkingEnvironmentCapabilities {
        WorkingEnvironmentCapabilities {
            discover: true,
            open: false,
            focus: true,
            select: true,
            multi_project: true,
            editor_surface: true,
            terminal_surface: true,
            conversation_surface: true,
            diff_surface: true,
            preview_surface: true,
            test_surface: true,
            surface_attach_detach: true,
            agent_session_attach_detach: false,
            reconstruct: false,
        }
    }

    fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
        self.clients().map(|clients| self.observation(clients))
    }

    fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
        self.observe()
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let address = self.surface_bindings.get(surface).ok_or_else(|| {
            AikitError::new(
                "hyprland.surface_unbound",
                format!("Surface {surface} has no explicit Hyprland address binding"),
            )
        })?;
        self.dispatch_window("focuswindow", address)
    }

    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        let address = self.surface_bindings.get(surface).ok_or_else(|| {
            AikitError::new(
                "hyprland.surface_unbound",
                format!("Surface {surface} has no explicit Hyprland address binding"),
            )
        })?;
        self.dispatch_window("closewindow", address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_graphical_state_as_provider_evidence() {
        let clients = parse_hyprland_clients(
            r#"[{"address":"0xabc","class":"kitty","title":"agent","workspace":{"id":4,"name":"4"},"monitor":1,"floating":false,"mapped":true,"hidden":false}]"#,
        )
        .unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].address, "0xabc");
        assert_eq!(clients[0].workspace.as_deref(), Some("4"));
        assert_eq!(clients[0].class.as_deref(), Some("kitty"));
    }

    #[test]
    fn missing_address_is_not_allowed_to_mint_surface_identity() {
        let error = parse_hyprland_clients(r#"[{"class":"kitty"}]"#).unwrap_err();
        assert_eq!(error.code(), "hyprland.client_missing_address");
    }
}
