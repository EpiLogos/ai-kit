from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f'anchor missing in {path}: {old[:150]!r}')
    p.write_text(text.replace(old, new, count))

# TUI semantic service: expose the same Knowledge family, source-compatible by default.
replace(
    'crates/aikit-tui/src/application.rs',
    'use aikit_core::Result;',
    '''use aikit_core::{
    ForgetScope, KnowledgeAddress, KnowledgeContextPack, KnowledgeProviderStatus, KnowledgeReading,
    KnowledgeRoute, KnowledgeSources, Result,
};''',
)
replace(
    'crates/aikit-tui/src/application.rs',
    '    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel>;\n\n    /// Record that the actor actually traversed/opened this Resource.',
    '''    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel>;

    /// Rich Knowledge operations are part of the application faculty, not renderer
    /// semantics. Minimal services may return None; the production service exposes
    /// the same materialised runtime used by CLI and final TUI search/read/explain.
    fn knowledge_read(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
        Ok(None)
    }

    fn knowledge_route(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeRoute>> {
        Ok(None)
    }

    fn knowledge_frame(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeContextPack>> {
        Ok(None)
    }

    fn knowledge_sources(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeSources>> {
        Ok(None)
    }

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        Ok(None)
    }

    fn knowledge_forget(&mut self, _scope: ForgetScope) -> Result<bool> {
        Ok(false)
    }

    /// Record that the actor actually traversed/opened this Resource.''',
)

# Final TUI application service consumes rich Knowledge from the same backend.
replace(
    'crates/aikit-tui/src/application_service.rs',
    '    AikitError, FamiliarityContext, FamiliarityObservation, FamiliarityUse, Result,\n    DEFAULT_FAMILIARITY_HALF_LIFE_MS,\n};',
    '''    AikitError, FamiliarityContext, FamiliarityObservation, FamiliarityUse, ForgetScope,
    KnowledgeAddress, KnowledgeContextPack, KnowledgeProviderStatus, KnowledgeReading,
    KnowledgeRoute, KnowledgeSources, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};
use aikit_store::KnowledgeHistoryOperation;''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '''        let resources = index
            .search(query, 256)
            .into_iter()
            .map(|hit| ResourceListItem {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: summary_with_navigation_evidence(hit.summary, &hit.navigation_evidence),
            })
            .collect();''',
    '''        let mut resources = index
            .search(query, 256)
            .into_iter()
            .map(|hit| ResourceListItem {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: summary_with_navigation_evidence(hit.summary, &hit.navigation_evidence),
            })
            .collect::<Vec<_>>();
        if let Some(knowledge) = self.backend.knowledge_search(query, 256)? {
            for hit in knowledge.hits {
                if resources.iter().any(|item| item.resource == hit.resource) {
                    continue;
                }
                let provider = hit
                    .provider
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "provider-neutral".into());
                resources.push(ResourceListItem {
                    resource: hit.resource,
                    kind: hit.kind,
                    label: hit.label,
                    summary: format!("{} · {provider} · {:?}", hit.snippet, hit.authority),
                });
            }
        }''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value> {\n        let index = self.navigation_index()?;',
    '''    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value> {
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(reading) = self.backend.knowledge_read(&address)? {
                return Ok(json!({
                    "resource": resource.as_str(),
                    "knowledgeAddress": address,
                    "reading": reading,
                    "context": to_value(self.backend.context()).map_err(json_error)?,
                    "catalogRevision": self.backend.view().catalog_revision,
                    "resolutionHash": self.backend.view().hash.to_string(),
                }));
            }
        }
        let index = self.navigation_index()?;''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '    fn explain(&self, resource: &ResourceRef) -> Result<Value> {\n        let learned = self.learned_accessibility(resource)?;',
    '''    fn explain(&self, resource: &ResourceRef) -> Result<Value> {
        let learned = self.learned_accessibility(resource)?;
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(explanation) = self.backend.knowledge_explain(&address)? {
                return Ok(json!({
                    "resource": resource.as_str(),
                    "knowledgeAddress": address,
                    "knowledge": explanation,
                    "learnedAccessibility": learned,
                    "catalogRevision": self.backend.view().catalog_revision,
                    "resolutionHash": self.backend.view().hash.to_string(),
                }));
            }
        }''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '        let mut entries = self\n            .backend\n            .recent()',
    '''        let mut entries = self
            .backend
            .knowledge_history(resource)?
            .into_iter()
            .map(|receipt| {
                let summary = match receipt.operation {
                    KnowledgeHistoryOperation::Route => receipt
                        .route
                        .as_ref()
                        .map(|route| {
                            format!(
                                "knowledge route · {} · {} step{}",
                                route.route,
                                route.steps.len(),
                                plural(route.steps.len())
                            )
                        })
                        .unwrap_or_else(|| "knowledge route receipt".into()),
                    KnowledgeHistoryOperation::Frame => receipt
                        .frame
                        .as_ref()
                        .map(|frame| {
                            format!(
                                "knowledge frame · {} reading{} · {} route{} · {} absence{}",
                                frame.readings.len(),
                                plural(frame.readings.len()),
                                frame.routes.len(),
                                plural(frame.routes.len()),
                                frame.absences.len(),
                                plural(frame.absences.len())
                            )
                        })
                        .unwrap_or_else(|| "knowledge frame receipt".into()),
                };
                HistoryEntry {
                    id: receipt.receipt_id,
                    summary,
                }
            })
            .collect::<Vec<_>>();
        entries.extend(self
            .backend
            .recent()''',
)
# Close the newly introduced entries.extend iterator expression.
replace(
    'crates/aikit-tui/src/application_service.rs',
    '''                HistoryEntry {
                    id: format!("recent-{index}"),
                    summary,
                }
            })
            .collect::<Vec<_>>();

        if let Some(store)''',
    '''                HistoryEntry {
                    id: format!("recent-{index}"),
                    summary,
                }
            })
            .collect::<Vec<_>>());

        if let Some(store)''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel> {\n        let index = self.navigation_index()?;',
    '''    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel> {
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(view) = self.backend.knowledge_relations(&address, 2, 256, 512)? {
                return Ok(RelationReadModel {
                    subject: resource.clone(),
                    value: to_value(view).map_err(json_error)?,
                });
            }
        }
        let index = self.navigation_index()?;''',
)
replace(
    'crates/aikit-tui/src/application_service.rs',
    '    fn observe_resource_use(&mut self, resource: &ResourceRef) -> Result<()> {',
    '''    fn knowledge_read(&self, address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
        self.backend.knowledge_read(address)
    }

    fn knowledge_route(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeRoute>> {
        self.backend.knowledge_route(query, addresses)
    }

    fn knowledge_frame(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeContextPack>> {
        self.backend.knowledge_frame(query, addresses)
    }

    fn knowledge_sources(&self, address: &KnowledgeAddress) -> Result<Option<KnowledgeSources>> {
        self.backend.knowledge_sources(address)
    }

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        self.backend.knowledge_status()
    }

    fn knowledge_forget(&mut self, scope: ForgetScope) -> Result<bool> {
        self.backend.knowledge_forget(scope)
    }

    fn observe_resource_use(&mut self, resource: &ResourceRef) -> Result<()> {''',
)

# Named CLI family: same production Service, round-trippable typed addresses.
replace(
    'crates/aikit-cli/src/cli.rs',
    '    /// Search the catalogue for capabilities.\n    Search(SearchArgs),',
    '    /// Search the catalogue for capabilities.\n    Search(SearchArgs),\n    /// Navigate provider-neutral project knowledge through the shared application faculty.\n    Knowledge(KnowledgeCmd),',
)
replace(
    'crates/aikit-cli/src/cli.rs',
    '''#[derive(Debug, Args)]
pub struct StatusArgs {''',
    '''#[derive(Debug, Args)]
pub struct KnowledgeCmd {
    #[command(subcommand)]
    pub command: KnowledgeSub,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeSub {
    Search(KnowledgeSearchArgs),
    Read(KnowledgeAddressArgs),
    Relations(KnowledgeRelationsArgs),
    Route(KnowledgeRouteArgs),
    Frame(KnowledgeRouteArgs),
    Sources(KnowledgeAddressArgs),
    Explain(KnowledgeAddressArgs),
    History(KnowledgeHistoryArgs),
    Status(KnowledgeStatusArgs),
    Forget(KnowledgeForgetCmd),
}

#[derive(Debug, Args)]
pub struct KnowledgeSearchArgs {
    #[arg(value_name = "QUERY", default_value = "")]
    pub query: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeAddressArgs {
    /// Typed address JSON from `knowledge search`, or `wiki=REF`, `source=REF`, `project=REF`.
    #[arg(value_name = "ADDRESS")]
    pub address: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeRelationsArgs {
    #[arg(value_name = "ADDRESS")]
    pub address: String,
    #[arg(long, default_value_t = 2)]
    pub depth: u8,
    #[arg(long, default_value_t = 256)]
    pub max_nodes: usize,
    #[arg(long, default_value_t = 512)]
    pub max_edges: usize,
}

#[derive(Debug, Args)]
pub struct KnowledgeRouteArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(value_name = "ADDRESS", required = true)]
    pub addresses: Vec<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeHistoryArgs {
    #[arg(value_name = "RESOURCE")]
    pub resource: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeStatusArgs {}

#[derive(Debug, Args)]
pub struct KnowledgeForgetCmd {
    #[command(subcommand)]
    pub command: KnowledgeForgetSub,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeForgetSub {
    Destination(KnowledgeForgetResourceArgs),
    Route(KnowledgeForgetResourceArgs),
    Project(KnowledgeForgetResourceArgs),
    All(KnowledgeForgetAllArgs),
}

#[derive(Debug, Args)]
pub struct KnowledgeForgetResourceArgs {
    #[arg(value_name = "RESOURCE")]
    pub resource: String,
}

#[derive(Debug, Args)]
pub struct KnowledgeForgetAllArgs {}

#[derive(Debug, Args)]
pub struct StatusArgs {''',
)

replace(
    'crates/aikit-cli/src/main.rs',
    '        Some(Command::Search(a)) => cmd_search(cwd, a),\n        Some(Command::Status(a)) => cmd_status(cwd, a),',
    '        Some(Command::Search(a)) => cmd_search(cwd, a),\n        Some(Command::Knowledge(c)) => cmd_knowledge(cwd, c),\n        Some(Command::Status(a)) => cmd_status(cwd, a),',
)
replace(
    'crates/aikit-cli/src/main.rs',
    'fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {',
    r'''fn cmd_knowledge(cwd: &std::path::Path, c: KnowledgeCmd) -> Result<Reply> {
    use aikit_core::{ForgetScope, KnowledgeAddress, ResourceRef, SourceRef};

    let mut service = Service::discover(cwd)?;
    let mut warnings = diagnostic_warnings(&service);
    let data = match c.command {
        KnowledgeSub::Search(a) => {
            let result = service.knowledge_search(&a.query, a.limit)?;
            warnings.extend(result.absences.clone());
            jval!(result)
        }
        KnowledgeSub::Read(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_read(&address)?)
        }
        KnowledgeSub::Relations(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_relations(&address, a.depth, a.max_nodes, a.max_edges)?)
        }
        KnowledgeSub::Route(a) => {
            let addresses = a
                .addresses
                .iter()
                .map(|raw| parse_knowledge_address(raw))
                .collect::<Result<Vec<_>>>()?;
            jval!(service.knowledge_route(a.query.as_deref(), &addresses)?)
        }
        KnowledgeSub::Frame(a) => {
            let addresses = a
                .addresses
                .iter()
                .map(|raw| parse_knowledge_address(raw))
                .collect::<Result<Vec<_>>>()?;
            let frame = service.knowledge_frame(a.query.as_deref(), &addresses)?;
            warnings.extend(frame.absences.clone());
            jval!(frame)
        }
        KnowledgeSub::Sources(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_sources(&address)?)
        }
        KnowledgeSub::Explain(a) => {
            let address = parse_knowledge_address(&a.address)?;
            jval!(service.knowledge_explain(&address)?)
        }
        KnowledgeSub::History(a) => {
            let resource = a
                .resource
                .as_deref()
                .map(ResourceRef::parse)
                .transpose()?;
            jval!(service.knowledge_history(resource.as_ref())?)
        }
        KnowledgeSub::Status(_) => {
            let status = service.knowledge_status()?;
            warnings.extend(status.absences.clone());
            jval!(status)
        }
        KnowledgeSub::Forget(a) => {
            let scope = match a.command {
                KnowledgeForgetSub::Destination(a) => {
                    ForgetScope::Destination(ResourceRef::parse(&a.resource)?)
                }
                KnowledgeForgetSub::Route(a) => ForgetScope::Route(ResourceRef::parse(&a.resource)?),
                KnowledgeForgetSub::Project(a) => {
                    ForgetScope::Project(ResourceRef::parse(&a.resource)?)
                }
                KnowledgeForgetSub::All(_) => ForgetScope::All,
            };
            service.knowledge_forget(scope.clone())?;
            jval!({
                "forgot": scope,
                "preserved": ["canonical-resource-identity", "provider-truth", "knowledge-operation-history"]
            })
        }
    };
    Ok(reply(&service, data, warnings))
}

fn parse_knowledge_address(raw: &str) -> Result<aikit_core::KnowledgeAddress> {
    use aikit_core::{KnowledgeAddress, ResourceRef, SourceRef};

    let raw = raw.trim();
    if raw.starts_with('{') {
        return serde_json::from_str(raw).map_err(|error| {
            AikitError::new(
                "knowledge.invalid_address",
                format!("invalid typed Knowledge address JSON: {error}"),
            )
            .with("address", raw)
        });
    }
    if let Some(value) = raw.strip_prefix("wiki=") {
        return Ok(KnowledgeAddress::Wiki(ResourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("source=") {
        return Ok(KnowledgeAddress::Source(SourceRef::parse(value)?));
    }
    if let Some(value) = raw.strip_prefix("project=") {
        return Ok(KnowledgeAddress::ProjectMap(ResourceRef::parse(value)?));
    }
    Err(AikitError::new(
        "knowledge.invalid_address",
        "Knowledge address must be typed JSON from search, or wiki=REF, source=REF, project=REF",
    )
    .with("address", raw))
}

fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {''',
)

# Native skill now names the live surface and the deliberate forget semantics.
replace(
    'skills/registry/capsules/skill/aikit/knowledge-navigation/payload/SKILL.md',
    'Provider status/degradation is an accompanying disclosure surface, not a replacement for any operation above.\n',
    '''Provider status/degradation is an accompanying disclosure surface, not a replacement for any operation above.

The production CLI exposes the same faculty as `aikit knowledge search|read|relations|route|frame|sources|explain|history|status|forget`. Search returns typed addresses that can be passed back as JSON; `wiki=REF`, `source=REF` and `project=REF` are convenience forms for stable address classes.
''',
)
replace(
    'skills/registry/capsules/skill/aikit/knowledge-navigation/payload/SKILL.md',
    '7. Use `history` to recover prior destinations/routes in the same Project/actor/Focus context without manufacturing provider graph history.\n',
    '7. Use `history` to recover durable AIKit-owned route/frame receipts in the same Project/actor/Focus context without manufacturing provider graph history. `forget` resets learned familiarity influence only; it does not erase canonical Resource identity, provider truth or the operation audit trail.\n',
)
