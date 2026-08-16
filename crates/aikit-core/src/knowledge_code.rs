use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resource::{ProviderRef, ResourceRef, SourceRef, SourceRevision};
use crate::Result;

/// Git/source-owned code identity. Provider UIDs (GitNexus node ids, language
/// server ids, etc.) are deliberately absent: they are bindings to this
/// canonical reference, never its identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeReference {
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub path: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
}

impl CodeReference {
    pub fn resource_ref(&self) -> ResourceRef {
        let identity = format!(
            "{}\0{}\0{}\0{}",
            self.source,
            self.path,
            self.symbol.as_deref().unwrap_or(""),
            self.kind.as_deref().unwrap_or("")
        );
        let digest = blake3::hash(identity.as_bytes()).to_hex();
        ResourceRef::parse(&format!("code:{}", &digest.as_str()[..24]))
            .expect("derived code ResourceRef must be valid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexCapabilities {
    pub provider: ProviderRef,
    pub version: Option<String>,
    pub index: bool,
    pub search: bool,
    pub context: bool,
    pub impact: bool,
    pub trace: bool,
    pub detect_changes: bool,
    pub structural_check: bool,
    pub structured_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexStatus {
    pub provider: ProviderRef,
    pub available: bool,
    pub version: Option<String>,
    pub tested_version: Option<String>,
    pub version_drift: bool,
    pub indexed: bool,
    pub capabilities: CodeIndexCapabilities,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeSearchHit {
    pub reference: CodeReference,
    pub resource: ResourceRef,
    pub title: String,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub snippet: String,
    pub provider: ProviderRef,
    /// Opaque provider node/row binding for observability only.
    #[serde(default)]
    pub provider_binding: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeContext {
    pub reference: CodeReference,
    pub provider: ProviderRef,
    pub detail: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeImpact {
    pub reference: CodeReference,
    pub provider: ProviderRef,
    pub detail: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeTrace {
    pub from: CodeReference,
    pub to: CodeReference,
    pub provider: ProviderRef,
    pub detail: Value,
}

/// Derived code intelligence behind ProjectMap. Git/source remains canonical;
/// implementations may rebuild/discard their indexes without changing code
/// identity or SemanticWiki authority.
pub trait CodeIndexProvider {
    fn capabilities(&self) -> CodeIndexCapabilities;
    fn status(&self) -> CodeIndexStatus;
    fn index(&mut self, root: &Path, force: bool) -> Result<CodeIndexStatus>;
    fn search(&self, query: &str, limit: usize) -> Result<Vec<CodeSearchHit>>;
    fn context(&self, reference: &CodeReference) -> Result<CodeContext>;
    fn impact(&self, reference: &CodeReference, direction: &str) -> Result<CodeImpact>;
    fn trace(&self, from: &CodeReference, to: &CodeReference) -> Result<CodeTrace>;
}

/// Current upstream GitNexus CLI contract explicitly accepted by AIKit.
pub const GITNEXUS_TESTED_VERSION: &str = "1.6.9";
