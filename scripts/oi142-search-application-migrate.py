#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    source = target.read_text()
    if source.count(old) < count:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    target.write_text(source.replace(old, new, count))


application = "crates/aikit-tui/src/application.rs"
patch(
    application,
    '''use aikit_core::resource::{\n    search_contextual_actions, ContextualActionDescriptor, ResourceKind, ResourceRef,\n};\n''',
    '''use aikit_core::resource::{\n    search_contextual_actions, ContextualActionDescriptor, ResolveExpression, ResolvePath,\n    ResourceKind, ResourceRef,\n};\n''',
)
patch(
    application,
    '''impl ResourceListReadModel {\n    pub fn contains(&self, resource: &ResourceRef) -> bool {\n''',
    '''/// One human/Agent Search projection of the same typed Resolve operation.\n/// The list is presentation; `expression` and `path` are the semantic request and\n/// its actual canonical-Ref traversal.\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ResolvedSearchReadModel {\n    pub expression: ResolveExpression,\n    pub path: ResolvePath,\n    pub resources: ResourceListReadModel,\n}\n\nimpl ResourceListReadModel {\n    pub fn contains(&self, resource: &ResourceRef) -> bool {\n''',
)

service = "crates/aikit-tui/src/application_service.rs"
patch(
    service,
    '''use aikit_core::resource::{\n    ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass, ResourceIndex,\n    ResourceKind, ResourceRef, ResourceSearchIndex,\n};\n''',
    '''use aikit_core::resource::{\n    parse_or_search_expression, resolve_expression, ContextualActionDescriptor,\n    NavigationEvidence, NavigationEvidenceClass, ResolveExpression, ResourceDescriptor,\n    ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,\n};\n''',
)
patch(
    service,
    '''    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,\n    RelationReadModel, ResourceListItem, ResourceListReadModel, StagedChanges,\n    TuiApplicationService,\n};\n''',
    '''    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,\n    RelationReadModel, ResolvedSearchReadModel, ResourceListItem, ResourceListReadModel,\n    StagedChanges, TuiApplicationService,\n};\n''',
)
patch(
    service,
    '''    /// Resolve the deliberately retained package compatibility identity for a\n''',
    '''    /// Resolve Search once against the canonical heterogeneous Resource field.\n    /// Deep Knowledge providers may contribute canonical refs for subject terms\n    /// before the expression is evaluated; they do not receive punctuation as a\n    /// fake prose query and they do not mint a second path identity.\n    pub fn resolve_search(&self, query: &str) -> Result<ResolvedSearchReadModel> {\n        let mut index = self.navigation_index()?;\n        let expression = parse_or_search_expression(query)?;\n\n        for subject in resolve_subjects(&expression) {\n            if subject.trim().is_empty() {\n                continue;\n            }\n            if let Some(knowledge) = self.backend.knowledge_search(subject, 256)? {\n                for hit in knowledge.hits {\n                    if ResourceIndex::resource(&index, &hit.resource).is_some() {\n                        continue;\n                    }\n                    let mut descriptor = ResourceDescriptor::new(\n                        hit.resource.clone(),\n                        hit.kind,\n                        hit.label,\n                        hit.snippet,\n                    );\n                    descriptor\n                        .annotations\n                        .insert("knowledge.provider".into(), hit.provider.to_string());\n                    descriptor.annotations.insert(\n                        "knowledge.authority".into(),\n                        format!("{:?}", hit.authority),\n                    );\n                    index.insert_resource(ResourceRecord::new(descriptor), Vec::new());\n                }\n            }\n        }\n\n        let path = resolve_expression(&expression, &index, 256);\n        let resources = path\n            .candidates\n            .iter()\n            .filter_map(|candidate| {\n                let record = ResourceIndex::resource(&index, &candidate.resource)?;\n                let evidence = index\n                    .search(candidate.resource.as_str(), 256)\n                    .into_iter()\n                    .find(|hit| hit.resource == candidate.resource)\n                    .map(|hit| hit.navigation_evidence)\n                    .unwrap_or_default();\n                Some(ResourceListItem {\n                    resource: candidate.resource.clone(),\n                    kind: candidate.kind,\n                    label: record.descriptor.name.clone(),\n                    summary: summary_with_navigation_evidence(\n                        record.descriptor.description.clone(),\n                        &evidence,\n                    ),\n                })\n            })\n            .collect::<Vec<_>>();\n        let revision = format!(\n            "aikit.resolve-search/v1:{}:{}:{}",\n            self.backend.view().catalog_revision,\n            self.backend.view().hash,\n            path.identity\n        );\n\n        Ok(ResolvedSearchReadModel {\n            expression,\n            path,\n            resources: ResourceListReadModel {\n                revision,\n                resources,\n            },\n        })\n    }\n\n    /// Resolve the deliberately retained package compatibility identity for a\n''',
)
# Replace the old split shallow/deep implementation: the concrete service now
# materialises one field and returns the presentation part of the same Resolve.
old_search = '''    fn search(&self, query: &str) -> Result<ResourceListReadModel> {\n        let index = self.navigation_index()?;\n        let mut resources = index\n            .search(query, 256)\n            .into_iter()\n            .map(|hit| ResourceListItem {\n                resource: hit.resource,\n                kind: hit.kind,\n                label: hit.label,\n                summary: summary_with_navigation_evidence(hit.summary, &hit.navigation_evidence),\n            })\n            .collect::<Vec<_>>();\n        if let Some(knowledge) = self.backend.knowledge_search(query, 256)? {\n            for hit in knowledge.hits {\n                if resources.iter().any(|item| item.resource == hit.resource) {\n                    continue;\n                }\n                let provider = hit.provider.to_string();\n                resources.push(ResourceListItem {\n                    resource: hit.resource,\n                    kind: hit.kind,\n                    label: hit.label,\n                    summary: format!("{} · {provider} · {:?}", hit.snippet, hit.authority),\n                });\n            }\n        }\n        Ok(ResourceListReadModel {\n            revision: format!(\n                "aikit.resource-search/v2:{}:{}:{}",\n                self.backend.view().catalog_revision,\n                self.backend.view().hash,\n                query\n            ),\n            resources,\n        })\n    }\n'''
new_search = '''    fn search(&self, query: &str) -> Result<ResourceListReadModel> {\n        Ok(self.resolve_search(query)?.resources)\n    }\n'''
patch(service, old_search, new_search)
# Local recursive subject extraction lets providers receive semantic subject text
# while punctuation remains owned by the typed expression.
patch(
    service,
    '''fn familiarity_context(context: &aikit_core::ContextDescriptor) -> FamiliarityContext {\n''',
    '''fn resolve_subjects(expression: &ResolveExpression) -> Vec<&str> {\n    let mut subjects = Vec::new();\n    collect_resolve_subjects(expression, &mut subjects);\n    subjects.sort_unstable();\n    subjects.dedup();\n    subjects\n}\n\nfn collect_resolve_subjects<'a>(expression: &'a ResolveExpression, subjects: &mut Vec<&'a str>) {\n    match expression {\n        ResolveExpression::Subject { value } => subjects.push(value.as_str()),\n        ResolveExpression::Address { expression, .. }\n        | ResolveExpression::Unary { expression, .. }\n        | ResolveExpression::Frame { expression } => {\n            collect_resolve_subjects(expression, subjects);\n        }\n        ResolveExpression::Binary { left, right, .. } => {\n            collect_resolve_subjects(left, subjects);\n            collect_resolve_subjects(right, subjects);\n        }\n    }\n}\n\nfn familiarity_context(context: &aikit_core::ContextDescriptor) -> FamiliarityContext {\n''',
)

cli = "crates/aikit-cli/src/main.rs"
patch(
    cli,
    '''fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {\n    use aikit_cli::app::SearchRequest;\n    let service = Service::discover(cwd)?;\n    let results = service.search(SearchRequest {\n        query: a.query,\n        limit: a.limit,\n    })?;\n    let rows: Vec<Value> = results\n        .rows\n        .iter()\n        .map(|r| {\n            jval!({\n                "id": r.id.to_string(),\n                "name": r.name,\n                "kind": r.kind.as_str(),\n                "active": r.active,\n                "runnable": r.runnable,\n            })\n        })\n        .collect();\n    Ok(reply(&service, jval!({ "rows": rows }), results.warnings))\n}\n''',
    '''fn cmd_search(cwd: &std::path::Path, a: SearchArgs) -> Result<Reply> {\n    let mut service = Service::discover(cwd)?;\n    let resolved = {\n        let application = ApplicationService::new(&mut service);\n        application.resolve_search(&a.query)?\n    };\n    let rows: Vec<Value> = resolved\n        .resources\n        .resources\n        .iter()\n        .take(a.limit)\n        .map(|row| {\n            let capsule = CapsuleId::parse(row.resource.as_str()).ok();\n            let package = capsule\n                .as_ref()\n                .and_then(|id| service.resolved().catalog_index.get(id));\n            jval!({\n                "id": row.resource.to_string(),\n                "name": row.label,\n                "kind": package.map(|entry| entry.kind.as_str()).unwrap_or(row.kind.as_str()),\n                "resource_kind": row.kind.as_str(),\n                "summary": row.summary,\n                "active": capsule.as_ref().is_some_and(|id| service.resolved().is_active(id)),\n                "runnable": capsule.as_ref().is_some_and(|id| service.resolved().can_run(id)),\n            })\n        })\n        .collect();\n    Ok(reply(\n        &service,\n        jval!({\n            "expression": resolved.expression,\n            "path": resolved.path,\n            "rows": rows,\n        }),\n        diagnostic_warnings(&service),\n    ))\n}\n''',
)
