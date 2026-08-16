use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::familiarity::FamiliarityContext;
use crate::knowledge::{
    KnowledgeContextPack, KnowledgeReading, KnowledgeRelationView, KnowledgeRoute,
    KnowledgeRouteStep, RelationDirection, RelationEdge, RelationNode, RelationOrigin,
    RelationQuery,
};
use crate::knowledge_code::{CodeIndexProvider, CodeReference};
use crate::knowledge_source_pool::{
    SourceMaterial, SourcePoolProvider, SourceProviderStatus, SourceSearchMode,
};
use crate::knowledge_wiki_provider::{SemanticWikiProvider, SemanticWikiProviderStatus};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority, SourceRef};
use crate::{AikitError, Result};

pub const KNOWLEDGE_APPLICATION_VERSION: &str = "aikit.knowledge-application/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum KnowledgeAddress {
    Wiki(ResourceRef),
    Source(SourceRef),
    Code(CodeReference),
}

impl KnowledgeAddress {
    pub fn resource_ref(&self) -> ResourceRef {
        match self {
            Self::Wiki(resource) => resource.clone(),
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
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    pub provider: ProviderRef,
    pub authority: SourceAuthority,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSearchResult {
    pub query: String,
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

pub struct SourcePoolBinding<'a> {
    pub provider: &'a dyn SourcePoolProvider,
    pub material: &'a [SourceMaterial],
}

/// One project-scoped application field over independent Knowledge providers.
///
/// This is federation, not a universal graph: providers retain their relation,
/// ranking and identity semantics. The application only normalises addressability,
/// degradation, operational routes and Context projection.
pub struct KnowledgeApplication<'a> {
    context: FamiliarityContext,
    wiki: Option<SemanticWikiProvider<'a>>,
    sources: Vec<SourcePoolBinding<'a>>,
    code: Option<&'a dyn CodeIndexProvider>,
}

impl<'a> KnowledgeApplication<'a> {
    pub fn new(context: FamiliarityContext) -> Self {
        Self {
            context,
            wiki: None,
            sources: Vec::new(),
            code: None,
        }
    }

    #[must_use]
    pub fn with_wiki(mut self, wiki: SemanticWikiProvider<'a>) -> Self {
        self.wiki = Some(wiki);
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

    pub fn status(&self) -> KnowledgeProviderStatus {
        let wiki = self.wiki.as_ref().map(SemanticWikiProvider::status);
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
        KnowledgeProviderStatus {
            version: KNOWLEDGE_APPLICATION_VERSION.into(),
            wiki,
            sources,
            code,
            absences,
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> KnowledgeSearchResult {
        if limit == 0 {
            return KnowledgeSearchResult {
                query: query.into(),
                hits: Vec::new(),
                absences: Vec::new(),
            };
        }
        let mut hits = Vec::new();
        let mut absences = Vec::new();

        if let Some(wiki) = &self.wiki {
            hits.extend(wiki.search(query, limit).into_iter().map(|hit| {
                let kind = match hit.object.as_str() {
                    "space" => ResourceKind::KnowledgeSpace,
                    "frame" => ResourceKind::KnowledgeFrame,
                    _ => ResourceKind::KnowledgeNode,
                };
                KnowledgeSearchHit {
                    address: KnowledgeAddress::Wiki(hit.resource.clone()),
                    resource: hit.resource,
                    kind,
                    label: hit.label,
                    score: 1.0 / (1.0 + f64::from(hit.score)),
                    snippet: hit.summary,
                    provider: wiki.status().provider,
                    authority: SourceAuthority::Authored,
                }
            }));
        } else {
            absences.push("SemanticWiki search unavailable: provider absent".into());
        }

        for binding in &self.sources {
            let status = binding.provider.status();
            if !status.available {
                absences.push(format!(
                    "SourcePool search unavailable from {}",
                    status.provider
                ));
                continue;
            }
            let mode = if status.capabilities.hybrid {
                SourceSearchMode::Hybrid
            } else {
                SourceSearchMode::Fulltext
            };
            match binding.provider.search(query, mode, &[], limit) {
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
                    Ok(code_hits) => hits.extend(code_hits.into_iter().map(|hit| {
                        KnowledgeSearchHit {
                            address: KnowledgeAddress::Code(hit.reference.clone()),
                            resource: hit.resource,
                            kind: ResourceKind::CodeReference,
                            label: hit.title,
                            score: hit.score.unwrap_or(0.5),
                            snippet: hit.snippet,
                            provider: hit.provider,
                            authority: SourceAuthority::Derived,
                        }
                    })),
                    Err(error) => absences.push(format!(
                        "ProjectMap code search degraded: {}",
                        error.message()
                    )),
                }
            } else {
                absences.push("ProjectMap code search unavailable: index absent or degraded".into());
            }
        } else {
            absences.push("ProjectMap code search unavailable: provider absent".into());
        }

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.resource.cmp(&right.resource))
        });
        let mut seen = HashSet::new();
        hits.retain(|hit| seen.insert(hit.resource.to_string()));
        hits.truncate(limit);
        KnowledgeSearchResult {
            query: query.into(),
            hits,
            absences,
        }
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
                    AikitError::new(
                        "knowledge.source_missing",
                        format!("Source {source} is not materialised in the project horizon"),
                    )
                })?;
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
                let code = self.code.ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                let context = code.context(reference)?;
                Ok(KnowledgeReading {
                    resource: reference.resource_ref(),
                    provider: Some(context.provider),
                    lens: Some("code-index".into()),
                    revision: reference.revision.as_ref().map(ToString::to_string),
                    freshness: None,
                    authority: SourceAuthority::Derived,
                    content: Some(serde_json::to_string_pretty(&context.detail).map_err(|error| {
                        AikitError::new(
                            "knowledge.code_context_serialization",
                            format!("could not render code context: {error}"),
                        )
                    })?),
                    evidence: vec![reference.source.clone()],
                    why_selected: "selected from derived ProjectMap code intelligence".into(),
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
        match address {
            KnowledgeAddress::Wiki(resource) => {
                self.wiki_relations(resource.clone(), depth, max_nodes, max_edges)
            }
            KnowledgeAddress::Source(source) => {
                self.source_relations(source, max_nodes, max_edges)
            }
            KnowledgeAddress::Code(reference) => {
                self.code_relations(reference, max_nodes, max_edges)
            }
        }
    }

    pub fn explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        match address {
            KnowledgeAddress::Wiki(resource) => {
                let wiki = self.wiki.as_ref().ok_or_else(|| provider_absent("SemanticWiki"))?;
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
                let (binding, material) = self.source_material(source).ok_or_else(|| {
                    AikitError::new("knowledge.source_missing", format!("Source {source} is absent"))
                })?;
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
                let code = self.code.ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                let context = code.context(reference)?;
                Ok(KnowledgeExplanation {
                    address: address.clone(),
                    provider: Some(context.provider),
                    authority: SourceAuthority::Derived,
                    summary: "Git/source is canonical; ProjectMap code graph is derived intelligence"
                        .into(),
                    sources: vec![reference.source.clone()],
                    detail: Some(context.detail),
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
            route.steps.push(KnowledgeRouteStep {
                resource: address.resource_ref(),
                provider,
                lens: Some(
                    match address {
                        KnowledgeAddress::Wiki(_) => "semantic-wiki",
                        KnowledgeAddress::Source(_) => "source-pool",
                        KnowledgeAddress::Code(_) => "code-index",
                    }
                    .into(),
                ),
                transition: (index > 0).then(|| "knowledge-route".into()),
                revision,
                authority,
            });
        }
        Ok(route)
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
        pack
    }

    pub fn history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute> {
        routes
            .iter()
            .filter(|route| route.context == self.context)
            .collect()
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

    fn wiki_relations(
        &self,
        focus: ResourceRef,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let wiki = self.wiki.as_ref().ok_or_else(|| provider_absent("SemanticWiki"))?;
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
        self.source_material(source).ok_or_else(|| {
            AikitError::new("knowledge.source_missing", format!("Source {source} is absent"))
        })?;
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
            RelationNode::new(focus.clone(), ResourceKind::KnowledgeSource, source.to_string()),
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
            let object = wiki.resolve(&resource).expect("discovered Wiki ref resolves");
            let kind = match object {
                crate::knowledge_wiki::WikiObject::Space(_) => ResourceKind::KnowledgeSpace,
                crate::knowledge_wiki::WikiObject::Frame(_) => ResourceKind::KnowledgeFrame,
                _ => ResourceKind::KnowledgeNode,
            };
            if !view.push_node(RelationNode::new(resource.clone(), kind, resource.to_string())) {
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
        let code = self.code.ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
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
                    let Some(path) = code_string(object, &["filePath", "file_path", "path"])
                    else {
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

    fn route_metadata(
        &self,
        address: &KnowledgeAddress,
    ) -> Result<(Option<ProviderRef>, SourceAuthority, Option<String>)> {
        match address {
            KnowledgeAddress::Wiki(resource) => {
                let wiki = self.wiki.as_ref().ok_or_else(|| provider_absent("SemanticWiki"))?;
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
                let (binding, material) = self.source_material(source).ok_or_else(|| {
                    AikitError::new("knowledge.source_missing", format!("Source {source} is absent"))
                })?;
                Ok((
                    Some(binding.provider.status().provider),
                    SourceAuthority::Observed,
                    Some(material.binding.revision.to_string()),
                ))
            }
            KnowledgeAddress::Code(reference) => {
                let code = self.code.ok_or_else(|| provider_absent("ProjectMap CodeIndex"))?;
                Ok((
                    Some(code.status().provider),
                    SourceAuthority::Derived,
                    reference.revision.as_ref().map(ToString::to_string),
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
    use crate::resource::SourceRevision;

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
        let wiki_address =
            KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:auth").unwrap());
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
        assert!(route.familiarity_observation("event:route", 42).is_ok());
        let pack =
            app.context_pack(Some("Authentication"), &[wiki_address, source_address]);
        assert_eq!(pack.readings.len(), 2);
        assert_eq!(app.history(&[route]).len(), 1);
        assert!(app
            .status()
            .absences
            .iter()
            .any(|value| value.contains("CodeIndex")));
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
}
