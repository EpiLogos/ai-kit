use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_adapters::bkmr::BkmrSourcePoolProvider;
use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::SystemRunner;
use aikit_core::knowledge::{KnowledgeContextPack, KnowledgeRelationView, KnowledgeRoute};
use aikit_core::knowledge_code::CodeIndexProvider;
use aikit_core::knowledge_source_pool::{
    material_for_actor, NativeSourcePoolProvider, SourceMaterial, SourcePool, SourcePoolProvider,
};
use aikit_core::knowledge_wiki::{parse_wiki_objects, OkfWikiBundle, WikiObject};
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::knowledge_wiki_provider::SemanticWikiProvider;
use aikit_core::project_map::{ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint};
use aikit_core::resource::{
    ProviderRef, ResourceIndex, ResourceKind, ResourceRef, SourceAuthority, SourceRef,
};
use aikit_core::{
    FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation,
    KnowledgeProviderStatus, KnowledgeRankingEvidence, KnowledgeSearchResult, KnowledgeSources,
    Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};
use aikit_store::{
    append_familiarity_observation, append_familiarity_reset, KnowledgeApplicationReceipt,
    KnowledgeApplicationStore,
};
use aikit_tui::backend::PaletteBackend;

use super::Service;

const MAX_DISCOVERY_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DISCOVERY_FILES: usize = 4096;

pub(super) struct KnowledgeRuntime {
    wiki: Option<SemanticWikiIndex>,
    material: Vec<SourceMaterial>,
    native_source: NativeSourcePoolProvider,
    bkmr: Option<BkmrSourcePoolProvider<SystemRunner>>,
    code: Option<GitNexusCodeIndexProvider<SystemRunner>>,
    project_map: ProjectMap,
    absences: Vec<String>,
}

impl KnowledgeRuntime {
    fn application(&self, context: FamiliarityContext) -> KnowledgeApplication<'_> {
        let mut application = KnowledgeApplication::new(context)
            .with_source_pool(&self.native_source, &self.material)
            .with_project_map(&self.project_map);
        if let Some(index) = &self.wiki {
            application = application.with_wiki(SemanticWikiProvider::new(index));
        }
        if let Some(provider) = &self.bkmr {
            application = application.with_source_pool(provider, &self.material);
        }
        if let Some(provider) = &self.code {
            application = application.with_code(provider);
        }
        application
    }
}

impl Service {
    pub(super) fn invalidate_knowledge_runtime(&self) {
        self.knowledge_runtime.borrow_mut().take();
    }

    fn knowledge_context(&self) -> FamiliarityContext {
        FamiliarityContext {
            project: self
                .descriptor
                .project_id
                .as_ref()
                .and_then(|project| ResourceRef::parse(format!("project/{project}")).ok()),
            actor: None,
            agency: None,
            focus: self.descriptor.task.clone(),
        }
    }

    fn knowledge_store(&self) -> KnowledgeApplicationStore {
        KnowledgeApplicationStore::new(self.home.clone())
    }

    fn with_knowledge<T>(
        &self,
        operation: impl FnOnce(&KnowledgeRuntime, KnowledgeApplication<'_>) -> Result<T>,
    ) -> Result<T> {
        if self.knowledge_runtime.borrow().is_none() {
            let runtime = self.materialize_knowledge_runtime()?;
            *self.knowledge_runtime.borrow_mut() = Some(runtime);
        }
        let runtime = self.knowledge_runtime.borrow();
        let runtime = runtime
            .as_ref()
            .expect("Knowledge runtime was materialised");
        let application = runtime.application(self.knowledge_context());
        operation(runtime, application)
    }

    pub fn knowledge_search(&self, query: &str, limit: usize) -> Result<KnowledgeSearchResult> {
        let candidate_limit = if limit == 0 { 0 } else { limit.max(256) };
        let mut result = self.with_knowledge(|runtime, application| {
            let mut result = application.search(query, candidate_limit);
            result.absences.extend(runtime.absences.clone());
            Ok(result)
        })?;
        self.apply_learned_accessibility(query, &mut result)?;
        result.hits.truncate(limit);
        if let Err(error) = self.knowledge_store().remember_search_hits(&result.hits) {
            result.absences.push(format!(
                "Knowledge address cache unavailable; live search results remain valid: {}",
                error.message()
            ));
        }
        Ok(result)
    }

    fn apply_learned_accessibility(
        &self,
        query: &str,
        result: &mut KnowledgeSearchResult,
    ) -> Result<()> {
        let Some(store) = PaletteBackend::familiarity(self)? else {
            return Ok(());
        };
        if store.is_empty() {
            return Ok(());
        }
        let context = self.knowledge_context();
        let now = now_ms();
        let history = self.knowledge_store().history(Some(&context), None)?;
        let mut influenced = false;
        for hit in &mut result.hits {
            let destination = store.assess_destination(
                &hit.resource,
                &context,
                now,
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
            let route = history
                .iter()
                .filter_map(|receipt| receipt.route.as_ref())
                .filter(|route| route.destination() == Some(&hit.resource))
                .map(|route| {
                    store.assess_route(
                        &route.route,
                        &hit.resource,
                        &context,
                        now,
                        DEFAULT_FAMILIARITY_HALF_LIFE_MS,
                    )
                })
                .filter(|assessment| !assessment.is_empty())
                .max_by(|left, right| {
                    left.contextual_frecency
                        .total_cmp(&right.contextual_frecency)
                        .then_with(|| left.frecency.total_cmp(&right.frecency))
                });
            let learned = destination.contextual_frecency
                + route
                    .as_ref()
                    .map(|assessment| assessment.contextual_frecency)
                    .unwrap_or_default();
            // Bounded, monotonic application boost. It can re-order eligible fuzzy
            // candidates but can never change provider score or eligibility.
            let boost = (learned.ln_1p() * 0.08).min(0.35);
            influenced |= boost > 0.0;
            hit.ranking = Some(KnowledgeRankingEvidence {
                provider_score: hit.score,
                navigation_score: hit.score + boost,
                destination,
                route,
            });
        }
        if influenced {
            result.hits.sort_by(|left, right| {
                exact_knowledge_hit(left, query)
                    .cmp(&exact_knowledge_hit(right, query))
                    .reverse()
                    .then_with(|| {
                        let left_score = left
                            .ranking
                            .as_ref()
                            .map(|ranking| ranking.navigation_score)
                            .unwrap_or(left.score);
                        let right_score = right
                            .ranking
                            .as_ref()
                            .map(|ranking| ranking.navigation_score)
                            .unwrap_or(right.score);
                        right_score.total_cmp(&left_score)
                    })
                    .then_with(|| left.resource.cmp(&right.resource))
            });
        }
        Ok(())
    }

    pub fn knowledge_address(&self, resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
        if let Some(address) = self.knowledge_store().address(resource)? {
            return Ok(Some(address));
        }
        self.with_knowledge(|runtime, application| {
            if runtime
                .wiki
                .as_ref()
                .is_some_and(|index| index.contains(resource))
            {
                return Ok(Some(KnowledgeAddress::Wiki(resource.clone())));
            }
            if runtime
                .material
                .iter()
                .any(|material| material.binding.source.as_str() == resource.as_str())
            {
                return Ok(Some(KnowledgeAddress::Source(SourceRef::parse(
                    resource.as_str(),
                )?)));
            }
            if runtime.project_map.endpoint(resource).is_some() {
                return Ok(Some(KnowledgeAddress::ProjectMap(resource.clone())));
            }
            let result = application.search(resource.as_str(), 256);
            Ok(result
                .hits
                .into_iter()
                .find(|hit| hit.resource == *resource)
                .map(|hit| hit.address))
        })
    }

    pub fn knowledge_read(
        &self,
        address: &KnowledgeAddress,
    ) -> Result<aikit_core::KnowledgeReading> {
        self.with_knowledge(|_, application| application.read(address))
    }

    pub fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        self.with_knowledge(|_, application| {
            application.relations(address, depth, max_nodes, max_edges)
        })
    }

    pub fn knowledge_route(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<KnowledgeRoute> {
        let route = self.with_knowledge(|_, application| application.route(query, addresses))?;
        self.knowledge_store().append_route(route.clone())?;
        let observation = route
            .familiarity_observation(
                format!("knowledge-route-use/{}", aikit_core::EventId::generate()),
                now_ms(),
            )?
            .from_surface(ResourceRef::parse("surface/aikit/knowledge")?);
        append_familiarity_observation(&self.index, observation)?;
        Ok(route)
    }

    pub fn knowledge_frame(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<KnowledgeContextPack> {
        let mut frame = self.with_knowledge(|runtime, application| {
            let mut frame = application.context_pack(query, addresses);
            frame.absences.extend(runtime.absences.clone());
            Ok(frame)
        })?;
        frame.derive_uncertainty();
        self.knowledge_store().append_frame(frame.clone())?;
        Ok(frame)
    }

    pub fn knowledge_sources(&self, address: &KnowledgeAddress) -> Result<KnowledgeSources> {
        self.with_knowledge(|_, application| {
            use aikit_core::KnowledgeOperations;
            application.sources(address)
        })
    }

    pub fn knowledge_explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        let mut explanation = self.with_knowledge(|_, application| application.explain(address))?;
        // Explain keeps provider-native detail and learned ranking evidence separate.
        let resource = address.resource_ref();
        let ranking = self
            .knowledge_search(resource.as_str(), 256)?
            .hits
            .into_iter()
            .find(|hit| hit.resource == resource)
            .and_then(|hit| hit.ranking);
        if let Some(ranking) = ranking {
            explanation.detail = Some(serde_json::json!({
                "provider": explanation.detail,
                "ranking": ranking,
                "signalClasses": ["provider-relevance", "frecency", "context"]
            }));
        }
        Ok(explanation)
    }

    pub fn knowledge_history(
        &self,
        resource: Option<&ResourceRef>,
    ) -> Result<Vec<KnowledgeApplicationReceipt>> {
        self.knowledge_store()
            .history(Some(&self.knowledge_context()), resource)
    }

    pub fn knowledge_status(&self) -> Result<KnowledgeProviderStatus> {
        self.with_knowledge(|runtime, application| {
            let mut status = application.status();
            status.absences.extend(runtime.absences.clone());
            Ok(status)
        })
    }

    pub fn knowledge_forget(&mut self, scope: ForgetScope) -> Result<()> {
        append_familiarity_reset(&self.index, scope, now_ms())
    }

    fn materialize_knowledge_runtime(&self) -> Result<KnowledgeRuntime> {
        let root = self
            .descriptor
            .project_root
            .as_deref()
            .unwrap_or(&self.invocation_cwd);
        let mut absences = Vec::new();
        let discovered = discover_material(root, self.home.root(), &mut absences)?;

        let wiki = if discovered.wiki.is_empty() {
            absences.push("SemanticWiki material absent from the project horizon".into());
            None
        } else {
            match SemanticWikiIndex::rebuild(discovered.wiki) {
                Ok(index) => Some(index),
                Err(error) => {
                    absences.push(format!(
                        "SemanticWiki materialisation degraded: {}",
                        error.message()
                    ));
                    None
                }
            }
        };

        let mut material = Vec::new();
        let mut bindings = Vec::new();
        for item in discovered.sources.into_values() {
            bindings.push(item.binding.clone());
            material.push(item);
        }
        let pool = SourcePool::new("pool:project", bindings)?;
        material = material_for_actor(&pool, &material, None, true)?;
        let mut native_source = NativeSourcePoolProvider::new();
        native_source.rebuild(&material)?;

        let mut bkmr = None;
        if let Some(config) = self.active_provider_config("tool/search/bkmr") {
            let db = config.get("db").and_then(|value| value.as_str());
            if let Some(db) = db {
                let db_path = resolve_provider_path(root, db);
                let embeddings = config
                    .get("embeddings")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let mut provider = BkmrSourcePoolProvider::new(
                    SystemRunner::new().with_cwd(root),
                    db_path,
                    embeddings,
                );
                if provider.status().available {
                    if let Err(error) = provider.rebuild(&material) {
                        absences.push(format!("bkmr SourcePool degraded: {}", error.message()));
                    }
                } else {
                    absences.push(
                        "bkmr SourcePool configured but provider executable is unavailable".into(),
                    );
                }
                bkmr = Some(provider);
            } else {
                absences.push("bkmr is active but has no resolved `db` provider binding".into());
            }
        }

        let mut code = None;
        if let Some(project_id) = self.descriptor.project_id.as_ref() {
            let source = SourceRef::parse(format!("source:project-code:{project_id}"))?;
            let mut provider = GitNexusCodeIndexProvider::new(
                SystemRunner::new().with_cwd(root),
                project_id.to_string(),
                source,
                None,
            );
            let status = provider.status();
            if status.available && status.capabilities.index {
                if let Err(error) = provider.index(root, false) {
                    absences.push(format!("GitNexus CodeIndex degraded: {}", error.message()));
                }
            } else {
                absences.push("GitNexus CodeIndex unavailable for this Project".into());
            }
            code = Some(provider);
        } else {
            absences.push("ProjectMap CodeIndex unavailable: no canonical Project identity".into());
        }

        let project_map = self.build_project_map(wiki.as_ref(), &material)?;

        Ok(KnowledgeRuntime {
            wiki,
            material,
            native_source,
            bkmr,
            code,
            project_map,
            absences,
        })
    }

    fn active_provider_config(&self, id: &str) -> Option<&aikit_core::ConfigTable> {
        let id = aikit_core::CapsuleId::parse(id).ok()?;
        self.view
            .active
            .get(&id)
            .map(|capability| &capability.config)
    }
    fn build_project_map(
        &self,
        wiki: Option<&SemanticWikiIndex>,
        material: &[SourceMaterial],
    ) -> Result<ProjectMap> {
        let mut map = ProjectMap::new();
        let shallow = PaletteBackend::navigation_index(self);
        let mut project_resource = None;

        for record in ResourceIndex::resources(&shallow) {
            let authority = record
                .descriptor
                .sources
                .iter()
                .find_map(|source| source.authority)
                .unwrap_or(SourceAuthority::Derived);
            let provider = record.providers.first().map(|offer| offer.provider.clone());
            let revision = record
                .descriptor
                .sources
                .iter()
                .find_map(|source| source.revision.as_ref().map(ToString::to_string));
            map.add_endpoint(ProjectMapEndpoint {
                resource: record.descriptor.id.clone(),
                kind: record.descriptor.kind,
                lens: ProjectLens::Canon,
                authority,
                provider,
                revision,
                label: Some(record.descriptor.name.clone()),
            })?;
            if record.descriptor.kind == ResourceKind::Project {
                project_resource = Some(record.descriptor.id.clone());
            }
        }

        if let Some(index) = wiki {
            for resource in index.discover() {
                let object = index
                    .resolve(&resource)
                    .expect("discovered Wiki ref resolves");
                let kind = match object {
                    WikiObject::Space(_) => ResourceKind::KnowledgeSpace,
                    WikiObject::Frame(_) => ResourceKind::KnowledgeFrame,
                    _ => ResourceKind::KnowledgeNode,
                };
                map.add_endpoint(ProjectMapEndpoint {
                    resource: resource.clone(),
                    kind,
                    lens: ProjectLens::SemanticWiki,
                    authority: SourceAuthority::Authored,
                    provider: Some(ProviderRef::parse(
                        aikit_core::NATIVE_SEMANTIC_WIKI_PROVIDER,
                    )?),
                    revision: Some(object.revision().to_string()),
                    label: None,
                })?;
            }
        }

        for item in material {
            map.add_endpoint(ProjectMapEndpoint {
                resource: ResourceRef::parse(item.binding.source.as_str())?,
                kind: ResourceKind::KnowledgeSource,
                lens: ProjectLens::SourcePool,
                authority: SourceAuthority::Observed,
                provider: Some(ProviderRef::parse("provider/source-pool/native")?),
                revision: Some(item.binding.revision.to_string()),
                label: Some(item.binding.title.clone()),
            })?;
        }

        if let Some(project) = project_resource.as_ref() {
            let endpoints = map
                .endpoints()
                .map(|endpoint| endpoint.resource.clone())
                .filter(|resource| resource != project)
                .collect::<Vec<_>>();
            for resource in endpoints {
                map.bind(ProjectMapBinding {
                    from: project.clone(),
                    to: resource,
                    relation: "contains".into(),
                    reversible: true,
                    authority: SourceAuthority::Derived,
                    provider: None,
                    provenance: Vec::new(),
                })?;
            }
        }

        if let Some(index) = wiki {
            for wiki_ref in index.discover() {
                for source in index.sources(&wiki_ref) {
                    let source_ref = ResourceRef::parse(source.as_str())?;
                    if map.endpoint(&source_ref).is_none() {
                        continue;
                    }
                    map.bind(ProjectMapBinding {
                        from: wiki_ref.clone(),
                        to: source_ref,
                        relation: "source".into(),
                        reversible: true,
                        authority: SourceAuthority::Authored,
                        provider: Some(ProviderRef::parse(
                            aikit_core::NATIVE_SEMANTIC_WIKI_PROVIDER,
                        )?),
                        provenance: Vec::new(),
                    })?;
                }
            }
        }
        Ok(map)
    }
}

#[derive(Default)]
struct DiscoveredMaterial {
    wiki: Vec<WikiObject>,
    sources: BTreeMap<SourceRef, SourceMaterial>,
}

fn discover_material(
    root: &Path,
    home: &Path,
    absences: &mut Vec<String>,
) -> Result<DiscoveredMaterial> {
    let mut discovered = DiscoveredMaterial::default();
    let mut stack = vec![root.to_path_buf()];
    let mut files = 0usize;
    let mut conflicted_sources = BTreeSet::new();

    while let Some(dir) = stack.pop() {
        if dir == home || is_ignored_dir(&dir) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                absences.push(format!(
                    "Knowledge discovery could not read {}: {error}",
                    dir.display()
                ));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !is_ignored_dir(&path) && path != home {
                    stack.push(path);
                }
                continue;
            }
            if files >= MAX_DISCOVERY_FILES {
                absences.push(format!(
                    "Knowledge discovery stopped after {MAX_DISCOVERY_FILES} candidate files"
                ));
                stack.clear();
                break;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            files += 1;
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.len() > MAX_DISCOVERY_FILE_BYTES {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(_) => continue,
            };

            if text.contains("okf-wiki/v1") {
                match parse_wiki_objects(&text) {
                    Ok(objects) => discovered.wiki.extend(objects),
                    Err(collection_error) => match OkfWikiBundle::parse_json(&text) {
                        Ok(bundle) => discovered.wiki.push(bundle.wiki),
                        Err(_) => absences.push(format!(
                            "self-identified SemanticWiki material at {} is invalid: {}",
                            path.display(),
                            collection_error.message()
                        )),
                    },
                }
            }

            let source_items = serde_json::from_str::<SourceMaterial>(&text)
                .map(|item| vec![item])
                .or_else(|_| serde_json::from_str::<Vec<SourceMaterial>>(&text));
            if let Ok(items) = source_items {
                for item in items {
                    let source = item.binding.source.clone();
                    if conflicted_sources.contains(&source) {
                        continue;
                    }
                    if let Some(previous) = discovered.sources.get(&source) {
                        if previous != &item {
                            discovered.sources.remove(&source);
                            conflicted_sources.insert(source.clone());
                            absences.push(format!(
                                "SourcePool material conflict for stable SourceRef {source}; conflicting copies were withheld"
                            ));
                        }
                    } else {
                        discovered.sources.insert(source, item);
                    }
                }
            }
        }
    }
    Ok(discovered)
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules" | ".next" | "dist"))
}

fn resolve_provider_path(root: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn exact_knowledge_hit(hit: &aikit_core::KnowledgeSearchHit, query: &str) -> bool {
    !query.is_empty()
        && (hit.resource.as_str().eq_ignore_ascii_case(query)
            || hit.label.eq_ignore_ascii_case(query))
}
