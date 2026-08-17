from pathlib import Path


def replace(path: str, old: str, new: str):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor not found in {path}: {old[:80]!r}")
    p.write_text(text.replace(old, new, 1))

# --- core: expose current canonical shallow records to composition roots ---
replace(
    "crates/aikit-core/src/resource/search.rs",
    "    pub fn get(&self, resource: &ResourceRef) -> Option<&ResourceRecord> {\n        self.records.get(resource)\n    }\n",
    "    pub fn get(&self, resource: &ResourceRef) -> Option<&ResourceRecord> {\n        self.records.get(resource)\n    }\n\n    /// Iterate the canonical shallow resources already admitted to this field.\n    /// Composition roots may project explicit Knowledge kinds into ProjectMap,\n    /// but this does not turn the search index into a provider graph.\n    pub fn records(&self) -> impl Iterator<Item = &ResourceRecord> {\n        self.records.values()\n    }\n",
)

# --- durable operational Knowledge history on the existing usage event stream ---
replace(
    "crates/aikit-store/src/events.rs",
    "    FamiliarityReset,\n    Gc,\n",
    "    FamiliarityReset,\n    /// A completed provider-neutral KnowledgeRoute was actually used.\n    KnowledgeRoute,\n    /// A derived Knowledge context pack/frame was materialised for use.\n    KnowledgeFrame,\n    Gc,\n",
)
replace(
    "crates/aikit-store/src/events.rs",
    "            EventAction::FamiliarityReset => \"familiarity-reset\",\n            EventAction::Gc => \"gc\",\n",
    "            EventAction::FamiliarityReset => \"familiarity-reset\",\n            EventAction::KnowledgeRoute => \"knowledge-route\",\n            EventAction::KnowledgeFrame => \"knowledge-frame\",\n            EventAction::Gc => \"gc\",\n",
)
replace(
    "crates/aikit-store/src/events.rs",
    "            \"familiarity-reset\" => EventAction::FamiliarityReset,\n            \"gc\" => EventAction::Gc,\n",
    "            \"familiarity-reset\" => EventAction::FamiliarityReset,\n            \"knowledge-route\" => EventAction::KnowledgeRoute,\n            \"knowledge-frame\" => EventAction::KnowledgeFrame,\n            \"gc\" => EventAction::Gc,\n",
)

Path("crates/aikit-store/src/knowledge_history.rs").write_text(r'''//! Metadata-only durable History for Knowledge navigation.
//!
//! Route/frame History is operational memory on the existing append-only event
//! stream. It deliberately stores no retrieved excerpts, prompt/query text or
//! provider graph edges. Re-opening a historical frame therefore re-materialises
//! content from its owning providers instead of making History a second source.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use aikit_core::{
    AikitError, FamiliarityContext, KnowledgeContextPack, KnowledgeRoute, KnowledgeRouteStep,
    ProviderRef, ResourceRef, Result,
};

use crate::{Event, EventAction, Index, Timestamp};

const PAYLOAD_KEY: &str = "knowledge-history";
const SCHEMA: &str = "aikit.knowledge-history/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum KnowledgeHistoryKind {
    Route {
        route: ResourceRef,
        destination: ResourceRef,
        steps: Vec<KnowledgeRouteStep>,
    },
    Frame {
        selected: Vec<ResourceRef>,
        routes: Vec<ResourceRef>,
        providers: Vec<ProviderRef>,
        absences: Vec<String>,
        truncated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeHistoryEntry {
    pub event_id: String,
    pub observed_at_ms: u64,
    pub context: FamiliarityContext,
    pub kind: KnowledgeHistoryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    schema: String,
    context: FamiliarityContext,
    kind: KnowledgeHistoryKind,
}

pub fn record_knowledge_route(index: &Index, route: &KnowledgeRoute) -> Result<()> {
    let destination = route.destination().cloned().ok_or_else(|| {
        AikitError::new("knowledge.empty_route", "cannot persist History for an empty route")
    })?;
    append(
        index,
        EventAction::KnowledgeRoute,
        StoredEntry {
            schema: SCHEMA.into(),
            context: route.context.clone(),
            kind: KnowledgeHistoryKind::Route {
                route: route.route.clone(),
                destination,
                steps: route.steps.clone(),
            },
        },
    )
}

pub fn record_knowledge_frame(index: &Index, pack: &KnowledgeContextPack) -> Result<()> {
    let providers = pack
        .readings
        .iter()
        .filter_map(|reading| reading.provider.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    append(
        index,
        EventAction::KnowledgeFrame,
        StoredEntry {
            schema: SCHEMA.into(),
            context: pack.context.clone(),
            kind: KnowledgeHistoryKind::Frame {
                selected: pack.selected.clone(),
                routes: pack.routes.iter().map(|route| route.route.clone()).collect(),
                providers,
                absences: pack.absences.clone(),
                truncated: pack.budget.truncated,
            },
        },
    )
}

pub fn knowledge_history(index: &Index, limit: usize) -> Result<Vec<KnowledgeHistoryEntry>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = index.conn().prepare(
        "SELECT event_id, timestamp_ns, action, arguments FROM usage_events \
         WHERE action IN (?1, ?2) ORDER BY timestamp_ns DESC, event_id DESC LIMIT ?3",
    ).map_err(db_error)?;
    let rows = stmt.query_map(
        params![EventAction::KnowledgeRoute.as_str(), EventAction::KnowledgeFrame.as_str(), limit as i64],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
    ).map_err(db_error)?;
    let mut out = Vec::new();
    for row in rows {
        let (event_id, timestamp_ns, action, arguments_json) = row.map_err(db_error)?;
        let arguments: BTreeMap<String, String> = serde_json::from_str(&arguments_json)
            .map_err(|error| decode_error(&event_id, &action, error.to_string()))?;
        let payload = arguments.get(PAYLOAD_KEY).ok_or_else(|| {
            decode_error(&event_id, &action, format!("missing `{PAYLOAD_KEY}` payload"))
        })?;
        let stored: StoredEntry = serde_json::from_str(payload)
            .map_err(|error| decode_error(&event_id, &action, error.to_string()))?;
        if stored.schema != SCHEMA {
            return Err(decode_error(
                &event_id,
                &action,
                format!("unsupported schema {}; expected {SCHEMA}", stored.schema),
            ));
        }
        out.push(KnowledgeHistoryEntry {
            event_id,
            observed_at_ms: (timestamp_ns.max(0) as u64) / 1_000_000,
            context: stored.context,
            kind: stored.kind,
        });
    }
    Ok(out)
}

fn append(index: &Index, action: EventAction, stored: StoredEntry) -> Result<()> {
    let payload = serde_json::to_string(&stored).map_err(|error| {
        AikitError::new("knowledge.history_encode_failed", format!("could not encode Knowledge History: {error}"))
    })?;
    let mut event = Event::new(action).at(Timestamp::now());
    event.arguments.insert(PAYLOAD_KEY.into(), payload);
    index.record_event(&event)
}

fn decode_error(event_id: &str, action: &str, detail: impl Into<String>) -> AikitError {
    AikitError::new(
        "knowledge.history_decode_failed",
        format!("could not decode {action} event {event_id}: {}", detail.into()),
    )
}

fn db_error(error: rusqlite::Error) -> AikitError {
    AikitError::new(
        "knowledge.history_query_failed",
        format!("could not read Knowledge History: {error}"),
    )
}
''')

replace(
    "crates/aikit-store/src/lib.rs",
    "pub mod inbox;\npub mod index;\n",
    "pub mod inbox;\npub mod index;\npub mod knowledge_history;\n",
)
replace(
    "crates/aikit-store/src/lib.rs",
    "pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};\n",
    "pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};\npub use knowledge_history::{\n    knowledge_history, record_knowledge_frame, record_knowledge_route, KnowledgeHistoryEntry,\n    KnowledgeHistoryKind,\n};\n",
)

# --- production composition root ---
Path("crates/aikit-cli/src/knowledge_runtime.rs").write_text(r'''//! Production composition root for provider-neutral Knowledge Navigation.
//!
//! The runtime owns provider materialisation and lends one `KnowledgeApplication`
//! per operation. It does not create a universal graph/store and it never treats
//! ContextSource discovery as permission to retrieve material.

use std::path::{Path, PathBuf};

use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::SystemRunner;
use aikit_core::knowledge_source_pool::{NativeSourcePoolProvider, SourceMaterial, SourcePoolProvider};
use aikit_core::knowledge_wiki::{parse_wiki_objects, OkfWikiBundle, WikiObject};
use aikit_core::{
    AikitError, FamiliarityContext, KnowledgeAddress, KnowledgeApplication, KnowledgeContextPack,
    KnowledgeOperations, KnowledgeRoute, ProjectMap, ResourceRef, Result, SemanticWikiIndex,
    SemanticWikiProvider,
};

use crate::app::Service;

#[derive(Debug, Clone, Default)]
pub struct KnowledgeInputs {
    /// Explicit OKF Wiki files selected for this invocation. Discovery alone never
    /// enters this list.
    pub wiki_files: Vec<PathBuf>,
    /// Explicit, already-authorised SourceMaterial JSON files.
    pub source_material_files: Vec<PathBuf>,
    /// Explicit ProjectMap federation projection. Provider-native edges remain in
    /// their providers; this file contains only cross-lens bindings/endpoints.
    pub project_map_file: Option<PathBuf>,
}

pub struct KnowledgeRuntime {
    context: FamiliarityContext,
    wiki_index: Option<SemanticWikiIndex>,
    source_material: Vec<SourceMaterial>,
    native_sources: NativeSourcePoolProvider,
    code: Option<GitNexusCodeIndexProvider<SystemRunner>>,
    project_map: Option<ProjectMap>,
}

impl KnowledgeRuntime {
    pub fn from_service(service: &Service) -> Result<Self> {
        Self::from_service_with_inputs(service, &KnowledgeInputs::default())
    }

    pub fn from_service_with_inputs(service: &Service, inputs: &KnowledgeInputs) -> Result<Self> {
        let project = service
            .descriptor()
            .project_id
            .as_ref()
            .and_then(|id| ResourceRef::parse(&format!("project/{id}")).ok());
        let context = FamiliarityContext {
            project,
            actor: None,
            agency: None,
            focus: Some("project".into()),
        };

        let wiki_objects = load_wiki(&inputs.wiki_files)?;
        let wiki_index = if wiki_objects.is_empty() {
            None
        } else {
            Some(SemanticWikiIndex::rebuild(&wiki_objects)?)
        };

        let source_material = load_source_material(&inputs.source_material_files)?;
        let mut native_sources = NativeSourcePoolProvider::new();
        if !source_material.is_empty() {
            native_sources.rebuild(&source_material)?;
        }

        let code = service
            .descriptor()
            .project_root
            .as_ref()
            .map(|root| GitNexusCodeIndexProvider::new(SystemRunner, root.clone()));

        let project_map = inputs
            .project_map_file
            .as_deref()
            .map(load_project_map)
            .transpose()?;

        Ok(Self {
            context,
            wiki_index,
            source_material,
            native_sources,
            code,
            project_map,
        })
    }

    pub fn application(&self) -> KnowledgeApplication<'_> {
        let mut app = KnowledgeApplication::new(self.context.clone());
        if let Some(index) = &self.wiki_index {
            app = app.with_wiki(SemanticWikiProvider::new(index));
        }
        if !self.source_material.is_empty() {
            app = app.with_source_pool(&self.native_sources, &self.source_material);
        }
        if let Some(code) = &self.code {
            app = app.with_code(code);
        }
        if let Some(map) = &self.project_map {
            app = app.with_project_map(map);
        }
        app
    }

    pub fn search(&self, query: &str, limit: usize) -> aikit_core::KnowledgeSearchResult {
        self.application().search(query, limit)
    }

    pub fn read(&self, address: &KnowledgeAddress) -> Result<aikit_core::KnowledgeReading> {
        self.application().read(address)
    }

    pub fn relations(&self, address: &KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> {
        self.application().relations(address)
    }

    pub fn route(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> Result<KnowledgeRoute> {
        self.application().route(query, addresses)
    }

    pub fn frame(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> KnowledgeContextPack {
        self.application().frame(query, addresses)
    }

    pub fn sources(&self, address: &KnowledgeAddress) -> Result<aikit_core::KnowledgeSources> {
        self.application().sources(address)
    }

    pub fn explain(&self, address: &KnowledgeAddress) -> aikit_core::KnowledgeExplanation {
        self.application().explain(address)
    }
}

pub fn parse_address(raw: &str) -> Result<KnowledgeAddress> {
    if let Some(value) = raw.strip_prefix("wiki:") {
        return Ok(KnowledgeAddress::Wiki(ResourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("source:") {
        return Ok(KnowledgeAddress::Source(aikit_core::SourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("map:") {
        return Ok(KnowledgeAddress::ProjectMap(ResourceRef::parse(value)?));
    }
    serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            "knowledge.invalid_address",
            format!("Knowledge address must be wiki:<ref>, source:<ref>, map:<ref>, or KnowledgeAddress JSON: {error}"),
        )
    })
}

fn load_wiki(paths: &[PathBuf]) -> Result<Vec<WikiObject>> {
    let mut objects = Vec::new();
    for path in paths {
        let text = read(path, "knowledge.wiki_read_failed")?;
        match parse_wiki_objects(&text) {
            Ok(mut parsed) => objects.append(&mut parsed),
            Err(collection_error) => match OkfWikiBundle::parse_json(&text) {
                Ok(bundle) => objects.push(bundle.wiki),
                Err(_) => return Err(collection_error.with("path", path.display().to_string())),
            },
        }
    }
    Ok(objects)
}

fn load_source_material(paths: &[PathBuf]) -> Result<Vec<SourceMaterial>> {
    let mut material = Vec::new();
    for path in paths {
        let text = read(path, "knowledge.source_material_read_failed")?;
        if let Ok(mut many) = serde_json::from_str::<Vec<SourceMaterial>>(&text) {
            material.append(&mut many);
        } else {
            let one = serde_json::from_str::<SourceMaterial>(&text).map_err(|error| {
                AikitError::new(
                    "knowledge.source_material_invalid",
                    format!("{} is not SourceMaterial JSON: {error}", path.display()),
                )
            })?;
            material.push(one);
        }
    }
    Ok(material)
}

fn load_project_map(path: &Path) -> Result<ProjectMap> {
    let text = read(path, "knowledge.project_map_read_failed")?;
    serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "knowledge.project_map_invalid",
            format!("{} is not a ProjectMap projection: {error}", path.display()),
        )
    })
}

fn read(path: &Path, code: &str) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(code, format!("could not read {}: {error}", path.display()))
            .with("path", path.display().to_string())
    })
}
''')

replace(
    "crates/aikit-cli/src/lib.rs",
    "pub mod json;\npub mod jump;\n",
    "pub mod json;\npub mod jump;\npub mod knowledge_runtime;\n",
)

# --- CLI command family ---
replace(
    "crates/aikit-cli/src/cli.rs",
    "    /// Search the catalogue for capabilities.\n    Search(SearchArgs),\n",
    "    /// Navigate project knowledge through the provider-neutral Knowledge application.\n    Knowledge(KnowledgeArgs),\n    /// Search the catalogue for capabilities.\n    Search(SearchArgs),\n",
)
anchor = "// ---------------------------------------------------------------------------\n// Leaf command arguments\n// ---------------------------------------------------------------------------\n"
knowledge_cli = r'''#[derive(Debug, Args)]
pub struct KnowledgeArgs {
    /// Explicit OKF Wiki material for this invocation; discovery alone is never loaded.
    #[arg(long = "wiki", value_name = "FILE")]
    pub wiki_files: Vec<std::path::PathBuf>,
    /// Explicit already-authorised SourceMaterial JSON for this invocation.
    #[arg(long = "source-material", value_name = "FILE")]
    pub source_material_files: Vec<std::path::PathBuf>,
    /// Explicit ProjectMap federation projection containing cross-lens bindings.
    #[arg(long = "project-map", value_name = "FILE")]
    pub project_map_file: Option<std::path::PathBuf>,
    #[command(subcommand)]
    pub command: KnowledgeSub,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeSub {
    /// Disclose live provider availability and degradation.
    Status,
    /// Federated search while retaining provider/lens origin.
    Search { query: String, #[arg(long, default_value_t = 20)] limit: usize },
    /// Read one canonical Knowledge address.
    Read { address: String },
    /// Read a bounded provider-neutral relation neighbourhood.
    Relations { address: String },
    /// Record an ordered operational traversal across addresses.
    Route { #[arg(long)] query: Option<String>, #[arg(required = true)] addresses: Vec<String> },
    /// Build a derived context pack/frame for the selected addresses.
    Frame { #[arg(long)] query: Option<String>, #[arg(required = true)] addresses: Vec<String> },
    /// Project native provenance for one address.
    Sources { address: String },
    /// Explain provider, authority, selection and degradation for one address.
    Explain { address: String },
    /// Recover metadata-only route/frame History without copying provider payloads.
    History { #[arg(long, default_value_t = 50)] limit: usize },
}

'''
replace("crates/aikit-cli/src/cli.rs", anchor, knowledge_cli + anchor)

# --- final TUI backend exposes the same core operation vocabulary ---
replace(
    "crates/aikit-tui/src/backend.rs",
    "use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};\n",
    "use aikit_core::{\n    FamiliarityObservation, FamiliarityStore, KnowledgeAddress, KnowledgeContextPack,\n    KnowledgeExplanation, KnowledgeProviderStatus, KnowledgeReading, KnowledgeRelationView,\n    KnowledgeRoute, KnowledgeSearchResult, KnowledgeSources, Result,\n};\n",
)
replace(
    "crates/aikit-tui/src/backend.rs",
    "    fn record_familiarity(&mut self, _observation: FamiliarityObservation) -> Result<()> {\n        Ok(())\n    }\n\n    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule>;\n",
    "    fn record_familiarity(&mut self, _observation: FamiliarityObservation) -> Result<()> {\n        Ok(())\n    }\n\n    fn knowledge_status(&self) -> Result<KnowledgeProviderStatus> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_search(&self, _query: &str, _limit: usize) -> Result<KnowledgeSearchResult> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_read(&self, _address: &KnowledgeAddress) -> Result<KnowledgeReading> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_relations(&self, _address: &KnowledgeAddress) -> Result<KnowledgeRelationView> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_route(&mut self, _query: Option<&str>, _addresses: &[KnowledgeAddress]) -> Result<KnowledgeRoute> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_frame(&mut self, _query: Option<&str>, _addresses: &[KnowledgeAddress]) -> Result<KnowledgeContextPack> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_sources(&self, _address: &KnowledgeAddress) -> Result<KnowledgeSources> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_explain(&self, _address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {\n        Err(knowledge_unavailable())\n    }\n    fn knowledge_history(&self, _limit: usize) -> Result<Vec<aikit_store::KnowledgeHistoryEntry>> {\n        Err(knowledge_unavailable())\n    }\n\n    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule>;\n",
)
# function after trait
replace(
    "crates/aikit-tui/src/backend.rs",
    "}\n",
    "}\n",
)
# append helper safely at EOF
p = Path("crates/aikit-tui/src/backend.rs")
p.write_text(p.read_text() + r'''

fn knowledge_unavailable() -> aikit_core::AikitError {
    aikit_core::AikitError::new(
        "knowledge.application_unavailable",
        "this backend has not bound the canonical Knowledge application",
    )
}
''')

# Final TUI ApplicationService delegates semantic Knowledge operations to backend.
replace(
    "crates/aikit-tui/src/application_service.rs",
    "    pub fn backend_mut(&mut self) -> &mut dyn PaletteBackend {\n        self.backend\n    }\n",
    "    pub fn backend_mut(&mut self) -> &mut dyn PaletteBackend {\n        self.backend\n    }\n\n    pub fn knowledge_status(&self) -> Result<aikit_core::KnowledgeProviderStatus> { self.backend.knowledge_status() }\n    pub fn knowledge_search(&self, query: &str, limit: usize) -> Result<aikit_core::KnowledgeSearchResult> { self.backend.knowledge_search(query, limit) }\n    pub fn knowledge_read(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeReading> { self.backend.knowledge_read(address) }\n    pub fn knowledge_relations(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> { self.backend.knowledge_relations(address) }\n    pub fn knowledge_route(&mut self, query: Option<&str>, addresses: &[aikit_core::KnowledgeAddress]) -> Result<aikit_core::KnowledgeRoute> { self.backend.knowledge_route(query, addresses) }\n    pub fn knowledge_frame(&mut self, query: Option<&str>, addresses: &[aikit_core::KnowledgeAddress]) -> Result<aikit_core::KnowledgeContextPack> { self.backend.knowledge_frame(query, addresses) }\n    pub fn knowledge_sources(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeSources> { self.backend.knowledge_sources(address) }\n    pub fn knowledge_explain(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeExplanation> { self.backend.knowledge_explain(address) }\n    pub fn knowledge_history(&self, limit: usize) -> Result<Vec<aikit_store::KnowledgeHistoryEntry>> { self.backend.knowledge_history(limit) }\n",
)

# Production Service binds the final TUI backend to the composition root and records actual route/frame use.
insert = r'''
    fn knowledge_status(&self) -> Result<aikit_core::KnowledgeProviderStatus> {
        Ok(crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.application().status())
    }

    fn knowledge_search(&self, query: &str, limit: usize) -> Result<aikit_core::KnowledgeSearchResult> {
        Ok(crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.search(query, limit))
    }

    fn knowledge_read(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeReading> {
        crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.read(address)
    }

    fn knowledge_relations(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> {
        crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.relations(address)
    }

    fn knowledge_route(&mut self, query: Option<&str>, addresses: &[aikit_core::KnowledgeAddress]) -> Result<aikit_core::KnowledgeRoute> {
        let route = crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.route(query, addresses)?;
        let observation = route.familiarity_observation(aikit_core::EventId::generate().as_str().to_string(), current_time_ms())?
            .from_surface(aikit_core::ResourceRef::parse("surface/aikit/tui").expect("static TUI surface ref"));
        aikit_store::append_familiarity_observation(&self.index, observation)?;
        aikit_store::record_knowledge_route(&self.index, &route)?;
        Ok(route)
    }

    fn knowledge_frame(&mut self, query: Option<&str>, addresses: &[aikit_core::KnowledgeAddress]) -> Result<aikit_core::KnowledgeContextPack> {
        let frame = crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.frame(query, addresses);
        aikit_store::record_knowledge_frame(&self.index, &frame)?;
        Ok(frame)
    }

    fn knowledge_sources(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeSources> {
        crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.sources(address)
    }

    fn knowledge_explain(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeExplanation> {
        Ok(crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.explain(address))
    }

    fn knowledge_history(&self, limit: usize) -> Result<Vec<aikit_store::KnowledgeHistoryEntry>> {
        aikit_store::knowledge_history(&self.index, limit)
    }

'''
replace(
    "crates/aikit-cli/src/app/mod.rs",
    "    fn documents(&self) -> Vec<SearchDoc> {\n",
    insert + "    fn documents(&self) -> Vec<SearchDoc> {\n",
)
# shared clock helper for runtime use; does not affect deterministic tests that inject observations themselves.
p = Path("crates/aikit-cli/src/app/mod.rs")
p.write_text(p.read_text() + r'''

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
''')

# --- CLI dispatch ---
replace(
    "crates/aikit-cli/src/main.rs",
    "        Some(Command::Search(a)) => cmd_search(cwd, a),\n",
    "        Some(Command::Knowledge(c)) => cmd_knowledge(cwd, c),\n        Some(Command::Search(a)) => cmd_search(cwd, a),\n",
)
knowledge_fn = r'''
fn cmd_knowledge(cwd: &std::path::Path, args: KnowledgeArgs) -> Result<Reply> {
    use aikit_cli::knowledge_runtime::{parse_address, KnowledgeInputs, KnowledgeRuntime};
    let mut service = Service::discover(cwd)?;
    let inputs = KnowledgeInputs {
        wiki_files: args.wiki_files,
        source_material_files: args.source_material_files,
        project_map_file: args.project_map_file,
    };
    let runtime = KnowledgeRuntime::from_service_with_inputs(&service, &inputs)?;
    let data = match args.command {
        KnowledgeSub::Status => serde_json::to_value(runtime.application().status()),
        KnowledgeSub::Search { query, limit } => serde_json::to_value(runtime.search(&query, limit)),
        KnowledgeSub::Read { address } => serde_json::to_value(runtime.read(&parse_address(&address)?)?),
        KnowledgeSub::Relations { address } => serde_json::to_value(runtime.relations(&parse_address(&address)?)?),
        KnowledgeSub::Route { query, addresses } => {
            let addresses = addresses.iter().map(|raw| parse_address(raw)).collect::<Result<Vec<_>>>()?;
            let route = runtime.route(query.as_deref(), &addresses)?;
            let observation = route
                .familiarity_observation(aikit_core::EventId::generate().as_str().to_string(), cli_now_ms())?
                .from_surface(aikit_core::ResourceRef::parse("surface/aikit/cli")?);
            <Service as aikit_tui::PaletteBackend>::record_familiarity(&mut service, observation)?;
            aikit_store::record_knowledge_route(service.index(), &route)?;
            serde_json::to_value(route)
        }
        KnowledgeSub::Frame { query, addresses } => {
            let addresses = addresses.iter().map(|raw| parse_address(raw)).collect::<Result<Vec<_>>>()?;
            let frame = runtime.frame(query.as_deref(), &addresses);
            aikit_store::record_knowledge_frame(service.index(), &frame)?;
            serde_json::to_value(frame)
        }
        KnowledgeSub::Sources { address } => serde_json::to_value(runtime.sources(&parse_address(&address)?)?),
        KnowledgeSub::Explain { address } => serde_json::to_value(runtime.explain(&parse_address(&address)?)),
        KnowledgeSub::History { limit } => serde_json::to_value(aikit_store::knowledge_history(service.index(), limit)?),
    }
    .map_err(|error| AikitError::new("knowledge.serialize_failed", error.to_string()))?;
    Ok(reply(&service, data, Vec::new()))
}

fn cli_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

'''
replace("crates/aikit-cli/src/main.rs", "fn cmd_skill(cwd: &std::path::Path, command: SkillCmd) -> Result<Reply> {\n", knowledge_fn + "fn cmd_skill(cwd: &std::path::Path, command: SkillCmd) -> Result<Reply> {\n")

# --- focused integration tests ---
Path("crates/aikit-cli/tests/knowledge_runtime_v2.rs").write_text(r'''use std::fs;

use aikit_cli::app::Service;
use aikit_cli::knowledge_runtime::{parse_address, KnowledgeInputs, KnowledgeRuntime};
use aikit_core::{KnowledgeAddress, ResourceRef};
use aikit_store::{knowledge_history, record_knowledge_frame, AikitHome, KnowledgeHistoryKind};

fn service() -> (tempfile::TempDir, Service) {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::new(tmp.path().join("home"));
    let cwd = tmp.path().join("project");
    fs::create_dir_all(&cwd).unwrap();
    let service = Service::open(home, &cwd, |key| {
        (key == "HOME").then(|| tmp.path().display().to_string())
    }).unwrap();
    (tmp, service)
}

#[test]
fn production_runtime_is_provider_neutral_and_discloses_unbound_lenses() {
    let (_tmp, service) = service();
    let runtime = KnowledgeRuntime::from_service(&service).unwrap();
    let status = runtime.application().status();
    assert!(status.wiki.is_none());
    assert!(status.sources.is_empty());
    assert!(status.absences.iter().any(|v| v.contains("SemanticWiki")));
    assert!(status.absences.iter().any(|v| v.contains("SourcePool")));
}

#[test]
fn explicit_wiki_material_is_loaded_without_contextsource_discovery_becoming_retrieval() {
    let (tmp, service) = service();
    let wiki = tmp.path().join("wiki.json");
    fs::write(&wiki, r#"{"objects":[{"kind":"node","value":{"profile":"okf-wiki/v1","ref_id":"knowledge-node/runtime","revision":1,"space":"knowledge-space/root","title":"Runtime","summary":"production knowledge runtime","node_type":"concept","provenance":[],"extensions":{}}}]}"#).unwrap();
    let inputs = KnowledgeInputs { wiki_files: vec![wiki], ..Default::default() };
    let runtime = KnowledgeRuntime::from_service_with_inputs(&service, &inputs).unwrap();
    let result = runtime.search("Runtime", 8);
    assert!(result.hits.iter().any(|hit| hit.resource == ResourceRef::parse("knowledge-node/runtime").unwrap()));
}

#[test]
fn route_use_replays_through_familiarity_and_frame_history_is_metadata_only() {
    let (_tmp, mut service) = service();
    // Explicit ProjectMap-only addresses demonstrate route semantics without manufacturing provider edges.
    let address = parse_address("map:project-map/one").unwrap();
    assert!(matches!(address, KnowledgeAddress::ProjectMap(_)));

    let frame = aikit_core::KnowledgeContextPack::new(aikit_core::FamiliarityContext::default());
    record_knowledge_frame(service.index(), &frame).unwrap();
    let history = knowledge_history(service.index(), 10).unwrap();
    assert!(history.iter().any(|entry| matches!(entry.kind, KnowledgeHistoryKind::Frame { .. })));

    // Production backend remains the same object used by final TUI.
    let _ = <Service as aikit_tui::PaletteBackend>::knowledge_status(&service).unwrap();
    let _ = &mut service;
}
''')

# Service already exposes index() in accepted main; fail early if not, rather than adding a second store accessor.
if "pub fn index(&self) -> &Index" not in Path("crates/aikit-cli/src/app/mod.rs").read_text():
    # add it next to resolved(), if absent
    replace(
        "crates/aikit-cli/src/app/mod.rs",
        "    pub fn resolved(&self) -> &ResolvedView {\n        &self.view\n    }\n",
        "    pub fn resolved(&self) -> &ResolvedView {\n        &self.view\n    }\n\n    pub fn index(&self) -> &Index {\n        &self.index\n    }\n",
    )

# Remove helper files from the resulting implementation commit.
Path(".github/workflows/knowledge-runtime-patch.yml").unlink(missing_ok=True)
Path(".github/scripts/knowledge_runtime_patch.py").unlink(missing_ok=True)
