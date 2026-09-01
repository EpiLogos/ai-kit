#!/usr/bin/env python3
from pathlib import Path


def replace(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    if old not in text:
        raise SystemExit(f"expected source fragment missing in {path}: {old[:100]!r}")
    file.write_text(text.replace(old, new, 1))


# Quoted human search material is a literal subject, not text including quote marks.
replace(
    "crates/aikit-core/src/resource/operative.rs",
    '''    if has_operative_syntax(trimmed) {\n        parse_resolve_expression(trimmed)\n    } else {\n        Ok(ResolveExpression::ordinary_search(trimmed))\n    }\n''',
    '''    if trimmed.starts_with(['\\"', '\\'']) {\n        return match parse_resolve_expression(trimmed)? {\n            ResolveExpression::Subject { value } => Ok(ResolveExpression::ordinary_search(value)),\n            expression => Ok(expression),\n        };\n    }\n    if has_operative_syntax(trimmed) {\n        parse_resolve_expression(trimmed)\n    } else {\n        Ok(ResolveExpression::ordinary_search(trimmed))\n    }\n''',
)

# The old palette query model stays for ranking/filter compatibility, but @/+ are
# no longer stolen as front-character lanes. Every query now also carries the
# common operative AST parsed by the same core parser used by structured clients.
replace(
    "crates/aikit-core/src/search.rs",
    "use crate::resolve::ResolvedView;\n",
    "use crate::resolve::ResolvedView;\nuse crate::resource::{parse_or_search_expression, ResolveExpression};\n",
)
replace(
    "crates/aikit-core/src/search.rs",
    '''    pub const ALL: [FastPrefix; 4] = [\n        FastPrefix::Run,\n        FastPrefix::Capabilities,\n        FastPrefix::Sessions,\n        FastPrefix::Manage,\n    ];\n''',
    '''    /// Legacy lanes that do not conflict with the O:I operative language.\n    /// `+` and `@` remain enum variants for persisted compatibility, but new\n    /// parsing no longer assigns them by front-character switching.\n    pub const ALL: [FastPrefix; 2] = [FastPrefix::Run, FastPrefix::Manage];\n''',
)
replace(
    "crates/aikit-core/src/search.rs",
    '''            '>' => FastPrefix::Run,\n            '+' => FastPrefix::Capabilities,\n            '@' => FastPrefix::Sessions,\n            ':' => FastPrefix::Manage,\n''',
    '''            '>' => FastPrefix::Run,\n            ':' => FastPrefix::Manage,\n''',
)
replace(
    "crates/aikit-core/src/search.rs",
    '''    pub raw: String,\n    pub text: String,\n    #[serde(default)]\n    pub prefix: Option<FastPrefix>,\n''',
    '''    pub raw: String,\n    pub text: String,\n    /// Provider-neutral O:I expression shared by CLI/TUI/structured clients.\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub expression: Option<ResolveExpression>,\n    #[serde(default)]\n    pub prefix: Option<FastPrefix>,\n''',
)
replace(
    "crates/aikit-core/src/search.rs",
    '''    let trimmed = raw.trim_start();\n    let body = match trimmed.chars().next().and_then(FastPrefix::from_char) {\n''',
    '''    query.expression = parse_or_search_expression(raw).ok();\n\n    let trimmed = raw.trim_start();\n    let body = match trimmed.chars().next().and_then(FastPrefix::from_char) {\n''',
)
replace(
    "crates/aikit-core/src/search.rs",
    '''    fn an_empty_query_is_recognized_as_empty() {\n        assert!(parse_query("").is_empty());\n        assert!(parse_query("   ").is_empty());\n        assert!(!parse_query("kind:script").is_empty());\n        assert!(!parse_query(">").is_empty());\n    }\n''',
    '''    fn an_empty_query_is_recognized_as_empty() {\n        assert!(parse_query("").is_empty());\n        assert!(parse_query("   ").is_empty());\n        assert!(!parse_query("kind:script").is_empty());\n        assert!(!parse_query(">").is_empty());\n    }\n\n    #[test]\n    fn operative_at_and_plus_are_ast_syntax_not_legacy_fast_prefixes() {\n        let addressed = parse_query("@ project:demo");\n        assert_eq!(addressed.prefix, None);\n        assert!(matches!(addressed.expression, Some(ResolveExpression::Address { .. })));\n\n        let affirmed = parse_query("+ @5 action:verify");\n        assert_eq!(affirmed.prefix, None);\n        assert!(matches!(affirmed.expression, Some(ResolveExpression::Unary { .. })));\n    }\n''',
)

# Extend #29's existing observation stream with a typed ResolvePath variant rather
# than introducing another familiarity store.
replace(
    "crates/aikit-core/src/familiarity.rs",
    "use crate::resource::ResourceRef;\n",
    "use crate::resource::{AddressHorizon, RelationOp, ResolveExpression, ResourceRef};\n",
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(tag = "kind", rename_all = "kebab-case")]\npub enum FamiliarityUse {\n''',
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct OperativePathEvidence {\n    pub path_identity: String,\n    pub expression: ResolveExpression,\n    #[serde(default)]\n    pub relation_ops: Vec<RelationOp>,\n    #[serde(default)]\n    pub horizons: Vec<AddressHorizon>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub method: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub action: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub surface: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub activity: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub return_ref: Option<ResourceRef>,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(tag = "kind", rename_all = "kebab-case")]\npub enum FamiliarityUse {\n''',
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''    Route {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n    },\n}\n''',
    '''    Route {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n    },\n    /// Same accepted #29 route evidence, enriched with the operative language\n    /// actually traversed. Provider/lens/revision stay on each route step.\n    ResolvePath {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n    },\n}\n''',
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''    #[must_use]\n    pub fn from_surface(mut self, surface: ResourceRef) -> Self {\n''',
    '''    pub fn resolve_path(\n        observation_id: impl Into<String>,\n        route: ResourceRef,\n        destination: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n        context: FamiliarityContext,\n        observed_at_ms: u64,\n    ) -> Result<Self> {\n        if steps.is_empty() {\n            return Err(AikitError::new(\n                "familiarity.empty_resolve_path",\n                "a ResolvePath familiarity observation must contain at least one route step",\n            ));\n        }\n        if steps.last().is_none_or(|step| step.resource != destination) {\n            return Err(AikitError::new(\n                "familiarity.resolve_path_destination_mismatch",\n                "the final ResolvePath step must be the recorded destination",\n            ));\n        }\n        Ok(Self {\n            observation_id: observation_id.into(),\n            destination,\n            context,\n            use_kind: FamiliarityUse::ResolvePath { route, steps, operative },\n            source_surface: None,\n            source_action: None,\n            observed_at_ms,\n            fitness: None,\n        })\n    }\n\n    #[must_use]\n    pub fn from_surface(mut self, surface: ResourceRef) -> Self {\n''',
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''                (Some(wanted), FamiliarityUse::Route { route, .. }) => route == wanted,\n                _ => false,\n''',
    '''                (Some(wanted), FamiliarityUse::Route { route, .. })\n                | (Some(wanted), FamiliarityUse::ResolvePath { route, .. }) => route == wanted,\n                _ => false,\n''',
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''            ForgetScope::Route(route) => !matches!(\n                &observation.use_kind,\n                FamiliarityUse::Route {\n                    route: observed, ..\n                } if observed == route\n            ),\n''',
    '''            ForgetScope::Route(route) => match &observation.use_kind {\n                FamiliarityUse::Route { route: observed, .. }\n                | FamiliarityUse::ResolvePath { route: observed, .. } => observed != route,\n                _ => true,\n            },\n''',
)
replace(
    "crates/aikit-core/src/familiarity.rs",
    '''                FamiliarityUse::Route {\n                    route: observed,\n                    steps,\n                } if observed == route => Some(\n''',
    '''                FamiliarityUse::Route {\n                    route: observed,\n                    steps,\n                }\n                | FamiliarityUse::ResolvePath {\n                    route: observed,\n                    steps,\n                    ..\n                } if observed == route => Some(\n''',
)

# Add the actual thin six-horizon disclosure for the current actor bootstrap.
replace(
    "crates/aikit-core/src/actor_bootstrap.rs",
    "use crate::resource::{ProviderOffer, ResourceKind, ResourceRef, ResourceSource};\n",
    "use crate::resource::{AddressHorizon, ProviderOffer, RelationOp, ResolveExpression, ResourceKind, ResourceRef, ResourceSource};\n",
)
actor_path = Path("crates/aikit-core/src/actor_bootstrap.rs")
actor = actor_path.read_text()
actor += r'''

/// Render the bootstrap's actual current world through the common O:I operative
/// language. This is a disclosure pointer field: every subject is an existing
/// canonical ref already reachable through Search/Explain/History.
pub fn actor_world_disclosure(bootstrap: &ActorBootstrap) -> ResolveExpression {
    fn resolved(reference: &Option<BootstrapReference>) -> Option<ResourceRef> {
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
        [resolved(&bootstrap.host), resolved(&bootstrap.harness), resolved(&bootstrap.model)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H1, resource)),
    );
    clauses.extend(
        [resolved(&bootstrap.agent), resolved(&bootstrap.agency)]
            .into_iter()
            .flatten()
            .map(|resource| clause(AddressHorizon::H2, resource)),
    );
    clauses.extend(
        [resolved(&bootstrap.harness), resolved(&bootstrap.model)]
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
        [resolved(&bootstrap.agent), resolved(&bootstrap.host)]
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
actor_path.write_text(actor)

# This migration script is intentionally one-shot; its resulting source changes
# are the product delta. The PR can be squash-merged without carrying bootstrap
# machinery into main.
