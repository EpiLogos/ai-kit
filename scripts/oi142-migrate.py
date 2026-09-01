#!/usr/bin/env python3
from pathlib import Path


def patch(path, old, new):
    p = Path(path)
    s = p.read_text()
    if old not in s:
        raise SystemExit(f"missing patch anchor in {path}: {old[:80]!r}")
    p.write_text(s.replace(old, new, 1))


op = "crates/aikit-core/src/resource/operative.rs"
patch(
    op,
    '''    if has_operative_syntax(trimmed) {\n        parse_resolve_expression(trimmed)\n    } else {\n        Ok(ResolveExpression::ordinary_search(trimmed))\n    }\n''',
    '''    if trimmed.starts_with('"') || trimmed.starts_with('\\'') {\n        return match parse_resolve_expression(trimmed)? {\n            ResolveExpression::Subject { value } => Ok(ResolveExpression::ordinary_search(value)),\n            expression => Ok(expression),\n        };\n    }\n    if has_operative_syntax(trimmed) {\n        parse_resolve_expression(trimmed)\n    } else {\n        Ok(ResolveExpression::ordinary_search(trimmed))\n    }\n''',
)

search = "crates/aikit-core/src/search.rs"
patch(
    search,
    "use crate::resolve::ResolvedView;\n",
    "use crate::resolve::ResolvedView;\nuse crate::resource::{parse_or_search_expression, ResolveExpression};\n",
)
patch(
    search,
    '''    pub const ALL: [FastPrefix; 4] = [\n        FastPrefix::Run,\n        FastPrefix::Capabilities,\n        FastPrefix::Sessions,\n        FastPrefix::Manage,\n    ];\n''',
    '''    /// Only non-conflicting historical lanes remain front-character shortcuts.\n    /// `+` and `@` stay deserializable enum values for compatibility, but new\n    /// input parses them as O:I operative syntax.\n    pub const ALL: [FastPrefix; 2] = [FastPrefix::Run, FastPrefix::Manage];\n''',
)
patch(
    search,
    '''            '>' => FastPrefix::Run,\n            '+' => FastPrefix::Capabilities,\n            '@' => FastPrefix::Sessions,\n            ':' => FastPrefix::Manage,\n''',
    '''            '>' => FastPrefix::Run,\n            ':' => FastPrefix::Manage,\n''',
)
patch(
    search,
    '''    pub raw: String,\n    pub text: String,\n    #[serde(default)]\n    pub prefix: Option<FastPrefix>,\n''',
    '''    pub raw: String,\n    pub text: String,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub expression: Option<ResolveExpression>,\n    #[serde(default)]\n    pub prefix: Option<FastPrefix>,\n''',
)
patch(
    search,
    '''    let trimmed = raw.trim_start();\n    let body = match trimmed.chars().next().and_then(FastPrefix::from_char) {\n''',
    '''    query.expression = parse_or_search_expression(raw).ok();\n\n    let trimmed = raw.trim_start();\n    let body = match trimmed.chars().next().and_then(FastPrefix::from_char) {\n''',
)
patch(
    search,
    '''    fn a_bare_colon_token_is_not_read_as_a_filter() {\n        let q = parse_query("what: ");\n        assert_eq!(q.text, "what:");\n        assert!(!q.has_filters());\n    }\n''',
    '''    fn a_bare_colon_token_is_not_read_as_a_filter() {\n        let q = parse_query("what: ");\n        assert_eq!(q.text, "what:");\n        assert!(!q.has_filters());\n    }\n\n    #[test]\n    fn operative_at_and_plus_are_not_legacy_fast_prefixes() {\n        let addressed = parse_query("@ project:demo");\n        assert_eq!(addressed.prefix, None);\n        assert!(matches!(addressed.expression, Some(ResolveExpression::Address { .. })));\n\n        let affirmed = parse_query("+ @5 action:verify");\n        assert_eq!(affirmed.prefix, None);\n        assert!(matches!(affirmed.expression, Some(ResolveExpression::Unary { .. })));\n    }\n''',
)

fam = "crates/aikit-core/src/familiarity.rs"
patch(
    fam,
    "use crate::resource::ResourceRef;\n",
    "use crate::resource::{AddressHorizon, RelationOp, ResolveExpression, ResourceRef};\n",
)
patch(
    fam,
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(tag = "kind", rename_all = "kebab-case")]\npub enum FamiliarityUse {\n''',
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct OperativePathEvidence {\n    pub path_identity: String,\n    pub expression: ResolveExpression,\n    #[serde(default)]\n    pub relation_ops: Vec<RelationOp>,\n    #[serde(default)]\n    pub horizons: Vec<AddressHorizon>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub method: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub action: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub surface: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub activity: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub return_ref: Option<ResourceRef>,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(tag = "kind", rename_all = "kebab-case")]\npub enum FamiliarityUse {\n''',
)
patch(
    fam,
    '''    Route {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n    },\n}\n''',
    '''    Route {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n    },\n    ResolvePath {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n    },\n}\n''',
)
patch(
    fam,
    '''    #[must_use]\n    pub fn from_surface(mut self, surface: ResourceRef) -> Self {\n''',
    '''    pub fn resolve_path(\n        observation_id: impl Into<String>,\n        route: ResourceRef,\n        destination: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n        context: FamiliarityContext,\n        observed_at_ms: u64,\n    ) -> Result<Self> {\n        if steps.is_empty() {\n            return Err(AikitError::new(\n                "familiarity.empty_resolve_path",\n                "a ResolvePath observation needs route steps",\n            ));\n        }\n        if steps.last().is_none_or(|step| step.resource != destination) {\n            return Err(AikitError::new(\n                "familiarity.resolve_path_destination_mismatch",\n                "the final ResolvePath step must be the destination",\n            ));\n        }\n        Ok(Self {\n            observation_id: observation_id.into(),\n            destination,\n            context,\n            use_kind: FamiliarityUse::ResolvePath { route, steps, operative },\n            source_surface: None,\n            source_action: None,\n            observed_at_ms,\n            fitness: None,\n        })\n    }\n\n    #[must_use]\n    pub fn from_surface(mut self, surface: ResourceRef) -> Self {\n''',
)
patch(
    fam,
    '''                (Some(wanted), FamiliarityUse::Route { route, .. }) => route == wanted,\n                _ => false,\n''',
    '''                (Some(wanted), FamiliarityUse::Route { route, .. })\n                | (Some(wanted), FamiliarityUse::ResolvePath { route, .. }) => route == wanted,\n                _ => false,\n''',
)
patch(
    fam,
    '''            ForgetScope::Route(route) => !matches!(\n                &observation.use_kind,\n                FamiliarityUse::Route {\n                    route: observed, ..\n                } if observed == route\n            ),\n''',
    '''            ForgetScope::Route(route) => match &observation.use_kind {\n                FamiliarityUse::Route { route: observed, .. }\n                | FamiliarityUse::ResolvePath { route: observed, .. } => observed != route,\n                _ => true,\n            },\n''',
)
patch(
    fam,
    '''                FamiliarityUse::Route {\n                    route: observed,\n                    steps,\n                } if observed == route => Some(\n''',
    '''                FamiliarityUse::Route { route: observed, steps }\n                | FamiliarityUse::ResolvePath { route: observed, steps, .. }\n                    if observed == route => Some(\n''',
)

actor = "crates/aikit-core/src/actor_bootstrap.rs"
patch(
    actor,
    "use crate::resource::{ProviderOffer, ResourceKind, ResourceRef, ResourceSource};\n",
    "use crate::resource::{AddressHorizon, ProviderOffer, RelationOp, ResolveExpression, ResourceKind, ResourceRef, ResourceSource};\n",
)
p = Path(actor)
s = p.read_text()
s += r'''

/// Express the actual actor bootstrap as the shared six-horizon O:I disclosure.
pub fn actor_world_disclosure(bootstrap: &ActorBootstrap) -> ResolveExpression {
    fn actual(reference: &Option<BootstrapReference>) -> Option<ResourceRef> {
        match reference {
            Some(BootstrapReference::Resolved { resource, .. }) => Some(resource.clone()),
            _ => None,
        }
    }

    fn clause(horizon: AddressHorizon, resource: ResourceRef) -> ResolveExpression {
        ResolveExpression::Unary {
            op: RelationOp::Affirm,
            expression: Box::new(ResolveExpression::horizon(
                horizon,
                ResolveExpression::subject(resource.to_string()),
            )),
        }
    }

    let mut clauses = Vec::new();
    clauses.extend(
        bootstrap
            .context_sources
            .examples
            .iter()
            .cloned()
            .map(|resource| clause(AddressHorizon::H0, resource)),
    );
    clauses.extend(
        [actual(&bootstrap.host), actual(&bootstrap.harness), actual(&bootstrap.model)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H1, resource)),
    );
    clauses.extend(
        [actual(&bootstrap.agent), actual(&bootstrap.agency)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H2, resource)),
    );
    clauses.extend(
        [actual(&bootstrap.harness), actual(&bootstrap.model)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H3, resource)),
    );
    if let Ok(project) = ResourceRef::parse(bootstrap.project.project.as_str()) {
        clauses.push(clause(AddressHorizon::H4, project));
    }
    if let Some(run) = bootstrap.run.clone() {
        clauses.push(clause(AddressHorizon::H4, run));
    }
    clauses.extend(
        [actual(&bootstrap.agent), actual(&bootstrap.host)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H4, resource)),
    );
    clauses.extend(
        bootstrap
            .capabilities
            .examples
            .iter()
            .chain(&bootstrap.actions.examples)
            .cloned()
            .map(|resource| clause(AddressHorizon::H5, resource)),
    );

    let expression = clauses
        .into_iter()
        .reduce(|left, right| ResolveExpression::Binary {
            op: RelationOp::Contextualise,
            left: Box::new(left),
            right: Box::new(right),
        })
        .unwrap_or_else(|| ResolveExpression::ordinary_search(""));
    ResolveExpression::Frame {
        expression: Box::new(expression),
    }
}
'''
p.write_text(s)
