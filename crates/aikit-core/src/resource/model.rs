use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Capability,
    Action,
    Agent,
    Agency,
    ContextSource,
    Model,
    Harness,
    Host,
    ExecutionOffer,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Action => "action",
            Self::Agent => "agent",
            Self::Agency => "agency",
            Self::ContextSource => "context-source",
            Self::Model => "model",
            Self::Harness => "harness",
            Self::Host => "host",
            Self::ExecutionOffer => "execution-offer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum ResourceLocator {
    Path(PathBuf),
    Uri(String),
    Opaque(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceAuthority {
    Authored,
    Observed,
    Derived,
    Learned,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SourceState {
    /// A source reference was imported, but AIKit has not observed its current availability.
    Unresolved,
    Available,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSource {
    pub source: SourceRef,
    #[serde(default)]
    pub authority: Option<SourceAuthority>,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    #[serde(default)]
    pub locator: Option<ResourceLocator>,
    pub state: SourceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ProviderState {
    /// A provider reference was declared, but AIKit has not resolved a live offer yet.
    Unresolved,
    Available,
    /// The provider remains usable, but one or more advertised faculties are impaired.
    /// Degradation is therefore operationally different from absence and remains
    /// inspectable rather than being flattened into Available.
    Degraded { reason: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOffer {
    pub provider: ProviderRef,
    #[serde(default)]
    pub locator: Option<ResourceLocator>,
    pub state: ProviderState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Eligibility {
    #[default]
    Undetermined,
    Eligible,
    Ineligible { reasons: Vec<String> },
}

impl Eligibility {
    pub fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenceIntent {
    pub source: SourceRef,
    pub rank: i32,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub id: ResourceRef,
    pub kind: ResourceKind,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub owner: Option<OwnerRef>,
    #[serde(default)]
    pub sources: Vec<ResourceSource>,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
}

impl ResourceDescriptor {
    pub fn new(
        id: ResourceRef,
        kind: ResourceKind,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id,
            kind,
            name: name.into(),
            description: description.into(),
            owner: None,
            sources: Vec::new(),
            annotations: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRecord {
    pub descriptor: ResourceDescriptor,
    #[serde(default)]
    pub providers: Vec<ProviderOffer>,
    #[serde(default)]
    pub eligibility: Eligibility,
    #[serde(default)]
    pub preference: Option<PreferenceIntent>,
}

impl ResourceRecord {
    pub fn new(descriptor: ResourceDescriptor) -> Self {
        Self {
            descriptor,
            providers: Vec::new(),
            eligibility: Eligibility::Undetermined,
            preference: None,
        }
    }

    pub fn explanation(&self) -> ResourceExplanation {
        ResourceExplanation {
            id: self.descriptor.id.clone(),
            kind: self.descriptor.kind,
            owner: self.descriptor.owner.clone(),
            sources: self.descriptor.sources.clone(),
            providers: self.providers.clone(),
            eligibility: self.eligibility.clone(),
            preference: self.preference.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceExplanation {
    pub id: ResourceRef,
    pub kind: ResourceKind,
    #[serde(default)]
    pub owner: Option<OwnerRef>,
    pub sources: Vec<ResourceSource>,
    pub providers: Vec<ProviderOffer>,
    pub eligibility: Eligibility,
    #[serde(default)]
    pub preference: Option<PreferenceIntent>,
}
