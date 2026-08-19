use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Project,
    Profile,
    SkillSet,
    Method,
    Capability,
    Action,
    Procedure,
    Agent,
    Agency,
    ContextSource,
    Model,
    Harness,
    /// Addressable composable runtime unit. A Component may expose powers or
    /// Surfaces but never becomes their semantic identity merely by providing them.
    Component,
    /// Runtime service/interface seam against which providers and consumers bind.
    Contract,
    /// Encounter/operation locus (CLI, tool, TUI region, API, trajectory, etc.).
    Surface,
    Host,
    ExecutionOffer,
    KnowledgeSpace,
    KnowledgeNode,
    KnowledgeSource,
    KnowledgeFrame,
    KnowledgeRoute,
    CodeReference,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Profile => "profile",
            Self::SkillSet => "skill-set",
            Self::Method => "method",
            Self::Capability => "capability",
            Self::Action => "action",
            Self::Procedure => "procedure",
            Self::Agent => "agent",
            Self::Agency => "agency",
            Self::ContextSource => "context-source",
            Self::Model => "model",
            Self::Harness => "harness",
            Self::Component => "component",
            Self::Contract => "contract",
            Self::Surface => "surface",
            Self::Host => "host",
            Self::ExecutionOffer => "execution-offer",
            Self::KnowledgeSpace => "knowledge-space",
            Self::KnowledgeNode => "knowledge-node",
            Self::KnowledgeSource => "knowledge-source",
            Self::KnowledgeFrame => "knowledge-frame",
            Self::KnowledgeRoute => "knowledge-route",
            Self::CodeReference => "code-reference",
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
