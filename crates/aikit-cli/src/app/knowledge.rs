use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_adapters::bkmr::{
    bkmr_config_dir, discover_bkmr_stores, BkmrSourcePoolProvider, BkmrStoreSearchProvider,
};
use aikit_adapters::central_file_map::CentralFileMapProvider;
use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::now_field::{NowFieldScope, NowFieldSourcePoolProvider};
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::work_repos::{
    decode_work_file_source_ref, discover_native_work_projects,
    NativeWorkProjectEntry, NativeWorkRepoProject, WorkRepoProject, WorkReposSourcePoolProvider,
};
use aikit_core::knowledge::{KnowledgeContextPack, KnowledgeRelationView, KnowledgeRoute};
use aikit_core::knowledge_code::CodeIndexProvider;
use aikit_core::knowledge_navigation::ProjectAuthoredPending;
use aikit_core::knowledge_source_pool::{
    material_for_actor, NativeSourcePoolProvider, SourceMaterial, SourcePool, SourcePoolProvider,
};
use aikit_core::knowledge_wiki::{
    parse_wiki_objects, OkfWikiBundle, WikiObject, PROJECT_WIKI_SPACE_REF_PREFIX,
};
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
    KnowledgeApplicationStore, KnowledgeCoverageStore, SqliteWikiProvider,
};
use aikit_tui::backend::PaletteBackend;

use super::Service;

const MAX_DISCOVERY_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DISCOVERY_FILES: usize = 4096;
// Native source bytes remain limited independently. JSON escaping can expand
// a 16 MiB source to 96 MiB before its envelope; transport is explicit.
const NATIVE_SOURCE_TRANSPORT_BYTES: u64 = 128 * 1024 * 1024;
const NATIVE_SOURCE_INVOCATION_SECONDS: u64 = 60;

fn native_source_runner() -> SystemRunner {
    SystemRunner::new()
        .with_timeout(std::time::Duration::from_secs(NATIVE_SOURCE_INVOCATION_SECONDS))
        .with_output_limit_bytes(NATIVE_SOURCE_TRANSPORT_BYTES)
}

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

#[cfg(test)]
mod project_scope_tests {
    use super::ref_belongs_to_project_scope;
    use std::collections::BTreeMap;

    fn belongs(resource: &str, display: &str) -> bool {
        let work = BTreeMap::new();
        let central = BTreeMap::new();
        ref_belongs_to_project_scope(resource, display, &work, &BTreeMap::new(), &central)
    }

    #[test]
    fn work_scope_uses_decoded_literal_id_and_native_roots_use_exact_binding_keys() {
        use aikit_adapters::work_repos::work_file_source_ref;
        use std::path::Path;
        let work = BTreeMap::from([("a:b".into(),"Work/left".into()),("a".into(),"Work/right".into())]);
        let roots = BTreeMap::from([("source:project:a:b:root".into(),"Work/left".into())]);
        let central = BTreeMap::new();
        let left = work_file_source_ref("a:b",Path::new("c.md")).unwrap();
        let right = work_file_source_ref("a",Path::new("b:c.md")).unwrap();
        assert!(ref_belongs_to_project_scope(left.as_str(),"Work/left",&work,&roots,&central));
        assert!(!ref_belongs_to_project_scope(left.as_str(),"Work/right",&work,&roots,&central));
        assert!(ref_belongs_to_project_scope(right.as_str(),"Work/right",&work,&roots,&central));
        assert!(!ref_belongs_to_project_scope("source:project:a:b:c.md","Work/right",&work,&roots,&central));
        assert!(ref_belongs_to_project_scope("source:project:a:b:root","Work/left",&work,&roots,&central));
        assert!(!ref_belongs_to_project_scope("source:project:a:root","Work/left",&work,&roots,&central));
    }

    #[test]
    fn a_clearing_drops_whole_from_a_project_scope() {
        // The packet A incident shape: a clearing's evidence tree holding a
        // sibling-Project state copy must not re-enter through the graph.
        assert!(!belongs(
            "central:source:control:root:Control/agents/now/clearings/a5cbbe83/T/evidence/disposable-held-decision/Central/Work/Factory/.factory/development-state.json",
            "Work/O-I",
        ));
        assert!(!belongs(
            "central:source:control:root:Control/agents/now/clearings/a5cbbe83/T/acceptance.json",
            "Work/O-I",
        ));
    }

    #[test]
    fn a_sibling_wiki_space_names_the_sibling_and_the_root_space_passes() {
        // Explicit declared identity is an algorithm input here. The separate
        // native gate obtains these mappings from actual ctrl-created Projects.
        let central = BTreeMap::from([
            ("central:source:project:editor-walk:".into(), "Work/Editor".into()),
            ("central:source:project:editor:".into(), "Work/Other".into()),
            ("central:source:project:team/editor:".into(), "Work/Team".into()),
        ]);
        let bound = |resource, display| ref_belongs_to_project_scope(
            resource, display, &BTreeMap::new(), &BTreeMap::new(), &central,
        );
        assert!(bound("central:wiki:project:editor-walk", "Work/Editor"));
        assert!(!bound("central:wiki:project:editor-walk", "Work/Other"));
        assert!(!bound("central:wiki:project:editor", "Work/Editor"));
        assert!(bound("central:wiki:project:editor", "Work/Other"));
        assert!(bound("central:wiki:project:team/editor", "Work/Team"));
        assert!(!bound("central:wiki:project:team/editor", "Work/Editor"));
        assert!(!bound("central:wiki:project:EDITOR-WALK", "Work/Editor"));
        assert!(!bound("central:wiki:project:", "Work/Editor"));
        assert!(!bound("central:wiki:project:unknown/path", "Work/Editor"));
        // No declaration means no Project attribution, even when ID=folder.
        assert!(!belongs("central:wiki:project:Factory", "Work/O-I"));
        assert!(!belongs("central:wiki:project:O-I", "Work/O-I"));
        assert!(belongs("central:wiki:root", "Work/O-I"));
    }

    #[test]
    fn a_raw_path_under_another_work_tree_names_the_sibling_by_layout() {
        assert!(!belongs(
            "/Users/admin/Central/Work/Factory/ProjectCentral/user/capability-matrix.csv",
            "Work/O-I",
        ));
        // Own-tree raw paths stay inside their Project's scope.
        assert!(belongs(
            "/Users/admin/Central/Work/O-I/docs/overview.md",
            "Work/O-I",
        ));
        // Paths outside any Work tree pass: the root lineage is broader.
        assert!(belongs("/Users/admin/notes/overview.md", "Work/O-I"));
        // A segment named `Workfile` is not a Work segment.
        assert!(belongs("/Users/admin/Workfile/x.md", "Work/O-I"));
    }

    #[test]
    fn canonical_work_refs_keep_their_owner_rule() {
        assert!(belongs(
            "central:source:control:root:Work/O-I/docs/overview.md",
            "Work/O-I",
        ));
        assert!(!belongs(
            "central:source:control:root:Work/Factory/ProjectCentral/user/capability-matrix.csv",
            "Work/O-I",
        ));
    }
}

/// True when an attribution map names `resource` as owned by a Project
/// other than `display` — the check that keeps another Project's compiled
/// objects (authored edges, folder basis, capability matrices) out of a
/// scoped reply or graph. Unattributed refs pass: the root lineage is a
/// distinct, legitimately broader aperture.
fn attributed_to_other_project(
    attribution: &BTreeMap<String, String>,
    resource: &str,
    display: &str,
) -> bool {
    attribution
        .get(resource)
        .is_some_and(|project| project != display)
}

/// Whether `resource` may enter a reply scoped to `display` (`Work/<name>`).
/// Canonical Source refs retain Project ownership regardless of whether the
/// caller reaches them through SourcePool or ProjectMap; clearings drop
/// whole; Wiki spaces require unique native identity attribution, while
/// sibling raw work-tree paths name the sibling by layout. The root lineage
/// keeps every shape: this predicate is
/// consulted only when a reply HAS a Project scope.
fn ref_belongs_to_project_scope(
    resource: &str,
    display: &str,
    work_repo_scopes: &BTreeMap<String, String>,
    native_root_source_scopes: &BTreeMap<String, String>,
    central_project_source_scopes: &BTreeMap<String, String>,
) -> bool {
    if resource.starts_with("source:work-file:v1:") {
        return SourceRef::parse(resource)
            .and_then(|source| decode_work_file_source_ref(&source))
            .ok().flatten()
            .is_some_and(|address| work_repo_scopes.get(&address.project_id).is_some_and(|project| project == display));
    }
    if resource.starts_with("source:project:") {
        // Only an actual native binding attributes a Project-root Source.
        // Old Work-file addresses have no recoverable issuing tuple.
        return native_root_source_scopes.get(resource).is_some_and(|project| project == display);
    }
    if resource.starts_with("central:source:project:") {
        return central_project_source_scopes
            .iter()
            .any(|(prefix, project)| resource.starts_with(prefix.as_str()) && project == display);
    }
    if let Some(rest) = resource.strip_prefix("central:source:control:root:Work/") {
        return rest
            .split_once('/')
            .is_some_and(|(project, _)| format!("Work/{project}") == display);
    }
    // Project NOW-field law (packet A): a clearing drops whole from a
    // Project-scoped reply — it is a working horizon, never a common record.
    // The graph and every other consumer of this predicate obey the scope
    // the search repair established, so a clearing's evidence tree —
    // including sibling-Project copies it holds — cannot re-enter a
    // ProjectWorld through the graph door.
    if resource.starts_with("central:source:control:root:Control/agents/now/clearings/") {
        return false;
    }
    // A Wiki space retains its native Project ID, which need not spell its
    // Work folder. Reuse the owner's exact, unique ID-to-display attribution.
    // Unknown or ambiguous IDs cannot acquire scope from a matching label.
    // The root composition space (`central:wiki:root`) still passes.
    if let Some(project_id) = resource.strip_prefix(PROJECT_WIKI_SPACE_REF_PREFIX) {
        return central_project_source_scopes
            .get(&format!("central:source:project:{project_id}:"))
            .is_some_and(|project| project == display);
    }
    // An unattributed raw filesystem path under another Project's work tree
    // names the sibling by machine layout; a ProjectWorld reply discloses
    // owned refs. Paths outside any `Work/<Project>/` segment pass.
    if resource.starts_with('/') {
        let own = display.strip_prefix("Work/");
        let mut segments = resource.split('/');
        while let Some(segment) = segments.next() {
            if segment == "Work" {
                if let Some(project) = segments.next() {
                    if own.is_none_or(|name| !project.eq_ignore_ascii_case(name)) {
                        return false;
                    }
                }
            }
        }
    }
    true
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
    central: Option<Arc<CentralFileMapProvider<SystemRunner>>>,
    now_field: Option<NowFieldSourcePoolProvider<SystemRunner>>,
    now_field_roster: Vec<SourceMaterial>,
    /// The live Work-repos pool, one repo per discovered project. Registered
    /// only when projects were discovered, so a non-Central context keeps its
    /// old shape.
    work_repos: Option<WorkReposSourcePoolProvider<SystemRunner>>,
    /// Exact opaque Project ID → display for the owner's decoded Work address.
    work_repo_scopes: BTreeMap<String, String>,
    /// Exact native Project-root Source → display from actual inspected bindings.
    native_root_source_scopes: BTreeMap<String, String>,
    /// `central:source:project:<project_id>:` prefix → `Work/<name>` for
    /// canonical owner refs returned by Central's project file map.
    central_project_source_scopes: BTreeMap<String, String>,
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
    /// Compiled capability-matrix object ref — or its cited carrier path —
    /// → Work-relative project display, for scoped queries and graphs to
    /// keep another project's matrix material out of their results. The
    /// root composition stays unattributed: the root lineage is a distinct,
    /// legitimately broader aperture.
    matrix_object_projects: BTreeMap<String, String>,
    /// This invocation's own project in Work-relative display (`Work/demo`),
    /// when the invocation root sits in a Central Work project.
    current_project: Option<String>,
}

impl KnowledgeRuntime {
    /// Canonical Source refs retain Project ownership regardless of whether
    /// the caller reaches them through SourcePool or ProjectMap.
    fn source_belongs_to_scope(&self, resource: &str, display: &str) -> bool {
        ref_belongs_to_project_scope(
            resource,
            display,
            &self.work_repo_scopes,
            &self.native_root_source_scopes,
            &self.central_project_source_scopes,
        )
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
        self.central
            .as_ref()
            .map(|p| p.as_ref() as &dyn SourcePoolProvider)
    }
    fn application(&self, context: FamiliarityContext) -> KnowledgeApplication<'_> {
        self.application_with_project_scope(
            context,
            None,
            self.now_field.as_ref(),
            self.work_repos.as_ref().map(|provider| provider as &dyn SourcePoolProvider),
        )
    }

    fn application_with_project_scope<'a>(
        &'a self,
        context: FamiliarityContext,
        scoped_project: Option<&'a str>,
        now_field: Option<&'a NowFieldSourcePoolProvider<SystemRunner>>,
        work_repos: Option<&'a dyn SourcePoolProvider>,
    ) -> KnowledgeApplication<'a> {
        let mut application =
            KnowledgeApplication::new(context).with_project_map(&self.project_map);
        if let Some(scope) = scoped_project {
            // Scoped replies consult the runtime's attribution at the
            // source: sibling-owned authored citations produce neither hits
            // nor unreadable-source absences (knowledge_navigation).
            application = application.with_project_attribution(
                scope,
                &self.authored_edge_projects,
                &self.folder_subject_projects,
                &self.matrix_object_projects,
            );
        }
        if let Some(provider) = &self.wiki {
            application = application.with_wiki(provider);
        }
        if let Some(provider) = &self.central {
            application = application.with_source_pool(provider.as_ref(), provider.descriptors());
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
            // refs use the separate injective Work-file transport language.
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

    fn knowledge_coverage_store(&self) -> KnowledgeCoverageStore {
        KnowledgeCoverageStore::new(self.home.clone())
    }

    /// Read an exact selected span. The span ride is never cached: an exact
    /// selection is an exact request, and it records the extent it actually
    /// delivered as observed coverage.
    pub fn knowledge_read_span(
        &mut self,
        address: &KnowledgeAddress,
        selector: &aikit_core::knowledge_facets::SourceSelector,
    ) -> Result<aikit_core::KnowledgeReading> {
        let reading =
            self.with_knowledge(|_, application| application.read_selected(address, selector))?;
        self.record_reading_coverage(&reading);
        Ok(reading)
    }

    /// Every full source read observes its whole extent — checksums prove
    /// retention, only reads prove consideration.
    fn record_reading_coverage(&self, reading: &aikit_core::KnowledgeReading) {
        let Some(revision) = reading.revision.clone() else {
            return;
        };
        let total = reading
            .content
            .as_ref()
            .map(|body| body.chars().count() as u64);
        if let Some((start, end)) = match (&reading.span, total) {
            (Some(span), _) => Some((span.start, span.end)),
            (None, Some(total)) => Some((0, total)),
            (None, None) => None,
        } {
            let _ = self.knowledge_coverage_store().record_observed(
                reading.resource.as_str(),
                &revision,
                vec![aikit_store::knowledge_coverage::CoverageExtent { start, end }],
            );
        }
    }

    /// Declare a semantic reading: a named actor states it considered the
    /// named extents of a source at its exact revision.
    #[allow(clippy::too_many_arguments)]
    pub fn knowledge_coverage_declare(
        &mut self,
        source_ref: &str,
        content_revision: &str,
        extents: Vec<(u64, u64)>,
        actor: String,
        note: Option<String>,
    ) -> Result<aikit_store::knowledge_coverage::CoverageRow> {
        self.knowledge_coverage_store().declare_reading(
            source_ref,
            content_revision,
            extents
                .into_iter()
                .map(|(start, end)| aikit_store::knowledge_coverage::CoverageExtent { start, end })
                .collect(),
            Some(actor),
            note,
        )
    }

    pub fn knowledge_coverage_mark_unreadable(
        &mut self,
        source_ref: &str,
        content_revision: &str,
        note: Option<String>,
    ) -> Result<aikit_store::knowledge_coverage::CoverageRow> {
        self.knowledge_coverage_store()
            .mark_unreadable(source_ref, content_revision, note)
    }

    pub fn knowledge_coverage_show(
        &mut self,
        wanted: &[(String, String)],
    ) -> Result<Vec<aikit_store::knowledge_coverage::CoverageReading>> {
        self.knowledge_coverage_store().readings(wanted)
    }

    pub(super) fn with_knowledge<T>(
        &self,
        operation: impl FnOnce(&KnowledgeRuntime, KnowledgeApplication<'_>) -> Result<T>,
    ) -> Result<T> {
        self.ensure_knowledge_project_scope()?;
        // An operation observes current owner inputs. A later operation,
        // including a nested Flow read, must not inherit a held material
        // snapshot or borrow another operation's runtime.
        let runtime = self.materialize_knowledge_runtime()?;
        let application = runtime.application(self.knowledge_context());
        operation(&runtime, application)
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

    /// The Jev selection join over an oversized pull (`tool/search/jev-rerank`).
    /// A no-op unless the capability is configured and the pull carries more
    /// candidates than the surface will show; every outcome is disclosed in
    /// the result's absences.
    pub fn jev_rerank_result(&self, result: &mut KnowledgeSearchResult, limit: usize) {
        if result.hits.len() <= limit {
            return;
        }
        let table = self
            .active_provider_config("tool/search/jev-rerank")
            .cloned();
        crate::knowledge_jev::maybe_rerank(table.as_ref(), result, limit);
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
        // Query the current native horizon before ranking or remembering
        // destinations; an old answer cannot grant source admission.
        let mut result = self.with_knowledge(|runtime, _application| {
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
            let scoped_now_field = match (
                scoped_display.as_deref(),
                runtime.now_field.as_ref(),
                runtime.central.as_ref(),
            ) {
                (Some(_), Some(provider), Some(owner)) => {
                    let scope = provider
                        .scope()
                        .for_project(discovered_project.map(|p| p.name.as_str()));
                    match NowFieldSourcePoolProvider::connect(
                        aikit_adapters::now_field::default_runner(&scope.central_root),
                        aikit_adapters::ripgrep::executable(),
                        scope,
                    ) {
                        Ok(provider) => Some(provider.with_native_owner(Arc::clone(owner))),
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
            let scoped_work_repos = discovered_project.and_then(|project| {
                let owner = runtime.work_repos.as_ref()?;
                match owner.for_project(&project.project_id) {
                    Ok(provider) => Some(provider),
                    Err(error) => {
                        scoped_provider_absences.push(format!("Work source scope unavailable: {error}"));
                        None
                    }
                }
            });
            let work_repos: Option<&dyn SourcePoolProvider> = if scoped_display.is_some() {
                scoped_work_repos.as_ref().map(|provider| provider as &dyn SourcePoolProvider)
            } else {
                runtime.work_repos.as_ref().map(|provider| provider as &dyn SourcePoolProvider)
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
            // their exact project in the owner-decoded address, so
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
                    ) || attributed_to_other_project(
                        &runtime.matrix_object_projects,
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
        })?;
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
        self.knowledge_central_root.as_deref()
            .or(self.central_meta_root.as_deref()).or_else(|| {
            root.ancestors().find(|candidate| {
                candidate.join("Control").is_dir() && candidate.join("Work").is_dir()
            })
        })
    }

    fn current_knowledge_central_root<'a>(&'a self, root: &'a Path) -> Result<Option<&'a Path>> {
        let candidate = self.knowledge_central_root(root);
        if let Some(candidate) = candidate {
            let canonical = std::fs::canonicalize(candidate).map_err(|error| {
                aikit_core::AikitError::new("central.root_context_unavailable",
                    format!("{} is unavailable: {error}", candidate.display()))
                    .with("path", candidate.display().to_string())
                    .with("observation_stage", "owner_root").with_io_source(error)
            })?;
            // Existing native binding owns this form check. It does not turn
            // a label or physical directory into a new semantic identity.
            super::root_context::binding(&canonical)?;
        }
        Ok(candidate)
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
        let binding = NativeWorkRepoProject::inspect(project_root, "external checkout", None)
            .map_err(|error| aikit_core::AikitError::new(
                "knowledge.project_scope_unresolved", format!("cannot resolve ProjectCentral identity at {}: {error}",project_root.display()))
                .with("native_code",error.code()).with_io_source_from(&error))?;
        let project_id = &binding.project().project_id;
        let mut matches = Vec::new();
        for entry in discover_native_work_projects(&central_root)? {
            match entry {
                NativeWorkProjectEntry::Project(project) if project.project().project_id == *project_id => {
                    matches.push(project.project().name.clone());
                }
                NativeWorkProjectEntry::Absence { name,error } => {
                    // Unknown native declarations cannot be erased while proving
                    // unique attribution of an externally selected checkout.
                    return Err(aikit_core::AikitError::new("knowledge.project_scope_unresolved",
                        format!("Work/{name} native identity unavailable: {error}"))
                        .with("native_code",error.code()).with_io_source_from(&error));
                }
                _ => {},
            }
        }
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
        if let Some(central_root) = self.current_knowledge_central_root(&self.invocation_cwd)? {
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
        let reading = self.with_knowledge(|_, application| application.read(address))?;
        self.record_reading_coverage(&reading);
        Ok(reading)
    }

    /// Additive native document facet; plain KnowledgeReading users keep their API.
    pub fn knowledge_read_document(&self, address: &KnowledgeAddress) -> Result<serde_json::Value> {
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
        self.with_knowledge(|runtime, _| {
            let mut objects: Vec<_> = runtime
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
                objects.retain(|object| {
                    let reference = object.ref_id().as_str();
                    // The same scope predicate the material obeys: another
                    // Project's wiki space (or any ref naming a sibling)
                    // stays out of a ProjectWorld's graph reply.
                    runtime.source_belongs_to_scope(reference, &display)
                        && !attributed_to_other_project(
                            &runtime.authored_edge_projects,
                            reference,
                            &display,
                        )
                        && !attributed_to_other_project(
                            &runtime.folder_subject_projects,
                            reference,
                            &display,
                        )
                        && !attributed_to_other_project(
                            &runtime.matrix_object_projects,
                            reference,
                            &display,
                        )
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
    }

    pub fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
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
        status.notes.push(
            "Knowledge operations read current native inputs; application result-cache shortcuts are retired"
                .into(),
        );
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
        let mut native_work_projects: Vec<NativeWorkRepoProject> = Vec::new();
        let mut native_root_source_scopes = BTreeMap::new();
        let mut wiki_registers = Vec::new();
        let mut authored_pending = Vec::new();
        let mut project_absences = Vec::new();
        let mut authored_edge_projects = BTreeMap::new();
        let mut folder_subject_projects = BTreeMap::new();
        let mut matrix_object_projects = BTreeMap::new();
        let central_root = self.current_knowledge_central_root(root)?;
        let mut discovered = discover_material(
            root,
            self.home.root(),
            &mut absences,
            central_root.is_none(),
            central_root,
        )?;
        if let Some(central_root) = central_root {
            let executable = std::env::var_os("CENTRAL_CTRL_BIN")
                .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("ctrl"));
            match aikit_adapters::central_wiki::read_central_wiki(
                &native_source_runner(),
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
            matrix_object_projects = matrices.object_projects;
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
                // The owner distinguishes a present native Project facet,
                // genuine manifest-less membership and current unavailability.
                // Directory names and raw manifest reads do not grant lineage.
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
            match discover_native_work_projects(central_root) {
                Ok(entries) => for entry in entries {
                    match entry {
                        NativeWorkProjectEntry::Project(native) => {
                            work_projects.push(native.project().clone());
                            native_work_projects.push(native);
                        }
                        NativeWorkProjectEntry::Absence { name, error } => {
                            project_absences.push(ProjectOwnedAbsence {
                                project: format!("Work/{name}"),
                                message: format!("Work/{name} {error}"),
                            });
                        }
                    }
                },
                Err(error) => absences.push(format!("Native Work discovery unavailable: {error}")),
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
                .and_then(|binding| {
                    native_root_source_scopes.insert(
                        binding.semantic.native_project_root.as_str().to_owned(),
                        format!("Work/{}", project.name),
                    );
                    binding.load_project_wiki().ok()
                })
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
                native_source_runner(),
                aikit_adapters::central_file_map::executable(),
                central_root,
                project.as_deref(),
            ) {
                Ok(provider) => {
                    note_central_map_freshness(central_root, &mut status_notes);
                    Some(Arc::new(provider))
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
            } else if let Some(owner) = central.as_ref() {
                match NowFieldSourcePoolProvider::connect(
                    aikit_adapters::now_field::default_runner(central_root),
                    aikit_adapters::ripgrep::executable(),
                    NowFieldScope::standard(central_root),
                ) {
                    Ok(provider) => Some(provider.with_native_owner(Arc::clone(owner))),
                    Err(error) => {
                        absences.push(format!("NOW-field search unavailable: {error}"));
                        None
                    }
                }
            } else {
                // A configured native owner failed to attach. Independent
                // Control-record mode cannot stand in for that known owner.
                absences.push(
                    "NOW-field search unavailable: native Central file map is unavailable".into(),
                );
                None
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
            match WorkReposSourcePoolProvider::connect_native(
                native_source_runner().with_env_removed("RIPGREP_CONFIG_PATH"),
                aikit_adapters::ripgrep::executable(), native_work_projects,
            ) {
                Ok(provider) => Some(provider),
                Err(error) => {
                    absences.push(format!("Native Work source attachment unavailable: {error}"));
                    None
                }
            }
        };
        let work_repo_scopes: BTreeMap<String, String> = work_projects
            .iter()
            .map(|project| {
                (
                    project.project_id.clone(),
                    format!("Work/{}", project.name),
                )
            })
            .collect();
        let mut central_project_source_scopes = BTreeMap::new();
        let mut ambiguous_project_source_scopes = BTreeSet::new();
        for project in &work_projects {
            let prefix = format!("central:source:project:{}:", project.project_id);
            if ambiguous_project_source_scopes.contains(&prefix) {
                continue;
            }
            if central_project_source_scopes
                .insert(prefix.clone(), format!("Work/{}", project.name))
                .is_some()
            {
                // A shared ID cannot select the first or last Work folder.
                // External invocation already refuses this ambiguity; an
                // internal scoped reply must withhold the same ambiguous ref.
                central_project_source_scopes.remove(&prefix);
                ambiguous_project_source_scopes.insert(prefix);
            }
        }
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
        // disclosed rather than stalling the query. Reads no longer index at
        // all; per-project admission still runs in bounded parallel.
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
                            // A Knowledge read admits the owner index that
                            // already exists and queries a private copy of
                            // it; it never builds, rebuilds or registers one
                            // (`aikit knowledge code index` does). A Project
                            // without an admissible index — no index yet, or
                            // not a Git repository — is disclosed as its own
                            // degradation instead of being indexed here.
                            let admission = provider.open_existing(&project.root).err();
                            let index_error = if status.available { admission } else { None };
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
                    // An index that cannot be admitted — absent, incomplete,
                    // or without a recorded commit — is disclosed as this
                    // project's degradation, never silently dropped.
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
            native_root_source_scopes,
            central_project_source_scopes,
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
            matrix_object_projects,
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

// Per-operation physical continuity only. The selected root is supplied by
// the existing runtime, not inferred as a native World/Project or audience.
struct DiscoveryRootObservation {
    requested: PathBuf,
    canonical: PathBuf,
    identity: (u64, u64),
}

fn discovery_io(error: std::io::Error) -> aikit_core::AikitError {
    aikit_core::AikitError::new("knowledge.discovery_unavailable",
        "Current discovery physical observation is unavailable")
        .with_io_source(error)
}

fn discovery_refusal() -> aikit_core::AikitError {
    aikit_core::AikitError::new("knowledge.discovery_withheld",
        "Current native discovery admission or physical affiliation was refused")
}

fn discovery_directory_identity(path: &Path) -> Result<(u64, u64)> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).map_err(discovery_io)?;
        if !metadata.is_dir() { return Err(discovery_refusal()); }
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(aikit_core::AikitError::new("knowledge.discovery_observation_unsupported",
            "Native bounded physical discovery is unavailable on this platform"))
    }
}

impl DiscoveryRootObservation {
    fn capture(path: &Path) -> Result<Self> {
        let observation = Self {
            requested: if path.is_absolute() { path.to_path_buf() } else {
                std::env::current_dir().map_err(discovery_io)?.join(path)
            },
            canonical: fs::canonicalize(path).map_err(discovery_io)?,
            identity: discovery_directory_identity(path)?,
        };
        observation.check()?;
        Ok(observation)
    }

    fn declared_member(&self, path: &Path) -> Result<Option<PathBuf>> {
        // Match the actual World ancestor while retaining the caller's
        // lexical route, including aliases and invocation parent components.
        let mut observed_member = None;
        for ancestor in path.ancestors() {
            if fs::canonicalize(ancestor).map_err(discovery_io)? == self.canonical {
                // An in-World alias back to its root cannot erase earlier
                // selected lexical ancestors; retain the outermost boundary.
                observed_member = Some(path.strip_prefix(ancestor)
                    .expect("an observed path ancestor is a lexical prefix").to_path_buf());
            }
        }
        Ok(observed_member)
    }

    fn check(&self) -> Result<()> {
        if fs::canonicalize(&self.requested).map_err(discovery_io)? != self.canonical
            || discovery_directory_identity(&self.requested)? != self.identity
        { return Err(discovery_refusal()); }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct DiscoveryLimits {
    entries: usize,
    queued: usize,
    seen: usize,
    path_bytes: usize,
    single_path_bytes: usize,
    candidates: usize,
    canonical_entries: usize,
    canonical_paths: usize,
    canonical_path_bytes: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            entries: 65_536,
            queued: 4096,
            seen: 8192,
            path_bytes: 4 * 1024 * 1024,
            single_path_bytes: 16 * 1024,
            candidates: MAX_DISCOVERY_FILES,
            canonical_entries: 65_536,
            canonical_paths: 4096,
            canonical_path_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DiscoveryCapacity {
    dimension: &'static str,
    limit: usize,
}

#[derive(Debug)]
enum DirectoryVisit {
    Complete,
    Stopped(DiscoveryCapacity),
}

enum DirectoryControl {
    Continue,
    Stop(DiscoveryCapacity),
}

struct DirectoryBudget {
    observed: usize,
    limit: usize,
}

impl DirectoryBudget {
    fn new(limit: usize) -> Self { Self { observed: 0, limit } }
}

struct DiscoveryFrontier {
    limits: DiscoveryLimits,
    queued: Vec<PathBuf>,
    seen: BTreeSet<PathBuf>,
    queued_bytes: usize,
    seen_bytes: usize,
    max_queued: usize,
    max_seen: usize,
    max_path_bytes: usize,
}

impl DiscoveryFrontier {
    fn new(limits: DiscoveryLimits) -> Self {
        Self { limits, queued: Vec::new(), seen: BTreeSet::new(), queued_bytes: 0,
            seen_bytes: 0, max_queued: 0, max_seen: 0, max_path_bytes: 0 }
    }

    fn path_charge(&self, path: &PathBuf) -> std::result::Result<usize, DiscoveryCapacity> {
        let charge = path.capacity();
        if charge > self.limits.single_path_bytes {
            return Err(DiscoveryCapacity { dimension: "single_path_capacity_bytes",
                limit: self.limits.single_path_bytes });
        }
        if self.queued_bytes.checked_add(self.seen_bytes)
            .and_then(|bytes| bytes.checked_add(charge))
            .is_none_or(|bytes| bytes > self.limits.path_bytes)
        {
            return Err(DiscoveryCapacity { dimension: "retained_path_capacity_bytes",
                limit: self.limits.path_bytes });
        }
        Ok(charge)
    }

    fn enqueue(&mut self, path: PathBuf) -> std::result::Result<(), DiscoveryCapacity> {
        if self.queued.len() >= self.limits.queued {
            return Err(DiscoveryCapacity { dimension: "queued_directories", limit: self.limits.queued });
        }
        let charge = self.path_charge(&path)?;
        self.queued.push(path);
        self.queued_bytes += charge;
        self.record_maxima();
        Ok(())
    }

    fn pop(&mut self) -> Option<PathBuf> {
        let path = self.queued.pop()?;
        self.queued_bytes -= path.capacity();
        Some(path)
    }

    fn admit_seen(&mut self, path: PathBuf) -> std::result::Result<bool, DiscoveryCapacity> {
        if self.seen.contains(&path) { return Ok(false); }
        if self.seen.len() >= self.limits.seen {
            return Err(DiscoveryCapacity { dimension: "seen_canonical_directories", limit: self.limits.seen });
        }
        let charge = self.path_charge(&path)?;
        self.seen.insert(path);
        self.seen_bytes += charge;
        self.record_maxima();
        Ok(true)
    }

    fn record_maxima(&mut self) {
        self.max_queued = self.max_queued.max(self.queued.len());
        self.max_seen = self.max_seen.max(self.seen.len());
        self.max_path_bytes = self.max_path_bytes.max(self.queued_bytes + self.seen_bytes);
    }
}

struct CanonicalPaths {
    paths: Vec<PathBuf>,
    bytes: usize,
    limits: DiscoveryLimits,
}

impl CanonicalPaths {
    fn new(limits: DiscoveryLimits) -> Self { Self { paths: Vec::new(), bytes: 0, limits } }

    fn push(&mut self, path: PathBuf) -> std::result::Result<(), DiscoveryCapacity> {
        if self.paths.len() >= self.limits.canonical_paths {
            return Err(DiscoveryCapacity { dimension: "canonical_register_paths", limit: self.limits.canonical_paths });
        }
        let charge = path.capacity();
        if charge > self.limits.single_path_bytes {
            return Err(DiscoveryCapacity { dimension: "single_path_capacity_bytes", limit: self.limits.single_path_bytes });
        }
        if self.bytes.checked_add(charge).is_none_or(|bytes| bytes > self.limits.canonical_path_bytes) {
            return Err(DiscoveryCapacity { dimension: "canonical_path_capacity_bytes", limit: self.limits.canonical_path_bytes });
        }
        self.paths.push(path);
        self.bytes += charge;
        Ok(())
    }
}

fn discovery_capacity_absence(
    absences: &mut Vec<String>, phase: &str, capacity: DiscoveryCapacity,
    observed: usize, candidates: usize, queued: usize, seen: usize, path_bytes: usize,
) {
    absences.push(format!(
        "Knowledge discovery {phase} capacity exhausted: {} limit {}; observed {observed} directory entries, \
         {candidates} candidate files, {queued} queued paths, {seen} seen directories, \
         {path_bytes} retained path capacity bytes; unseen remainder is unknown",
        capacity.dimension, capacity.limit,
    ));
}

fn directory_observation_failure(
    error: aikit_core::AikitError, checkpoint: Result<()>,
) -> aikit_core::AikitError {
    use std::error::Error;
    match checkpoint {
        Ok(()) => error,
        Err(cause) => {
            let io = cause.source().and_then(|source| source.downcast_ref::<std::io::Error>());
            error.with("directory_affiliation_failure", serde_json::json!({
                "code": cause.code(), "details": cause.details(),
                "io_kind": io.map(|io| format!("{:?}", io.kind())),
                "raw_os_error": io.and_then(std::io::Error::raw_os_error),
            }).to_string())
        }
    }
}

struct DiscoveryBoundary {
    selected: DiscoveryRootObservation,
    world: Option<DiscoveryRootObservation>,
}

impl DiscoveryBoundary {
    fn capture(root: &Path, native_world_root: Option<&Path>) -> Result<Self> {
        let selected = DiscoveryRootObservation::capture(root)?;
        let world = match native_world_root {
            Some(root) => {
                // An unreadable supplied native root is unknown, never a
                // reason to downgrade the selected source to standalone.
                let observed = DiscoveryRootObservation::capture(root)?;
                selected.canonical.starts_with(&observed.canonical).then_some(observed)
            }
            None => None,
        };
        Ok(Self { selected, world })
    }

    fn admit(&self, path: &Path, directory: bool) -> Result<PathBuf> {
        use aikit_adapters::projectcentral::path_agent_readability;
        self.selected.check()?;
        let original = path.strip_prefix(&self.selected.requested).map_err(|_| discovery_refusal())?;
        if original.components().any(|part| !matches!(part, std::path::Component::Normal(_))) {
            return Err(discovery_refusal());
        }
        // The real marker member queries the directory's own native aperture
        // before listing. It is not read as material or assigned a SourceRef.
        let original_policy = if directory { original.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER) }
            else { original.to_path_buf() };
        if !path_agent_readability(&self.selected.requested, &original_policy).map_err(discovery_io)? {
            return Err(discovery_refusal());
        }
        let canonical = fs::canonicalize(path).map_err(discovery_io)?;
        let member = canonical.strip_prefix(&self.selected.canonical).map_err(|_| discovery_refusal())?;
        if member.components().any(|part| !matches!(part, std::path::Component::Normal(_))) {
            return Err(discovery_refusal());
        }
        let canonical_policy = if directory { member.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER) }
            else { member.to_path_buf() };
        if !path_agent_readability(&self.selected.requested, &canonical_policy).map_err(discovery_io)? {
            return Err(discovery_refusal());
        }
        if let Some(world) = &self.world {
            world.check()?;
            // Preserve an actual World-relative lexical route when supplied,
            // and always check the admitted physical target under that World.
            if let Some(relative) = world.declared_member(path)? {
                let policy = if directory { relative.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER) }
                    else { relative.to_path_buf() };
                if !path_agent_readability(&world.requested, &policy).map_err(discovery_io)? {
                    return Err(discovery_refusal());
                }
            }
            let relative = canonical.strip_prefix(&world.canonical).map_err(|_| discovery_refusal())?;
            let policy = if directory { relative.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER) }
                else { relative.to_path_buf() };
            if !path_agent_readability(&world.requested, &policy).map_err(discovery_io)? {
                return Err(discovery_refusal());
            }
            world.check()?;
        }
        self.selected.check()?;
        Ok(canonical)
    }

    fn visit_entries<T>(
        &self, path: &Path, budget: &mut DirectoryBudget,
        mut observe: impl FnMut(fs::DirEntry) -> Result<T>,
        mut commit: impl FnMut(Result<T>) -> Result<DirectoryControl>,
    ) -> Result<DirectoryVisit> {
        let original_mapping = self.admit(path, true)?;
        let original_identity = discovery_directory_identity(path)?;
        let checkpoint = || -> Result<()> {
            if self.admit(path, true)? != original_mapping
                || discovery_directory_identity(path)? != original_identity
            { return Err(discovery_refusal()); }
            Ok(())
        };
        checkpoint()?;
        let mut entries = fs::read_dir(path).map_err(|error| {
            directory_observation_failure(discovery_io(error), checkpoint())
        })?;
        loop {
            checkpoint()?;
            if budget.observed >= budget.limit {
                // Do not request another entry just to guess whether anything
                // remains. Exact-bound EOF is still unknown until observed.
                checkpoint()?;
                return Ok(DirectoryVisit::Stopped(DiscoveryCapacity {
                    dimension: "observed_directory_entries", limit: budget.limit,
                }));
            }
            let Some(entry) = entries.next() else {
                checkpoint()?;
                return Ok(DirectoryVisit::Complete);
            };
            budget.observed += 1;
            let entry = entry.map_err(|error| {
                directory_observation_failure(discovery_io(error), checkpoint())
            })?;
            let pending = observe(entry);
            if let Err(cause) = checkpoint() {
                return Err(match pending {
                    Err(error) => directory_observation_failure(error, Err(cause)),
                    Ok(_) => cause,
                });
            }
            match commit(pending).map_err(|error| {
                directory_observation_failure(error, checkpoint())
            })? {
                DirectoryControl::Continue => {}
                DirectoryControl::Stop(capacity) => {
                    checkpoint()?;
                    return Ok(DirectoryVisit::Stopped(capacity));
                }
            }
        }
    }

    fn read(&self, path: &Path, limit: u64) -> Result<String> {
        let original_mapping = self.admit(path, false)?;
        let member = original_mapping.strip_prefix(&self.selected.canonical)
            .map_err(|_| discovery_refusal())?;
        let bytes = aikit_adapters::projectcentral::publication::material_bytes_affiliated(
            &self.selected.requested, self.selected.identity, member, limit,
        )?;
        if self.admit(path, false)? != original_mapping { return Err(discovery_refusal()); }
        String::from_utf8(bytes).map_err(|_| aikit_core::AikitError::new(
            "knowledge.discovery_encoding", "Admitted discovery material is not UTF-8"))
    }
}

fn discovery_absence(absences: &mut Vec<String>, error: &aikit_core::AikitError) {
    use std::error::Error;
    // Availability keeps actual cause facts without exposing a denied member,
    // body, title, SourceRef or an inferred marker in the diagnostic surface.
    let cause = error.source().and_then(|cause| cause.downcast_ref::<std::io::Error>());
    absences.push(match cause {
        Some(cause) => format!("Knowledge discovery unavailable ({}, IO {:?}, errno {:?})",
            error.code(), cause.kind(), cause.raw_os_error()),
        None => format!("Knowledge discovery withheld or unavailable ({})", error.code()),
    });
}

fn discover_material(
    root: &Path, home: &Path, absences: &mut Vec<String>,
    discover_wiki: bool, native_world_root: Option<&Path>,
) -> Result<DiscoveredMaterial> {
    discover_material_with_limits(root, home, absences, discover_wiki,
        native_world_root, DiscoveryLimits::default())
}

fn discovery_file_exists(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(discovery_io(error)),
    }
}

fn read_canonical_discovery(
    boundary: &DiscoveryBoundary, path: &Path, discovered: &mut DiscoveredMaterial,
    seen: &mut BTreeSet<String>, absences: &mut Vec<String>,
) {
    let text = match boundary.read(path, 16 * 1024 * 1024) {
        Ok(text) => text,
        Err(error) => { discovery_absence(absences, &error); return; }
    };
    match parse_wiki_objects(&text) {
        Ok(objects) => {
            for object in objects {
                if seen.insert(object.ref_id().as_str().to_owned()) { discovered.wiki.push(object); }
            }
        }
        Err(error) => absences.push(format!(
            "Canonical wiki register {} is invalid: {error}", path.display(),
        )),
    }
}

enum PendingDiscovery {
    Skip,
    Directory(PathBuf),
    Capacity(DiscoveryCapacity),
    File {
        sources: Option<Vec<SourceMaterial>>,
        wiki: Vec<WikiObject>,
        warning: Option<String>,
    },
}

fn discover_material_with_limits(
    root: &Path, home: &Path, absences: &mut Vec<String>, discover_wiki: bool,
    native_world_root: Option<&Path>, limits: DiscoveryLimits,
) -> Result<DiscoveredMaterial> {
    let mut discovered = DiscoveredMaterial::default();
    let boundary = match DiscoveryBoundary::capture(root, native_world_root) {
        Ok(boundary) => boundary,
        Err(error) => { discovery_absence(absences, &error); return Ok(discovered); }
    };
    let root = boundary.selected.requested.as_path();
    let home = if home.is_absolute() { home.to_path_buf() } else {
        std::env::current_dir().map_err(discovery_io)?.join(home)
    };
    let mut seen_wiki_refs = BTreeSet::new();
    let mut conflicted_sources = BTreeSet::new();

    if discover_wiki {
        // Root Wiki is considered first and independently of both bounded
        // Work-register enumeration and the generic JSON candidate count.
        let root_wiki = root.join("Control/agents/wiki/wiki.json");
        match boundary.selected.check().and_then(|_| discovery_file_exists(&root_wiki)) {
            Ok(true) => read_canonical_discovery(&boundary, &root_wiki,
                &mut discovered, &mut seen_wiki_refs, absences),
            Ok(false) => { if let Err(error) = boundary.selected.check() { discovery_absence(absences, &error); } }
            Err(error) => discovery_absence(absences, &error),
        }
        let work = root.join("Work");
        let mut canonical = CanonicalPaths::new(limits);
        let mut budget = DirectoryBudget::new(limits.canonical_entries);
        let visit = match fs::metadata(&work) {
            Ok(metadata) if metadata.is_dir() => Some(boundary.visit_entries(
                &work, &mut budget,
                |entry| {
                    let path = entry.path();
                    let metadata = fs::metadata(&path).map_err(discovery_io)?;
                    if !metadata.is_dir() { return Ok(None); }
                    boundary.admit(&path, true)?;
                    let wiki = path.join("ProjectCentral/agents/wiki/wiki.json");
                    Ok(discovery_file_exists(&wiki)?.then_some(wiki))
                },
                |pending| {
                    match pending {
                        Ok(Some(path)) => {
                            if let Err(capacity) = canonical.push(path) { return Ok(DirectoryControl::Stop(capacity)); }
                        }
                        Ok(None) => {}
                        Err(error) => discovery_absence(absences, &error),
                    }
                    Ok(DirectoryControl::Continue)
                },
            )),
            Ok(_) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Err(error) = boundary.selected.check() { discovery_absence(absences, &error); }
                None
            }
            Err(error) => { discovery_absence(absences, &discovery_io(error)); None }
        };
        if let Some(visit) = visit {
            match visit {
                Ok(DirectoryVisit::Stopped(capacity)) => discovery_capacity_absence(
                    absences, "canonical Work", capacity, budget.observed, 0,
                    canonical.paths.len(), 0, canonical.bytes,
                ),
                Ok(DirectoryVisit::Complete) => {}
                Err(error) => discovery_absence(absences, &error),
            }
        }
        canonical.paths.sort();
        canonical.paths.dedup();
        for path in canonical.paths {
            read_canonical_discovery(&boundary, &path,
                &mut discovered, &mut seen_wiki_refs, absences);
        }
    }

    let mut frontier = DiscoveryFrontier::new(limits);
    if let Err(error) = boundary.admit(root, true) {
        discovery_absence(absences, &error);
        return Ok(discovered);
    }
    if let Err(capacity) = frontier.enqueue(root.to_path_buf()) {
        discovery_capacity_absence(absences, "generic", capacity, 0, 0, 0, 0, 0);
        return Ok(discovered);
    }
    let mut budget = DirectoryBudget::new(limits.entries);
    let files = std::cell::Cell::new(0usize);
    while let Some(dir) = frontier.pop() {
        if dir == home || is_ignored_dir(&dir) { continue; }
        let canonical_directory = match boundary.admit(&dir, true) {
            Ok(directory) => directory,
            Err(error) => { discovery_absence(absences, &error); continue; }
        };
        match frontier.admit_seen(canonical_directory) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(capacity) => {
                discovery_capacity_absence(absences, "generic", capacity, budget.observed,
                    files.get(), frontier.queued.len(), frontier.seen.len(),
                    frontier.queued_bytes + frontier.seen_bytes);
                break;
            }
        }
        let visit = boundary.visit_entries(&dir, &mut budget,
            |entry| {
                let path = entry.path();
                if path.capacity() > limits.single_path_bytes {
                    return Ok(PendingDiscovery::Capacity(DiscoveryCapacity {
                        dimension: "single_path_capacity_bytes", limit: limits.single_path_bytes,
                    }));
                }
                let metadata = fs::metadata(&path).map_err(discovery_io)?;
                if metadata.is_dir() {
                    if path == home || is_ignored_dir(&path) { return Ok(PendingDiscovery::Skip); }
                    // Current refusal happens before frontier retention/listing.
                    boundary.admit(&path, true)?;
                    return Ok(PendingDiscovery::Directory(path));
                }
                if files.get() >= limits.candidates {
                    return Ok(PendingDiscovery::Capacity(DiscoveryCapacity {
                        dimension: "generic_candidate_files", limit: limits.candidates,
                    }));
                }
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    return Ok(PendingDiscovery::Skip);
                }
                // This is an actually observed JSON candidate, including a
                // refused/failed body read; counters are not derived products.
                files.set(files.get() + 1);
                let text = boundary.read(&path, MAX_DISCOVERY_FILE_BYTES)?;
                let source_items = serde_json::from_str::<SourceMaterial>(&text)
                    .map(|item| vec![item])
                    .or_else(|_| serde_json::from_str::<Vec<SourceMaterial>>(&text));
                let mut wiki = Vec::new();
                let mut warning = None;
                if source_items.is_err() && discover_wiki && text.contains("okf-wiki/v1") {
                    match parse_wiki_objects(&text) {
                        Ok(objects) => wiki = objects,
                        Err(collection_error) => match OkfWikiBundle::parse_json(&text) {
                            Ok(bundle) => wiki.push(bundle.wiki),
                            Err(_) => warning = Some(format!(
                                "self-identified SemanticWiki material at {} is invalid: {collection_error}", path.display(),
                            )),
                        },
                    }
                }
                Ok(PendingDiscovery::File { sources: source_items.ok(), wiki, warning })
            },
            |pending| {
                match pending {
                    Err(error) => discovery_absence(absences, &error),
                    Ok(PendingDiscovery::Skip) => {}
                    Ok(PendingDiscovery::Directory(path)) => {
                        if let Err(capacity) = frontier.enqueue(path) { return Ok(DirectoryControl::Stop(capacity)); }
                    }
                    Ok(PendingDiscovery::Capacity(capacity)) => return Ok(DirectoryControl::Stop(capacity)),
                    Ok(PendingDiscovery::File { sources, wiki, warning }) => {
                        if let Some(warning) = warning { absences.push(warning); }
                        for object in wiki {
                            if seen_wiki_refs.insert(object.ref_id().as_str().to_owned()) { discovered.wiki.push(object); }
                        }
                        if let Some(items) = sources {
                            for item in items {
                                let source = item.binding.source.clone();
                                if conflicted_sources.contains(&source) { continue; }
                                if let Some(previous) = discovered.sources.get(&source) {
                                    if previous != &item {
                                        discovered.sources.remove(&source);
                                        conflicted_sources.insert(source.clone());
                                        absences.push(format!(
                                            "SourcePool material conflict for stable SourceRef {source}; conflicting copies were withheld",
                                        ));
                                    }
                                } else { discovered.sources.insert(source, item); }
                            }
                        }
                    }
                }
                Ok(DirectoryControl::Continue)
            },
        );
        match visit {
            Ok(DirectoryVisit::Complete) => {}
            Ok(DirectoryVisit::Stopped(capacity)) => {
                discovery_capacity_absence(absences, "generic", capacity, budget.observed,
                    files.get(), frontier.queued.len(), frontier.seen.len(),
                    frontier.queued_bytes + frontier.seen_bytes);
                break;
            }
            Err(error) => discovery_absence(absences, &error),
        }
    }
    Ok(discovered)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod discovery_physical_tests {
    use super::*;
    use aikit_adapters::runner::CommandRunner;
    use std::os::unix::fs::symlink;

    fn native_discovery_tempdir() -> tempfile::TempDir {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        tempfile::Builder::new().prefix("knowledge-discovery-").tempdir_in(&scratch).unwrap()
    }

    fn visited_entry_count(boundary: &DiscoveryBoundary, path: &Path) -> Result<usize> {
        let count = std::cell::Cell::new(0usize);
        let mut budget = DirectoryBudget::new(DiscoveryLimits::default().entries);
        match boundary.visit_entries(path, &mut budget, |_| Ok(()), |pending| {
            pending?;
            count.set(count.get() + 1);
            Ok(DirectoryControl::Continue)
        })? {
            DirectoryVisit::Complete => Ok(count.get()),
            DirectoryVisit::Stopped(_) => Err(aikit_core::AikitError::new(
                "knowledge.discovery_capacity", "Entry-count proof exceeded its actual allowance")),
        }
    }

    fn enumeration_wiki(source_ref: &str, title: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"objects":[{
            "profile":"okf-wiki/v1", "object":"node", "ref":source_ref,
            "type":"Module", "title":title, "source_refs":[],
            "provenance":[{"source_ref":"source:physical-enumeration-fixture"}],
        }]})).unwrap()
    }

    #[test]
    fn real_wide_directory_uses_one_bounded_native_stream() {
        let root = native_discovery_tempdir();
        let neighbour = native_discovery_tempdir();
        let neighbour_source = neighbour.path().join("untouched.txt");
        fs::write(&neighbour_source, b"neighbour bytes").unwrap();
        for n in 0..128 { fs::write(root.path().join(format!("entry-{n:03}.txt")), b"retained").unwrap(); }
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        let mut budget = DirectoryBudget::new(7);
        let observed = std::cell::Cell::new(0usize);
        let committed = std::cell::Cell::new(0usize);
        let result = boundary.visit_entries(root.path(), &mut budget, |entry| {
            assert_eq!(fs::read(entry.path()).unwrap(), b"retained");
            observed.set(observed.get() + 1);
            Ok(())
        }, |pending| {
            pending?;
            committed.set(committed.get() + 1);
            Ok(DirectoryControl::Continue)
        }).unwrap();
        assert!(matches!(result, DirectoryVisit::Stopped(DiscoveryCapacity {
            dimension: "observed_directory_entries", limit: 7,
        })));
        assert_eq!((budget.observed, observed.get(), committed.get()), (7, 7, 7));
        assert_eq!(visited_entry_count(&boundary, root.path()).unwrap(), 128);
        assert_eq!(fs::read(neighbour_source).unwrap(), b"neighbour bytes");
    }

    #[test]
    fn real_frontier_seen_and_actual_path_capacity_refuse_before_retention() {
        let root = native_discovery_tempdir();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        for n in 0..8 { fs::create_dir(root.path().join(format!("room-{n}"))).unwrap(); }
        let limits = DiscoveryLimits { queued: 3, seen: 2, ..DiscoveryLimits::default() };
        let mut frontier = DiscoveryFrontier::new(limits);
        let mut budget = DirectoryBudget::new(100);
        let result = boundary.visit_entries(root.path(), &mut budget, |entry| {
            let path = entry.path();
            boundary.admit(&path, true)?;
            Ok(path)
        }, |pending| match frontier.enqueue(pending?) {
            Ok(()) => Ok(DirectoryControl::Continue),
            Err(capacity) => Ok(DirectoryControl::Stop(capacity)),
        }).unwrap();
        assert!(matches!(result, DirectoryVisit::Stopped(DiscoveryCapacity {
            dimension: "queued_directories", limit: 3,
        })));
        assert_eq!((frontier.queued.len(), frontier.max_queued), (3, 3));
        assert_eq!(budget.observed, 4);
        let bytes = frontier.queued_bytes;
        let path = frontier.pop().unwrap();
        assert_eq!(frontier.queued_bytes, bytes - path.capacity());
        frontier.admit_seen(fs::canonicalize(&path).unwrap()).unwrap();
        let second = frontier.pop().unwrap();
        frontier.admit_seen(fs::canonicalize(&second).unwrap()).unwrap();
        symlink(&second, root.path().join("same-room-alias")).unwrap();
        assert!(!frontier.admit_seen(fs::canonicalize(root.path().join("same-room-alias")).unwrap()).unwrap());
        let third = frontier.pop().unwrap();
        assert_eq!(frontier.admit_seen(fs::canonicalize(&third).unwrap()).unwrap_err().dimension,
            "seen_canonical_directories");
        assert_eq!((frontier.seen.len(), frontier.max_seen), (2, 2));
        assert_eq!(frontier.queued_bytes, 0);
        let actual = root.path().join("room-0");
        let retained = actual.clone();
        let mut paths = DiscoveryFrontier::new(DiscoveryLimits {
            single_path_bytes: retained.capacity() - 1, ..DiscoveryLimits::default()
        });
        assert_eq!(paths.enqueue(retained).unwrap_err().dimension, "single_path_capacity_bytes");
        assert!(paths.queued.is_empty());
        assert_eq!(paths.queued_bytes, 0);
        let mut paths = DiscoveryFrontier::new(DiscoveryLimits {
            path_bytes: actual.capacity() - 1, ..DiscoveryLimits::default()
        });
        assert_eq!(paths.enqueue(actual).unwrap_err().dimension, "retained_path_capacity_bytes");
        assert!(paths.queued.is_empty());
        assert_eq!(paths.max_path_bytes, 0);
        assert!(root.path().join("room-0").is_dir());
    }

    #[test]
    fn actual_directory_alias_retarget_prevents_pending_body_commit() {
        let root = native_discovery_tempdir();
        let old = root.path().join("old-room");
        let foreign = root.path().join("other-room");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&foreign).unwrap();
        fs::write(old.join("body.json"), b"actual original bytes").unwrap();
        fs::write(foreign.join("body.json"), b"actual foreign bytes").unwrap();
        let alias = root.path().join("selected-room");
        symlink(&old, &alias).unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        let mut budget = DirectoryBudget::new(10);
        let committed = std::cell::Cell::new(false);
        let error = boundary.visit_entries(&alias, &mut budget, |entry| {
            let text = boundary.read(&entry.path(), MAX_DISCOVERY_FILE_BYTES)?;
            assert_eq!(text, "actual original bytes");
            fs::remove_file(&alias).unwrap();
            symlink(&foreign, &alias).unwrap();
            Ok(text)
        }, |_| { committed.set(true); Ok(DirectoryControl::Continue) }).unwrap_err();
        assert_eq!(error.code(), "knowledge.discovery_withheld");
        assert!(!committed.get());
        assert_eq!(fs::read(old.join("body.json")).unwrap(), b"actual original bytes");
        assert_eq!(fs::read(foreign.join("body.json")).unwrap(), b"actual foreign bytes");
    }

    #[test]
    fn actual_member_and_parent_loss_preserve_first_io_and_failed_affiliation() {
        use std::error::Error;
        let root = native_discovery_tempdir();
        let requested = root.path().join("requested-room");
        let retained = root.path().join("retained-room");
        fs::create_dir(&requested).unwrap();
        fs::write(requested.join("body.json"), b"retained source bytes").unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        let mut budget = DirectoryBudget::new(10);
        let committed = std::cell::Cell::new(false);
        let expected_errno = std::cell::Cell::new(None);
        let error = boundary.visit_entries(&requested, &mut budget, |entry| {
            fs::rename(&requested, &retained).unwrap();
            let cause = fs::read(entry.path()).unwrap_err();
            expected_errno.set(cause.raw_os_error());
            Err::<(), _>(discovery_io(cause))
        }, |_| { committed.set(true); Ok(DirectoryControl::Continue) }).unwrap_err();
        let cause = error.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(cause.raw_os_error(), expected_errno.get());
        assert!(cause.raw_os_error().is_some());
        assert!(error.details().contains_key("directory_affiliation_failure"));
        assert!(!committed.get());
        assert_eq!(fs::read(retained.join("body.json")).unwrap(), b"retained source bytes");
        assert!(!requested.exists());
    }

    #[test]
    fn marked_subtree_does_not_consume_frontier_or_hide_an_admitted_sibling() {
        let root = native_discovery_tempdir();
        let home = native_discovery_tempdir();
        let marked = root.path().join("marked");
        fs::create_dir(&marked).unwrap();
        fs::write(marked.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER), b"withheld").unwrap();
        let secret = enumeration_wiki("wiki:node:unselected-secret", "Unselected secret title");
        fs::write(marked.join("secret.json"), &secret).unwrap();
        let allowed = root.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        fs::write(allowed.join("wiki.json"), enumeration_wiki("wiki:node:admitted-sibling", "Admitted sibling")).unwrap();
        let mut absences = Vec::new();
        let material = discover_material_with_limits(root.path(), home.path(), &mut absences,
            true, None, DiscoveryLimits { queued: 1, ..DiscoveryLimits::default() }).unwrap();
        assert_eq!(material.wiki.len(), 1);
        assert_eq!(material.wiki[0].ref_id().as_str(), "wiki:node:admitted-sibling");
        assert!(!absences.iter().any(|absence| absence.contains("capacity exhausted")));
        assert!(!absences.join(" ").contains("Unselected secret title"));
        assert_eq!(fs::read(marked.join("secret.json")).unwrap(), secret);
    }

    #[test]
    fn canonical_wiki_floor_is_whole_and_independent_of_generic_candidate_capacity() {
        let root = native_discovery_tempdir();
        let home = native_discovery_tempdir();
        fs::create_dir_all(root.path().join("Control/agents/wiki")).unwrap();
        let body = enumeration_wiki("wiki:node:canonical-root-floor", "Whole canonical root");
        let path = root.path().join("Control/agents/wiki/wiki.json");
        fs::write(&path, &body).unwrap();
        let project = root.path().join("Work/native-fixture/ProjectCentral/agents/wiki");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("wiki.json"), enumeration_wiki("wiki:node:canonical-project-floor", "Whole project register")).unwrap();
        let candidates = root.path().join("many-candidates");
        fs::create_dir(&candidates).unwrap();
        for n in 0..=MAX_DISCOVERY_FILES { fs::write(candidates.join(format!("{n:04}.json")), b"{}").unwrap(); }
        let mut absences = Vec::new();
        let material = discover_material(root.path(), home.path(), &mut absences, true, None).unwrap();
        assert!(material.wiki.iter().any(|object| object.ref_id().as_str() == "wiki:node:canonical-root-floor"));
        assert!(material.wiki.iter().any(|object| object.ref_id().as_str() == "wiki:node:canonical-project-floor"));
        assert!(absences.iter().any(|absence| absence.contains("generic_candidate_files")
            && absence.contains("unseen remainder is unknown")));
        let mut absences = Vec::new();
        let material = discover_material_with_limits(root.path(), home.path(), &mut absences,
            true, None, DiscoveryLimits { candidates: 0, canonical_paths: 0, ..DiscoveryLimits::default() }).unwrap();
        assert_eq!(material.wiki.len(), 1);
        assert_eq!(material.wiki[0].ref_id().as_str(), "wiki:node:canonical-root-floor");
        assert!(absences.iter().any(|absence| absence.contains("canonical_register_paths")
            && absence.contains("unseen remainder is unknown")));
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn actual_canonical_path_collection_preserves_order_and_refuses_capacity_before_retention() {
        let root = native_discovery_tempdir();
        let mut paths = Vec::new();
        for name in ["z-last", "a-first", "m-middle"] {
            let directory = root.path().join(name);
            fs::create_dir(&directory).unwrap();
            let path = directory.join("wiki.json");
            fs::write(&path, b"retained canonical bytes").unwrap();
            paths.push(path);
        }
        let mut canonical = CanonicalPaths::new(DiscoveryLimits {
            canonical_paths: 2, ..DiscoveryLimits::default()
        });
        canonical.push(paths[0].clone()).unwrap();
        canonical.push(paths[1].clone()).unwrap();
        let before = canonical.bytes;
        assert_eq!(canonical.push(paths[2].clone()).unwrap_err().dimension, "canonical_register_paths");
        assert_eq!((canonical.paths.len(), canonical.bytes), (2, before));
        canonical.paths.sort();
        assert_eq!(canonical.paths, vec![paths[1].clone(), paths[0].clone()]);
        let actual = paths[2].clone();
        let mut canonical = CanonicalPaths::new(DiscoveryLimits {
            canonical_path_bytes: actual.capacity() - 1, ..DiscoveryLimits::default()
        });
        assert_eq!(canonical.push(actual).unwrap_err().dimension, "canonical_path_capacity_bytes");
        assert!(canonical.paths.is_empty());
        assert_eq!(canonical.bytes, 0);
        for path in paths { assert_eq!(fs::read(path).unwrap(), b"retained canonical bytes"); }
    }

    #[test]
    fn actual_directory_marker_prunes_before_listing_and_material_sniff() {
        let root = native_discovery_tempdir();
        let home = native_discovery_tempdir();
        let marked = root.path().join("marked");
        fs::create_dir(&marked).unwrap();
        fs::write(marked.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER), b"withheld").unwrap();
        fs::write(marked.join("unselected-secret.json"), b"malformed okf-wiki/v1 secret-body").unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        assert_eq!(visited_entry_count(&boundary, &marked).unwrap_err().code(), "knowledge.discovery_withheld");
        assert_eq!(boundary.read(&marked.join("unselected-secret.json"), MAX_DISCOVERY_FILE_BYTES)
            .unwrap_err().code(), "knowledge.discovery_withheld");
        let mut absences = Vec::new();
        let material = discover_material(root.path(), home.path(), &mut absences, true, None).unwrap();
        assert!(material.wiki.is_empty() && material.sources.is_empty());
        let diagnostic = absences.join("\n");
        assert!(diagnostic.contains("knowledge.discovery_withheld"));
        assert!(!diagnostic.contains("unselected-secret") && !diagnostic.contains("secret-body"));
        assert!(!diagnostic.contains("invalid:"), "withheld material was never parsed as Wiki");
    }

    #[test]
    fn actual_known_world_ancestor_marker_cannot_be_downgraded_to_selected_root() {
        let world = native_discovery_tempdir();
        let work = world.path().join("Work");
        let project = work.join("Project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("body.json");
        fs::write(&path, b"actual-body").unwrap();
        let boundary = DiscoveryBoundary::capture(&project, Some(world.path())).unwrap();
        assert_eq!(boundary.read(&path, MAX_DISCOVERY_FILE_BYTES).unwrap(), "actual-body");
        fs::write(work.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER), b"withheld").unwrap();
        assert_eq!(boundary.read(&path, MAX_DISCOVERY_FILE_BYTES).unwrap_err().code(),
            "knowledge.discovery_withheld");
        let fresh = DiscoveryBoundary::capture(&project, Some(world.path())).unwrap();
        assert_eq!(visited_entry_count(&fresh, &project).unwrap_err().code(), "knowledge.discovery_withheld");
        assert_eq!(fs::read(&path).unwrap(), b"actual-body");
    }

    #[test]
    fn actual_external_root_remains_standalone_but_unknown_native_root_does_not() {
        use std::error::Error;
        let selected = native_discovery_tempdir();
        let world = native_discovery_tempdir();
        let path = selected.path().join("body.json");
        fs::write(&path, b"external-body").unwrap();
        let external = DiscoveryBoundary::capture(selected.path(), Some(world.path())).unwrap();
        assert!(external.world.is_none());
        assert_eq!(external.read(&path, MAX_DISCOVERY_FILE_BYTES).unwrap(), "external-body");
        let missing = world.path().join("absent-native-root");
        let error = match DiscoveryBoundary::capture(selected.path(), Some(&missing)) {
            Ok(_) => panic!("unreadable supplied native root cannot become standalone"),
            Err(error) => error,
        };
        let cause = error.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert!(cause.raw_os_error().is_some());
        let mut absences = Vec::new();
        let material = discover_material(selected.path(), world.path(), &mut absences, false, Some(&missing)).unwrap();
        assert!(material.sources.is_empty());
        assert!(absences.iter().any(|absence| absence.contains("NotFound")));
    }

    #[test]
    fn actual_in_root_directory_alias_is_admitted_and_cycle_is_visited_once() {
        let root = native_discovery_tempdir();
        let home = native_discovery_tempdir();
        let room = root.path().join("room");
        fs::create_dir(&room).unwrap();
        fs::write(room.join("plain.json"), b"{}").unwrap();
        symlink(&room, root.path().join("room-alias")).unwrap();
        symlink(root.path(), room.join("cycle")).unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        assert_eq!(boundary.read(&root.path().join("room-alias/plain.json"), MAX_DISCOVERY_FILE_BYTES)
            .unwrap(), "{}");
        assert_eq!(visited_entry_count(&boundary, &root.path().join("room-alias")).unwrap(), 2);
        let started = std::time::Instant::now();
        let mut absences = Vec::new();
        discover_material(root.path(), home.path(), &mut absences, false, None).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(!absences.iter().any(|absence| absence.contains("stopped after")));
    }

    #[test]
    fn actual_final_symlink_and_external_directory_alias_cannot_disclose_foreign_bytes() {
        let root = native_discovery_tempdir();
        let external = native_discovery_tempdir();
        let foreign = external.path().join("foreign.json");
        fs::write(&foreign, b"unselected-foreign").unwrap();
        symlink(&foreign, root.path().join("final.json")).unwrap();
        symlink(external.path(), root.path().join("external")).unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        assert_eq!(boundary.read(&root.path().join("final.json"), MAX_DISCOVERY_FILE_BYTES)
            .unwrap_err().code(), "knowledge.discovery_withheld");
        assert_eq!(visited_entry_count(&boundary, &root.path().join("external")).unwrap_err().code(),
            "knowledge.discovery_withheld");
        assert_eq!(fs::read(&foreign).unwrap(), b"unselected-foreign");
    }

    #[test]
    fn actual_fifo_and_hardlink_are_refused_without_blocking_or_changing_retained_bytes() {
        let root = native_discovery_tempdir();
        let ordinary = root.path().join("body.json");
        let alias = root.path().join("body-alias.json");
        fs::write(&ordinary, b"retained-original").unwrap();
        fs::hard_link(&ordinary, &alias).unwrap();
        let fifo = root.path().join("pipe.json");
        let created = SystemRunner::new().with_timeout(std::time::Duration::from_secs(2))
            .run(&["mkfifo".into(), fifo.display().to_string()]).unwrap();
        assert_eq!(created.status, 0);
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(boundary.read(&fifo, MAX_DISCOVERY_FILE_BYTES).unwrap_err().code(),
            "knowledge.discovery_withheld");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(boundary.read(&ordinary, MAX_DISCOVERY_FILE_BYTES).is_err());
        assert_eq!(fs::read(&ordinary).unwrap(), b"retained-original");
        assert_eq!(fs::read(&alias).unwrap(), b"retained-original");
    }

    #[test]
    fn actual_generic_capacity_and_canonical_wiki_capacity_remain_distinct() {
        let root = native_discovery_tempdir();
        let path = root.path().join("large.json");
        let bytes = vec![b'x'; MAX_DISCOVERY_FILE_BYTES as usize + 1];
        fs::write(&path, &bytes).unwrap();
        let boundary = DiscoveryBoundary::capture(root.path(), None).unwrap();
        assert_eq!(boundary.read(&path, MAX_DISCOVERY_FILE_BYTES).unwrap_err().code(),
            "knowledge.wiki_publication_budget");
        assert_eq!(boundary.read(&path, 16 * 1024 * 1024).unwrap().as_bytes(), bytes.as_slice());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        // This is mechanical capacity, not a fabricated canonical Wiki owner.
    }

    #[test]
    fn native_owner_constructor_keeps_one_finite_deadline_and_transport_capacity() {
        assert_eq!(native_source_runner().timeout(), Some(std::time::Duration::from_secs(60)));
        assert_eq!(native_source_runner().output_limit_bytes(), 128 * 1024 * 1024);
        assert_eq!(SystemRunner::new().timeout(), None, "generic live-leader default is unchanged");
    }

    #[test]
    fn actual_stalled_process_on_the_native_constructor_retains_execution_uncertainty() {
        // SAME production constructor with its existing explicit mechanical
        // test allowance; this does not fabricate a native owner's response.
        let runner = native_source_runner().with_timeout(std::time::Duration::from_millis(150));
        let started = std::time::Instant::now();
        let error = runner.run(&["/bin/sh".into(), "-c".into(),
            "printf 'actual-mechanical-native-command-entered'; exec /bin/sleep 30".into()]).unwrap_err();
        assert_eq!(error.code(), "mux.command_timeout");
        assert_eq!(error.details().get("execution_started").map(String::as_str), Some("true"));
        assert_eq!(error.details().get("effects").map(String::as_str), Some("unknown"));
        assert_eq!(error.details().get("automatic_retry").map(String::as_str), Some("false"));
        assert!(error.details().get("captured_stdout").unwrap().contains("actual-mechanical-native-command-entered"));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        // The deadline is a real clock observation, not an OS syscall failure;
        // no invented TimedOut errno/source is asserted. Actual IO paths retain
        // their original cause through the existing runner capture contract.
    }

    fn invocation_relative_route(path: &Path) -> PathBuf {
        let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        for (up, ancestor) in cwd.ancestors().enumerate() {
            if let Ok(member) = path.strip_prefix(ancestor) {
                let mut relative = PathBuf::new();
                for _ in 0..up { relative.push(".."); }
                relative.push(member);
                return relative;
            }
        }
        panic!("actual Unix fixture and invocation must have a common root");
    }

    #[test]
    fn actual_original_world_alias_floor_survives_mixed_invocation_coordinates() {
        let owned = native_discovery_tempdir();
        let home = native_discovery_tempdir();
        let world = owned.path().join("world");
        let project = world.join("Work/allowed/Project");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(world.join("Work/withheld")).unwrap();
        let world = fs::canonicalize(&world).unwrap();
        let project = world.join("Work/allowed/Project");
        let body = br#"{"objects":[{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:actual-independent-room","type":"Module","title":"Actual independent Wiki fixture","source_refs":[],"provenance":[{"source_ref":"source:test"}]}]}"#;
        let source = project.join("independent-wiki.json");
        fs::write(&source, body).unwrap();
        symlink(&project, world.join("Work/withheld/alias")).unwrap();
        symlink(&world, world.join("Work/withheld/world-alias")).unwrap();
        let world_relative = invocation_relative_route(&world);
        let marker = world.join("Work/withheld/.no-agent-retrieval");
        for member in ["Work/withheld/alias", "Work/withheld/world-alias/Work/allowed/Project"] {
            let selected = world.join(member);
            let selected_relative = invocation_relative_route(&selected);
            let routes = [(&selected, &world), (&selected_relative, &world),
                (&selected, &world_relative), (&selected_relative, &world_relative)];
            for (selected, world) in routes {
                let boundary = DiscoveryBoundary::capture(selected, Some(world)).unwrap();
                let path = boundary.selected.requested.join("independent-wiki.json");
                assert_eq!(boundary.read(&path, MAX_DISCOVERY_FILE_BYTES).unwrap().as_bytes(), body);
                let mut absences = Vec::new();
                let material = discover_material(selected, home.path(), &mut absences, true, Some(world)).unwrap();
                assert_eq!(material.wiki.len(), 1, "{selected:?} {world:?}: {absences:?}");
            }
            fs::write(&marker, b"actual owner withheld original World ancestry").unwrap();
            for (selected, world) in routes {
                let mut absences = Vec::new();
                let material = discover_material(selected, home.path(), &mut absences, true, Some(world)).unwrap();
                assert!(material.wiki.is_empty() && material.sources.is_empty());
                assert!(absences.iter().any(|absence| absence.contains("knowledge.discovery_withheld")),
                    "{selected:?} {world:?}: {absences:?}");
                assert_eq!(fs::read(&source).unwrap(), body);
            }
            fs::remove_file(&marker).unwrap();
        }
        assert_eq!(fs::read(&source).unwrap(), body);
    }

    #[test]
    fn actual_selected_root_alias_retarget_refuses_same_bytes_at_foreign_root() {
        let fixture = native_discovery_tempdir();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        fs::create_dir(&first).unwrap(); fs::create_dir(&second).unwrap();
        fs::write(first.join("body.json"), b"same-body").unwrap();
        fs::write(second.join("body.json"), b"same-body").unwrap();
        let selected = fixture.path().join("selected");
        symlink(&first, &selected).unwrap();
        let boundary = DiscoveryBoundary::capture(&selected, None).unwrap();
        assert_eq!(boundary.read(&selected.join("body.json"), MAX_DISCOVERY_FILE_BYTES).unwrap(), "same-body");
        fs::remove_file(&selected).unwrap(); symlink(&second, &selected).unwrap();
        assert_eq!(boundary.read(&selected.join("body.json"), MAX_DISCOVERY_FILE_BYTES).unwrap_err().code(),
            "knowledge.discovery_withheld");
        assert_eq!(fs::read(first.join("body.json")).unwrap(), b"same-body");
        assert_eq!(fs::read(second.join("body.json")).unwrap(), b"same-body");
    }
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
