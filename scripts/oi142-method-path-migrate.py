#!/usr/bin/env python3
from pathlib import Path

# acceptance trigger: Method authored expected Resolve seam, 2026-09-01


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    source = p.read_text()
    if source.count(old) < count:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    p.write_text(source.replace(old, new, count))


method = "crates/aikit-core/src/method.rs"
patch(
    method,
    "use crate::resource::{ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, SourceRef, SourceRevision};\n",
    "use crate::resource::{\n    ResolveExpression, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, SourceRef,\n    SourceRevision,\n};\n",
)
patch(
    method,
    '''    #[serde(default)]\n    pub verification: Vec<ResourceRef>,\n    #[serde(default)]\n    pub expected_return_forms: Vec<String>,\n''',
    '''    #[serde(default)]\n    pub verification: Vec<ResourceRef>,\n    /// Source-authored semantic movement this Method expects to be useful. This is\n    /// an intention/pattern only: actual Invocation/Activity may return a different\n    /// observed ResolvePath, which remains observed evidence rather than Method truth.\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub expected_resolve: Option<ResolveExpression>,\n    #[serde(default)]\n    pub expected_return_forms: Vec<String>,\n''',
)
patch(
    method,
    '''        if let Some(revision) = &self.revision {\n            descriptor\n                .annotations\n                .insert("method.revision".into(), revision.to_string());\n        }\n        ResourceRecord::new(descriptor)\n''',
    '''        if let Some(revision) = &self.revision {\n            descriptor\n                .annotations\n                .insert("method.revision".into(), revision.to_string());\n        }\n        if let Some(expected) = &self.expected_resolve {\n            descriptor\n                .annotations\n                .insert("method.expected-resolve".into(), expected.render());\n        }\n        ResourceRecord::new(descriptor)\n''',
)
patch(
    method,
    '''    pub verification: Vec<MethodResolvedRef>,\n    pub overlays: Vec<UsageOverlayRef>,\n    pub expected_return_forms: Vec<String>,\n''',
    '''    pub verification: Vec<MethodResolvedRef>,\n    pub overlays: Vec<UsageOverlayRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub expected_resolve: Option<ResolveExpression>,\n    pub expected_return_forms: Vec<String>,\n''',
)
patch(
    method,
    '''        overlays: method\n            .skills\n            .iter()\n            .filter_map(|value| value.usage_overlay.clone())\n            .collect(),\n        expected_return_forms: method.expected_return_forms.clone(),\n''',
    '''        overlays: method\n            .skills\n            .iter()\n            .filter_map(|value| value.usage_overlay.clone())\n            .collect(),\n        expected_resolve: method.expected_resolve.clone(),\n        expected_return_forms: method.expected_return_forms.clone(),\n''',
)
# Two Method literals in this file.
patch(
    method,
    '''            verification: vec![ResourceRef::parse("action:verify").unwrap()],\n            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],\n''',
    '''            verification: vec![ResourceRef::parse("action:verify").unwrap()],\n            expected_resolve: Some(\n                crate::resource::parse_resolve_expression(\n                    "@0 context:project-ground x @5 action:verify",\n                )\n                .unwrap(),\n            ),\n            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],\n''',
)
patch(
    method,
    '''            verification: vec![],\n            expected_return_forms: vec![],\n''',
    '''            verification: vec![],\n            expected_resolve: None,\n            expected_return_forms: vec![],\n''',
)
patch(
    method,
    '''        assert_eq!(resolved.expected_return_forms.len(), 2);\n        assert_eq!(method.resource_record().descriptor.kind, ResourceKind::Method);\n''',
    '''        assert_eq!(resolved.expected_return_forms.len(), 2);\n        assert_eq!(\n            resolved.expected_resolve.as_ref().map(ResolveExpression::render),\n            Some("@0 context:project-ground x @5 action:verify".into())\n        );\n        let record = method.resource_record();\n        assert_eq!(record.descriptor.kind, ResourceKind::Method);\n        assert_eq!(\n            record.descriptor.annotations.get("method.expected-resolve"),\n            Some(&"@0 context:project-ground x @5 action:verify".into())\n        );\n''',
)

praxis = "crates/aikit-core/src/praxis.rs"
patch(
    praxis,
    '''            for reference in resolved_refs(resolution) {\n''',
    '''            if let Some(expected) = &resolution.expected_resolve {\n                facts.push(ExplainFact {\n                    relation: "expected-resolve".into(),\n                    authority: Some(SourceAuthority::Authored),\n                    summary: format!("Method expects Resolve pattern {}", expected.render()),\n                    canonical_refs: Vec::new(),\n                    provenance: vec![EvidenceProvenance {\n                        source: source_ref.clone(),\n                        revision: resolution.revision.as_ref().map(ToString::to_string),\n                        ..EvidenceProvenance::default()\n                    }],\n                });\n            }\n\n            for reference in resolved_refs(resolution) {\n''',
)
patch(
    praxis,
    '''            details.insert(\n                "expectedReturns".into(),\n                resolution.expected_return_forms.join(","),\n            );\n''',
    '''            details.insert(\n                "expectedResolve".into(),\n                resolution\n                    .expected_resolve\n                    .as_ref()\n                    .map(|expected| expected.render())\n                    .unwrap_or_default(),\n            );\n            details.insert(\n                "expectedReturns".into(),\n                resolution.expected_return_forms.join(","),\n            );\n''',
)
# Two Method literals in praxis tests.
patch(
    praxis,
    '''            verification: vec![],\n            expected_return_forms: vec!["evidence".into()],\n''',
    '''            verification: vec![],\n            expected_resolve: None,\n            expected_return_forms: vec!["evidence".into()],\n''',
)
patch(
    praxis,
    '''            verification: vec![],\n            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],\n''',
    '''            verification: vec![],\n            expected_resolve: Some(\n                crate::resource::parse_resolve_expression(\n                    "@0 context:ground x @5 cap:wayfinder",\n                )\n                .unwrap(),\n            ),\n            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],\n''',
)
patch(
    praxis,
    '''        assert!(explained[0]\n            .facts\n            .iter()\n            .any(|fact| fact.relation == "usage-overlay" && fact.summary.contains(&"a".repeat(64))));\n''',
    '''        assert!(explained[0]\n            .facts\n            .iter()\n            .any(|fact| fact.relation == "usage-overlay" && fact.summary.contains(&"a".repeat(64))));\n        assert!(explained[0].facts.iter().any(|fact| {\n            fact.relation == "expected-resolve"\n                && fact.authority == Some(SourceAuthority::Authored)\n                && fact.summary.contains("@0 context:ground x @5 cap:wayfinder")\n        }));\n''',
)
patch(
    praxis,
    '''        assert_eq!(\n            history[0].details.get("contextResolutionVersion"),\n            Some(&context.version)\n        );\n''',
    '''        assert_eq!(\n            history[0].details.get("contextResolutionVersion"),\n            Some(&context.version)\n        );\n        assert_eq!(\n            history[0].details.get("expectedResolve"),\n            Some(&"@0 context:ground x @5 cap:wayfinder".into())\n        );\n''',
)
