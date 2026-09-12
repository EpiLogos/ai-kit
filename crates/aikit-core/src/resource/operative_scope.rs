//! Source-qualified scopes on nodes of the existing operative AST.
//!
//! The expression, Resource field, ranking law, ContextResolution and semantic
//! provider remain their native owners. This is a qualification of those objects,
//! not another parser, World, Method store or permission system. A current binding
//! permits interpretation; it does not authorise an Action or a model invocation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context_resolution::ContextResolution;
use crate::{AikitError, Result};

use super::{
    resolve_action_candidates, resolve_expression, resolve_path_identity, ActionRef,
    OperativeSemanticProvider, OperativeSemanticProviderStatus, OwnerRef, ProviderRef,
    ResolveExpression, ResolvePath, ResolveRankingSignals, ResolvedActionCandidate, ResourceIndex,
    ResourceRecord, ResourceRef, SourceRef, SourceRevision,
};

pub mod knowledge;

pub const OPERATIVE_SCOPE_VERSION: &str = "aikit.operative-scope/v1";
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 64;
const MAX_REF_BYTES: usize = 16384;

fn require(condition: bool, code: &'static str, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(AikitError::new(code, message))
    }
}

fn reference(raw: &str) -> Result<()> {
    require(
        !raw.is_empty()
            && raw == raw.trim()
            && !raw.contains('\0')
            && raw.len() <= MAX_REF_BYTES,
        "resolve.invalid_scope_reference",
        "scope references must be bounded, nonempty, trimmed and NUL-free",
    )
}

fn digest<T: Serialize>(domain: &str, value: &T) -> Result<String> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| AikitError::new("resolve.scope_encoding", error.to_string()))?;
    let mut hash = blake3::Hasher::new();
    hash.update(domain.as_bytes());
    hash.update(&[0]);
    hash.update(&encoded);
    Ok(hash.finalize().to_hex().to_string())
}

/// Edges of ResolveExpression, not a second expression grammar. Operand means
/// the single child of Address, Unary or grouping Frame; Left/Right are Binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpressionEdge {
    Operand,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeSource {
    pub source: SourceRef,
    pub revision: SourceRevision,
}

/// Exact provider-native interpretation and occasion. The selected whole and
/// subject are independently identified; neither is inferred from rendered text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperativeScope {
    pub provider: ProviderRef,
    pub binding: ResourceRef,
    pub owner: OwnerRef,
    pub owner_revision: SourceRevision,
    pub interpretation: ResourceRef,
    pub interpretation_revision: SourceRevision,
    pub world: ResourceRef,
    pub generation: SourceRevision,
    pub whole: ResourceRef,
    pub subject: ResourceRef,
    pub sources: Vec<ScopeSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method_skill: Option<ResourceRef>,
}

impl OperativeScope {
    fn canonical(&self) -> Result<Self> {
        for raw in [
            self.provider.as_str(),
            self.binding.as_str(),
            self.owner.as_str(),
            self.owner_revision.as_str(),
            self.interpretation.as_str(),
            self.interpretation_revision.as_str(),
            self.world.as_str(),
            self.generation.as_str(),
            self.whole.as_str(),
            self.subject.as_str(),
        ] {
            reference(raw)?;
        }
        if let Some(method) = &self.method_skill {
            reference(method.as_str())?;
        }
        require(
            !self.sources.is_empty() && self.sources.len() <= MAX_NODES,
            "resolve.scope_source_count",
            "a binding requires bounded exact source revisions",
        )?;
        let mut copy = self.clone();
        copy.sources.sort();
        let mut seen = BTreeSet::new();
        for source in &copy.sources {
            reference(source.source.as_str())?;
            reference(source.revision.as_str())?;
            require(
                seen.insert(&source.source),
                "resolve.ambiguous_scope_source",
                "a source has repeated or conflicting revision pins",
            )?;
        }
        Ok(copy)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionScope {
    /// Empty identifies the root; child identity survives nested/grouped syntax.
    pub node: Vec<ExpressionEdge>,
    pub binding: OperativeScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedResolveExpression {
    pub expression: ResolveExpression,
    #[serde(default)]
    pub scopes: Vec<ExpressionScope>,
}

impl ScopedResolveExpression {
    pub fn unbound(expression: ResolveExpression) -> Self {
        Self {
            expression,
            scopes: Vec::new(),
        }
    }

    pub fn canonical(&self) -> Result<Self> {
        // Validate depth before recursive render/clone/serialization. This also
        // bounds structured clients, not only input that passed the text parser.
        let mut pending = vec![(&self.expression, 0usize)];
        let mut count = 0usize;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            require(
                count <= MAX_NODES && depth <= MAX_DEPTH,
                "resolve.scope_expression_budget",
                "expression exceeds the structural budget",
            )?;
            match node {
                ResolveExpression::Subject { value } => {
                    require(
                        value.len() <= MAX_REF_BYTES,
                        "resolve.scope_subject_budget",
                        "expression subject exceeds the byte budget",
                    )?;
                }
                ResolveExpression::Address { expression, .. }
                | ResolveExpression::Unary { expression, .. }
                | ResolveExpression::Frame { expression } => pending.push((expression, depth + 1)),
                ResolveExpression::Binary { left, right, .. } => {
                    pending.push((left, depth + 1));
                    pending.push((right, depth + 1));
                }
            }
        }
        require(
            self.scopes.len() <= MAX_NODES,
            "resolve.scope_count",
            "expression has too many scope bindings",
        )?;
        let mut scopes = Vec::with_capacity(self.scopes.len());
        for scope in &self.scopes {
            self.node(&scope.node)?;
            scopes.push(ExpressionScope {
                node: scope.node.clone(),
                binding: scope.binding.canonical()?,
            });
        }
        scopes.sort_by(|a, b| (&a.node, &a.binding.provider).cmp(&(&b.node, &b.binding.provider)));
        for pair in scopes.windows(2) {
            require(
                pair[0].node != pair[1].node
                    || pair[0].binding.provider != pair[1].binding.provider,
                "resolve.ambiguous_node_scope",
                "the same AST node has multiple bindings from one provider",
            )?;
        }
        Ok(Self {
            expression: self.expression.clone(),
            scopes,
        })
    }

    pub fn node(&self, path: &[ExpressionEdge]) -> Result<&ResolveExpression> {
        require(
            path.len() <= MAX_DEPTH,
            "resolve.scope_node_depth",
            "scope path is too deep",
        )?;
        let mut node = &self.expression;
        for edge in path {
            node = match (node, edge) {
                (ResolveExpression::Address { expression, .. }, ExpressionEdge::Operand)
                | (ResolveExpression::Unary { expression, .. }, ExpressionEdge::Operand)
                | (ResolveExpression::Frame { expression }, ExpressionEdge::Operand) => expression,
                (ResolveExpression::Binary { left, .. }, ExpressionEdge::Left) => left,
                (ResolveExpression::Binary { right, .. }, ExpressionEdge::Right) => right,
                _ => {
                    return Err(AikitError::new(
                        "resolve.scope_node_missing",
                        "scope path does not identify a node in this expression",
                    ))
                }
            };
        }
        Ok(node)
    }

    pub fn identity(&self) -> Result<String> {
        let canonical = self.canonical()?;
        if canonical.scopes.is_empty() {
            return Ok(resolve_path_identity(&canonical.expression));
        }
        Ok(format!(
            "resolve-scoped-path:{}",
            digest(OPERATIVE_SCOPE_VERSION, &canonical)?
        ))
    }

    /// A narrower explicit binding shadows its ancestor for the same provider.
    /// The full expression still retains both bindings as evidence, not a lossy
    /// flattened effective map. A sibling never acquires another sibling's scope.
    pub fn effective_scopes(&self, node: &[ExpressionEdge]) -> Result<Vec<ExpressionScope>> {
        let canonical = self.canonical()?;
        canonical.node(node)?;
        let mut selected = BTreeMap::new();
        for scope in canonical.scopes {
            if node.starts_with(&scope.node) {
                selected.insert(scope.binding.provider.clone(), scope);
            }
        }
        Ok(selected.into_values().collect())
    }
}

/// Read-only evidence projection. Construction is through the native resolver;
/// clients submit ScopedResolveExpression, not a self-certified resolved path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedResolvePath {
    version: &'static str,
    path: ResolvePath,
    scopes: Vec<ExpressionScope>,
}

impl ScopedResolvePath {
    pub fn native(&self) -> &ResolvePath {
        &self.path
    }

    pub fn scopes(&self) -> &[ExpressionScope] {
        &self.scopes
    }
}

struct ScopedIndex<'a> {
    inner: &'a dyn ResourceIndex,
    identity: &'a str,
}

impl ResourceIndex for ScopedIndex<'_> {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {
        self.inner.resource(id)
    }

    fn resources(&self) -> Vec<&ResourceRecord> {
        self.inner.resources()
    }

    fn resolve_ranking(&self, id: &ResourceRef) -> ResolveRankingSignals {
        self.inner.resolve_ranking(id)
    }

    fn resolve_path_ranking(&self, _: &str, id: &ResourceRef) -> ResolveRankingSignals {
        self.inner.resolve_path_ranking(self.identity, id)
    }
}

pub fn resolve_scoped_expression(
    expression: &ScopedResolveExpression,
    resources: &dyn ResourceIndex,
    limit: usize,
) -> Result<ScopedResolvePath> {
    let expression = expression.canonical()?;
    let identity = expression.identity()?;
    // Only the lookup identity is qualified. All candidate discovery, scoring,
    // authored preferences and contextual/familiarity ordering stay native.
    let scoped = ScopedIndex {
        inner: resources,
        identity: &identity,
    };
    let mut path = resolve_expression(&expression.expression, &scoped, limit);
    path.identity = identity;
    Ok(ScopedResolvePath {
        version: OPERATIVE_SCOPE_VERSION,
        path,
        scopes: expression.scopes,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ScopeObservation {
    Current {
        binding: OperativeScope,
        evidence: Vec<ResourceRef>,
    },
    Missing {
        reason: String,
    },
    Ambiguous {
        candidates: Vec<ResourceRef>,
        reason: String,
    },
    Stale {
        observed: OperativeScope,
        reason: String,
    },
    Unavailable {
        reason: String,
    },
    Unsupported {
        reason: String,
    },
}

/// Optional capability of the existing semantic provider. A provider must read
/// its current owner/source binding in this native context; echoing a request is
/// not an observation. This does not create a provider registry or a QL parser.
pub trait ScopeAwareOperativeProvider: OperativeSemanticProvider {
    fn observe_scope(
        &self,
        requested: &OperativeScope,
        context: &ContextResolution,
    ) -> Result<ScopeObservation>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedExpressionScope {
    pub scope: ExpressionScope,
    pub observation: ScopeObservation,
}

/// An exact native ContextResolution joined to a qualified path. No independent
/// context lifetime or store is introduced; freshness must be checked at effect.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopedContextResolution {
    version: &'static str,
    path: ScopedResolvePath,
    context: ContextResolution,
    observations: Vec<ObservedExpressionScope>,
    context_digest: String,
}

impl ScopedContextResolution {
    pub fn path(&self) -> &ScopedResolvePath {
        &self.path
    }

    pub fn context(&self) -> &ContextResolution {
        &self.context
    }

    pub fn observations(&self) -> &[ObservedExpressionScope] {
        &self.observations
    }

    pub fn require_current(&self) -> Result<()> {
        for observed in &self.observations {
            require(
                matches!(observed.observation, ScopeObservation::Current { .. }),
                "resolve.scope_not_current",
                "a scope is missing, ambiguous, stale or unavailable",
            )?;
        }
        Ok(())
    }

    /// This is stageability, not an invocation grant. Native Action policy,
    /// approval and trust checks remain required even after this succeeds.
    pub fn action(
        &self,
        action: &ResourceRef,
        resources: &dyn ResourceIndex,
    ) -> Result<ResolvedActionCandidate> {
        self.require_current()?;
        let action = ActionRef::parse(action.clone(), resources)?;
        resolve_action_candidates(&self.path.path, resources, &self.context)
            .into_iter()
            .find(|candidate| candidate.action == action && candidate.available_in_context)
            .ok_or_else(|| {
                AikitError::new(
                    "resolve.scoped_action_unavailable",
                    "the Action is not resolved and available in the native ContextResolution",
                )
            })
    }

    pub fn revalidate<P: ScopeAwareOperativeProvider>(
        &self,
        current: &ContextResolution,
        provider: &P,
    ) -> Result<()> {
        require(
            digest("aikit.native-context-resolution", current)? == self.context_digest,
            "resolve.scope_context_changed",
            "native context changed after scoped resolution",
        )?;
        self.require_current()?;
        for observed in &self.observations {
            let fresh = observe_one(&observed.scope.binding, current, provider)?;
            require(
                matches!(fresh, ScopeObservation::Current { .. }),
                "resolve.scope_changed_before_effect",
                "provider scope changed before effect",
            )?;
        }
        Ok(())
    }

    /// A failed or late completion observation must not erase a performed act.
    /// The original scope remains immutable and each fresh observation is kept
    /// separately so native receiving can decide how to use the returned work.
    pub fn completion_observations<P: ScopeAwareOperativeProvider>(
        &self,
        current: &ContextResolution,
        provider: &P,
    ) -> Vec<ObservedExpressionScope> {
        self.observations
            .iter()
            .map(|previous| ObservedExpressionScope {
                scope: previous.scope.clone(),
                observation: observe_one(&previous.scope.binding, current, provider)
                    .unwrap_or_else(|error| ScopeObservation::Unavailable {
                        reason: error.to_string(),
                    }),
            })
            .collect()
    }
}

fn observe_one<P: ScopeAwareOperativeProvider>(
    requested: &OperativeScope,
    context: &ContextResolution,
    provider: &P,
) -> Result<ScopeObservation> {
    let descriptor = provider.descriptor();
    if descriptor.provider != requested.provider {
        return Ok(ScopeObservation::Unsupported {
            reason: "binding belongs to another provider".into(),
        });
    }
    if !matches!(descriptor.status, OperativeSemanticProviderStatus::Available) {
        return Ok(ScopeObservation::Unavailable {
            reason: "semantic provider is not available".into(),
        });
    }
    let observation = provider.observe_scope(requested, context)?;
    if let ScopeObservation::Current { binding, evidence } = &observation {
        if binding.canonical()? != requested.canonical()? {
            return Ok(ScopeObservation::Stale {
                observed: binding.clone(),
                reason: "observed owner/source/whole binding differs from the requested scope".into(),
            });
        }
        require(
            !evidence.is_empty() && evidence.len() <= MAX_NODES,
            "resolve.scope_observation_without_evidence",
            "current scope needs native source evidence",
        )?;
        for item in evidence {
            reference(item.as_str())?;
        }
    }
    Ok(observation)
}

pub fn compose_scoped_context<P: ScopeAwareOperativeProvider>(
    expression: &ScopedResolveExpression,
    resources: &dyn ResourceIndex,
    context: &ContextResolution,
    limit: usize,
    provider: &P,
) -> Result<ScopedContextResolution> {
    let path = resolve_scoped_expression(expression, resources, limit)?;
    let mut observations = Vec::with_capacity(path.scopes.len());
    for scope in &path.scopes {
        if let Some(method) = &scope.binding.method_skill {
            let record = resources.resource(method).ok_or_else(|| {
                AikitError::new(
                    "resolve.scope_method_missing",
                    "scope Method Skill is absent from the Resource field",
                )
            })?;
            require(
                record.descriptor.kind == super::ResourceKind::Capability
                    && crate::method::method_payload(&record.descriptor.description).is_some(),
                "resolve.scope_method_not_skill",
                "scope Method must remain a METHOD:-classified Skill",
            )?;
        }
        observations.push(ObservedExpressionScope {
            scope: scope.clone(),
            observation: observe_one(&scope.binding, context, provider)?,
        });
    }
    Ok(ScopedContextResolution {
        version: OPERATIVE_SCOPE_VERSION,
        path,
        context: context.clone(),
        observations,
        context_digest: digest("aikit.native-context-resolution", context)?,
    })
}

#[cfg(test)]
mod tests;
