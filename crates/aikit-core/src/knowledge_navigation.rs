use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::familiarity::{AccessibilityAssessment, FamiliarityContext};
use crate::knowledge::{
    KnowledgeContextPack, KnowledgeReading, KnowledgeRelationView, KnowledgeRoute,
    KnowledgeRouteStep, RelationDirection, RelationEdge, RelationNode, RelationOrigin,
    RelationQuery,
};
use crate::knowledge_code::{CodeIndexProvider, CodeReference};
use crate::knowledge_source_pool::{
    SourceMaterial, SourcePoolProvider, SourceProviderStatus, SourceSearchMode,
};
use crate::knowledge_wiki_index::WikiSearchAddress;
use crate::knowledge_wiki_provider::{SemanticWikiProviderStatus, WikiProvider};
use crate::project_map::{ProjectLens, ProjectMap, ProjectMapEndpoint, ProjectMapStep};
use crate::resource::{
    horizons_for_kind, parse_or_search_expression, resolve_path_identity, ProviderRef,
    ResolveExpression, ResourceKind, ResourceRef, SourceAuthority, SourceRef,
};
use crate::{AikitError, Result};

pub const KNOWLEDGE_APPLICATION_VERSION: &str = "aikit.knowledge-application/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum KnowledgeAddress {
    Wiki(ResourceRef),
    Source(SourceRef),
    Code(CodeReference),
    /// Stable endpoint in the ProjectMap federation when the endpoint is not
    /// materialised through a richer native provider address in this process.
    ProjectMap(ResourceRef),
}

impl KnowledgeAddress {
    pub fn resource_ref(&self) -> ResourceRef {
        match self {
            Self::Wiki(resource) | Self::ProjectMap(resource) => resource.clone(),
            Self::Source(source) => ResourceRef::parse(source.as_str())
                .expect("SourceRef validation is compatible with ResourceRef validation"),
            Self::Code(reference) => reference.resource_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSearchHit {
    pub address: KnowledgeAddress,
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    /// Provider-native relevance score. Learned accessibility never overwrites it.
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    pub provider: ProviderRef,
    pub authority: SourceAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranking: Option<KnowledgeRankingEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeRankingEvidence {
    pub provider_score: f64,
    pub navigation_score: f64,
    pub destination: AccessibilityAssessment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<AccessibilityAssessment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSearchResult {
    pub query: String,
    /// The operative Resolve expression this retrieval expressed through.
    ///
    /// Knowledge has no second query path: every hit below was produced by
    /// evaluating this expression against the federated providers. A raw string
    /// is still permitted as *input* — it is lowered to `@# (@ text)` — but the
    /// retrieval itself is the resolver's, and this field is the receipt.
    pub expression: ResolveExpression,
    /// `resolve_path_identity(&expression)` — the one path identity, shared with
    /// `aikit search` so learned evidence rides one path rather than two.
    pub path_identity: String,
    pub hits: Vec<KnowledgeSearchHit>,
    #[serde(default)]
    pub absences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeProviderStatus {
    pub version: String,
    #[serde(default)]
    pub wiki: Option<SemanticWikiProviderStatus>,
    #[serde(default)]
    pub sources: Vec<SourceProviderStatus>,
    #[serde(default)]
    pub code: Option<crate::knowledge_code::CodeIndexStatus>,
    pub project_map: bool,
    #[serde(default)]
    pub absences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeExplanation {
    pub address: KnowledgeAddress,
    pub provider: Option<ProviderRef>,
    pub authority: SourceAuthority,
    pub summary: String,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    #[serde(default)]
    pub detail: Option<Value>,
}

/// Split a query into its free text and its `tag:<value>` filters.
///
/// The filter form is explicit rather than inferred: a bare `#word` is
/// ordinary prose in a corpus that writes markdown headings, and guessing
/// would turn text searches into silently-narrowed ones. `tag:` is dropped
/// from the text so it is not also matched as a word.
pub fn split_tag_filters(query: &str) -> (String, Vec<String>) {
    let mut text: Vec<&str> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    for term in query.split_whitespace() {
        match term.strip_prefix("tag:") {
            Some(tag) if !tag.is_empty() => {
                let tag = tag.to_owned();
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            _ => text.push(term),
        }
    }
    (text.join(" "), tags)
}

pub struct SourcePoolBinding<'a> {
    pub provider: &'a dyn SourcePoolProvider,
    pub material: &'a [SourceMaterial],
}

/// One project-scoped application field over independent Knowledge providers.
///
/// This is federation, not a universal graph: providers retain their relation,
/// ranking and identity semantics. The application only normalises addressability,
/// degradation, explicit ProjectMap cross-lens bindings, operational routes and
/// Context projection.
pub struct KnowledgeApplication<'a> {
    context: FamiliarityContext,
    wiki: Option<Box<dyn WikiProvider + 'a>>,
    sources: Vec<SourcePoolBinding<'a>>,
    code: Option<&'a dyn CodeIndexProvider>,
    project_map: Option<&'a ProjectMap>,
}

impl<'a> KnowledgeApplication<'a> {
    pub fn new(context: FamiliarityContext) -> Self {
        Self {
            context,
            wiki: None,
            sources: Vec::new(),
            code: None,
            project_map: None,
        }
    }

    #[must_use]
    pub fn with_wiki(mut self, wiki: impl WikiProvider + 'a) -> Self {
        self.wiki = Some(Box::new(wiki));
        self
    }

    #[must_use]
    pub fn with_source_pool(
        mut self,
        provider: &'a dyn SourcePoolProvider,
        material: &'a [SourceMaterial],
    ) -> Self {
        self.sources.push(SourcePoolBinding { provider, material });
        self
    }

    #[must_use]
    pub fn with_code(mut self, provider: &'a dyn CodeIndexProvider) -> Self {
        self.code = Some(provider);
        self
    }

    #[must_use]
    pub fn with_project_map(mut self, project_map: &'a ProjectMap) -> Self {
        self.project_map = Some(project_map);
        self
    }

    pub fn status(&self) -> KnowledgeProviderStatus {
        let wiki = self.wiki.as_ref().map(|provider| provider.status());
        let sources = self
            .sources
            .iter()
            .map(|binding| binding.provider.status())
            .collect::<Vec<_>>();
        let code = self.code.map(|provider| provider.status());
        let mut absences = Vec::new();
        if wiki.is_none() {
            absences.push("SemanticWiki provider absent".into());
        }
        if sources.is_empty() {
            absences.push("SourcePool provider absent".into());
        }
        if code.is_none() {
            absences.push("ProjectMap CodeIndex provider absent".into());
        }
        if self.project_map.is_none() {
            absences.push("ProjectMap federation absent".into());
        }
        KnowledgeProviderStatus {
            version: KNOWLEDGE_APPLICATION_VERSION.into(),
            wiki,
            sources,
            code,
            project_map: self.project_map.is_some(),
            absences,
        }
    }

    /// Human/shell front for [`Self::resolve`].
    ///
    /// A plain typed string remains legitimate *input*; a second retrieval path
    /// does not exist. This parses the input through the one operative grammar
    /// and delegates — it performs no retrieval of its own. Input the grammar
    /// cannot read is disclosed and lowered to a single ordinary search subject
    /// rather than silently answering a different question.
    pub fn search(&self, query: &str, limit: usize) -> KnowledgeSearchResult {
        let (expression, parse_absence) = match parse_or_search_expression(query) {
            Ok(expression) => (expression, None),
            Err(error) => (
                ResolveExpression::ordinary_search(query),
                Some(format!(
                    "Resolve expression did not parse ({}); the input was read as one ordinary \
                     search subject",
                    error.message()
                )),
            ),
        };
        let mut result = self.resolve(&expression, limit);
        // The caller asked in its own words; the receipt of *how* it was
        // resolved rides `expression`/`path_identity` beside it.
        result.query = query.into();
        if let Some(absence) = parse_absence {
            result.absences.insert(0, absence);
        }
        result
    }

    /// Canonical Knowledge retrieval: evaluate one operative Resolve expression
    /// against the federated providers.
    ///
    /// This is the same contract `aikit search` resolves through — address
    /// horizons narrow, relations combine, and the path identity is minted once.
    /// Providers keep their own relevance; the expression decides what is asked.
    pub fn resolve(&self, expression: &ResolveExpression, limit: usize) -> KnowledgeSearchResult {
        let path_identity = resolve_path_identity(expression);
        if limit == 0 {
            return KnowledgeSearchResult {
                query: expression.render(),
                expression: expression.clone(),
                path_identity,
                hits: Vec::new(),
                absences: Vec::new(),
            };
        }
        let mut absences = Vec::new();
        let mut hits = self.evaluate(expression, limit, &mut absences);

        // ProjectMap is a federation fallback, not a richer operational
        // address. If a provider-native hit for the same canonical ResourceRef
        // is already present, keep that native address while leaving provider
        // relevance to rank distinct resources and duplicate native providers.
        let native_resources = hits
            .iter()
            .filter(|hit| !matches!(hit.address, KnowledgeAddress::ProjectMap(_)))
            .map(|hit| hit.resource.to_string())
            .collect::<HashSet<_>>();
        hits.retain(|hit| {
            !matches!(hit.address, KnowledgeAddress::ProjectMap(_))
                || !native_resources.contains(&hit.resource.to_string())
        });
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.resource.cmp(&right.resource))
        });
        let mut seen = HashSet::new();
        hits.retain(|hit| seen.insert(hit.resource.to_string()));
        hits.truncate(limit);
        // A relation evaluates each side against the same provider field, so a
        // shared absence is one absence, reported once.
        let mut disclosed = HashSet::new();
        absences.retain(|absence| disclosed.insert(absence.clone()));
        KnowledgeSearchResult {
            query: expression.render(),
            expression: expression.clone(),
            path_identity,
            hits,
            absences,
        }
    }

    /// Walk the expression. The shape mirrors the Resource-field resolver:
    /// subjects reach providers, an address narrows by horizon, a relation
    /// combines, a frame groups.
    fn evaluate(
        &self,
        expression: &ResolveExpression,
        limit: usize,
        absences: &mut Vec<String>,
    ) -> Vec<KnowledgeSearchHit> {
        match expression {
            ResolveExpression::Subject { value } => self.subject_hits(value, limit, absences),
            ResolveExpression::Address {
                horizon,
                expression,
            } => {
                let mut hits = self.evaluate(expression, limit, absences);
                if let Some(horizon) = horizon {
                    // The horizon table is the Resource field's own, read here
                    // from the hit's canonical kind — not a second reading.
                    hits.retain(|hit| horizons_for_kind(hit.kind).contains(horizon));
                }
                hits
            }
            ResolveExpression::Unary { expression, .. }
            | ResolveExpression::Frame { expression } => self.evaluate(expression, limit, absences),
            ResolveExpression::Binary { left, right, .. } => {
                let mut hits = self.evaluate(left, limit, absences);
                hits.extend(self.evaluate(right, limit, absences));
                hits
            }
        }
    }

    /// Reach every federated provider for one subject term.
    fn subject_hits(
        &self,
        query: &str,
        limit: usize,
        absences: &mut Vec<String>,
    ) -> Vec<KnowledgeSearchHit> {
        let mut hits = Vec::new();

        let mut unreadable: Vec<SourceRef> = Vec::new();
        if let Some(wiki) = &self.wiki {
            hits.extend(wiki.search(query, limit).into_iter().map(|hit| {
                match &hit.address {
                    // A curated Wiki object keeps the Wiki address and the
                    // KnowledgeNode/Space/Frame kind it always had.
                    WikiSearchAddress::Curated { resource } => {
                        let kind = match hit.object.as_str() {
                            "space" => ResourceKind::KnowledgeSpace,
                            "frame" => ResourceKind::KnowledgeFrame,
                            _ => ResourceKind::KnowledgeNode,
                        };
                        KnowledgeSearchHit {
                            address: KnowledgeAddress::Wiki(resource.clone()),
                            resource: resource.clone(),
                            kind,
                            label: hit.label,
                            score: 1.0 / (1.0 + f64::from(hit.score)),
                            snippet: hit.summary,
                            provider: wiki.status().provider,
                            authority: SourceAuthority::Authored,
                            ranking: None,
                        }
                    }
                    // A source cited by a curated node is findable, but it is
                    // not itself curated Wiki identity: it reaches the
                    // product surface as a Source address, addressable by
                    // the CLI's own `source=REF` form. Unlike a SourcePool
                    // hit — an eligible, `Observed` project artefact — this
                    // is the owner's own authored citation, so it keeps
                    // `Authored` authority; only its provenance house
                    // differs from a curated Wiki object.
                    WikiSearchAddress::AuthoredSource { source } => {
                        // Findability must not outrun openability in silence.
                        // If this horizon cannot materialise the source, the
                        // result says so here — at the point the address is
                        // handed over — rather than letting the caller
                        // discover it only by trying to read.
                        if self.source_material(source).is_none() {
                            unreadable.push(source.clone());
                        }
                        let resource = ResourceRef::parse(source.as_str())
                            .expect("SourceRef validation is compatible with ResourceRef validation");
                        KnowledgeSearchHit {
                            address: KnowledgeAddress::Source(source.clone()),
                            resource,
                            kind: ResourceKind::KnowledgeSource,
                            label: hit.label,
                            score: 1.0 / (1.0 + f64::from(hit.score)),
                            snippet: hit.summary,
                            provider: wiki.status().provider,
                            authority: SourceAuthority::Authored,
                            ranking: None,
                        }
                    }
                }
            }));
        } else {
            absences.push("SemanticWiki search unavailable: provider absent".into());
        }
        for source in &unreadable {
            absences.push(format!(
                "Authored source {source} is cited by the Wiki but not materialised in this \
                 horizon: findable and explainable, not readable"
            ));
        }

        // A `tag:<value>` term narrows the SourcePool rather than being
        // matched as text. This is the one tag facility AIKit has: the
        // corpus's authored vocabulary rides `SourceBinding::tags`, and
        // every provider — the native baseline and bkmr's `--tags` alike —
        // already filters on it. Passing an empty filter here, as this call
        // did, left that facility implemented, capability-detected and never
        // reachable from a query.
        let (source_query, tag_filter) = split_tag_filters(query);
        for binding in &self.sources {
            let status = binding.provider.status();
            if !status.available {
                absences.push(format!(
                    "SourcePool search unavailable from {}",
                    status.provider
                ));
                continue;
            }
            if !tag_filter.is_empty() && !status.capabilities.tags {
                // Dropping the filter and searching anyway would answer a
                // narrower question than the one that was asked, silently.
                absences.push(format!(
                    "SourcePool provider {} cannot filter by tag; the tag filter [{}] was not \
                     applied and this provider was not searched",
                    status.provider,
                    tag_filter.join(", ")
                ));
                continue;
            }
            let mode = if status.capabilities.hybrid {
                SourceSearchMode::Hybrid
            } else {
                SourceSearchMode::Fulltext
            };
            match binding
                .provider
                .search(&source_query, mode, &tag_filter, limit)
            {
                Ok(provider_hits) => hits.extend(provider_hits.into_iter().map(|hit| {
                    let resource = ResourceRef::parse(hit.source.as_str())
                        .expect("SourceRef is a valid ResourceRef");
                    KnowledgeSearchHit {
                        address: KnowledgeAddress::Source(hit.source),
                        resource,
                        kind: ResourceKind::KnowledgeSource,
                        label: hit.title,
                        score: hit.score.unwrap_or(0.5),
                        snippet: hit.snippet,
                        provider: hit.provider,
                        authority: SourceAuthority::Observed,
                        ranking: None,
                    }
                })),
                Err(error) => absences.push(format!(
                    "SourcePool search degraded for {}: {}",
                    status.provider,
                    error.message()
                )),
            }
        }

        if let Some(code) = self.code {
            let status = code.status();
            if status.available && status.indexed && status.capabilities.search {
                match code.search(query, limit) {
                    Ok(code_hits) => {
                        hits.extend(code_hits.into_iter().map(|hit| KnowledgeSearchHit {
                            address: KnowledgeAddress::Code(hit.reference.clone()),
                            resource: hit.resource,
                            kind: ResourceKind::CodeReference,
                            label: hit.title,
                            score: hit.score.unwrap_or(0.5),
                            snippet: hit.snippet,
                            provider: hit.provider,
                            authority: SourceAuthority::Derived,
                            ranking: None,
                        }))
                    }
                    Err(error) => absences.push(format!(
                        "ProjectMap code search degraded: {}",
                        error.message()
                    )),
                }
            } else {
                absences
                    .push("ProjectMap code search unavailable: index absent or degraded".into());
            }
        } else {
            absences.push("ProjectMap code search unavailable: provider absent".into());
        }

        if let Some(project_map) = self.project_map {
            let needle = query.to_lowercase();
            hits.extend(project_map.endpoints().filter_map(|endpoint| {
                let label = endpoint
                    .label
                    .clone()
                    .unwrap_or_else(|| endpoint.resource.to_string());
                let searchable =
                    format!("{} {} {:?}", endpoint.resource, label, endpoint.lens).to_lowercase();
                if !needle.is_empty() && !searchable.contains(&needle) {
                    return None;
                }
                Some(KnowledgeSearchHit {
                    address: KnowledgeAddress::ProjectMap(endpoint.resource.clone()),
                    resource: endpoint.resource.clone(),
                    kind: endpoint.kind,
                    label,
                    score: if endpoint.resource.as_str() == query {
                        1.25
                    } else {
                        0.4
                    },
                    snippet: format!("explicit {:?} ProjectMap endpoint", endpoint.lens),
                    provider: endpoint
                        .provider
                        .clone()
                        .unwrap_or_else(project_map_provider),
                    authority: endpoint.authority,
                    ranking: None,
                })
            }));
        } else {
            absences.push("ProjectMap endpoint search unavailable: federation absent".into());
        }

        hits
    }

    pub fn read(&self, address: &KnowledgeAddress) -> Result<KnowledgeReading> {
        match address {
            KnowledgeAddress::Wiki(resource) => self
                .wiki
                .as_ref()
                .ok_or_else(|| provider_absent("SemanticWiki"))?
                .read(resource),
            KnowledgeAddress::Source(source) => {
                let (binding, material) = self.source_material(source).ok_or_else(|| {
                    // Search can hand back a source a curated node cites. If
                    // this horizon cannot materialise it, say which citation
                    // it came from rather than reporting it simply missing.
                    let citing = self.wiki_citations(source);
                    if citing.is_empty() {
                        AikitError::new(
                            "knowledge.source_missing",
                            format!("Source {source} is not materialised in the project horizon"),
                        )
                    } else {
                        Self::unmaterialised_cited_source(source, &citing)
                    }
                })?;
                let live = binding.provider.read(source)?;
                let material = live.as_ref().unwrap_or(material);
                Ok(KnowledgeReading {
                    resource: ResourceRef::parse(source.as_str())?,
                    provider: Some(binding.provider.status().provider),
                    lens: Some("source-pool".into()),
                    revision: Some(material.binding.revision.to_string()),
                    freshness: None,
                    authority: SourceAuthority::Observed,
                    content: Some(material.body.clone()),
                    evidence: vec![source.clone()],
                    why_selected: "selected from the eligible project SourcePool".into(),
                })
            }
            KnowledgeAddress::Code(reference) => {
                let code = self
                    .code
                    .ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                let context = code.context(reference)?;
                Ok(KnowledgeReading {
                    resource: reference.resource_ref(),
                    provider: Some(context.provider),
                    lens: Some("code-index".into()),
                    revision: reference.revision.as_ref().map(ToString::to_string),
                    freshness: None,
                    authority: SourceAuthority::Derived,
                    content: Some(serde_json::to_string_pretty(&context.detail).map_err(
                        |error| {
                            AikitError::new(
                                "knowledge.code_context_serialization",
                                format!("could not render code context: {error}"),
                            )
                        },
                    )?),
                    evidence: vec![reference.source.clone()],
                    why_selected: "selected from derived ProjectMap code intelligence".into(),
                })
            }
            KnowledgeAddress::ProjectMap(resource) => {
                let endpoint = self.project_map_endpoint(resource)?;
                Ok(KnowledgeReading {
                    resource: resource.clone(),
                    provider: endpoint
                        .provider
                        .clone()
                        .or_else(|| Some(project_map_provider())),
                    lens: Some(project_lens_name(endpoint.lens).into()),
                    revision: endpoint.revision.clone(),
                    freshness: None,
                    authority: endpoint.authority,
                    content: endpoint.label.clone(),
                    evidence: Vec::new(),
                    why_selected: "selected from an explicit ProjectMap federation endpoint".into(),
                })
            }
        }
    }

    pub fn relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let mut view = match address {
            KnowledgeAddress::Wiki(resource) => {
                self.wiki_relations(resource.clone(), depth, max_nodes, max_edges)?
            }
            KnowledgeAddress::Source(source) => {
                self.source_relations(source, max_nodes, max_edges)?
            }
            KnowledgeAddress::Code(reference) => {
                self.code_relations(reference, max_nodes, max_edges)?
            }
            KnowledgeAddress::ProjectMap(resource) => {
                self.project_map_relations(resource, depth, max_nodes, max_edges)?
            }
        };
        self.augment_project_map_relations(address.resource_ref(), &mut view)?;
        Ok(view)
    }

    pub fn explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        match address {
            KnowledgeAddress::Wiki(resource) => {
                let wiki = self
                    .wiki
                    .as_ref()
                    .ok_or_else(|| provider_absent("SemanticWiki"))?;
                let explanation = wiki.explain(resource)?;
                Ok(KnowledgeExplanation {
                    address: address.clone(),
                    provider: Some(explanation.provider),
                    authority: explanation.authority,
                    summary: format!(
                        "{} r{}; {} native relations",
                        explanation.object_kind,
                        explanation.revision,
                        explanation.relations.len()
                    ),
                    sources: explanation.sources,
                    detail: serde_json::to_value(explanation.provenance).ok(),
                })
            }
            KnowledgeAddress::Source(source) => {
                let Some((binding, material)) = self.source_material(source) else {
                    // Explaining is not reading. When a curated node cites a
                    // source this horizon cannot materialise, the Wiki still
                    // holds the one fact worth having — that the citation is
                    // authored, and whose it is. Answer with that instead of
                    // refusing, and let `read` be the operation that admits
                    // the content is out of reach.
                    let citing = self.wiki_citations(source);
                    if citing.is_empty() {
                        return Err(AikitError::new(
                            "knowledge.source_missing",
                            format!("Source {source} is absent"),
                        ));
                    }
                    let wiki = self.wiki.as_ref().expect("a citation implies a Wiki provider");
                    return Ok(KnowledgeExplanation {
                        address: address.clone(),
                        provider: Some(wiki.status().provider),
                        authority: SourceAuthority::Authored,
                        summary: format!(
                            "authored source cited by {} curated node(s); not materialised in \
                             this horizon, so it is findable and explainable but not readable",
                            citing.len()
                        ),
                        sources: vec![source.clone()],
                        // Named, not a bare list: this rides under the
                        // explain payload's `provider` key alongside other
                        // providers' native detail, where an unlabelled
                        // array of refs would not say what it is.
                        detail: serde_json::to_value(serde_json::json!({
                            "citing_nodes": citing
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>(),
                            "readable": false,
                        }))
                        .ok(),
                    });
                };
                Ok(KnowledgeExplanation {
                    address: address.clone(),
                    provider: Some(binding.provider.status().provider),
                    authority: SourceAuthority::Observed,
                    summary: format!(
                        "eligible SourcePool material; visibility={:?}; media_type={}",
                        material.binding.visibility, material.binding.media_type
                    ),
                    sources: vec![source.clone()],
                    detail: serde_json::to_value(&material.binding).ok(),
                })
            }
            KnowledgeAddress::Code(reference) => {
                let code = self
                    .code
                    .ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                let context = code.context(reference)?;
                Ok(KnowledgeExplanation {
                    address: address.clone(),
                    provider: Some(context.provider),
                    authority: SourceAuthority::Derived,
                    summary:
                        "Git/source is canonical; ProjectMap code graph is derived intelligence"
                            .into(),
                    sources: vec![reference.source.clone()],
                    detail: Some(context.detail),
                })
            }
            KnowledgeAddress::ProjectMap(resource) => {
                let endpoint = self.project_map_endpoint(resource)?;
                let bindings = self
                    .project_map
                    .expect("endpoint lookup proves ProjectMap is present")
                    .neighbours(resource);
                Ok(KnowledgeExplanation {
                    address: address.clone(),
                    provider: endpoint
                        .provider
                        .clone()
                        .or_else(|| Some(project_map_provider())),
                    authority: endpoint.authority,
                    summary: format!(
                        "explicit {:?} ProjectMap endpoint; {} cross-lens binding(s)",
                        endpoint.lens,
                        bindings.len()
                    ),
                    sources: Vec::new(),
                    detail: serde_json::to_value(bindings).ok(),
                })
            }
        }
    }

    pub fn route(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<KnowledgeRoute> {
        if addresses.is_empty() {
            return Err(AikitError::new(
                "knowledge.empty_route",
                "KnowledgeRoute requires at least one traversed address",
            ));
        }
        let material = addresses
            .iter()
            .map(|address| address.resource_ref().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let digest = blake3::hash(material.as_bytes()).to_hex();
        let mut route = KnowledgeRoute::new(
            ResourceRef::parse(&format!("knowledge-route:{}", &digest.as_str()[..24]))?,
            self.context.clone(),
        );
        if let Some(query) = query {
            route.query = Some(query.into());
        }
        for (index, address) in addresses.iter().enumerate() {
            let (provider, authority, revision) = self.route_metadata(address)?;
            let transition = if index == 0 {
                None
            } else {
                Some(self.transition_between(&addresses[index - 1], address)?)
            };
            route.steps.push(KnowledgeRouteStep {
                resource: address.resource_ref(),
                provider,
                lens: Some(self.address_lens(address)?.into()),
                transition,
                revision,
                authority,
            });
        }
        Ok(route)
    }

    /// Traverse an explicit bounded ProjectMap path without copying provider
    /// graphs into AIKit. Intermediate endpoints remain ProjectMap addresses;
    /// the caller-provided endpoints retain their richer provider addresses.
    pub fn route_via_project_map(
        &self,
        query: Option<&str>,
        from: KnowledgeAddress,
        to: KnowledgeAddress,
        max_hops: usize,
    ) -> Result<KnowledgeRoute> {
        let map = self
            .project_map
            .ok_or_else(|| provider_absent("ProjectMap federation"))?;
        let from_ref = from.resource_ref();
        let to_ref = to.resource_ref();
        let path = map.route(&from_ref, &to_ref, max_hops).ok_or_else(|| {
            AikitError::new(
                "knowledge.project_map_route_missing",
                format!("no explicit ProjectMap route from {from_ref} to {to_ref}"),
            )
        })?;
        if path.is_empty() {
            return self.route(query, &[from]);
        }

        let mut addresses = vec![from];
        for (index, step) in path.iter().enumerate() {
            if index + 1 == path.len() {
                addresses.push(to.clone());
            } else {
                addresses.push(KnowledgeAddress::ProjectMap(step.to.clone()));
            }
        }
        self.route(query, &addresses)
    }

    pub fn context_pack(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> KnowledgeContextPack {
        let mut pack = KnowledgeContextPack::new(self.context.clone());
        pack.query = query.map(str::to_string);
        for address in addresses {
            pack.selected.push(address.resource_ref());
            match self.read(address) {
                Ok(reading) => pack.readings.push(reading),
                Err(error) => pack.absences.push(error.message().to_string()),
            }
            if let Ok(explanation) = self.explain(address) {
                pack.explanations.push(explanation.summary);
            }
        }
        if let Ok(route) = self.route(query, addresses) {
            pack.routes.push(route);
        }
        pack.derive_uncertainty();
        pack
    }

    pub fn history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute> {
        routes
            .iter()
            .filter(|route| route.context == self.context)
            .collect()
    }

    /// The curated nodes citing `source`, or empty when the Wiki is absent or
    /// nothing cites it.
    ///
    /// A cited source is authored ground this horizon may or may not be able
    /// to open. When the SourcePool cannot materialise it, this is what turns
    /// the refusal into a fact the caller can act on — the citation is real
    /// and named, only its content is out of reach.
    fn wiki_citations(&self, source: &SourceRef) -> Vec<ResourceRef> {
        self.wiki
            .as_ref()
            .map(|wiki| wiki.citing_nodes(source))
            .unwrap_or_default()
    }

    /// The refusal a cited-but-unmaterialised source earns: distinct from a
    /// source nothing knows about, because the difference matters to whoever
    /// has to decide what to do next.
    fn unmaterialised_cited_source(source: &SourceRef, citing: &[ResourceRef]) -> AikitError {
        let names = citing
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        AikitError::new(
            "knowledge.source_cited_but_unmaterialised",
            format!(
                "Source {source} is cited by {names} but is not materialised in the project \
                 horizon: it can be found and explained, not read"
            ),
        )
        .with("source", source.as_str())
    }

    fn source_material(
        &self,
        source: &SourceRef,
    ) -> Option<(&SourcePoolBinding<'a>, &SourceMaterial)> {
        self.sources.iter().find_map(|binding| {
            binding
                .material
                .iter()
                .find(|material| &material.binding.source == source)
                .map(|material| (binding, material))
        })
    }

    fn project_map_endpoint(&self, resource: &ResourceRef) -> Result<&ProjectMapEndpoint> {
        self.project_map
            .ok_or_else(|| provider_absent("ProjectMap federation"))?
            .endpoint(resource)
            .ok_or_else(|| {
                AikitError::new(
                    "knowledge.project_map_endpoint_missing",
                    format!("ProjectMap endpoint {resource} is absent"),
                )
            })
    }

    fn address_lens(&self, address: &KnowledgeAddress) -> Result<&'static str> {
        Ok(match address {
            KnowledgeAddress::Wiki(_) => "semantic-wiki",
            KnowledgeAddress::Source(_) => "source-pool",
            KnowledgeAddress::Code(_) => "code-index",
            KnowledgeAddress::ProjectMap(resource) => {
                project_lens_name(self.project_map_endpoint(resource)?.lens)
            }
        })
    }

    fn transition_between(&self, from: &KnowledgeAddress, to: &KnowledgeAddress) -> Result<String> {
        let from_ref = from.resource_ref();
        let to_ref = to.resource_ref();
        if from_ref == to_ref {
            return Ok("same-resource".into());
        }

        if let Some(project_map) = self.project_map {
            if let Some(step) = project_map
                .neighbours(&from_ref)
                .into_iter()
                .find(|step| step.to == to_ref)
            {
                return Ok(format!("project-map:{}", step.relation));
            }
        }

        match (from, to) {
            (KnowledgeAddress::Wiki(left), KnowledgeAddress::Wiki(right)) => {
                let wiki = self
                    .wiki
                    .as_ref()
                    .ok_or_else(|| provider_absent("SemanticWiki"))?;
                if let Some(neighbour) = wiki
                    .neighbours(left, usize::MAX)
                    .into_iter()
                    .find(|neighbour| neighbour.resource == *right)
                {
                    return Ok(neighbour.relation);
                }
            }
            (KnowledgeAddress::Wiki(wiki_ref), KnowledgeAddress::Source(source)) => {
                let wiki = self
                    .wiki
                    .as_ref()
                    .ok_or_else(|| provider_absent("SemanticWiki"))?;
                if wiki.sources(wiki_ref).contains(source) {
                    return Ok("source".into());
                }
            }
            (KnowledgeAddress::Source(source), KnowledgeAddress::Wiki(wiki_ref)) => {
                let wiki = self
                    .wiki
                    .as_ref()
                    .ok_or_else(|| provider_absent("SemanticWiki"))?;
                if wiki.sources(wiki_ref).contains(source) {
                    return Ok("source".into());
                }
            }
            (KnowledgeAddress::Code(left), KnowledgeAddress::Code(right)) => {
                let view = self.code_relations(left, 256, 512)?;
                if let Some(edge) = view.edges.iter().find(|edge| {
                    (edge.from == left.resource_ref() && edge.to == right.resource_ref())
                        || (edge.to == left.resource_ref() && edge.from == right.resource_ref())
                }) {
                    return Ok(edge.relation.clone());
                }
            }
            _ => {}
        }

        Err(AikitError::new(
            "knowledge.route_unbound_transition",
            format!(
                "no provider-native or explicit ProjectMap transition binds {from_ref} to {to_ref}"
            ),
        ))
    }

    fn wiki_relations(
        &self,
        focus: ResourceRef,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let wiki = self
            .wiki
            .as_ref()
            .ok_or_else(|| provider_absent("SemanticWiki"))?;
        let query = RelationQuery {
            focus: focus.clone(),
            depth,
            max_nodes,
            max_edges,
            filters: Vec::new(),
        };
        let mut view = wiki.relations(query)?;
        for source in wiki.sources(&focus) {
            let resource = ResourceRef::parse(source.as_str())?;
            if !view.push_node(RelationNode::new(
                resource.clone(),
                ResourceKind::KnowledgeSource,
                source.to_string(),
            )) {
                continue;
            }
            let _ = view.push_edge(RelationEdge::new(
                focus.clone(),
                resource,
                "source",
                RelationDirection::Outgoing,
                RelationOrigin::new(SourceAuthority::Authored)
                    .from_provider(wiki.status().provider)
                    .in_lens("semantic-wiki"),
            ))?;
        }
        Ok(view)
    }

    fn source_relations(
        &self,
        source: &SourceRef,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        // A source's relations are the curated nodes that cite it. That is a
        // fact the Wiki holds whether or not the SourcePool can materialise
        // the source's content, so requiring material here refused to answer
        // a question we could answer: a cited-but-unmaterialised source came
        // back `source_missing` while dozens of nodes demonstrably cited it.
        // Refuse only when nothing knows the source at all.
        if self.source_material(source).is_none() && self.wiki_citations(source).is_empty() {
            return Err(AikitError::new(
                "knowledge.source_missing",
                format!("Source {source} is absent"),
            ));
        }
        let focus = ResourceRef::parse(source.as_str())?;
        let query = RelationQuery {
            focus: focus.clone(),
            depth: 1,
            max_nodes,
            max_edges,
            filters: Vec::new(),
        };
        let mut view = KnowledgeRelationView::focus_only(
            query,
            RelationNode::new(
                focus.clone(),
                ResourceKind::KnowledgeSource,
                source.to_string(),
            ),
        )?;
        let Some(wiki) = &self.wiki else {
            view.warnings
                .push("SemanticWiki absent; source backlinks unavailable".into());
            return Ok(view);
        };
        for resource in wiki.discover() {
            if !wiki.sources(&resource).contains(source) {
                continue;
            }
            let object = wiki
                .resolve(&resource)
                .expect("discovered Wiki ref resolves");
            let kind = match object {
                crate::knowledge_wiki::WikiObject::Space(_) => ResourceKind::KnowledgeSpace,
                crate::knowledge_wiki::WikiObject::Frame(_) => ResourceKind::KnowledgeFrame,
                _ => ResourceKind::KnowledgeNode,
            };
            if !view.push_node(RelationNode::new(
                resource.clone(),
                kind,
                resource.to_string(),
            )) {
                continue;
            }
            let _ = view.push_edge(RelationEdge::new(
                resource,
                focus.clone(),
                "source",
                RelationDirection::Incoming,
                RelationOrigin::new(SourceAuthority::Authored)
                    .from_provider(wiki.status().provider)
                    .in_lens("semantic-wiki"),
            ))?;
        }
        Ok(view)
    }

    fn code_relations(
        &self,
        reference: &CodeReference,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let code = self
            .code
            .ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
        let context = code.context(reference)?;
        let focus = reference.resource_ref();
        let query = RelationQuery {
            focus: focus.clone(),
            depth: 1,
            max_nodes,
            max_edges,
            filters: Vec::new(),
        };
        let mut view = KnowledgeRelationView::focus_only(
            query,
            RelationNode::new(
                focus.clone(),
                ResourceKind::CodeReference,
                reference
                    .symbol
                    .clone()
                    .unwrap_or_else(|| reference.path.clone()),
            ),
        )?;
        for (key, direction) in [
            ("outgoing", RelationDirection::Outgoing),
            ("incoming", RelationDirection::Incoming),
        ] {
            let Some(groups) = context.detail.get(key).and_then(Value::as_object) else {
                continue;
            };
            for (relation, values) in groups {
                let Some(values) = values.as_array() else {
                    continue;
                };
                for value in values {
                    let Some(object) = value.as_object() else {
                        continue;
                    };
                    let Some(path) = code_string(object, &["filePath", "file_path", "path"]) else {
                        continue;
                    };
                    let related = CodeReference {
                        source: reference.source.clone(),
                        revision: reference.revision.clone(),
                        path,
                        symbol: code_string(object, &["name", "symbol", "qualifiedName"]),
                        kind: code_string(object, &["kind", "type", "label"]),
                        line: None,
                    };
                    let related_ref = related.resource_ref();
                    if !view.push_node(RelationNode::new(
                        related_ref.clone(),
                        ResourceKind::CodeReference,
                        related
                            .symbol
                            .clone()
                            .unwrap_or_else(|| related.path.clone()),
                    )) {
                        continue;
                    }
                    let (from, to) = match direction {
                        RelationDirection::Outgoing => (focus.clone(), related_ref),
                        RelationDirection::Incoming => (related_ref, focus.clone()),
                        RelationDirection::Bidirectional => unreachable!(),
                    };
                    if !view.push_edge(RelationEdge::new(
                        from,
                        to,
                        relation.clone(),
                        direction,
                        RelationOrigin::new(SourceAuthority::Derived)
                            .from_provider(context.provider.clone())
                            .in_lens("code-index"),
                    ))? {
                        return Ok(view);
                    }
                }
            }
        }
        Ok(view)
    }

    fn project_map_relations(
        &self,
        resource: &ResourceRef,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let endpoint = self.project_map_endpoint(resource)?;
        let query = RelationQuery {
            focus: resource.clone(),
            depth,
            max_nodes,
            max_edges,
            filters: Vec::new(),
        };
        KnowledgeRelationView::focus_only(
            query,
            RelationNode::new(
                resource.clone(),
                endpoint.kind,
                endpoint
                    .label
                    .clone()
                    .unwrap_or_else(|| resource.to_string()),
            ),
        )
    }

    fn augment_project_map_relations(
        &self,
        focus: ResourceRef,
        view: &mut KnowledgeRelationView,
    ) -> Result<()> {
        let Some(project_map) = self.project_map else {
            view.warnings
                .push("ProjectMap federation absent; cross-lens bindings unavailable".into());
            return Ok(());
        };
        for step in project_map.neighbours(&focus) {
            let Some(endpoint) = project_map.endpoint(&step.to) else {
                continue;
            };
            if !view.push_node(RelationNode::new(
                step.to.clone(),
                endpoint.kind,
                endpoint
                    .label
                    .clone()
                    .unwrap_or_else(|| step.to.to_string()),
            )) {
                continue;
            }
            let origin = project_map_origin(&step);
            let edge = if step.reversed {
                RelationEdge::new(
                    step.to.clone(),
                    focus.clone(),
                    step.relation,
                    RelationDirection::Incoming,
                    origin,
                )
            } else {
                RelationEdge::new(
                    focus.clone(),
                    step.to,
                    step.relation,
                    RelationDirection::Outgoing,
                    origin,
                )
            };
            if !view.push_edge(edge)? {
                break;
            }
        }
        Ok(())
    }

    fn route_metadata(
        &self,
        address: &KnowledgeAddress,
    ) -> Result<(Option<ProviderRef>, SourceAuthority, Option<String>)> {
        match address {
            KnowledgeAddress::Wiki(resource) => {
                let wiki = self
                    .wiki
                    .as_ref()
                    .ok_or_else(|| provider_absent("SemanticWiki"))?;
                let object = wiki.resolve(resource).ok_or_else(|| {
                    AikitError::new(
                        "knowledge.wiki_object_missing",
                        format!("Wiki object {resource} is absent"),
                    )
                })?;
                Ok((
                    Some(wiki.status().provider),
                    self.read(address)?.authority,
                    Some(object.revision().to_string()),
                ))
            }
            KnowledgeAddress::Source(source) => {
                let Some((binding, material)) = self.source_material(source) else {
                    // A cited source routes through the Wiki that cites it.
                    // It carries no revision here — the Wiki knows the
                    // citation, not the material's version — and saying so
                    // is better than failing a route for an address search
                    // legitimately returned.
                    let citing = self.wiki_citations(source);
                    if citing.is_empty() {
                        return Err(AikitError::new(
                            "knowledge.source_missing",
                            format!("Source {source} is absent"),
                        ));
                    }
                    let wiki = self.wiki.as_ref().expect("a citation implies a Wiki provider");
                    return Ok((Some(wiki.status().provider), SourceAuthority::Authored, None));
                };
                Ok((
                    Some(binding.provider.status().provider),
                    SourceAuthority::Observed,
                    Some(material.binding.revision.to_string()),
                ))
            }
            KnowledgeAddress::Code(reference) => {
                let code = self
                    .code
                    .ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                Ok((
                    Some(code.status().provider),
                    SourceAuthority::Derived,
                    reference.revision.as_ref().map(ToString::to_string),
                ))
            }
            KnowledgeAddress::ProjectMap(resource) => {
                let endpoint = self.project_map_endpoint(resource)?;
                Ok((
                    endpoint
                        .provider
                        .clone()
                        .or_else(|| Some(project_map_provider())),
                    endpoint.authority,
                    endpoint.revision.clone(),
                ))
            }
        }
    }
}

fn provider_absent(name: &str) -> AikitError {
    AikitError::new(
        "knowledge.provider_absent",
        format!("{name} provider is absent from this Project world"),
    )
}

fn project_map_provider() -> ProviderRef {
    ProviderRef::parse("provider/project-map/federation")
        .expect("static ProjectMap federation provider ref must be valid")
}

fn project_lens_name(lens: ProjectLens) -> &'static str {
    match lens {
        ProjectLens::Git => "git",
        ProjectLens::Code => "code",
        ProjectLens::SemanticWiki => "semantic-wiki",
        ProjectLens::SourcePool => "source-pool",
        ProjectLens::Canon => "canon",
        ProjectLens::Run => "run",
        ProjectLens::Decision => "decision",
        ProjectLens::Verification => "verification",
        ProjectLens::Evolution => "evolution",
    }
}

fn project_map_origin(step: &ProjectMapStep) -> RelationOrigin {
    let mut origin = RelationOrigin::new(step.authority).in_lens("project-map");
    origin.provider = step
        .provider
        .clone()
        .or_else(|| Some(project_map_provider()));
    origin
}

fn code_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::knowledge_source_pool::{
        NativeSourcePoolProvider, SourceBinding, SourcePoolProvider, SourceVisibility,
    };
    use crate::knowledge_wiki::{parse_wiki_objects, WikiObject};
    use crate::knowledge_wiki_index::SemanticWikiIndex;
    use crate::knowledge_wiki_provider::SemanticWikiProvider;
    use crate::project_map::{ProjectMapBinding, ProjectMapEndpoint};
    use crate::resource::{AddressHorizon, SourceRevision};

    use super::*;

    fn wiki() -> SemanticWikiIndex {
        SemanticWikiIndex::rebuild(
            parse_wiki_objects(
                r#"{"objects":[
                  {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
                   "provenance":[],"title":"Root","parent_space_refs":[],"child_space_refs":[],
                   "node_refs":["wiki:node:auth"]},
                  {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:auth","revision":1,
                   "provenance":[{"source_ref":"source:spec"}],"type":"Concept","title":"Authentication",
                   "space_refs":["wiki:space:root"],"source_refs":["source:spec"]}
                ]}"#,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn spec() -> SourceRef {
        SourceRef::parse("source:spec").unwrap()
    }

    fn material() -> SourceMaterial {
        SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse("source:spec").unwrap(),
                revision: SourceRevision::parse("sha256:spec").unwrap(),
                title: "Auth spec".into(),
                tags: vec!["auth".into()],
                visibility: SourceVisibility::Team,
                owners: Vec::new(),
                media_type: "text/markdown".into(),
                locator: None,
                metadata: BTreeMap::new(),
            },
            body: "Authentication sessions rotate tokens.".into(),
        }
    }

    fn endpoint(
        resource: &str,
        lens: ProjectLens,
        kind: ResourceKind,
        authority: SourceAuthority,
    ) -> ProjectMapEndpoint {
        ProjectMapEndpoint {
            resource: ResourceRef::parse(resource).unwrap(),
            kind,
            lens,
            authority,
            provider: None,
            revision: None,
            label: Some(resource.to_string()),
        }
    }

    /// A `tag:` term narrows the SourcePool instead of being matched as
    /// text. Before this, the one production call to a provider's search
    /// passed an empty tag slice, so the tag facility every provider
    /// implements — and bkmr advertises through its `--tags` flag — was
    /// unreachable from a query.
    #[test]
    fn a_tag_term_narrows_the_source_pool_rather_than_being_matched_as_text() {
        assert_eq!(
            split_tag_filters("rotate tag:auth tag:spec tokens"),
            ("rotate tokens".to_owned(), vec!["auth".to_owned(), "spec".to_owned()])
        );
        // `#word` is ordinary prose in a markdown corpus and is left alone.
        assert_eq!(
            split_tag_filters("the #1 position"),
            ("the #1 position".to_owned(), Vec::new())
        );

        let material = vec![material()];
        let mut native = NativeSourcePoolProvider::new();
        native.rebuild(&material).unwrap();
        let app = KnowledgeApplication::new(FamiliarityContext {
            project: None,
            actor: None,
            agency: None,
            focus: None,
        })
        .with_source_pool(&native, &material);

        let matched = app.search("Authentication tag:auth", 10);
        assert!(
            matched
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "source:spec"),
            "the tag filter selects the source and the remaining text still matches"
        );
        let excluded = app.search("Authentication tag:absent-tag", 10);
        assert!(
            !excluded
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "source:spec"),
            "a tag the source does not carry excludes it"
        );
        // A tag alone is a browse, not an empty query.
        let browsed = app.search("tag:auth", 10);
        assert!(browsed
            .hits
            .iter()
            .any(|hit| hit.resource.as_str() == "source:spec"));
    }

    /// Law 3, one query path: the raw-string front is a front, not a second
    /// retrieval path. It parses through the one operative grammar and
    /// delegates, so operative syntax is *read* by Knowledge — an address
    /// horizon narrows the federated result. A substring scanner over the
    /// literal text `@2 Authentication` could not do this, so this case goes
    /// red the moment a parallel path is reintroduced here.
    #[test]
    fn the_raw_string_front_parses_into_the_resolver_and_delegates() {
        let index = wiki();
        let wiki_provider = SemanticWikiProvider::new(&index);
        let material = vec![material()];
        let mut native = NativeSourcePoolProvider::new();
        native.rebuild(&material).unwrap();
        let app = KnowledgeApplication::new(FamiliarityContext {
            project: None,
            actor: None,
            agency: None,
            focus: None,
        })
        .with_wiki(wiki_provider)
        .with_source_pool(&native, &material);

        let plain = app.search("Authentication", 10);
        assert_eq!(
            plain.expression,
            ResolveExpression::ordinary_search("Authentication"),
            "a typed string is lowered into the contract, not handled beside it"
        );
        assert_eq!(
            plain.path_identity,
            resolve_path_identity(&ResolveExpression::ordinary_search("Authentication"))
        );
        assert!(
            plain
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "wiki:node:auth"),
            "the curated node is reachable"
        );
        assert!(
            plain
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "source:spec"),
            "the source is reachable"
        );

        // @2 is the reflection/meaning horizon: a KnowledgeNode participates,
        // a KnowledgeSource does not.
        let narrowed = app.search("@2 Authentication", 10);
        assert_eq!(
            narrowed.expression,
            ResolveExpression::horizon(
                AddressHorizon::H2,
                ResolveExpression::subject("Authentication")
            ),
            "the address was parsed by the one grammar, not taken as literal text"
        );
        assert!(
            narrowed
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "wiki:node:auth"),
            "the @2 participant survives the address"
        );
        assert!(
            !narrowed
                .hits
                .iter()
                .any(|hit| hit.resource.as_str() == "source:spec"),
            "the address narrowed the federated result: {:?}",
            narrowed.hits
        );

        // The front adds nothing the canonical entry does not do.
        let canonical = app.resolve(&parse_or_search_expression("@2 Authentication").unwrap(), 10);
        assert_eq!(canonical.hits, narrowed.hits);
        assert_eq!(canonical.path_identity, narrowed.path_identity);
    }

    #[test]
    fn universal_search_read_relations_route_and_context_pack_share_one_service() {
        let index = wiki();
        let wiki_provider = SemanticWikiProvider::new(&index);
        let material = vec![material()];
        let mut native = NativeSourcePoolProvider::new();
        native.rebuild(&material).unwrap();
        let context = FamiliarityContext {
            project: Some(ResourceRef::parse("project:demo").unwrap()),
            actor: None,
            agency: None,
            focus: Some("auth".into()),
        };
        let app = KnowledgeApplication::new(context)
            .with_wiki(wiki_provider)
            .with_source_pool(&native, &material);

        let result = app.search("Authentication", 10);
        assert!(result
            .hits
            .iter()
            .any(|hit| hit.resource.as_str() == "wiki:node:auth"));
        let wiki_address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:auth").unwrap());
        let source_address = KnowledgeAddress::Source(SourceRef::parse("source:spec").unwrap());
        assert!(app
            .read(&source_address)
            .unwrap()
            .content
            .unwrap()
            .contains("rotate"));
        let relations = app.relations(&wiki_address, 1, 16, 16).unwrap();
        assert!(relations
            .nodes
            .iter()
            .any(|node| node.resource.as_str() == "source:spec"));
        let route = app
            .route(
                Some("Authentication"),
                &[wiki_address.clone(), source_address.clone()],
            )
            .unwrap();
        assert_eq!(route.steps.len(), 2);
        assert_eq!(route.steps[1].transition.as_deref(), Some("source"));
        assert!(route.familiarity_observation("event:route", 42).is_ok());
        let pack = app.context_pack(Some("Authentication"), &[wiki_address, source_address]);
        assert_eq!(pack.readings.len(), 2);
        assert_eq!(app.history(&[route]).len(), 1);
        assert!(app
            .status()
            .absences
            .iter()
            .any(|value| value.contains("CodeIndex")));
    }

    #[test]
    fn project_map_bindings_are_native_cross_lens_route_transitions() {
        let index = wiki();
        let wiki_provider = SemanticWikiProvider::new(&index);
        let material = vec![material()];
        let mut native = NativeSourcePoolProvider::new();
        native.rebuild(&material).unwrap();
        let mut project_map = ProjectMap::new();
        for endpoint in [
            endpoint(
                "wiki:node:auth",
                ProjectLens::SemanticWiki,
                ResourceKind::KnowledgeNode,
                SourceAuthority::Authored,
            ),
            endpoint(
                "source:spec",
                ProjectLens::SourcePool,
                ResourceKind::KnowledgeSource,
                SourceAuthority::Observed,
            ),
            endpoint(
                "canon:auth-design",
                ProjectLens::Canon,
                ResourceKind::KnowledgeNode,
                SourceAuthority::Authored,
            ),
        ] {
            project_map.add_endpoint(endpoint).unwrap();
        }
        project_map
            .bind(ProjectMapBinding {
                from: ResourceRef::parse("wiki:node:auth").unwrap(),
                to: ResourceRef::parse("source:spec").unwrap(),
                relation: "supported-by".into(),
                reversible: true,
                authority: SourceAuthority::Authored,
                provider: None,
                provenance: vec![],
            })
            .unwrap();
        project_map
            .bind(ProjectMapBinding {
                from: ResourceRef::parse("source:spec").unwrap(),
                to: ResourceRef::parse("canon:auth-design").unwrap(),
                relation: "constrains".into(),
                reversible: true,
                authority: SourceAuthority::Authored,
                provider: None,
                provenance: vec![],
            })
            .unwrap();

        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(wiki_provider)
            .with_source_pool(&native, &material)
            .with_project_map(&project_map);
        let wiki_address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:auth").unwrap());
        let canon_address =
            KnowledgeAddress::ProjectMap(ResourceRef::parse("canon:auth-design").unwrap());
        let route = app
            .route_via_project_map(Some("auth design"), wiki_address.clone(), canon_address, 2)
            .unwrap();
        assert_eq!(route.steps.len(), 3);
        assert_eq!(
            route.steps[1].transition.as_deref(),
            Some("project-map:supported-by")
        );
        assert_eq!(
            route.steps[2].transition.as_deref(),
            Some("project-map:constrains")
        );

        let relations = app.relations(&wiki_address, 1, 16, 16).unwrap();
        assert!(relations.edges.iter().any(|edge| {
            edge.relation == "supported-by" && edge.origin.lens.as_deref() == Some("project-map")
        }));
        assert!(app.status().project_map);
    }

    #[test]
    fn search_prefers_provider_native_address_over_project_map_projection() {
        let index = wiki();
        let wiki_provider = SemanticWikiProvider::new(&index);
        let mut project_map = ProjectMap::new();
        project_map
            .add_endpoint(endpoint(
                "wiki:node:auth",
                ProjectLens::SemanticWiki,
                ResourceKind::KnowledgeNode,
                SourceAuthority::Authored,
            ))
            .unwrap();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(wiki_provider)
            .with_project_map(&project_map);

        let result = app.search("wiki:node:auth", 10);
        let hit = result
            .hits
            .iter()
            .find(|hit| hit.resource.as_str() == "wiki:node:auth")
            .expect("Wiki resource is discoverable");
        assert!(matches!(hit.address, KnowledgeAddress::Wiki(_)));
        assert_ne!(hit.provider, project_map_provider());
    }

    #[test]
    fn arbitrary_cross_lens_jumps_are_rejected_without_explicit_binding() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));
        let wiki_address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:auth").unwrap());
        let unrelated = KnowledgeAddress::ProjectMap(ResourceRef::parse("canon:unbound").unwrap());
        let error = app.route(None, &[wiki_address, unrelated]).unwrap_err();
        assert_eq!(error.code(), "knowledge.provider_absent");
    }

    #[test]
    fn source_backlinks_are_derived_from_canonical_wiki_source_refs() {
        let index = wiki();
        let objects = index.discover();
        assert!(objects
            .iter()
            .any(|value| value.as_str() == "wiki:node:auth"));
        assert!(matches!(
            index.resolve(&ResourceRef::parse("wiki:node:auth").unwrap()),
            Some(WikiObject::Node(_))
        ));
    }

    /// A source a curated node cites, which this horizon cannot materialise,
    /// is findable — so the result must say, at the point it hands over the
    /// address, that the address will not open. Silence here is what makes
    /// findability a trap instead of a capability.
    #[test]
    fn search_discloses_a_cited_source_this_horizon_cannot_open() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));

        let result = app.search("auth", 10);
        assert!(
            result
                .hits
                .iter()
                .any(|hit| hit.address == KnowledgeAddress::Source(spec())),
            "the cited source is still findable"
        );
        assert!(
            result
                .absences
                .iter()
                .any(|absence| absence.contains("source:spec") && absence.contains("not readable")),
            "the absence names the source and what cannot be done with it: {:?}",
            result.absences
        );
    }

    /// Reading it still fails — the Wiki holds the citation, not the content —
    /// but the refusal names the citation instead of reporting the source
    /// simply missing, which is a different fact with a different remedy.
    #[test]
    fn reading_a_cited_unmaterialised_source_names_the_citation() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));

        let error = app
            .read(&KnowledgeAddress::Source(spec()))
            .expect_err("the Wiki cites this source but cannot serve its content");
        assert_eq!(error.code(), "knowledge.source_cited_but_unmaterialised");
        assert!(
            error.message().contains("wiki:node:auth"),
            "the refusal names the citing node: {}",
            error.message()
        );
    }

    /// A source nothing cites keeps the older, plainer absence: it is not a
    /// citation this horizon failed to open, it is simply unknown.
    #[test]
    fn an_uncited_absent_source_is_still_plainly_missing() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));

        let stranger = SourceRef::parse("source:nobody-cites-me").unwrap();
        let error = app
            .read(&KnowledgeAddress::Source(stranger))
            .expect_err("nothing knows this source");
        assert_eq!(error.code(), "knowledge.source_missing");
    }

    /// Explaining is not reading. The Wiki can account for a citation whose
    /// content it cannot serve, so `explain` answers from that knowledge —
    /// authored, attributed, and honest that the material is out of reach.
    #[test]
    fn explaining_a_cited_unmaterialised_source_answers_from_the_wiki() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));

        let explanation = app
            .explain(&KnowledgeAddress::Source(spec()))
            .expect("the citation itself is explainable");
        assert_eq!(explanation.authority, SourceAuthority::Authored);
        assert_eq!(explanation.sources, vec![spec()]);
        assert!(
            explanation.summary.contains("not readable"),
            "the explanation admits what it cannot do: {}",
            explanation.summary
        );
        let detail = explanation.detail.expect("citing nodes ride the detail");
        assert!(
            detail.to_string().contains("wiki:node:auth"),
            "the detail names the citing node: {detail}"
        );
    }

    /// CASE 19: the authored source `wiki:node:auth` cites (`source:spec`)
    /// must reach the application surface as a distinct `Source` address —
    /// not folded into the curated `Wiki` hit for the node that cites it —
    /// carrying `KnowledgeSource`/`Authored`, so the CLI's existing
    /// `source=REF` dispatch can already resolve it.
    #[test]
    fn authored_source_search_hits_map_to_a_distinct_source_address() {
        let index = wiki();
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&index));

        // "auth" matches the curated node by its own title *and* matches the
        // cited source only through that same title, riding as the source's
        // citing-node label — exactly the second search pass this case adds.
        let result = app.search("auth", 10);
        let curated = result
            .hits
            .iter()
            .find(|hit| hit.resource.as_str() == "wiki:node:auth")
            .expect("the curated node itself remains findable");
        assert!(matches!(curated.address, KnowledgeAddress::Wiki(_)));
        assert_eq!(curated.kind, ResourceKind::KnowledgeNode);
        assert_eq!(curated.authority, SourceAuthority::Authored);

        let authored_source = result
            .hits
            .iter()
            .find(|hit| hit.resource.as_str() == "source:spec")
            .expect("the cited source is independently findable");
        assert_eq!(
            authored_source.address,
            KnowledgeAddress::Source(SourceRef::parse("source:spec").unwrap())
        );
        assert_eq!(authored_source.kind, ResourceKind::KnowledgeSource);
        assert_eq!(authored_source.authority, SourceAuthority::Authored);

        // Distinct hits in the same result set: neither collapses into the
        // other, and the authored source is dispatchable through the same
        // `source=REF` CLI form as any other Source address.
        assert_ne!(curated.address, authored_source.address);
    }
}
