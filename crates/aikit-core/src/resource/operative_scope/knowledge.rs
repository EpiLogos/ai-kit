//! Qualified scopes through the existing native Living Knowledge execution and
//! Return path. The supplied executor retains native model/action authority.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::context_resolution::ContextResolution;
use crate::knowledge_living::{ContemplateGenerated, ContemplateRequest};
use crate::knowledge_living_relations::KnowledgeResourceDependency;
use crate::knowledge_wiki::WikiObject;
use crate::knowledge_wiki_shape::{
    explicit_ql_shaped_resolve_contemplate, QlShapedContemplateExecutor, QlShapedContemplateOutcome,
    QlShapedContemplatePreflight,
};
use crate::{AikitError, Result};

use super::{
    require, ObservedExpressionScope, ScopeAwareOperativeProvider, ScopeObservation,
    ScopedContextResolution, OPERATIVE_SCOPE_VERSION,
};

pub const OPERATIVE_SCOPE_RETURN_EXTENSION: &str = "aikit.operative-scope-return/v1";

pub struct ScopedContemplateInput<'a> {
    pub request: &'a ContemplateRequest<'a>,
    pub resolution: &'a ScopedContextResolution,
    pub current_context: &'a ContextResolution,
    pub resource_dependencies: &'a [KnowledgeResourceDependency],
    pub max_objects: usize,
    pub relation_depth: usize,
    pub shape_budget: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopedKnowledgeReturnStanding {
    Current,
    ReobservationRequired,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedContemplateOutcome {
    pub original_resolution: ScopedContextResolution,
    pub native: QlShapedContemplateOutcome,
    pub completion: Vec<ObservedExpressionScope>,
    pub standing: ScopedKnowledgeReturnStanding,
}

fn attribute_generated(
    generated: &mut ContemplateGenerated,
    resolution: &ScopedContextResolution,
) -> Result<()> {
    let evidence = json!({
        "version": OPERATIVE_SCOPE_VERSION,
        "resolve_path_identity": resolution.path().native().identity,
        "expression": resolution.path().native().expression,
        "scopes": resolution.path().scopes(),
        "context_digest": resolution.context_digest,
        "observations": resolution.observations(),
        "standing": "performed-native-generation-not-source-promotion",
    });
    fn attach(
        extensions: &mut std::collections::BTreeMap<String, Value>,
        evidence: &Value,
    ) -> Result<()> {
        if let Some(existing) = extensions.get(OPERATIVE_SCOPE_RETURN_EXTENSION) {
            require(
                existing == evidence,
                "knowledge.scope_return_conflict",
                "producer supplied a conflicting original scope attribution",
            )?;
        } else {
            extensions.insert(OPERATIVE_SCOPE_RETURN_EXTENSION.into(), evidence.clone());
        }
        Ok(())
    }
    for object in &mut generated.wiki_upserts {
        if let WikiObject::Reading(reading) = object {
            attach(&mut reading.extensions, &evidence)?;
        }
    }
    for reading in &mut generated.integrative_readings {
        attach(&mut reading.reading.extensions, &evidence)?;
    }
    Ok(())
}

/// Calls the existing bounded/native Contemplate path exactly once after fresh
/// provider/context/source admission. A failed call is never retried implicitly;
/// an after-effect source change marks the actual Return rather than deleting it.
/// This function does not register a Method, promote a Wiki or mutate human text.
pub fn explicit_scoped_contemplate<P, F>(
    input: ScopedContemplateInput<'_>,
    provider: &P,
    mut execute: F,
) -> Result<ScopedContemplateOutcome>
where
    P: ScopeAwareOperativeProvider,
    F: FnMut(&QlShapedContemplatePreflight, &ScopedContextResolution) -> Result<ContemplateGenerated>,
{
    let resolution = input.resolution;
    resolution.revalidate(input.current_context, provider)?;
    require(
        input.request.project == input.current_context.project_binding.project,
        "knowledge.scope_project_mismatch",
        "Contemplate must retain the current native Project binding",
    )?;
    require(
        input.request.focus.iter().all(|focus| {
            resolution
                .path()
                .native()
                .candidates
                .iter()
                .any(|candidate| &candidate.resource == focus)
                || resolution
                    .path()
                    .scopes()
                    .iter()
                    .any(|scope| &scope.binding.subject == focus)
        }),
        "knowledge.scope_focus_mismatch",
        "Contemplate focus must be an explicitly resolved or provider-bound subject",
    )?;
    for dependency in input.request.dependencies {
        // Unbound callers retain the original source policy. Bound calls must
        // carry the precise source pins they are about to consume, not use scope
        // presence as a pretext for widening into unrelated or stale sources.
        if !resolution.path().scopes().is_empty() {
            let pin = resolution
                .path()
                .scopes()
                .iter()
                .flat_map(|scope| &scope.binding.sources)
                .find(|pin| pin.source == dependency.source)
                .ok_or_else(|| {
                    AikitError::new(
                        "knowledge.scope_source_unbound",
                        "Contemplate dependency is outside the qualified source basis",
                    )
                })?;
            require(
                input.request.horizon.sources.iter().any(|source| {
                    source.source == pin.source
                        && source.available
                        && source.revision.as_ref() == Some(&pin.revision)
                }),
                "knowledge.scope_source_changed",
                "the native source horizon does not confirm the bound revision",
            )?;
        }
    }

    struct NativeAdapter<'a, P, F> {
        resolution: &'a ScopedContextResolution,
        current_context: &'a ContextResolution,
        provider: &'a P,
        execute: &'a mut F,
    }
    impl<P, F> QlShapedContemplateExecutor for NativeAdapter<'_, P, F>
    where
        P: ScopeAwareOperativeProvider,
        F: FnMut(
            &QlShapedContemplatePreflight,
            &ScopedContextResolution,
        ) -> Result<ContemplateGenerated>,
    {
        fn execute(&mut self, preflight: &QlShapedContemplatePreflight) -> Result<ContemplateGenerated> {
            // Recheck at the actual execution aperture, not merely at Resolve.
            self.resolution
                .revalidate(self.current_context, self.provider)?;
            let mut generated = (self.execute)(preflight, self.resolution)?;
            attribute_generated(&mut generated, self.resolution)?;
            Ok(generated)
        }
    }
    let mut adapter = NativeAdapter {
        resolution,
        current_context: input.current_context,
        provider,
        execute: &mut execute,
    };
    let native = explicit_ql_shaped_resolve_contemplate(
        input.request,
        input.resource_dependencies,
        resolution.path().native(),
        input.max_objects,
        input.relation_depth,
        input.shape_budget,
        &mut adapter,
    )
    .map_err(|error| {
        error.with(
            "operative_scope_path",
            resolution.path().native().identity.clone(),
        )
    })?;
    let completion = resolution.completion_observations(input.current_context, provider);
    let standing = if completion
        .iter()
        .all(|value| matches!(value.observation, ScopeObservation::Current { .. }))
    {
        ScopedKnowledgeReturnStanding::Current
    } else {
        ScopedKnowledgeReturnStanding::ReobservationRequired
    };
    Ok(ScopedContemplateOutcome {
        original_resolution: resolution.clone(),
        native,
        completion,
        standing,
    })
}
