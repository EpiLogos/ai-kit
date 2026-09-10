//! UI-neutral working-environment observation vocabulary.
//!
//! A working-environment *provider* (tmux, cmux, Herdr, Hyprland, a desktop
//! host) performs I/O and therefore lives in `aikit-adapters`. What a provider
//! *observed* is plain data, and consumers that must never depend on an adapter
//! — `aikit-tui` above all — still have to read it. So the observation
//! vocabulary lives here, in the I/O-free core, and `aikit-adapters` re-exports
//! it so provider code keeps its established import path.
//!
//! The invariant these types carry: a provider-native id (`tmux` pane id,
//! `cmux` surface id, a window handle) is binding and provenance only. It is
//! never promoted to AIKit identity, and `canonical_ref` is populated solely by
//! an explicit caller/provider binding — never derived from `native_id`.

use serde::{Deserialize, Serialize};

use crate::resource::ResourceRef;

pub const WORKING_ENVIRONMENT_PROVIDER_VERSION: &str = "aikit.working-environment-provider/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingEnvironmentHealth {
    Healthy,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeBindingKind {
    Session,
    View,
    Surface,
    Project,
    AgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkingEnvironmentCapabilities {
    pub discover: bool,
    pub open: bool,
    pub focus: bool,
    pub select: bool,
    pub multi_project: bool,
    pub editor_surface: bool,
    pub terminal_surface: bool,
    pub conversation_surface: bool,
    pub diff_surface: bool,
    pub preview_surface: bool,
    pub test_surface: bool,
    pub surface_attach_detach: bool,
    pub agent_session_attach_detach: bool,
    pub reconstruct: bool,
}

/// One provider-native fact. `canonical_ref` is populated only by an explicit
/// caller/provider binding. It is never derived from `native_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderNativeBinding {
    pub kind: NativeBindingKind,
    pub native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_ref: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingEnvironmentObservation {
    pub schema: String,
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    pub health: WorkingEnvironmentHealth,
    pub capabilities: WorkingEnvironmentCapabilities,
    #[serde(default)]
    pub bindings: Vec<ProviderNativeBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_native_id: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl WorkingEnvironmentObservation {
    pub fn canonical_native_id(&self, canonical: &ResourceRef) -> Option<&str> {
        self.bindings
            .iter()
            .find(|binding| binding.canonical_ref.as_ref() == Some(canonical))
            .map(|binding| binding.native_id.as_str())
    }
}
