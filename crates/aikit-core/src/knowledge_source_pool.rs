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

use crate::context_source::{ContextSourcePrivacy, RetrievalTarget};
use crate::resource::{ProviderRef, ResourceLocator, ResourceSource, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const BKMR_GLADE_CONFORMANCE_VERSION: &str = "7.6.7";

/// Semantic lineage in the existing material metadata carrier. Physical owner
/// routes remain transient IO facts; this is neither an audience grant nor a
/// second source registry.
pub const SOURCE_ORIGIN_METADATA: &str = "aikit.source-origin/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceOrigin {
    pub schema: String,
    pub origin: SourceOriginKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceOriginKind {
    /// The IO caller selected this authored corpus. This does not prove that a
    /// subsequently discovered copy is independently accepted or still eligible.
    DeclaredCorpus,
    NativeSource {
        world_ref: String,
        source: ResourceSource,
        observed_binding: NativeOriginBinding,
    },
}

/// Body-free facts supplied by the native source owner. Retrieval permission
/// is an observation at that owner boundary, not permission to promote a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOriginBinding {
    pub roles: Vec<String>,
    pub provenance: String,
    pub standing: String,
    pub treatment: String,
    pub agent_retrieval_allowed: bool,
}

impl SourceOrigin {
    pub fn declared_corpus() -> Self {
        Self { schema: SOURCE_ORIGIN_METADATA.into(), origin: SourceOriginKind::DeclaredCorpus }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SOURCE_ORIGIN_METADATA {
            return Err(AikitError::new("knowledge.source_origin_invalid", "Unsupported source origin schema"));
        }
        if let SourceOriginKind::NativeSource { world_ref, source, observed_binding } = &self.origin {
            if world_ref.trim().is_empty() || source.revision.is_none()
                || source.locator.is_some() || observed_binding.provenance.trim().is_empty()
                || observed_binding.standing.trim().is_empty() || observed_binding.treatment.trim().is_empty()
            {
                return Err(AikitError::new("knowledge.source_origin_invalid",
                    "Native semantic origin needs the actual World, revision and binding without a physical locator"));
            }
        }
        Ok(())
    }

    pub fn disclosure_projection(&self) -> Result<Value> {
        self.validate()?;
        serde_json::to_value(self).map_err(|error| {
            AikitError::new("knowledge.source_origin_invalid", error.to_string())
        })
    }
}

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
    pub fn source_origin(&self) -> Result<Option<SourceOrigin>> {
        let Some(value) = self.metadata.get(SOURCE_ORIGIN_METADATA) else { return Ok(None); };
        let origin: SourceOrigin = serde_json::from_value(value.clone()).map_err(|error| {
            AikitError::new("knowledge.source_origin_invalid", error.to_string())
                .with("source", self.source.to_string())
        })?;
        origin.validate()?;
        Ok(Some(origin))
    }

    pub fn set_source_origin(&mut self, origin: SourceOrigin) -> Result<()> {
        self.metadata.insert(SOURCE_ORIGIN_METADATA.into(), origin.disclosure_projection()?);
        Ok(())
    }

    /// Only the ordinary already-authorised in-memory API permits held-body
    /// fallback. A declared or native origin and the legacy owner carrier do not.
    pub fn requires_live_origin_read(&self) -> Result<bool> {
        // The legacy Work producer explicitly wrote false here. Its retained
        // Project/member address has no current owner witness, so that bit
        // cannot turn the copied body into independent authorised material.
        let work_origin = if let Some(value) = self.metadata.get("work-repos") {
            if !value.as_object().is_some_and(|object| {
                object.get("project").and_then(Value::as_str).is_some_and(|value| !value.is_empty())
                    && object.get("project_id").and_then(Value::as_str).is_some_and(|value| !value.is_empty())
            }) {
                return Err(AikitError::new("knowledge.source_origin_invalid",
                    "A claimed Work producer has no attributable Project carrier")
                    .with("source", self.source.to_string()));
            }
            true
        } else { false };
        Ok(work_origin || self.source_origin()?.is_some()
            || self.metadata.get("owner_read_required") == Some(&Value::Bool(true))
            || self.metadata.contains_key("central")
            || self.metadata.contains_key("local_route"))
    }

    /// An outward view is separate from canonical material persistence. The
    /// legacy `central` payload contains physical routing and sometimes a body.
    pub fn disclosure_projection(&self) -> Result<Value> {
        let origin = self.source_origin()?;
        let mut projected = self.clone();
        if self.requires_live_origin_read()? {
            projected.locator = None;
            projected.metadata.remove("relative_path");
        }
        projected.metadata.remove("central");
        projected.metadata.remove("local_route");
        if let Some(origin) = origin {
            projected.metadata.insert(SOURCE_ORIGIN_METADATA.into(), origin.disclosure_projection()?);
        }
        serde_json::to_value(projected).map_err(|error| {
            AikitError::new("knowledge.source_origin_invalid", error.to_string())
        })
    }

    pub fn allows(&self, actor: Option<&str>, allow_team: bool) -> bool {
        match self.visibility {
            SourceVisibility::Public => true,
            SourceVisibility::Team => allow_team,
            SourceVisibility::Personal => {
                actor.is_some_and(|actor| self.owners.iter().any(|owner| owner == actor))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMaterial {
    pub binding: SourceBinding,
    pub body: String,
}

/// An operation-local current reading. Privacy is supplied by the owning
/// read relation; it is not a persisted SourceBinding grant.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePoolReading {
    pub material: SourceMaterial,
    pub privacy: ContextSourcePrivacy,
}

impl SourcePoolReading {
    /// Adapt the native ContextSource boundary without another privacy policy.
    pub fn check_target(privacy: ContextSourcePrivacy, target: RetrievalTarget) -> Result<()> {
        if let Some(boundary) = privacy.payload_boundary(target) {
            return Err(AikitError::new(
                "knowledge.source_target_withheld",
                boundary.reason.clone(),
            ).with("boundary", serde_json::to_string(&boundary).expect("absence is serialisable")));
        }
        Ok(())
    }

    pub fn admit(self, target: RetrievalTarget) -> Result<SourceMaterial> {
        Self::check_target(self.privacy, target)?;
        Ok(self.material)
    }
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
    /// Exact Source basis supplied by the search owner. A provider binding or
    /// matched line is not a revision of the selected Source payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<SourceRevision>,
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

    /// Live owner read, where this provider attaches to persistent native
    /// source rather than materialising a disposable local index. None keeps
    /// the existing in-memory-provider contract; an owner error never falls back.
    fn read(&self, _source: &SourceRef) -> Result<Option<SourceMaterial>> {
        Ok(None)
    }

    /// Selected payload delivery to this operation's actual target. The default
    /// preserves already-authorised independent local providers, without deriving
    /// external egress or native-current permission from copied visibility labels.
    fn read_for(&self, source: &SourceRef, target: RetrievalTarget) -> Result<Option<SourcePoolReading>> {
        // A declined live read is not a target refusal by this provider.
        // Internal already-authorised local retrieval is distinct from
        // delivering its returned payload to the operation's target.
        let Some(material) = self.read(source)? else { return Ok(None); };
        let privacy = ContextSourcePrivacy::default();
        SourcePoolReading::check_target(privacy, target)?;
        if material.binding.requires_live_origin_read()? {
            return Err(AikitError::new("knowledge.source_target_unavailable",
                "Origin-bound material needs its owning target-aware current read")
                .with("source", source.to_string()));
        }
        Ok(Some(SourcePoolReading { material, privacy }))
    }

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
        let required_tags: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
        // An empty query is only a no-op when nothing else narrows the pool.
        // With a tag filter it is an ordinary browse — "everything carrying
        // these tags" — and returning nothing would make the tag filter look
        // broken rather than empty.
        if query_tokens.is_empty() && required_tags.is_empty() {
            return Ok(Vec::new());
        }
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
                revision: Some(item.binding.revision.clone()),
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

    #[test]
    fn index_only_provider_declines_live_ownership_before_target_refusal() {
        use crate::{FamiliarityContext, KnowledgeAddress, KnowledgeApplication};
        let body = "Caller-authorised current material for an independent local pool";
        let mut supplied = material("source:pure:current", SourceVisibility::Public, &[], body);
        supplied.binding.revision = SourceRevision::parse(
            crate::knowledge_ingest::corpus_content_revision(body.as_bytes()),
        ).unwrap();
        let supplied = vec![supplied];
        let mut provider = NativeSourcePoolProvider::new();
        provider.rebuild(&supplied).unwrap();
        let unrelated = SourceRef::parse("source:pure:another-owner").unwrap();
        for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent, RetrievalTarget::ExternalProvider] {
            assert!(provider.read_for(&unrelated, target).unwrap().is_none());
        }
        let address = KnowledgeAddress::Source(supplied[0].binding.source.clone());
        let local = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &supplied);
        assert_eq!(local.read(&address).unwrap().content.as_deref(), Some(body));
        let hits = provider.search("independent local", SourceSearchMode::Fulltext, &[], 8).unwrap();
        assert_eq!(hits[0].revision.as_ref(), Some(&supplied[0].binding.revision));
        let external = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &supplied)
            .with_retrieval_target(RetrievalTarget::ExternalProvider);
        assert_eq!(external.read(&address).unwrap_err().code(), "knowledge.source_target_withheld");
    }

    #[test]
    fn an_old_real_index_match_is_not_relabelled_by_new_supplied_material() {
        use crate::{FamiliarityContext, KnowledgeAddress, KnowledgeApplication};
        let mut old = material("source:pure:changing", SourceVisibility::Public, &[],
            "Retiredneedle belongs to the earlier source body");
        old.binding.revision = SourceRevision::parse(
            crate::knowledge_ingest::corpus_content_revision(old.body.as_bytes()),
        ).unwrap();
        let mut current = old.clone();
        current.body = "Currentneedle belongs to the current source body".into();
        current.binding.revision = SourceRevision::parse(
            crate::knowledge_ingest::corpus_content_revision(current.body.as_bytes()),
        ).unwrap();
        let mut provider = NativeSourcePoolProvider::new();
        provider.rebuild(&[old]).unwrap();
        let current = vec![current];
        let address = KnowledgeAddress::Source(current[0].binding.source.clone());
        let application = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &current);
        assert_eq!(application.read(&address).unwrap().content.as_deref(), Some(current[0].body.as_str()));
        let stale = application.search("Retiredneedle", 8);
        assert!(stale.hits.iter().all(|hit| hit.address != address));
        assert!(stale.absences.iter().any(|absence| absence.contains("knowledge.source_origin_revision_conflict")));
        drop(application);
        provider.rebuild(&current).unwrap();
        let fresh = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &current);
        assert!(fresh.search("Currentneedle", 8).hits.iter().any(|hit| hit.address == address));
        assert!(fresh.search("Retiredneedle", 8).hits.iter().all(|hit| hit.address != address));
    }
}

#[cfg(test)]
mod work_producer_current_read_tests {
    use super::*;

    fn retained() -> SourceMaterial {
        SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse("source:project:a:b:c.md").unwrap(),
                revision: SourceRevision::parse("retained-revision").unwrap(),
                title: "retained historical Work file".into(), tags: vec![],
                visibility: SourceVisibility::Personal, owners: vec![],
                media_type: "text/markdown".into(), locator: None,
                metadata: BTreeMap::from([
                    ("work-repos".into(), serde_json::json!({"project":"Alpha","project_id":"a:b"})),
                    ("owner_read_required".into(), Value::Bool(false)),
                ]),
            },
            body: "Retained bytes are history, not a current source witness.".into(),
        }
    }

    #[test]
    fn actual_legacy_work_carrier_requires_current_read_despite_its_false_bit() {
        let material = retained();
        assert!(material.binding.requires_live_origin_read().unwrap());
        let mut index = NativeSourcePoolProvider::new();
        index.rebuild(std::slice::from_ref(&material)).unwrap();
        let held = vec![material.clone()];
        let app = crate::KnowledgeApplication::new(crate::FamiliarityContext::default()).with_source_pool(&index, &held);
        let address = crate::KnowledgeAddress::Source(material.binding.source.clone());
        assert_eq!(app.read(&address).unwrap_err().code(), "knowledge.source_origin_unavailable");
        assert_eq!(app.explain(&address).unwrap_err().code(), "knowledge.source_origin_unavailable");
        assert_eq!(app.route(None,std::slice::from_ref(&address)).unwrap_err().code(), "knowledge.source_origin_unavailable");
        let found = app.search("Retained bytes", 8);
        assert!(found.hits.is_empty());
        assert!(found.absences.iter().any(|line| line.contains("knowledge.source_origin_unavailable")));
        assert_eq!(material.body, "Retained bytes are history, not a current source witness.");
    }

    #[test]
    fn source_namespace_alone_does_not_turn_native_root_or_pure_material_into_work() {
        let mut material = retained();
        material.binding.metadata.clear();
        material.binding.source = SourceRef::parse("source:project:a:b:root").unwrap();
        assert!(!material.binding.requires_live_origin_read().unwrap());
        let mut index = NativeSourcePoolProvider::new();
        index.rebuild(std::slice::from_ref(&material)).unwrap();
        let held = vec![material.clone()];
        let address = crate::KnowledgeAddress::Source(material.binding.source.clone());
        let app = crate::KnowledgeApplication::new(crate::FamiliarityContext::default()).with_source_pool(&index, &held);
        assert_eq!(app.read(&address).unwrap().content.as_deref(),Some(material.body.as_str()));
        // This is the explicit authorised pure API, not native Root proof.
        let external = crate::KnowledgeApplication::new(crate::FamiliarityContext::default()).with_source_pool(&index,&held)
            .with_retrieval_target(RetrievalTarget::ExternalProvider);
        assert_eq!(external.read(&address).unwrap_err().code(),"knowledge.source_target_withheld");
    }

    #[test]
    fn malformed_claimed_work_carrier_is_unresolved_not_pure_fallback() {
        let mut material = retained();
        material.binding.metadata.insert("work-repos".into(), serde_json::json!({"project":"Alpha"}));
        assert_eq!(material.binding.requires_live_origin_read().unwrap_err().code(), "knowledge.source_origin_invalid");
    }
}
