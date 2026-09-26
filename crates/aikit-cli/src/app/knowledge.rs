use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::knowledge_cache::{address_cache_key, BasisScope};
use aikit_adapters::bkmr::{
    bkmr_config_dir, discover_bkmr_stores, BkmrSourcePoolProvider, BkmrStoreSearchProvider,
};
use aikit_adapters::central_file_map::CentralFileMapProvider;
use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::now_field::{NowFieldScope, NowFieldSourcePoolProvider};
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::work_repos::{
    discover_work_projects, WorkRepoProject, WorkReposSourcePoolProvider,
};
use aikit_core::knowledge::{KnowledgeContextPack, KnowledgeRelationView, KnowledgeRoute};
use aikit_core::knowledge_code::CodeIndexProvider;
use aikit_core::knowledge_navigation::ProjectAuthoredPending;
use aikit_core::knowledge_source_pool::{
    material_for_actor, NativeSourcePoolProvider, SourceMaterial, SourcePool, SourcePoolProvider,
};
use aikit_core::knowledge_wiki::{parse_wiki_objects, OkfWikiBundle, WikiObject};
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::project_map::{ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint};
use aikit_core::repair_absence_lines;
use aikit_core::resource::{
    expression_scope_project, parse_or_search_expression_in_scope, resolve_subjects, ProviderRef,
    ResolveExpression, ResourceIndex, ResourceKind, ResourceRef, SourceAuthority, SourceRef,
};
use aikit_core::{
    FamiliarityContext, ForgetScope, KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation,
    KnowledgeOpenReceipt, KnowledgeProviderStatus, KnowledgeRankingEvidence, KnowledgeSearchResult,
    KnowledgeSources, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};
use aikit_store::{
    append_familiarity_observation, append_familiarity_reset, KnowledgeApplicationReceipt,
    KnowledgeApplicationStore, SqliteWikiProvider,
};
use aikit_tui::backend::PaletteBackend;

use super::Service;

const MAX_DISCOVERY_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DISCOVERY_FILES: usize = 4096;

/// Default wall-clock budget for one GitNexus subprocess call (capability
/// probe, index, or search) issued by a project's code-lens provider.
/// Indexing a real repository is legitimate work, not a health probe, so
/// this sits well above `aikit_core::probe::probe_budget()` (10s, meant for
/// `--version`/`--help`-shaped checks) — but it is still a hard ceiling: a
/// live gate against the real Central ground found `gitnexus analyze
/// --index-only` on one large Work repo running past five minutes
/// unbounded, which is exactly the class of stall a query must never carry
/// silently.
const DEFAULT_GITNEXUS_BUDGET: std::time::Duration = std::time::Duration::from_secs(45);

/// Environment override for [`DEFAULT_GITNEXUS_BUDGET`], in whole seconds.
const GITNEXUS_BUDGET_VAR: &str = "AIKIT_GITNEXUS_BUDGET_SECS";

/// At most this many discovered Work projects index in parallel. GitNexus's
/// own process is heavy (observed over 1 GB resident indexing one large
/// repo), so parallelism is bounded rather than one thread per project.
const MAX_GITNEXUS_PARALLELISM: usize = 4;

/// The effective GitNexus subprocess budget: `AIKIT_GITNEXUS_BUDGET_SECS`
/// when it parses to at least one second, otherwise
/// [`DEFAULT_GITNEXUS_BUDGET`]. An unparseable or zero override falls back
/// to the default rather than disabling the bound — the bound has no off
/// switch, matching the probe-budget discipline it sits beside.
fn gitnexus_budget() -> std::time::Duration {
    parse_gitnexus_budget(std::env::var(GITNEXUS_BUDGET_VAR).ok().as_deref())
}

/// Pure parse behind [`gitnexus_budget`], split out so the fallback rules are
/// testable without mutating process-wide environment state.
fn parse_gitnexus_budget(raw: Option<&str>) -> std::time::Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs >= 1)
        .map(std::time::Duration::from_secs)
        .unwrap_or(DEFAULT_GITNEXUS_BUDGET)
}

#[cfg(test)]
mod gitnexus_budget_tests {
    use super::{parse_gitnexus_budget, DEFAULT_GITNEXUS_BUDGET};

    #[test]
    fn absent_or_junk_or_zero_falls_back_to_the_default_rather_than_disabling_the_bound() {
        assert_eq!(parse_gitnexus_budget(None), DEFAULT_GITNEXUS_BUDGET);
        assert_eq!(parse_gitnexus_budget(Some("")), DEFAULT_GITNEXUS_BUDGET);
        assert_eq!(
            parse_gitnexus_budget(Some("not-a-number")),
            DEFAULT_GITNEXUS_BUDGET
        );
        assert_eq!(parse_gitnexus_budget(Some("0")), DEFAULT_GITNEXUS_BUDGET);
        assert_eq!(parse_gitnexus_budget(Some("-5")), DEFAULT_GITNEXUS_BUDGET);
    }

    #[test]
    fn a_valid_override_wins() {
        assert_eq!(
            parse_gitnexus_budget(Some("90")),
            std::time::Duration::from_secs(90)
        );
        // Surrounding whitespace (a shell export quirk) does not defeat it.
        assert_eq!(
            parse_gitnexus_budget(Some(" 12 ")),
            std::time::Duration::from_secs(12)
        );
    }
}

/// One project's GitNexus code-index degradation: the binary was present and
/// index-capable, but building that project's index failed. Held per project
/// so a scoped reply carries only its own scope's line and `knowledge status`
/// names every project — the same discipline `authored_pending` and the
/// per-project anchor lines already follow.
struct ProjectCodeDegradation {
    /// Work-relative project display, e.g. `Work/demo`.
    project: String,
    /// The full disclosure line, already naming the project and the reason.
    message: String,
}

struct ProjectOwnedAbsence {
    project: String,
    message: String,
}

pub(super) struct KnowledgeRuntime {
    wiki: Option<SqliteWikiProvider>,
    material: Vec<SourceMaterial>,
    native_source: NativeSourcePoolProvider,
    bkmr: Option<BkmrSourcePoolProvider<SystemRunner>>,
    /// The default read-only pool over the configured bkmr stores (owner
    /// correction 2026-09-23: bkmr is part of agent context sourcing by
    /// default). Registered only when it is operable, so an inactive pool
    /// costs one status note, not a per-query absence.
    bkmr_stores: Option<BkmrStoreSearchProvider<SystemRunner>>,
    central: Option<CentralFileMapProvider<SystemRunner>>,
    now_field: Option<NowFieldSourcePoolProvider<SystemRunner>>,
    now_field_roster: Vec<SourceMaterial>,
    /// The live Work-repos pool, one repo per discovered project. Registered
    /// only when projects were discovered, so a non-Central context keeps its
    /// old shape.
    work_repos: Option<WorkReposSourcePoolProvider<SystemRunner>>,
    /// `source:project:<project_id>:` prefix → `Work/<name>` display, for
    /// scoped queries to keep another project's repo hits out of their
    /// results.
    work_repo_scopes: BTreeMap<String, String>,
    /// `central:source:project:<project_id>:` prefix → `Work/<name>` for
    /// canonical owner refs returned by Central's project file map.
    central_project_source_scopes: BTreeMap<String, String>,
    central_expected: bool,
    /// `source:project-code:<project_id>` → `Work/<name>` for real CodeIndex
    /// hits whose public resource refs are opaque code digests.
    code_source_scopes: BTreeMap<String, String>,
    /// Parallel to `code`: the discovered Project owning each native index.
    /// Select providers before a scoped query so search failures cannot name
    /// another Project in the reply's diagnostic lines.
    code_project_scopes: Vec<String>,
    /// One code-index provider per discovered project; unavailable ones are
    /// kept so the absence is per project, never a global "provider absent".
    code: Vec<GitNexusCodeIndexProvider<SystemRunner>>,
    /// Per-project code-index degradations, kept out of the global per-query
    /// `absences` so another project's code state never leaks into a scoped
    /// reply; a reply carries only its own scope's, and status carries all.
    code_degradations: Vec<ProjectCodeDegradation>,
    project_map: ProjectMap,
    absences: Vec<String>,
    /// Informational per-project and per-pool disclosure lines (anchor
    /// state, index freshness, pool posture). Notes are state, not failures;
    /// only `knowledge status` carries them.
    status_notes: Vec<String>,
    /// Per-project rollups of pending authored relations. Search/resolve/frame
    /// replies carry at most their own scope's rollup; status carries every
    /// project plus per-target detail.
    authored_pending: Vec<ProjectAuthoredPending>,
    /// Materialisation failures are attributed by their native producer;
    /// scoped replies show their own Project, while status shows the World.
    project_absences: Vec<ProjectOwnedAbsence>,
    /// Authored edge ref → Work-relative project display, for scoped queries
    /// to keep another project's authored edges out of their results.
    authored_edge_projects: BTreeMap<String, String>,
    /// Compiled folder-subject ref (folder node or its parent/contains edge)
    /// → Work-relative project display, for scoped queries to keep another
    /// project's folder basis out of their results.
    folder_subject_projects: BTreeMap<String, String>,
    /// This invocation's own project in Work-relative display (`Work/demo`),
    /// when the invocation root sits in a Central Work project.
    current_project: Option<String>,
}

impl KnowledgeRuntime {
    /// Canonical Source refs retain Project ownership regardless of whether
    /// the caller reaches them through SourcePool or ProjectMap.
    fn source_belongs_to_scope(&self, resource: &str, display: &str) -> bool {
        if resource.starts_with("source:project:") {
            return self.work_repo_scopes.iter().any(|(prefix, project)| {
                resource.starts_with(prefix.as_str()) && project == display
            });
        }
        if resource.starts_with("central:source:project:") {
            return self
                .central_project_source_scopes
                .iter()
                .any(|(prefix, project)| {
                    resource.starts_with(prefix.as_str()) && project == display
                });
        }
        if let Some(rest) = resource.strip_prefix("central:source:control:root:Work/") {
            return rest
                .split_once('/')
                .is_some_and(|(project, _)| format!("Work/{project}") == display);
        }
        true
    }

    /// Flow cognition (W1.4/W1.5) reads identity and material through these
    /// same owners; it creates no second wiki or source-pool access path.
    pub(super) fn wiki_index(&self) -> Option<&SemanticWikiIndex> {
        self.wiki.as_ref().map(SqliteWikiProvider::index)
    }

    /// The Work-relative project display a reply's scope resolves to. An
    /// explicit scope key matches a pending project (display, project id or
    /// bare Work name) or this invocation's own project; a bare unknown name
    /// names `Work/<name>` directly. Without ground for a key, nothing is
    /// claimed and nothing is narrowed.
    fn scoped_project_display(&self, explicit_scope: Option<&str>) -> Option<String> {
        let Some(key) = explicit_scope else {
            return self.current_project.clone();
        };
        let key = key.trim();
        if key.is_empty() || key.contains("..") {
            return None;
        }
        if let Some(pending) = self
            .authored_pending
            .iter()
            .find(|pending| pending.matches_key(key))
        {
            return Some(pending.project.clone());
        }
        if let Some(current) = &self.current_project {
            if display_matches_key(current, key) {
                return Some(current.clone());
            }
        }
        if let Some(rest) = key.strip_prefix("Work/") {
            return (!rest.is_empty() && !rest.contains('/')).then(|| key.to_owned());
        }
        (!key.contains('/')).then(|| format!("Work/{key}"))
    }

    fn document_material(&self) -> Vec<SourceMaterial> {
        let mut material = self.material.clone();
        if let Some(provider) = &self.central {
            material.extend_from_slice(provider.descriptors());
        }
        material.extend_from_slice(&self.now_field_roster);
        material
    }

    pub(super) fn source_material(&self) -> &[SourceMaterial] {
        &self.material
    }
    pub(super) fn owner_source_provider(&self) -> Option<&dyn SourcePoolProvider> {
        self.central.as_ref().map(|p| p as &dyn SourcePoolProvider)
    }
    fn application(&self, context: FamiliarityContext) -> KnowledgeApplication<'_> {
        self.application_with_project_scope(
            context,
            None,
            self.now_field.as_ref(),
            self.work_repos.as_ref(),
        )
    }

    fn application_with_project_scope<'a>(
        &'a self,
        context: FamiliarityContext,
        scoped_project: Option<&str>,
        now_field: Option<&'a NowFieldSourcePoolProvider<SystemRunner>>,
        work_repos: Option<&'a WorkReposSourcePoolProvider<SystemRunner>>,
    ) -> KnowledgeApplication<'a> {
        let mut application =
            KnowledgeApplication::new(context).with_project_map(&self.project_map);
        if let Some(provider) = &self.wiki {
            application = application.with_wiki(provider);
        }
        if let Some(provider) = &self.central {
            application = application.with_source_pool(provider, provider.descriptors());
        }
        if let Some(provider) = now_field {
            // The NOW field is live owner ground searched in place; its
            // roster carries identity only, and reads go back to the file.
            application = application.with_source_pool(provider, &self.now_field_roster);
        }
        if let Some(provider) = work_repos {
            // Live Work-repos search slots in after NOW-field and before the
            // native shard baseline (addendum A-1): existing pools keep
            // priority on material they already carry, and the live pool's
            // refs (`source:project:…`) collide with nothing.
            application = application.with_source_pool(provider, &[]);
        }
        application = application.with_source_pool(&self.native_source, &self.material);
        if let Some(provider) = &self.bkmr {
            application = application.with_source_pool(provider, &self.material);
        }
        if let Some(provider) = &self.bkmr_stores {
            application = application.with_source_pool(provider, &[]);
        }
        for (provider, project) in self.code.iter().zip(&self.code_project_scopes) {
            if scoped_project.is_none_or(|scope| scope == project) {
                application = application.with_code(provider);
            }
        }
        application
    }
}

impl Service {
    pub(super) fn invalidate_knowledge_runtime(&self) {
        self.knowledge_runtime.borrow_mut().take();
    }

    pub(super) fn knowledge_context(&self) -> FamiliarityContext {
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

    pub(super) fn with_knowledge<T>(
        &self,
        operation: impl FnOnce(&KnowledgeRuntime, KnowledgeApplication<'_>) -> Result<T>,
    ) -> Result<T> {
        self.ensure_knowledge_project_scope()?;
        let owner_backed = self
            .knowledge_runtime
            .borrow()
            .as_ref()
            .is_some_and(|r| r.central_expected);
        if owner_backed {
            self.invalidate_knowledge_runtime();
        }
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
        // A query asked inside a project stands in that project's world
        // through the grammar itself (`: demo (@# @ text)`) — never a flag.
        let expression =
            parse_or_search_expression_in_scope(query, self.knowledge_scope_project().as_deref())?;
        let mut result = self.knowledge_resolve(&expression, limit)?;
        result.query = query.into();
        Ok(result)
    }

    pub fn knowledge_resolve(
        &self,
        expression: &ResolveExpression,
        limit: usize,
    ) -> Result<KnowledgeSearchResult> {
        let candidate_limit = if limit == 0 { 0 } else { limit.max(256) };
        // The scope that governs this reply's disclosure: an explicit `:`
        // scope in the expression, else the invocation's own project.
        let explicit_scope = expression_scope_project(expression).map(str::to_owned);
        // The retrieval itself is pure compute over the ground: answered from
        // the result cache when the basis has not moved. Learned
        // accessibility and search-hit memory below stay live on every call.
        let expression_key = serde_json::to_string(expression).map_err(|error| {
            aikit_core::AikitError::new("knowledge.cache_key", error.to_string())
        })?;
        let mut result = self.with_knowledge_cached(
            BasisScope::Full,
            &format!("resolve\x1f{candidate_limit}\x1f{expression_key}"),
            || {
                self.with_knowledge(|runtime, _application| {
                    let scoped_display = runtime.scoped_project_display(explicit_scope.as_deref());
                    if explicit_scope.is_some() && scoped_display.is_none() {
                        return Err(aikit_core::AikitError::new(
                            "knowledge.scope_invalid",
                            "Explicit Project scope is invalid or cannot be resolved",
                        ));
                    }
                    let discovered_project = scoped_display.as_deref().and_then(|display| {
                        runtime
                            .work_repos
                            .as_ref()?
                            .projects()
                            .iter()
                            .find(|project| format!("Work/{}", project.name) == display)
                    });
                    // The root NOW provider includes every Work project, but a
                    // Project-scoped reply may only query its own NOW files plus
                    // common Control records. Selecting globs before ripgrep also
                    // prevents sibling read failures from surfacing as absences.
                    let mut scoped_provider_absences = Vec::new();
                    let scoped_now_field =
                        match (scoped_display.as_deref(), runtime.now_field.as_ref()) {
                            (Some(_), Some(provider)) => {
                                let scope = provider
                                    .scope()
                                    .for_project(discovered_project.map(|p| p.name.as_str()));
                                match NowFieldSourcePoolProvider::connect(
                                    aikit_adapters::now_field::default_runner(&scope.central_root),
                                    aikit_adapters::ripgrep::executable(),
                                    scope,
                                ) {
                                    Ok(provider) => Some(provider),
                                    Err(error) => {
                                        scoped_provider_absences.push(format!(
                                            "NOW-field scoped search unavailable: {error}"
                                        ));
                                        None
                                    }
                                }
                            }
                            _ => None,
                        };
                    let now_field = if scoped_display.is_some() {
                        scoped_now_field.as_ref()
                    } else {
                        runtime.now_field.as_ref()
                    };
                    // A Work-repos search can fail before producing any hits. Search
                    // only the resolved Project at the provider boundary so another
                    // repo's ripgrep failure cannot appear in this reply's absences.
                    // The root World deliberately retains the broad native provider.
                    let scoped_work_repos = discovered_project.map(|project| {
                        WorkReposSourcePoolProvider::connect(
                            SystemRunner::new().with_env_removed("RIPGREP_CONFIG_PATH"),
                            aikit_adapters::ripgrep::executable(),
                            vec![project.clone()],
                        )
                    });
                    let work_repos = if scoped_display.is_some() {
                        scoped_work_repos.as_ref()
                    } else {
                        runtime.work_repos.as_ref()
                    };
                    let mut result = runtime
                        .application_with_project_scope(
                            self.knowledge_context(),
                            scoped_display.as_deref(),
                            now_field,
                            work_repos,
                        )
                        .resolve(expression, candidate_limit);
                    result.absences.extend(scoped_provider_absences);
                    result.absences.extend(runtime.absences.clone());
                    result.absences.extend(
                        runtime
                            .project_absences
                            .iter()
                            .filter(|absence| {
                                scoped_display
                                    .as_deref()
                                    .is_none_or(|project| project == absence.project)
                            })
                            .map(|absence| absence.message.clone()),
                    );
                    // Pending authored relations are scoped: a query sees its own
                    // scope's rollup; other projects' pendings stay with
                    // `knowledge status`.
                    if let Some(pending) = scoped_display.as_deref().and_then(|display| {
                        runtime
                            .authored_pending
                            .iter()
                            .find(|pending| pending.project == display)
                    }) {
                        result.absences.push(pending.rollup_line());
                    }
                    // Code-index degradations are scoped the same way: a reply carries
                    // only its own scope's line, never another project's — the leak
                    // this fix closes. Every project's degradation stays in
                    // `knowledge status`.
                    if let Some(display) = scoped_display.as_deref() {
                        for degradation in &runtime.code_degradations {
                            if degradation.project == display {
                                result.absences.push(degradation.message.clone());
                            }
                        }
                    }
                    // A scoped query keeps another project's compiled authored edges
                    // — and its compiled folder subjects — out of its results;
                    // unattributable material passes through. Work-repo hits carry
                    // their project in the ref itself (`source:project:<id>:…`), so
                    // the same discipline applies to them.
                    if let Some(display) = &scoped_display {
                        let attributed_to_other_project =
                            |attribution: &BTreeMap<String, String>, resource: &str| {
                                attribution
                                    .get(resource)
                                    .is_some_and(|project| project != display)
                            };
                        result.hits.retain(|hit| {
                            let resource = hit.resource.as_str();
                            // A Source ref may surface through SourcePool or a
                            // ProjectMap endpoint. The address wrapper does not
                            // change which Project owns it.
                            if !runtime.source_belongs_to_scope(resource, display) {
                                return false;
                            }
                            if attributed_to_other_project(
                                &runtime.authored_edge_projects,
                                resource,
                            ) || attributed_to_other_project(
                                &runtime.folder_subject_projects,
                                resource,
                            ) {
                                return false;
                            }
                            match &hit.address {
                                aikit_core::KnowledgeAddress::Code(reference) => runtime
                                    .code_source_scopes
                                    .get(reference.source.as_str())
                                    .is_some_and(|project| project == display),
                                _ => true,
                            }
                        });
                    }
                    Ok(result)
                })
            },
        )?;
        self.apply_learned_accessibility(&resolve_subjects(expression), &mut result)?;
        result.hits.truncate(limit);
        if let Err(error) = self.knowledge_store().remember_search_hits(&result.hits) {
            result.absences.push(format!(
                "Knowledge address cache unavailable; live search results remain valid: {error}"
            ));
        }
        Ok(result)
    }

    /// Human/shell front over [`Self::knowledge_resolve`]: it parses the typed
    /// input through the one operative grammar and delegates. `aikit knowledge
    /// search` reaches retrieval only through here.
    pub fn knowledge_open(&mut self, resource: &ResourceRef) -> Result<KnowledgeOpenReceipt> {
        let address = self.knowledge_address(resource)?.ok_or_else(|| {
            aikit_core::AikitError::new(
                "knowledge.open_unresolved",
                format!("no knowledge provider resolves {resource}"),
            )
        })?;
        let reading = self.knowledge_read(&address)?;
        let observation_id = format!("knowledge-open-use/{}", aikit_core::EventId::generate());
        let observation = aikit_core::FamiliarityObservation::destination(
            observation_id.clone(),
            resource.clone(),
            self.knowledge_context(),
            now_ms(),
        )
        .from_surface(ResourceRef::parse("surface/aikit/knowledge")?);
        append_familiarity_observation(&self.index, observation)?;
        Ok(KnowledgeOpenReceipt {
            opened: resource.clone(),
            address,
            provider: reading.provider.map(|provider| provider.to_string()),
            recorded: "familiarity/resource-use".into(),
            observation_id,
        })
    }

    fn apply_learned_accessibility(
        &self,
        subjects: &[&str],
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
                exact_knowledge_hit(left, subjects)
                    .cmp(&exact_knowledge_hit(right, subjects))
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

    /// The invocation's own project in Work-relative display, when the
    /// invocation stands inside a Central Work project. The member comes from
    /// directory shape — the project root's Work member when the resolved root
    /// sits under one, else the invocation cwd's member — so a Work directory
    /// whose profile discovery collapsed onto the world root still scopes to
    /// its own project.
    fn current_project_display(&self) -> Option<String> {
        let root = self
            .descriptor
            .project_root
            .as_deref()
            .unwrap_or(&self.invocation_cwd);
        let central_root = self.knowledge_central_root(root)?;
        Some(format!(
            "Work/{}",
            self.invocation_project_member(central_root, root)?
        ))
    }

    pub(super) fn knowledge_central_root<'a>(&'a self, root: &'a Path) -> Option<&'a Path> {
        self.knowledge_central_root.as_deref().or_else(|| {
            root.ancestors().find(|candidate| {
                candidate.join("Control").is_dir() && candidate.join("Work").is_dir()
            })
        })
    }

    /// The Work member this invocation's project context belongs to, read from
    /// directory shape alone. The resolved project root decides when it sits
    /// under `Work/<member>` itself (a discovered, rescued or specified
    /// project); otherwise the invocation cwd decides, which is exactly the
    /// collapse case — a Work directory with no marker of its own under a world
    /// root that carries one resolves its project root to the world root, and
    /// only the cwd still names the project. A root outside `Work/` (the world
    /// root itself, a Control location) contributes nothing, so a
    /// Control-location invocation keeps world-root behaviour and nothing is
    /// ever guessed from shape the ground does not carry. The cwd comparison
    /// canonicalises both sides because a resolved project root can be
    /// canonical while the cwd keeps its invoked spelling (or the reverse);
    /// an unreadable path names nothing rather than guessing.
    fn invocation_project_member(&self, central_root: &Path, root: &Path) -> Option<String> {
        work_member(central_root, root)
            .or_else(|| {
                let cwd = std::fs::canonicalize(&self.invocation_cwd).ok()?;
                let central = std::fs::canonicalize(central_root).ok()?;
                work_member(&central, &cwd)
            })
            .or_else(|| {
                self.external_project_member(central_root, &self.invocation_cwd)
                    .ok()
                    .flatten()
            })
    }

    /// A real checkout outside `Work/` can still be the same Project. Resolve
    /// its ProjectCentral identity against discovered World manifests rather
    /// than treating its location as authority to search the whole World.
    fn external_project_member(&self, central_root: &Path, root: &Path) -> Result<Option<String>> {
        let root = root.canonicalize().map_err(|error| {
            aikit_core::AikitError::new("knowledge.project_scope_unresolved", error.to_string())
        })?;
        let central_root = central_root.canonicalize().map_err(|error| {
            aikit_core::AikitError::new("knowledge.project_scope_unresolved", error.to_string())
        })?;
        if root == central_root
            || root.starts_with(central_root.join("Control"))
            || root.starts_with(central_root.join("Work"))
        {
            return Ok(None);
        }
        let project_root = root
            .ancestors()
            .take_while(|path| *path != central_root)
            .find(|path| {
                path.join("ProjectCentral/project.json").is_file() || path.join(".git").exists()
            });
        let Some(project_root) = project_root else {
            if self.knowledge_central_root.is_some()
                || root.starts_with(central_root.join("worktrees"))
            {
                return Err(aikit_core::AikitError::new(
                    "knowledge.project_scope_unresolved",
                    format!(
                        "{} has no ProjectCentral identity in the configured Central World",
                        root.display()
                    ),
                ));
            }
            return Ok(None);
        };
        let binding = aikit_adapters::ProjectCentralFilesystemBinding::inspect(project_root, None)
            .map_err(|error| {
                aikit_core::AikitError::new(
                    "knowledge.project_scope_unresolved",
                    format!(
                        "cannot resolve ProjectCentral identity at {}: {error}",
                        project_root.display()
                    ),
                )
            })?;
        let project_id = &binding.semantic.project_id;
        let matches: Vec<_> = discover_work_projects(&central_root)
            .into_iter()
            .filter_map(|entry| match entry {
                aikit_adapters::work_repos::WorkProjectEntry::Project(project)
                    if project.project_id == *project_id =>
                {
                    Some(project.name)
                }
                _ => None,
            })
            .collect();
        match matches.as_slice() {
            [name] => Ok(Some(name.clone())),
            _ => Err(aikit_core::AikitError::new(
                "knowledge.project_scope_unresolved",
                format!(
                    "ProjectCentral identity {project_id} at {} matches {} discovered Work Projects; Knowledge refuses a broad query",
                    project_root.display(),
                    matches.len()
                ),
            )),
        }
    }

    fn ensure_knowledge_project_scope(&self) -> Result<()> {
        if let Some(central_root) = self.knowledge_central_root(&self.invocation_cwd) {
            self.external_project_member(central_root, &self.invocation_cwd)?;
        }
        Ok(())
    }

    /// The bare Work name used as the lowered scope key (`demo` for
    /// `Work/demo`) — the readable spelling of the project world.
    pub(super) fn knowledge_scope_project(&self) -> Option<String> {
        self.current_project_display().and_then(|display| {
            display
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
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
            let result =
                application.resolve(&ResolveExpression::ordinary_search(resource.as_str()), 256);
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
        let key = address_cache_key("read", address, "");
        self.with_knowledge_cached(BasisScope::Ground, &key, || {
            self.with_knowledge(|_, application| application.read(address))
        })
    }

    /// Additive native document facet; plain KnowledgeReading users keep their API.
    pub fn knowledge_read_document(&self, address: &KnowledgeAddress) -> Result<serde_json::Value> {
        let key = address_cache_key("read-document", address, "");
        self.with_knowledge_cached(BasisScope::Ground, &key, || {
            self.with_knowledge(|runtime, application| {
                let reading = application.read(address)?;
                let material = runtime.document_material();
                let objects: Vec<_> = runtime
                    .wiki_index()
                    .map(|index| {
                        index
                            .discover()
                            .into_iter()
                            .filter_map(|reference| index.resolve(&reference))
                            .collect()
                    })
                    .unwrap_or_default();
                let relations = application
                    .relations(address, 1, 256, 512)
                    .and_then(|view| {
                        aikit_adapters::wiki_document::complete_source_relations(
                            &reading, view, &material, &objects,
                        )
                    })
                    .ok();
                aikit_adapters::wiki_document::reading_document(
                    &reading,
                    relations.as_ref(),
                    &runtime.document_material(),
                )
            })
        })
    }

    /// Complete metadata projection within explicit caller budgets; ranking and
    /// admission remain the existing native query path, never a UI grammar.
    pub fn knowledge_graph(
        &self,
        query: &str,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<serde_json::Value> {
        if max_nodes == 0 || max_nodes > 20_000 || max_edges == 0 || max_edges > 100_000 {
            return Err(aikit_core::AikitError::new(
                "knowledge.graph_budget",
                "graph limits require 1..=20000 nodes and 1..=100000 edges",
            ));
        }
        let found =
            self.knowledge_search(query, max_nodes.saturating_add(max_edges).saturating_add(1))?;
        let expression =
            parse_or_search_expression_in_scope(query, self.knowledge_scope_project().as_deref())?;
        let graph_key = format!("graph\x1f{max_nodes}\x1f{max_edges}\x1f{query}");
        self.with_knowledge_cached(BasisScope::Full, &graph_key, || {
            self.with_knowledge(|runtime, _| {
                let objects: Vec<_> = runtime
                    .wiki_index()
                    .map(|index| {
                        index
                            .discover()
                            .into_iter()
                            .filter_map(|reference| index.resolve(&reference))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut material = runtime.document_material();
                if let Some(display) =
                    runtime.scoped_project_display(expression_scope_project(&expression))
                {
                    material.retain(|item| {
                        runtime.source_belongs_to_scope(item.binding.source.as_str(), &display)
                    });
                }
                    });
                }
                let mut hits = found.hits.clone();
                if query.trim().is_empty() {
                    for item in &material {
                        let provider = runtime
                            .central
                            .as_ref()
                            .filter(|p| {
                                p.descriptors()
                                    .iter()
                                    .any(|m| m.binding.source == item.binding.source)
                            })
                            .map(|p| p.status().provider)
                            .or_else(|| {
                                runtime
                                    .now_field
                                    .as_ref()
                                    .filter(|_| {
                                        runtime
                                            .now_field_roster
                                            .iter()
                                            .any(|m| m.binding.source == item.binding.source)
                                    })
                                    .map(|p| p.status().provider)
                            })
                            .unwrap_or_else(|| runtime.native_source.status().provider);
                        hits.push(aikit_core::knowledge_navigation::KnowledgeSearchHit {
                            address: KnowledgeAddress::Source(item.binding.source.clone()),
                            resource: ResourceRef::parse(item.binding.source.as_str())?,
                            kind: ResourceKind::KnowledgeSource,
                            label: item.binding.title.clone(),
                            score: 0.0,
                            snippet: String::new(),
                            provider,
                            authority: SourceAuthority::Observed,
                            ranking: None,
                            corroborated_by: Vec::new(),
                        });
                    }
                }
                Ok(aikit_adapters::wiki_graph::project_graph(
                    &hits,
                    &objects,
                    &material,
                    max_nodes,
                    max_edges,
                    &found.absences,
                ))
            })
        })
    }

    pub fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        let key = address_cache_key(
            "relations",
            address,
            &format!("\x1f{depth}\x1f{max_nodes}\x1f{max_edges}"),
        );
        self.with_knowledge_cached(BasisScope::Ground, &key, || {
            self.with_knowledge(|runtime, application| {
                let view = application.relations(address, depth, max_nodes, max_edges)?;
                if !matches!(address, KnowledgeAddress::Source(_)) || depth == 0 {
                    return Ok(view);
                }
                let reading = application.read(address)?;
                let objects: Vec<_> = runtime
                    .wiki_index()
                    .map(|index| {
                        index
                            .discover()
                            .into_iter()
                            .filter_map(|reference| index.resolve(&reference))
                            .collect()
                    })
                    .unwrap_or_default();
                aikit_adapters::wiki_document::complete_source_relations(
                    &reading,
                    view,
                    &runtime.document_material(),
                    &objects,
                )
            })
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
            frame.absences.extend(
                runtime
                    .project_absences
                    .iter()
                    .filter(|absence| {
                        runtime
                            .current_project
                            .as_deref()
                            .is_none_or(|project| project == absence.project)
                    })
                    .map(|absence| absence.message.clone()),
            );
            // A frame carries its own project's pending rollup and code-index
            // degradation, never other projects'.
            if let Some(current) = &runtime.current_project {
                if let Some(pending) = runtime
                    .authored_pending
                    .iter()
                    .find(|pending| &pending.project == current)
                {
                    frame.absences.push(pending.rollup_line());
                }
                for degradation in &runtime.code_degradations {
                    if &degradation.project == current {
                        frame.absences.push(degradation.message.clone());
                    }
                }
            }
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
            .knowledge_resolve(&ResolveExpression::ordinary_search(resource.as_str()), 256)?
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
        let mut status = self.with_knowledge(|runtime, application| {
            let mut status = application.status();
            status.absences.extend(runtime.absences.clone());
            status.absences.extend(
                runtime
                    .project_absences
                    .iter()
                    .map(|absence| absence.message.clone()),
            );
            // Notes are the loud per-project surface: anchor state, map
            // freshness, pool posture — state, not per-query failures.
            status.notes.extend(runtime.status_notes.clone());
            // Status is the only surface that carries every project's pending
            // rollup and the full per-target detail.
            for pending in &runtime.authored_pending {
                status.absences.push(pending.rollup_line());
            }
            // Likewise every project's code-index degradation: a scoped reply
            // carries only its own, but the diagnostic surface names all.
            for degradation in &runtime.code_degradations {
                status.absences.push(degradation.message.clone());
            }
            status.authored_pending = runtime.authored_pending.clone();
            Ok(status)
        })?;
        // The result cache's own disclosure: what it is, what this invocation
        // did with it, or why it stood down.
        status.notes.push(self.knowledge_result_cache_note());
        Ok(status)
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
        let mut status_notes = Vec::new();
        let mut work_projects: Vec<WorkRepoProject> = Vec::new();
        let mut wiki_registers = Vec::new();
        let mut authored_pending = Vec::new();
        let mut project_absences = Vec::new();
        let mut authored_edge_projects = BTreeMap::new();
        let mut folder_subject_projects = BTreeMap::new();
        let central_root = self.knowledge_central_root(root);
        let mut discovered = discover_material(
            root,
            self.home.root(),
            &mut absences,
            central_root.is_none(),
        )?;
        if let Some(central_root) = central_root {
            let executable = std::env::var_os("CENTRAL_CTRL_BIN")
                .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("ctrl"));
            match aikit_adapters::central_wiki::read_central_wiki(
                &SystemRunner::new(),
                &executable,
                central_root,
            ) {
                Ok(reading) => {
                    discovered.wiki = reading.objects;
                    wiki_registers = reading.registers;
                    absences.extend(reading.absences);
                }
                Err(error) => absences.push(format!("Central wiki discovery unavailable: {error}")),
            }
            // W10 V3: compiled entity materialisation joins the discovered
            // wiki before the index rebuild; colliding stand-in nodes adopt
            // the entity convention (wiki:node:identity keeps its ref).
            let entities =
                aikit_adapters::central_entities::materialise_central_entities(central_root);
            absences.extend(entities.absences);
            aikit_adapters::central_entities::adopt_into(&mut discovered.wiki, entities.objects);
            // W10 V4 extension: capability matrices compile in both placements
            // (project spaces + the Central root composition), origin Compiled.
            let matrices = aikit_adapters::capability_matrix::compile_world_matrices(central_root);
            absences.extend(matrices.absences);
            project_absences.extend(matrices.project_absences.into_iter().map(|absence| {
                ProjectOwnedAbsence {
                    project: absence.project,
                    message: absence.message,
                }
            }));
            aikit_adapters::central_entities::adopt_into(&mut discovered.wiki, matrices.objects);
            // CASE 19 / W10 V9.4: authored Markdown under each project's
            // ProjectCentral/user/** compiles its explicit [[wikilinks]]
            // into the same SemanticWiki as ordinary Compiled edges — never
            // as new WikiNodes. Unresolved links stay disclosed as
            // per-project rollups, never as synthetic edges.
            let authored_wiki =
                aikit_adapters::projectcentral_authored_wiki::compile_world_authored_wiki(
                    central_root,
                );
            project_absences.extend(authored_wiki.absences.into_iter().map(|absence| {
                ProjectOwnedAbsence {
                    project: absence.project,
                    message: absence.message,
                }
            }));
            authored_pending = authored_wiki.pending;
            authored_edge_projects = authored_wiki.edge_projects;
            aikit_adapters::central_entities::adopt_into(
                &mut discovered.wiki,
                authored_wiki
                    .edges
                    .into_iter()
                    .map(WikiObject::Edge)
                    .collect(),
            );
            // SharedField SF4: the projected world's Explore discovery seed
            // joins the same SemanticWiki — stable entry refs, typed
            // relations, derived presentation edges and SharedField
            // membership — so Search/Resolve reveals eligible presentations
            // without a second store. An absent seed is ordinary (nothing
            // projected yet); it never gates addressability.
            if let Some(seed_path) = aikit_adapters::oi_explore::discovery_seed_path(central_root) {
                match aikit_adapters::oi_explore::read_explore_discovery(&seed_path) {
                    Ok(reading) => {
                        absences.extend(reading.absences);
                        aikit_adapters::central_entities::adopt_into(
                            &mut discovered.wiki,
                            reading.objects,
                        );
                    }
                    Err(error) => {
                        absences.push(format!("Explore discovery seed unreadable: {error}"))
                    }
                }
            }
            // W10 V5: a project context binds the same entity refs through
            // Central's effective world sources — never a second subject;
            // declared exclusions withhold, per-hop provenance is recorded.
            // The project is read from directory shape (the project root's
            // Work member, else the invocation cwd's), so a Work directory
            // whose discovery collapsed onto the world root still becomes a
            // scoped context instead of running uncontextualised.
            if let Some(project) = self.invocation_project_member(central_root, root) {
                // Scoping does not depend on a manifest or a populated wiki: a
                // Work member with no ProjectCentral at all still scopes, as
                // `project:<name>`, and says so. No manifest also means no
                // project record, so the root lineage applies by convention —
                // the binding read below confirms it against Central and
                // discloses the inheritance (or withholds, if Central is
                // unreachable).
                if !central_root
                    .join("Work")
                    .join(&project)
                    .join("ProjectCentral/project.json")
                    .exists()
                {
                    absences.push(format!(
                        "Project Work/{project} has no ProjectCentral manifest; this context scopes as {} without a project wiki, and with no project record the root lineage applies",
                        aikit_adapters::central_world_sources::project_world_ref(
                            central_root, &project
                        )
                    ));
                }
                let world_binding = aikit_adapters::central_world_sources::read_project_binding(
                    &SystemRunner::new(),
                    &executable,
                    central_root,
                    &project,
                    &mut absences,
                );
                if let Some(binding) = world_binding {
                    aikit_adapters::central_world_sources::bind_project_context(
                        &mut discovered.wiki,
                        &binding,
                        &mut absences,
                    );
                } else {
                    // An unavailable owner policy must not publish root-private
                    // material into a Project. Absence has already been handled
                    // by read_project_binding's explicit root-lineage rule.
                    discovered.wiki.clear();
                    wiki_registers.clear();
                    absences.push("Central World disclosure unavailable; inherited Central graph withheld, not broadened".into());
                    // This does not revoke the current Project's independently
                    // accepted authored source relations. Rebuild ONLY those
                    // edges from its public filesystem binding, never from the
                    // already federated root or sibling-project graph. The
                    // binding still enforces agent-readable source descriptors
                    // and .no-agent-retrieval; no World inheritance is assumed.
                    let project_root = central_root.join("Work").join(&project);
                    let project_display = format!("Work/{project}");
                    let local_authored = aikit_adapters::ProjectCentralFilesystemBinding::inspect(
                        &project_root,
                        None,
                    )
                    .and_then(|binding| {
                        let project_id = binding.semantic.project_id.clone();
                        aikit_adapters::projectcentral_authored_wiki::projectcentral_authored_wiki(
                            &binding,
                        )
                        .map(|authored| (project_id, authored))
                    });
                    match local_authored {
                        Ok((project_id, authored)) => {
                            // The world compile may already carry this
                            // project's rollup; the local rebuild replaces it
                            // so a project discloses exactly one.
                            if let Some(rollup) =
                                aikit_adapters::projectcentral_authored_wiki::pending_rollup(
                                    project_display.clone(),
                                    Some(project_id),
                                    &authored.compilation.pending,
                                )
                            {
                                authored_pending
                                    .retain(|pending| pending.project != project_display);
                                authored_pending.push(rollup);
                            }
                            for edge in &authored.compilation.edges {
                                authored_edge_projects.insert(
                                    edge.ref_id.as_str().to_owned(),
                                    project_display.clone(),
                                );
                            }
                            // The project's OWN authored wiki objects come
                            // back with its edges: nodes and spaces restored
                            // beside them, so the withheld context keeps the
                            // project's own graph — a restored edge must not
                            // point at a target the rebuild dropped.
                            discovered.wiki.extend(authored.wiki_objects);
                            discovered.wiki.extend(
                                authored.compilation.edges.into_iter().map(WikiObject::Edge),
                            );
                        }
                        Err(error) => absences
                            .push(format!("Project-local authored graph unavailable: {error}")),
                    }
                }
            }

            // Folder subjects: each project's ProjectCentral register
            // compiles as the directory BASIS of this context's graph —
            // folder nodes under the project-root anchor, `contains`-wired
            // to the file-level subjects already in this materialised set.
            // A materialisation-time construct: nothing is written into
            // Central's wiki. A project context (the shape-derived scoping)
            // compiles its own project's folder basis only; a world context
            // keeps every project's.
            let materialised_refs: BTreeSet<String> = discovered
                .wiki
                .iter()
                .map(|object| object.ref_id().as_str().to_owned())
                .chain(discovered.wiki.iter().filter_map(|object| match object {
                    WikiObject::Edge(edge) => Some(edge.to_ref.as_str().to_owned()),
                    _ => None,
                }))
                .chain(discovered.wiki.iter().filter_map(|object| match object {
                    WikiObject::Edge(edge) => Some(edge.from_ref.as_str().to_owned()),
                    _ => None,
                }))
                .collect();
            let folder_subjects =
                aikit_adapters::projectcentral_folder_subjects::compile_world_folder_subjects(
                    central_root,
                    &materialised_refs,
                    self.invocation_project_member(central_root, root)
                        .as_deref(),
                );
            // Anchor gaps are status notes (Design D), never per-query
            // absences: the per-project state belongs in `knowledge status`,
            // which carries every project's line below.
            for absence in folder_subjects.absences {
                if is_anchor_gap(&absence) {
                    status_notes.push(absence);
                } else {
                    absences.push(absence);
                }
            }
            folder_subject_projects = folder_subjects.subject_projects;
            aikit_adapters::central_entities::adopt_into(
                &mut discovered.wiki,
                folder_subjects.objects,
            );

            // Projects from declarations, not env (Design B, A-4): the
            // manifests every Work folder already carries. A folder whose
            // manifest cannot be honoured is one named absence, never a
            // silent skip.
            for entry in discover_work_projects(central_root) {
                match entry {
                    aikit_adapters::work_repos::WorkProjectEntry::Project(project) => {
                        work_projects.push(project);
                    }
                    aikit_adapters::work_repos::WorkProjectEntry::Absence { name, reason } => {
                        project_absences.push(ProjectOwnedAbsence {
                            project: format!("Work/{name}"),
                            message: format!("Work/{name} {reason}"),
                        });
                    }
                }
            }
            // Per-project anchor state, loud in status (Design D): a missing
            // anchor degrades folder subjects, so it is named once per
            // project with the one command that fixes it.
            for project in &work_projects {
                let anchor_ref =
                    aikit_adapters::projectcentral_folder_subjects::project_root_anchor_ref(
                        &project.name,
                    );
                let anchored = aikit_adapters::ProjectCentralFilesystemBinding::inspect(
                    &project.root,
                    Some(central_root),
                )
                .ok()
                .and_then(|binding| binding.load_project_wiki().ok())
                .map(|objects| {
                    objects
                        .iter()
                        .any(|object| object.ref_id().as_str() == anchor_ref)
                });
                match anchored {
                    Some(true) => status_notes.push(format!(
                        "Work/{}: project-root anchor present",
                        project.name
                    )),
                    Some(false) => status_notes.push(format!(
                        "Work/{name}: project-root anchor absent — folder subjects are not compiled; run `aikit wiki root-anchor --project Work/{name}`",
                        name = project.name
                    )),
                    None => status_notes.push(format!(
                        "Work/{}: project wiki unreadable; anchor state unknown",
                        project.name
                    )),
                }
            }
            if work_projects.is_empty() {
                status_notes.push(
                    "Work projects: none — no Work/*/ProjectCentral/project.json manifests were found"
                        .into(),
                );
            } else {
                let names: Vec<&str> = work_projects.iter().map(|p| p.name.as_str()).collect();
                status_notes.push(format!(
                    "Projects: {} discovered from Work/*/ProjectCentral manifests: {}",
                    work_projects.len(),
                    names.join(", ")
                ));
            }
        }

        let central = if let Some(central_root) = central_root {
            let project = self.invocation_project_member(central_root, root);
            // A missing map degrades this lens, not independent Wiki/code
            // faculties. Its absence never activates a disposable substitute.
            match CentralFileMapProvider::connect(
                SystemRunner::new(),
                aikit_adapters::central_file_map::executable(),
                central_root,
                project.as_deref(),
            ) {
                Ok(provider) => {
                    note_central_map_freshness(central_root, &mut status_notes);
                    Some(provider)
                }
                Err(error) => {
                    absences.push(format!("Central file map unavailable: {error}"));
                    None
                }
            }
        } else {
            None
        };
        let now_field = if let Some(central_root) = central_root {
            if std::env::var_os("AIKIT_NOW_FIELD_SEARCH").is_some_and(|v| v == "off") {
                absences.push("NOW-field search disabled by AIKIT_NOW_FIELD_SEARCH=off".into());
                None
            } else {
                match NowFieldSourcePoolProvider::connect(
                    aikit_adapters::now_field::default_runner(central_root),
                    aikit_adapters::ripgrep::executable(),
                    NowFieldScope::standard(central_root),
                ) {
                    Ok(provider) => Some(provider),
                    Err(error) => {
                        absences.push(format!("NOW-field search unavailable: {error}"));
                        None
                    }
                }
            }
        } else {
            None
        };
        // The live Work-repos pool (Design A): one repo per discovered
        // project, searched at query time. It exists only where projects
        // were discovered; the NOW-field/native providers keep their priority.
        let work_repos = if work_projects.is_empty() {
            None
        } else {
            Some(WorkReposSourcePoolProvider::connect(
                SystemRunner::new().with_env_removed("RIPGREP_CONFIG_PATH"),
                aikit_adapters::ripgrep::executable(),
                work_projects.clone(),
            ))
        };
        let work_repo_scopes: BTreeMap<String, String> = work_projects
            .iter()
            .map(|project| {
                (
                    format!("source:project:{}:", project.project_id),
                    format!("Work/{}", project.name),
                )
            })
            .collect();
        let central_project_source_scopes: BTreeMap<String, String> = work_projects
            .iter()
            .map(|project| {
                (
                    format!("central:source:project:{}:", project.project_id),
                    format!("Work/{}", project.name),
                )
            })
            .collect();
        let code_source_scopes: BTreeMap<String, String> = work_projects
            .iter()
            .map(|project| {
                (
                    format!("source:project-code:{}", project.project_id),
                    format!("Work/{}", project.name),
                )
            })
            .collect();

        // The default read-only bkmr store pool (owner-corrected A-2): bkmr
        // is part of agent context sourcing by default, so the configured
        // stores — personal included — are searched read-only without any
        // opt-in capsule. Writes stay fenced in the provider; an inactive
        // pool costs one status note, never a per-query absence.
        let bkmr_stores = {
            let config_dir = bkmr_config_dir();
            let stores = discover_bkmr_stores(&config_dir);
            let provider = BkmrStoreSearchProvider::connect(SystemRunner::new(), "bkmr", stores);
            let status = provider.status();
            if status.available {
                status_notes.push(format!("bkmr store pool: {}", status.detail));
                Some(provider)
            } else {
                let reason = if provider.stores().is_empty() {
                    format!("no configured bkmr stores under {}", config_dir.display())
                } else {
                    status.detail
                };
                status_notes.push(format!("bkmr store pool inactive: {reason}"));
                None
            }
        };

        // Filesystem source shards are a standalone discovery mechanism. In a
        // Central World their copied bodies must not bypass the live source owner
        // (including a source withheld since an earlier cached corpus was written).
        // Central owns refs under its control-root source namespace. A Project's
        // own generated SourcePool shard remains the owner of a corpus-local ref:
        // dropping it would turn a cited source into a permanent unreadable
        // citation even when no Central source owns it.
        if central_root.is_some() {
            discovered
                .sources
                .retain(|source, _| !source.as_str().starts_with("central:source:control:root:"));
        }
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

        let ordinary = aikit_adapters::wiki_document::compile_material_sources(
            &material,
            &discovered.wiki,
            &mut absences,
        )?;
        aikit_adapters::central_entities::adopt_into(
            &mut discovered.wiki,
            ordinary.edges.into_iter().map(WikiObject::Edge).collect(),
        );
        let wiki = if discovered.wiki.is_empty() {
            absences.push("SemanticWiki material absent from the project horizon".into());
            None
        } else {
            let horizon = blake3::hash(root.to_string_lossy().as_bytes()).to_hex();
            let path = self
                .home
                .cache()
                .join("knowledge/wiki")
                .join(format!("{horizon}.sqlite3"));
            match SqliteWikiProvider::rebuild(&path, discovered.wiki, wiki_registers.clone()) {
                Ok(provider) => {
                    // The read index materialised past dangling references;
                    // every repair is disclosed as a named absence, one line
                    // per distinct fault. Strict write gates are untouched.
                    absences.extend(repair_absence_lines(provider.repairs()));
                    Some(provider)
                }
                Err(error) => {
                    absences.push(format!("SemanticWiki materialisation degraded: {error}"));
                    None
                }
            }
        };

        let mut bkmr = None;
        if central_root.is_none() {
            if let Some(config) = self.active_provider_config("tool/search/bkmr") {
                let db = config.get("db").and_then(|value| value.as_str());
                if let Some(db) = db {
                    if config.get("disposable").and_then(|v| v.as_bool()) != Some(true) {
                        return Err(aikit_core::AikitError::new("knowledge.bkmr_adoption_required", "standalone bkmr rebuild requires disposable=true; existing native databases must be adopted by Central"));
                    }
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
                            absences.push(format!("bkmr SourcePool degraded: {error}"));
                        }
                    } else {
                        absences.push(
                            "bkmr SourcePool configured but provider executable is unavailable"
                                .into(),
                        );
                    }
                    bkmr = Some(provider);
                } else {
                    absences
                        .push("bkmr is active but has no resolved `db` provider binding".into());
                }
            }
        }
        // Only descriptors join the map; payloads are fetched by the live
        // source owner at read/context/Flow time, not copied into this cache.
        if let Some(provider) = &central {
            material.extend(provider.descriptors().iter().cloned());
        }
        // GitNexus per discovered project (Design C): the structural layer is
        // capability-gated per project. Each provider joins the runtime even
        // when it cannot index, so the absence is per project — never a
        // global "provider absent"; unavailable projects share one grouped
        // line per distinct reason.
        //
        // Bounded and parallel (owner repair, W-knowledge-search-2026-09-24):
        // a live gate against the real Central ground found `knowledge
        // search`/`knowledge status` taking minutes — one large Work repo's
        // `gitnexus analyze --index-only` alone ran past five minutes
        // unbounded, and this loop ran one such call per discovered project,
        // strictly sequentially, on every single invocation (no index state
        // survives between CLI processes). Every GitNexus subprocess this
        // provider spawns — capability probe, index, search — now runs under
        // `gitnexus_budget()`, so one huge or hung repository is killed and
        // disclosed rather than stalling the query; indexing itself runs in
        // bounded parallel across projects so N repos cost roughly one
        // budget's wall time, not N of them summed.
        let code_budget = gitnexus_budget();
        let parallelism = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, MAX_GITNEXUS_PARALLELISM))
            .unwrap_or(2);
        let mut code = Vec::with_capacity(work_projects.len());
        let mut code_project_scopes = Vec::with_capacity(work_projects.len());
        let mut code_degradations: Vec<ProjectCodeDegradation> = Vec::new();
        // The binary is a seam: `AIKIT_GITNEXUS_BIN` (resolved through the
        // process environment at `Service::open`) overrides the PATH lookup,
        // so a test — and an operator — pins code intelligence to a known
        // binary instead of depending on whatever the host happens to have.
        let gitnexus_binary = self.gitnexus_binary.clone();
        // `gitnexus analyze` records each indexed repo in one global
        // `registry.json` by an unlocked read-modify-write, and `query --repo`
        // resolves the repo name through that registry. Two concurrent
        // analyses can each write back their own copy, silently dropping the
        // other repo's entry so its code never surfaces. Probes and adoption
        // stay parallel; the registry-writing index step runs one at a time.
        let index_lock = std::sync::Mutex::new(());
        let mut gitnexus_unavailable: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut project_sources = Vec::with_capacity(work_projects.len());
        for project in &work_projects {
            let source = SourceRef::parse(format!("source:project-code:{}", project.project_id))?;
            project_sources.push((project.clone(), source));
        }
        for batch in project_sources.chunks(parallelism) {
            let outcomes: Vec<(
                WorkRepoProject,
                GitNexusCodeIndexProvider<SystemRunner>,
                Option<aikit_core::AikitError>,
            )> = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|(project, source)| {
                        let project = project.clone();
                        let source = source.clone();
                        let binary = gitnexus_binary.clone();
                        let index_lock = &index_lock;
                        scope.spawn(move || {
                            let runner = SystemRunner::new()
                                .with_cwd(&project.root)
                                .with_timeout(code_budget);
                            let mut provider = match binary.as_deref() {
                                Some(binary) => GitNexusCodeIndexProvider::with_binary_memoised(
                                    runner,
                                    binary,
                                    project.project_id.clone(),
                                    source,
                                    None,
                                ),
                                None => GitNexusCodeIndexProvider::with_binary_memoised(
                                    runner,
                                    "gitnexus",
                                    project.project_id.clone(),
                                    source,
                                    None,
                                ),
                            };
                            let status = provider.status();
                            // A query reads the existing derived index; it
                            // never re-indexes one that exists (that made
                            // every `knowledge` call cost minutes). Only an
                            // unindexed project is indexed here; freshness
                            // of an existing index is disclosed, not
                            // re-checked (`aikit knowledge code index`).
                            let adopted = provider.adopt_existing_index(&project.root);
                            let index_error =
                                if status.available && status.capabilities.index && !adopted {
                                    let _registry = index_lock
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    provider.index(&project.root, false).err()
                                } else {
                                    None
                                };
                            (project, provider, index_error)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .expect("GitNexus index worker thread did not panic")
                    })
                    .collect()
            });
            for (project, provider, index_error) in outcomes {
                if let Some(error) = index_error {
                    // A timed-out call surfaces here exactly like any other
                    // index failure: `error` already names the budget it
                    // violated (`mux.command_timeout`), so the project is
                    // disclosed as degraded, never silently dropped.
                    // Per-project code state, scoped like `authored_pending`:
                    // never the global per-query absence that leaked another
                    // project's code degradation into a scoped reply.
                    code_degradations.push(ProjectCodeDegradation {
                        project: format!("Work/{}", project.name),
                        message: format!(
                            "GitNexus CodeIndex degraded for Work/{}: {error}",
                            project.name
                        ),
                    });
                }
                let status = provider.status();
                if !status.available {
                    let reason = provider
                        .unavailable_reason()
                        .unwrap_or_else(|| "GitNexus executable is unavailable".into());
                    gitnexus_unavailable
                        .entry(reason)
                        .or_default()
                        .push(format!("Work/{}", project.name));
                } else if !status.capabilities.index {
                    gitnexus_unavailable
                        .entry(format!(
                            "installed version {} does not expose the `analyze --index-only` surface this integration uses (tested {})",
                            status.version.as_deref().unwrap_or("unknown"),
                            aikit_core::knowledge_code::GITNEXUS_TESTED_VERSION
                        ))
                        .or_default()
                        .push(format!("Work/{}", project.name));
                }
                code.push(provider);
                code_project_scopes.push(format!("Work/{}", project.name));
            }
        }
        for (reason, projects) in gitnexus_unavailable {
            // A status note, not a per-query absence: capability state is the
            // same for every query, and a scoped reply must keep another
            // project's disclosures out (the discipline authored_pending and
            // the anchor lines already follow). Status names every project.
            status_notes.push(format!(
                "GitNexus unavailable for {}: {reason}",
                projects.join(", ")
            ));
        }

        let project_map =
            self.build_project_map(wiki.as_ref().map(SqliteWikiProvider::index), &material)?;

        let now_field_roster = now_field
            .as_ref()
            .map(NowFieldSourcePoolProvider::descriptors)
            .unwrap_or_default();
        let current_project = central_root.and_then(|central_root| {
            Some(format!(
                "Work/{}",
                self.invocation_project_member(central_root, root)?
            ))
        });
        Ok(KnowledgeRuntime {
            wiki,
            material,
            native_source,
            bkmr,
            bkmr_stores,
            central,
            now_field,
            now_field_roster,
            work_repos,
            work_repo_scopes,
            central_project_source_scopes,
            central_expected: central_root.is_some(),
            code_source_scopes,
            code_project_scopes,
            code,
            code_degradations,
            project_map,
            absences,
            status_notes,
            authored_pending,
            project_absences,
            authored_edge_projects,
            folder_subject_projects,
            current_project,
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
    discover_wiki: bool,
) -> Result<DiscoveredMaterial> {
    let mut discovered = DiscoveredMaterial::default();
    let mut seen_wiki_refs: BTreeSet<String> = BTreeSet::new();
    let mut conflicted_sources = BTreeSet::new();

    // Canonical authored ground is read directly, outside the walk (Design
    // E): the root wiki and every project wiki sit at known paths, and
    // authored ground never depends on walk order or on the candidate bound.
    // The walk below adds nothing these reads already hold.
    if discover_wiki {
        let mut canonical: Vec<PathBuf> = Vec::new();
        let root_wiki = root.join("Control/agents/wiki/wiki.json");
        if root_wiki.is_file() {
            canonical.push(root_wiki);
        }
        let work = root.join("Work");
        if work.is_dir() {
            if let Ok(entries) = fs::read_dir(&work) {
                for entry in entries.flatten() {
                    let wiki = entry.path().join("ProjectCentral/agents/wiki/wiki.json");
                    if wiki.is_file() {
                        canonical.push(wiki);
                    }
                }
            }
        }
        canonical.sort();
        canonical.dedup();
        for path in canonical {
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    absences.push(format!(
                        "Canonical wiki register {} could not be read: {error}",
                        path.display()
                    ));
                    continue;
                }
            };
            match parse_wiki_objects(&text) {
                Ok(objects) => {
                    for object in objects {
                        if seen_wiki_refs.insert(object.ref_id().as_str().to_owned()) {
                            discovered.wiki.push(object);
                        }
                    }
                }
                Err(error) => absences.push(format!(
                    "Canonical wiki register {} is invalid: {error}",
                    path.display()
                )),
            }
        }
    }

    let mut stack = vec![root.to_path_buf()];
    let mut files = 0usize;

    while let Some(dir) = stack.pop() {
        if dir == home || is_ignored_dir(&dir) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(error) => {
                absences.push(format!(
                    "Knowledge discovery could not read {}: {error}",
                    dir.display()
                ));
                continue;
            }
        };
        for (index, entry) in entries.iter().enumerate() {
            let path = entry.path();
            if path.is_dir() {
                if !is_ignored_dir(&path) && path != home {
                    stack.push(path);
                }
                continue;
            }
            if files >= MAX_DISCOVERY_FILES {
                // A bound that fires stays acceptable; one that hides what it
                // skipped is the defect (Design E): the absence names the
                // stopping directory and the approximate unexamined count.
                let skipped_here = entries[index..]
                    .iter()
                    .filter(|e| !e.path().is_dir())
                    .count();
                absences.push(format!(
                    "Knowledge discovery stopped after {MAX_DISCOVERY_FILES} candidate files in {}; \
                     ~{skipped_here} further files unexamined in that subtree; {} queued directories were never visited",
                    dir.display(),
                    stack.len()
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

            // SourcePool material is identified structurally, before the
            // Wiki's content sniff runs. A corpus source binding carries the
            // body of an authored file, and an authored file that happens to
            // discuss the wiki profile puts the literal `okf-wiki/v1` inside
            // that body — which used to make the sniff claim the shard as
            // malformed Wiki material and warn on every search. What parses
            // as SourceMaterial is SourceMaterial.
            let source_items = serde_json::from_str::<SourceMaterial>(&text)
                .map(|item| vec![item])
                .or_else(|_| serde_json::from_str::<Vec<SourceMaterial>>(&text));

            if source_items.is_err() && discover_wiki && text.contains("okf-wiki/v1") {
                match parse_wiki_objects(&text) {
                    Ok(objects) => {
                        for object in objects {
                            if seen_wiki_refs.insert(object.ref_id().as_str().to_owned()) {
                                discovered.wiki.push(object);
                            }
                        }
                    }
                    Err(collection_error) => match OkfWikiBundle::parse_json(&text) {
                        Ok(bundle) => {
                            if seen_wiki_refs.insert(bundle.wiki.ref_id().as_str().to_owned()) {
                                discovered.wiki.push(bundle.wiki);
                            }
                        }
                        Err(_) => absences.push(format!(
                            "self-identified SemanticWiki material at {} is invalid: {collection_error}",
                            path.display()
                        )),
                    },
                }
            }

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

/// The compiler's own anchor-gap disclosure, partitioned into status notes by
/// Design D so a missing anchor is loud in status, not per query.
fn is_anchor_gap(absence: &str) -> bool {
    absence.contains("project-root anchor") && absence.contains("no folder subjects compiled")
}

/// Status note naming how fresh Central's persistent file-map index is (the
/// bkmr-backed map only knows what its last refresh saw).
fn note_central_map_freshness(central_root: &Path, notes: &mut Vec<String>) {
    let index = central_root.join(".central/bkmr/index.db");
    let bindings = central_root.join(".central/bkmr/bindings.json");
    let Some(path) = [index, bindings]
        .into_iter()
        .find(|candidate| candidate.is_file())
    else {
        notes.push(
            "central-bkmr file map: no index found under .central/bkmr; refresh it through Central"
                .into(),
        );
        return;
    };
    let stamp = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| jiff::Timestamp::from_second(duration.as_secs() as i64).ok())
        .map(|timestamp| timestamp.to_string())
        .unwrap_or_else(|| "unknown time".into());
    notes.push(format!(
        "central-bkmr file map index last refreshed {stamp} ({})",
        path.display()
    ));
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

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn exact_knowledge_hit(hit: &aikit_core::KnowledgeSearchHit, subjects: &[&str]) -> bool {
    subjects.iter().any(|subject| {
        !subject.is_empty()
            && (hit.resource.as_str().eq_ignore_ascii_case(subject)
                || hit.label.eq_ignore_ascii_case(subject))
    })
}

/// Whether a Work-relative project display answers a scope key exactly or by
/// its bare Work name (`demo` answers `Work/demo`).
fn display_matches_key(display: &str, key: &str) -> bool {
    let key = key.trim().to_lowercase();
    if key.is_empty() {
        return false;
    }
    display.to_lowercase() == key
        || display
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.eq_ignore_ascii_case(&key))
}

/// The Work member a path sits in, from directory shape alone:
/// `<central_root>/Work/<member>/…` names `<member>`; anything else — the
/// world root itself, a Control location, a path outside the Central root —
/// names nothing. No marker, registration or manifest timing participates:
/// the directory shape is the scoping basis, so a project is scoped by where
/// it stands, not by what it has been registered as.
fn work_member(central_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(central_root).ok()?;
    let mut parts = relative.components();
    if parts.next()?.as_os_str() != "Work" {
        return None;
    }
    parts.next()?.as_os_str().to_str().map(str::to_owned)
}
