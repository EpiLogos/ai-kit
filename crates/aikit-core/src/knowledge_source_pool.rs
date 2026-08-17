//! Provider-neutral SourcePool contracts and the always-available native baseline.
//!
//! A Source is evidence/material; a WikiNode is compiled semantic knowledge. The
//! stable [`SourceRef`] never becomes a provider row/document ID. Provider
//! materialisation happens only after the privacy membrane has filtered the pool
//! for the actor/context.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const BKMR_GLADE_CONFORMANCE_VERSION: &str = "7.6.7";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceVisibility {
    Personal,
    Team,
    Public,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceBinding {
    pub source: SourceRef,
    pub revision: SourceRevision,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub visibility: SourceVisibility,
    #[serde(default)]
    pub owners: Vec<String>,
    #[serde(default = "markdown_media_type")]
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<ResourceLocator>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl SourceBinding {
    pub fn allows(&self, actor: Option<&str>, allow_team: bool) -> bool {
        match self.visibility {
            SourceVisibility::Public => true,
            SourceVisibility::Team => allow_team,
            SourceVisibility::Personal => actor
                .is_some_and(|actor| self.owners.iter().any(|owner| owner == actor)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMaterial {
    pub binding: SourceBinding,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePool {
    pub pool_ref: String,
    pub bindings: Vec<SourceBinding>,
}

impl SourcePool {
    pub fn new(pool_ref: impl Into<String>, bindings: Vec<SourceBinding>) -> Result<Self> {
        let pool_ref = pool_ref.into();
        if pool_ref.trim().is_empty() {
            return Err(AikitError::new(
                "knowledge.source_pool_invalid",
                "SourcePool ref cannot be empty",
            ));
        }
        let mut refs = BTreeSet::new();
        for binding in &bindings {
            if !refs.insert(binding.source.clone()) {
                return Err(AikitError::new(
                    "knowledge.source_pool_duplicate_ref",
                    "SourcePool contains duplicate stable SourceRefs",
                )
                .with("source", binding.source.to_string()));
            }
        }
        Ok(Self { pool_ref, bindings })
    }

    pub fn visible_to(&self, actor: Option<&str>, allow_team: bool) -> Self {
        Self {
            pool_ref: self.pool_ref.clone(),
            bindings: self
                .bindings
                .iter()
                .filter(|binding| binding.allows(actor, allow_team))
                .cloned()
                .collect(),
        }
    }

    pub fn binding(&self, source: &SourceRef) -> Option<&SourceBinding> {
        self.bindings
            .iter()
            .find(|binding| &binding.source == source)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceSearchMode {
    Fulltext,
    Semantic,
    Hybrid,
}

impl SourceSearchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fulltext => "fulltext",
            Self::Semantic => "semantic",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProviderCapabilities {
    pub provider: ProviderRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub fulltext: bool,
    pub fuzzy_interactive: bool,
    pub semantic: bool,
    pub hybrid: bool,
    pub tags: bool,
    pub structured_output: bool,
    #[serde(default)]
    pub reasons: BTreeMap<String, String>,
}

impl SourceProviderCapabilities {
    pub fn supports(&self, mode: SourceSearchMode) -> bool {
        match mode {
            SourceSearchMode::Fulltext => self.fulltext,
            SourceSearchMode::Semantic => self.semantic,
            SourceSearchMode::Hybrid => self.hybrid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProviderStatus {
    pub provider: ProviderRef,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tested_version: Option<String>,
    pub version_drift: bool,
    pub capabilities: SourceProviderCapabilities,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceHit {
    pub source: SourceRef,
    pub provider: ProviderRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub title: String,
    pub snippet: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_binding: Option<String>,
    pub retrieval_mode: SourceSearchMode,
}

pub trait SourcePoolProvider {
    fn capabilities(&self) -> SourceProviderCapabilities;

    /// Build/rebuild derived provider state from already-authorised material.
    fn rebuild(&mut self, material: &[SourceMaterial]) -> Result<()>;

    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>>;

    fn status(&self) -> SourceProviderStatus {
        let capabilities = self.capabilities();
        SourceProviderStatus {
            provider: capabilities.provider.clone(),
            available: capabilities.fulltext || capabilities.semantic || capabilities.hybrid,
            version: capabilities.version.clone(),
            tested_version: None,
            version_drift: false,
            capabilities,
            detail: String::new(),
        }
    }
}

/// Deterministic, dependency-free local correctness baseline.
///
/// This deliberately does not pretend to implement bkmr fuzzy/semantic/hybrid
/// algorithms. It provides token-aware full-text + tags so SourcePool correctness
/// survives optional provider loss.
#[derive(Debug, Clone)]
pub struct NativeSourcePoolProvider {
    provider: ProviderRef,
    material: Vec<SourceMaterial>,
}

impl NativeSourcePoolProvider {
    pub fn new() -> Self {
        Self {
            provider: ProviderRef::parse("provider/source-pool/native")
                .expect("static native SourcePool provider ref must be valid"),
            material: Vec::new(),
        }
    }
}

impl Default for NativeSourcePoolProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SourcePoolProvider for NativeSourcePoolProvider {
    fn capabilities(&self) -> SourceProviderCapabilities {
        SourceProviderCapabilities {
            provider: self.provider.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            fulltext: true,
            fuzzy_interactive: false,
            semantic: false,
            hybrid: false,
            tags: true,
            structured_output: true,
            reasons: BTreeMap::from([
                (
                    "fuzzy-interactive".into(),
                    "native baseline exposes deterministic token full-text only".into(),
                ),
                (
                    "semantic".into(),
                    "semantic retrieval requires a semantic-capable provider".into(),
                ),
                (
                    "hybrid".into(),
                    "hybrid retrieval requires a hybrid-capable provider".into(),
                ),
            ]),
        }
    }

    fn rebuild(&mut self, material: &[SourceMaterial]) -> Result<()> {
        let mut refs = BTreeSet::new();
        for item in material {
            if !refs.insert(item.binding.source.clone()) {
                return Err(AikitError::new(
                    "knowledge.source_pool_duplicate_ref",
                    "provider materialisation received duplicate stable SourceRefs",
                )
                .with("source", item.binding.source.to_string()));
            }
        }
        self.material = material.to_vec();
        Ok(())
    }

    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if mode != SourceSearchMode::Fulltext {
            return Err(AikitError::new(
                "knowledge.source_provider_capability",
                format!(
                    "native SourcePool provider does not support {}",
                    mode.as_str()
                ),
            ));
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_tokens = tokens(query);
        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }
        let required_tags: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
        let mut scored = self
            .material
            .iter()
            .filter_map(|item| {
                let actual_tags: BTreeSet<&str> =
                    item.binding.tags.iter().map(String::as_str).collect();
                if !required_tags.is_subset(&actual_tags) {
                    return None;
                }
                let title = item.binding.title.to_lowercase();
                let body = item.body.to_lowercase();
                let tag_text = item.binding.tags.join(" ").to_lowercase();
                let mut matched = 0usize;
                let mut score = 0f64;
                for token in &query_tokens {
                    let mut token_match = false;
                    if title.contains(token) {
                        score += 4.0;
                        token_match = true;
                    }
                    if tag_text.contains(token) {
                        score += 2.0;
                        token_match = true;
                    }
                    if body.contains(token) {
                        score += 1.0;
                        token_match = true;
                    }
                    if token_match {
                        matched += 1;
                    }
                }
                (matched == query_tokens.len()).then_some((score, item))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .partial_cmp(left_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.binding.source.cmp(&right.binding.source))
        });
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(score, item)| SourceHit {
                source: item.binding.source.clone(),
                provider: self.provider.clone(),
                score: Some(score),
                title: item.binding.title.clone(),
                snippet: snippet(&item.body),
                tags: item.binding.tags.clone(),
                provider_binding: None,
                retrieval_mode: mode,
            })
            .collect())
    }
}

/// Apply the SourcePool privacy membrane before handing bodies to any provider.
pub fn material_for_actor(
    pool: &SourcePool,
    material: &[SourceMaterial],
    actor: Option<&str>,
    allow_team: bool,
) -> Result<Vec<SourceMaterial>> {
    let allowed: BTreeSet<SourceRef> = pool
        .visible_to(actor, allow_team)
        .bindings
        .into_iter()
        .map(|binding| binding.source)
        .collect();
    let pool_refs: BTreeSet<SourceRef> = pool
        .bindings
        .iter()
        .map(|binding| binding.source.clone())
        .collect();
    let mut result = Vec::new();
    for item in material {
        if !pool_refs.contains(&item.binding.source) {
            return Err(AikitError::new(
                "knowledge.source_material_unknown",
                "Source material does not belong to the declared SourcePool",
            )
            .with("source", item.binding.source.to_string()));
        }
        if allowed.contains(&item.binding.source) {
            result.push(item.clone());
        }
    }
    Ok(result)
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn snippet(body: &str) -> String {
    body.chars()
        .take(240)
        .collect::<String>()
        .replace('\n', " ")
}

fn markdown_media_type() -> String {
    "text/markdown".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(
        source: &str,
        visibility: SourceVisibility,
        owners: &[&str],
        body: &str,
    ) -> SourceMaterial {
        SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse(source).unwrap(),
                revision: SourceRevision::parse("sha256:test").unwrap(),
                title: source.to_string(),
                tags: vec!["design".into()],
                visibility,
                owners: owners.iter().map(|value| (*value).to_string()).collect(),
                media_type: markdown_media_type(),
                locator: None,
                metadata: BTreeMap::new(),
            },
            body: body.into(),
        }
    }

    #[test]
    fn privacy_is_applied_before_native_provider_materialisation() {
        let public = material(
            "source:public",
            SourceVisibility::Public,
            &[],
            "semantic wiki design",
        );
        let private = material(
            "source:private",
            SourceVisibility::Personal,
            &["alice"],
            "private semantic wiki notes",
        );
        let pool = SourcePool::new(
            "pool:test",
            vec![public.binding.clone(), private.binding.clone()],
        )
        .unwrap();
        let visible =
            material_for_actor(&pool, &[public.clone(), private], Some("bob"), true).unwrap();
        assert_eq!(visible, vec![public]);

        let mut provider = NativeSourcePoolProvider::new();
        provider.rebuild(&visible).unwrap();
        let hits = provider
            .search("semantic wiki", SourceSearchMode::Fulltext, &[], 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source.as_str(), "source:public");
    }

    #[test]
    fn native_provider_discloses_optional_capability_absence() {
        let caps = NativeSourcePoolProvider::new().capabilities();
        assert!(caps.fulltext);
        assert!(caps.tags);
        assert!(!caps.semantic);
        assert!(!caps.hybrid);
        assert!(caps.reasons.contains_key("semantic"));
    }
}
