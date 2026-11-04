//! Shared, UI-neutral SessionSpace application contract.
//!
//! `SessionSpaceRuntime` remains the owner of live AgentSession, Component,
//! Surface and connection observations. This layer owns only durable semantic
//! relations AIKit can author: exact Project/ContextResolution evidence,
//! attachment intent, portable focus, and references to provider/host/Workcell
//! material identities owned elsewhere.
//!
//! Durable mutations follow the common application law:
//! inspect -> stage typed intent -> preview -> validate basis -> authoritative
//! apply -> receipt -> re-read. No resolver runs here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context_activation::ContextActivationReceipt;
use crate::context_resolution::{ContextResolution, ReferenceResolution, ScopeResolution};
use crate::project::{ProjectBinding, ProjectRef};
use crate::resource::ResourceRef;
use crate::session::SessionPlan;
use crate::session_space::{SessionSpaceDefinition, SessionSpaceReadModel, SessionSpaceRef};
use crate::{AikitError, Result};

pub const SESSION_SPACE_APPLICATION_VERSION: &str = "aikit.session-space-application/v1";

/// Stable, content-addressed reference to the evidence basis of one canonical
/// ContextResolution. It is evidence, not a second resolver identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextResolutionRef(ResourceRef);

impl ContextResolutionRef {
    pub fn as_resource_ref(&self) -> &ResourceRef {
        &self.0
    }
}

impl std::fmt::Display for ContextResolutionRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResolutionBasis {
    pub project_binding: ProjectBinding,
    pub resolver_hash: String,
    pub catalog_revision: String,
    /// Canonical precedence order from ContextResolution; never folded here.
    pub scopes: Vec<ScopeResolution>,
    #[serde(default)]
    pub context_sources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<ResourceRef>,
    /// Material activation evidence is part of the operative resolution basis,
    /// while remaining distinct from the deterministic resolver hash above.
    /// Empty evidence is omitted so pre-activation references do not churn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_activations: Vec<ContextActivationReceipt>,
    /// Source descriptors from the same once-observed resource snapshot.
    /// Byte digests and declared revisions remain separate source facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_source_resources: Vec<crate::resource::ResourceDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResolutionEvidence {
    pub reference: ContextResolutionRef,
    pub basis: ContextResolutionBasis,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ContextResolutionEvidence {
    pub fn from_resolution(resolution: &ContextResolution) -> Result<Self> {
        let mut context_activations = resolution.context_activations.clone();
        context_activations.sort();
        let basis = ContextResolutionBasis {
            project_binding: resolution.project_binding.clone(),
            resolver_hash: resolution.deterministic.hash.to_string(),
            catalog_revision: resolution.deterministic.catalog_revision.clone(),
            scopes: resolution.scopes.clone(),
            context_sources: resolution.retrieval.context_sources.clone(),
            host: resolution.host.as_ref().map(reference_identity),
            context_activations,
            observed_source_resources: resolution.observed_source_resources.clone(),
        };
        let encoded = serde_json::to_vec(&basis).map_err(|error| {
            AikitError::new(
                "session_space.context_basis_unserializable",
                format!("could not encode ContextResolution basis: {error}"),
            )
        })?;
        let digest = blake3::hash(&encoded).to_hex().to_string();
        let reference = ContextResolutionRef(ResourceRef::parse(&format!(
            "context-resolution/{}",
            &digest[..16]
        ))?);
        let mut provenance = resolution
            .scopes
            .iter()
            .map(|scope| format!("{}:{}", scope.kind.as_str(), scope.origin))
            .collect::<Vec<_>>();
        provenance.push(format!("resolver:{}", basis.resolver_hash));
        provenance.push(format!("catalog:{}", basis.catalog_revision));
        Ok(Self {
            reference,
            basis,
            provenance,
        })
    }

    pub fn project(&self) -> &ProjectRef {
        &self.basis.project_binding.project
    }
}

fn reference_identity(resolution: &ReferenceResolution) -> ResourceRef {
    match resolution {
        ReferenceResolution::Resolved { resource } => resource.resource.descriptor.id.clone(),
        ReferenceResolution::Missing { reference, .. }
        | ReferenceResolution::WrongKind { reference, .. } => reference.clone(),
    }
}

/// Exact Project membership plus independent ContextResolution evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceProjectContextBinding {
    pub project: ProjectRef,
    pub context: ContextResolutionEvidence,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionSpaceProjectContextBinding {
    pub fn new(project: ProjectRef, context: ContextResolutionEvidence) -> Result<Self> {
        if context.project() != &project {
            return Err(AikitError::new(
                "session_space.project_context_mismatch",
                format!(
                    "Project {project} cannot bind ContextResolution {} owned by {}",
                    context.reference,
                    context.project()
                ),
            ));
        }
        Ok(Self {
            project,
            context,
            provenance: vec!["explicit SessionSpace Project/Context binding".into()],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceAgentAttachmentIntent {
    pub agent_session: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceSurfaceAttachmentIntent {
    pub surface: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// The durable, provider-neutral address of one exact working Surface.
///
/// The `SessionPlan` is a validated snapshot because a mux can only focus a
/// pane through the plan that gives the pane its logical key. Provider-native
/// session/window/pane ids are re-observed on every operation and never become
/// canonical identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSpaceWorkingSurfaceBinding {
    /// Durable owner address for this binding. A Surface can have distinct
    /// bindings through distinct admitted providers without conflating them.
    pub binding: ResourceRef,
    pub surface: ResourceRef,
    pub agent_session: ResourceRef,
    pub provider: ResourceRef,
    pub plan: SessionPlan,
    pub plan_key: String,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl Eq for SessionSpaceWorkingSurfaceBinding {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionSpaceNativeReferenceKind {
    Provider,
    Host,
    Workcell,
    Material,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceNativeReferenceBinding {
    pub reference: ResourceRef,
    pub kind: SessionSpaceNativeReferenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// The Central agent set a SessionSpace runs (`central.agent-set/v1`): the
/// group's tie to the record that names its members, its orchestrator and how
/// their returns combine (`join`). AIKit keeps only the tie; the set is
/// Central's, read and written there. A space that runs no set names none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceAgentSetBinding {
    /// The set's ref in Central (`central.agent-set.read {scope, ref}`).
    pub agent_set_ref: String,
    /// The set's revision when the tie was made, where it was known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionSpaceAgentSetBinding {
    fn validate(&self) -> Result<()> {
        let reference = self.agent_set_ref.as_str();
        if reference.trim().is_empty()
            || reference.len() > 256
            || reference.chars().any(char::is_control)
        {
            return Err(AikitError::new(
                "session_space.agent_set_ref_invalid",
                "the agent set's ref must be a bounded, printable Central ref",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceFocus {
    pub target: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Canonical durable SessionSpace semantic state. The embedded definition stays
/// the existing SessionSpace identity/project-membership declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceAuthoredState {
    pub version: String,
    pub revision: u64,
    pub definition: SessionSpaceDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub project_contexts: BTreeMap<ProjectRef, ContextResolutionEvidence>,
    #[serde(default)]
    pub agent_sessions: BTreeMap<ResourceRef, SessionSpaceAgentAttachmentIntent>,
    #[serde(default)]
    pub surfaces: BTreeMap<ResourceRef, SessionSpaceSurfaceAttachmentIntent>,
    #[serde(default)]
    pub working_surfaces: BTreeMap<ResourceRef, SessionSpaceWorkingSurfaceBinding>,
    #[serde(default)]
    pub native_references: BTreeMap<ResourceRef, SessionSpaceNativeReferenceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<SessionSpaceFocus>,
    /// The Central agent set this space runs, where one is named (O-I #684).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_set: Option<SessionSpaceAgentSetBinding>,
    /// The sessions of this space on the bench (O-I manual §19.15, the Agent
    /// rack's Activator): each stays in the space and is given no work, so no
    /// launch reaches it. Absent from a space that benches nothing.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub benched: BTreeSet<ResourceRef>,
}

impl SessionSpaceAuthoredState {
    pub fn new(id: SessionSpaceRef) -> Self {
        Self {
            version: SESSION_SPACE_APPLICATION_VERSION.into(),
            revision: 0,
            definition: SessionSpaceDefinition::new(id),
            label: None,
            project_contexts: BTreeMap::new(),
            agent_sessions: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            working_surfaces: BTreeMap::new(),
            native_references: BTreeMap::new(),
            focus: None,
            agent_set: None,
            benched: BTreeSet::new(),
        }
    }

    pub fn id(&self) -> &SessionSpaceRef {
        &self.definition.id
    }

    pub fn basis(&self) -> Result<SessionSpaceBasis> {
        let encoded = serde_json::to_vec(self).map_err(|error| {
            AikitError::new(
                "session_space.state_unserializable",
                format!("could not encode SessionSpace state: {error}"),
            )
        })?;
        Ok(SessionSpaceBasis {
            revision: self.revision,
            state_hash: blake3::hash(&encoded).to_hex().to_string(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != SESSION_SPACE_APPLICATION_VERSION {
            return Err(AikitError::new(
                "session_space.unsupported_application_version",
                format!(
                    "unsupported SessionSpace application state {}",
                    self.version
                ),
            ));
        }
        for (project, context) in &self.project_contexts {
            if context.project() != project {
                return Err(AikitError::new(
                    "session_space.project_context_mismatch",
                    format!(
                        "Project {project} is paired with ContextResolution for {}",
                        context.project()
                    ),
                ));
            }
            let membership = ResourceRef::parse(project.as_str())?;
            if !self.definition.projects.contains(&membership) {
                return Err(AikitError::new(
                    "session_space.project_context_without_membership",
                    format!("Project {project} has Context evidence but is not a member"),
                ));
            }
        }
        if let Some(session) = self
            .benched
            .iter()
            .find(|session| !self.agent_sessions.contains_key(*session))
        {
            return Err(bench_unattached(session, self.id()));
        }
        for (binding_ref, binding) in &self.working_surfaces {
            if binding_ref != &binding.binding {
                return Err(AikitError::new(
                    "session_space.working_surface_key_mismatch",
                    "working Surface map key differs from the binding identity",
                ));
            }
            let surface = &binding.surface;
            if !self.surfaces.contains_key(surface) {
                return Err(AikitError::new(
                    "session_space.working_surface_unattached",
                    format!("working Surface {surface} has no SessionSpace Surface attachment"),
                ));
            }
            if !self.agent_sessions.contains_key(&binding.agent_session) {
                return Err(AikitError::new(
                    "session_space.working_surface_agent_session_unattached",
                    format!(
                        "working Surface {surface} names AgentSession {} which is not attached",
                        binding.agent_session
                    ),
                ));
            }
            if !self
                .native_references
                .get(&binding.provider)
                .is_some_and(|reference| {
                    reference.kind == SessionSpaceNativeReferenceKind::Provider
                })
            {
                return Err(AikitError::new(
                    "session_space.working_surface_provider_unbound",
                    format!(
                        "working Surface {surface} names unbound provider {}",
                        binding.provider
                    ),
                ));
            }
            let expected = binding
                .plan
                .views
                .iter()
                .flat_map(|view| {
                    view.steps.iter().map(move |step| {
                        (
                            format!("{}/{}", view.id, step.pane),
                            ResourceRef::parse(format!(
                                "surface/terminal/{}/{}",
                                view.id, step.pane
                            )),
                        )
                    })
                })
                .find_map(|(key, reference)| (key == binding.plan_key).then_some(reference));
            let Some(Ok(expected)) = expected else {
                return Err(AikitError::new(
                    "session_space.working_surface_plan_key_absent",
                    format!(
                        "working Surface {surface} has no plan pane `{}`",
                        binding.plan_key
                    ),
                ));
            };
            if expected != *surface {
                return Err(AikitError::new(
                    "session_space.working_surface_plan_identity_mismatch",
                    format!(
                        "working Surface {surface} does not match the canonical plan Surface {expected} for `{}`",
                        binding.plan_key
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceBasis {
    pub revision: u64,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum SessionSpaceMutation {
    Create {
        id: SessionSpaceRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    BindProjectContext {
        binding: Box<SessionSpaceProjectContextBinding>,
    },
    UnbindProjectContext {
        project: ProjectRef,
    },
    AttachAgentSession {
        attachment: SessionSpaceAgentAttachmentIntent,
    },
    DetachAgentSession {
        agent_session: ResourceRef,
    },
    /// Bench a session this space holds (`benched: true`), or lift its bench:
    /// a benched session stays in the space and is given no work.
    BenchAgentSession {
        agent_session: ResourceRef,
        benched: bool,
    },
    AttachSurface {
        attachment: SessionSpaceSurfaceAttachmentIntent,
    },
    DetachSurface {
        surface: ResourceRef,
    },
    BindWorkingSurface {
        binding: Box<SessionSpaceWorkingSurfaceBinding>,
    },
    UnbindWorkingSurface {
        binding: ResourceRef,
    },
    BindNativeReference {
        binding: SessionSpaceNativeReferenceBinding,
    },
    UnbindNativeReference {
        reference: ResourceRef,
    },
    Focus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<SessionSpaceFocus>,
    },
    /// Name the Central agent set this space runs, replacing any it named.
    BindAgentSet {
        binding: SessionSpaceAgentSetBinding,
    },
    /// Name no agent set.
    UnbindAgentSet,
    Restore {
        target: Box<SessionSpaceAuthoredState>,
        evidence: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum SessionSpaceOperation {
    List,
    Show {
        space: SessionSpaceRef,
    },
    Open {
        space: SessionSpaceRef,
    },
    Discover {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<ProjectRef>,
    },
    Mutate {
        intent: Box<SessionSpaceMutation>,
    },
    Reconcile {
        space: SessionSpaceRef,
    },
    Reconstruct {
        space: SessionSpaceRef,
    },
    Explain {
        space: SessionSpaceRef,
    },
    History {
        space: SessionSpaceRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceChange {
    pub relation: String,
    pub subject: String,
    pub change: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpacePreview {
    pub space: SessionSpaceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<SessionSpaceBasis>,
    pub intent: SessionSpaceMutation,
    pub proposed: SessionSpaceAuthoredState,
    #[serde(default)]
    pub changed: Vec<SessionSpaceChange>,
}

pub fn stage_session_space(
    current: Option<&SessionSpaceAuthoredState>,
    intent: SessionSpaceMutation,
) -> Result<SessionSpacePreview> {
    let (space, basis, mut proposed) = match (&intent, current) {
        (SessionSpaceMutation::Create { id, label }, None) => {
            let mut state = SessionSpaceAuthoredState::new(id.clone());
            state.label = label.clone();
            (id.clone(), None, state)
        }
        (SessionSpaceMutation::Create { id, .. }, Some(_)) => {
            return Err(AikitError::new(
                "session_space.already_exists",
                format!("SessionSpace {id} already exists"),
            ));
        }
        (_, None) => {
            return Err(AikitError::new(
                "session_space.not_found",
                "SessionSpace mutation requires an existing authored state",
            ));
        }
        (_, Some(state)) => (state.id().clone(), Some(state.basis()?), state.clone()),
    };

    let mut changed = Vec::new();
    if !matches!(intent, SessionSpaceMutation::Create { .. }) {
        apply_intent(&mut proposed, &intent, &mut changed)?;
        proposed.revision = proposed.revision.saturating_add(1);
    } else {
        changed.push(SessionSpaceChange {
            relation: "session-space".into(),
            subject: space.to_string(),
            change: "created".into(),
        });
    }
    proposed.validate()?;
    Ok(SessionSpacePreview {
        space,
        basis,
        intent,
        proposed,
        changed,
    })
}

fn apply_intent(
    state: &mut SessionSpaceAuthoredState,
    intent: &SessionSpaceMutation,
    changed: &mut Vec<SessionSpaceChange>,
) -> Result<()> {
    match intent {
        SessionSpaceMutation::Create { .. } => unreachable!("create handled before mutation"),
        SessionSpaceMutation::BindProjectContext { binding } => {
            if binding.context.project() != &binding.project {
                return Err(AikitError::new(
                    "session_space.project_context_mismatch",
                    "Project and ContextResolution evidence identify different Projects",
                ));
            }
            let member = ResourceRef::parse(binding.project.as_str())?;
            state.definition.projects.insert(member);
            state
                .project_contexts
                .insert(binding.project.clone(), binding.context.clone());
            changed.push(change("project-context", binding.project.as_str(), "bound"));
        }
        SessionSpaceMutation::UnbindProjectContext { project } => {
            let member = ResourceRef::parse(project.as_str())?;
            state.definition.projects.remove(&member);
            state.project_contexts.remove(project);
            if state
                .focus
                .as_ref()
                .is_some_and(|focus| focus.target == member)
            {
                state.focus = None;
            }
            changed.push(change("project-context", project.as_str(), "unbound"));
        }
        SessionSpaceMutation::AttachAgentSession { attachment } => {
            state
                .agent_sessions
                .insert(attachment.agent_session.clone(), attachment.clone());
            changed.push(change(
                "agent-session",
                attachment.agent_session.as_str(),
                "attachment-intent-added",
            ));
        }
        SessionSpaceMutation::DetachAgentSession { agent_session } => {
            state.agent_sessions.remove(agent_session);
            state.benched.remove(agent_session);
            if state
                .focus
                .as_ref()
                .is_some_and(|focus| focus.target == *agent_session)
            {
                state.focus = None;
            }
            changed.push(change(
                "agent-session",
                agent_session.as_str(),
                "attachment-intent-removed",
            ));
        }
        SessionSpaceMutation::BenchAgentSession {
            agent_session,
            benched,
        } => {
            if !state.agent_sessions.contains_key(agent_session) {
                return Err(bench_unattached(agent_session, state.id()));
            }
            if *benched {
                state.benched.insert(agent_session.clone());
            } else {
                state.benched.remove(agent_session);
            }
            changed.push(change(
                "agent-session",
                agent_session.as_str(),
                if *benched { "benched" } else { "bench-lifted" },
            ));
        }
        SessionSpaceMutation::AttachSurface { attachment } => {
            state
                .surfaces
                .insert(attachment.surface.clone(), attachment.clone());
            changed.push(change(
                "surface",
                attachment.surface.as_str(),
                "attachment-intent-added",
            ));
        }
        SessionSpaceMutation::DetachSurface { surface } => {
            state.surfaces.remove(surface);
            state
                .working_surfaces
                .retain(|_, binding| &binding.surface != surface);
            if state
                .focus
                .as_ref()
                .is_some_and(|focus| focus.target == *surface)
            {
                state.focus = None;
            }
            changed.push(change(
                "surface",
                surface.as_str(),
                "attachment-intent-removed",
            ));
        }
        SessionSpaceMutation::BindWorkingSurface { binding } => {
            state
                .working_surfaces
                .insert(binding.binding.clone(), (**binding).clone());
            changed.push(change(
                "working-surface",
                binding.binding.as_str(),
                "provider-binding-added",
            ));
        }
        SessionSpaceMutation::UnbindWorkingSurface { binding } => {
            state.working_surfaces.remove(binding);
            changed.push(change(
                "working-surface",
                binding.as_str(),
                "provider-binding-removed",
            ));
        }
        SessionSpaceMutation::BindNativeReference { binding } => {
            state
                .native_references
                .insert(binding.reference.clone(), binding.clone());
            changed.push(change(
                "native-reference",
                binding.reference.as_str(),
                "bound",
            ));
        }
        SessionSpaceMutation::UnbindNativeReference { reference } => {
            state.native_references.remove(reference);
            if state
                .focus
                .as_ref()
                .is_some_and(|focus| focus.target == *reference)
            {
                state.focus = None;
            }
            changed.push(change("native-reference", reference.as_str(), "unbound"));
        }
        SessionSpaceMutation::Focus { focus } => {
            state.focus = focus.clone();
            let subject = focus
                .as_ref()
                .map(|focus| focus.target.as_str())
                .unwrap_or("none");
            changed.push(change("focus", subject, "selected"));
        }
        SessionSpaceMutation::BindAgentSet { binding } => {
            binding.validate()?;
            state.agent_set = Some(binding.clone());
            changed.push(change("agent-set", binding.agent_set_ref.as_str(), "bound"));
        }
        SessionSpaceMutation::UnbindAgentSet => {
            let subject = state
                .agent_set
                .take()
                .map(|binding| binding.agent_set_ref)
                .unwrap_or_else(|| "none".into());
            changed.push(change("agent-set", subject, "unbound"));
        }
        SessionSpaceMutation::Restore { target, evidence } => {
            if target.id() != state.id() {
                return Err(AikitError::new(
                    "session_space.restore_identity_mismatch",
                    "historical SessionSpace identity does not match current identity",
                ));
            }
            let next_revision = state.revision;
            *state = (**target).clone();
            state.revision = next_revision;
            state
                .definition
                .provenance
                .push(format!("restored from {evidence}"));
            changed.push(change(
                "session-space",
                state.id().to_string(),
                "historical-state-restored",
            ));
        }
    }
    Ok(())
}

/// What Drag out stages for the space a session goes to (O-I manual §19.15,
/// the Agent rack's chain list: "Drag a chain out of the list onto an empty
/// track and it works there alone, chain and all"): the new space, the same
/// Project memberships on the same ContextResolution evidence, the session's
/// own attachment (and its bench, where it is benched), in apply order. The
/// owner applies these to `into` first and detaches the session from `from`
/// last, so the session never stands in no space of its Project. Refused in
/// words: a session `from` does not hold, the only session of `from` (it
/// already works there alone), the same space, a working surface that binds it
/// in `from`, and a membership `from` holds without its Context evidence.
pub fn drag_out_intents(
    from: &SessionSpaceAuthoredState,
    agent_session: &ResourceRef,
    into: SessionSpaceRef,
    label: Option<String>,
) -> Result<Vec<SessionSpaceMutation>> {
    let refused = |code: &'static str, message: String| Err(AikitError::new(code, message));
    let Some(attachment) = from.agent_sessions.get(agent_session) else {
        return refused(
            "session_space.drag_out_unattached",
            format!(
                "{agent_session} is not a session of {}, so there is nothing of it to drag out",
                from.id()
            ),
        );
    };
    if &into == from.id() {
        return refused(
            "session_space.drag_out_same_space",
            format!("{agent_session} is dragged out of {into} into a space of its own, not into the same one"),
        );
    }
    if from.agent_sessions.len() == 1 {
        return refused(
            "session_space.drag_out_alone",
            format!(
                "{agent_session} is the only session of {}: it already works there alone",
                from.id()
            ),
        );
    }
    if let Some(binding) = from
        .working_surfaces
        .values()
        .find(|binding| &binding.agent_session == agent_session)
    {
        return refused(
            "session_space.drag_out_working_surface",
            format!(
                "{agent_session} works in the surface {} of {}; unbind that working surface before dragging the session out",
                binding.binding,
                from.id()
            ),
        );
    }
    let mut intents = vec![SessionSpaceMutation::Create { id: into, label }];
    for member in &from.definition.projects {
        let project = ProjectRef::parse(member.as_str())?;
        let Some(context) = from.project_contexts.get(&project) else {
            return refused(
                "session_space.drag_out_membership_without_context",
                format!(
                    "{} is a member of Project {project} without its Context evidence, so a space of its own cannot hold that Project for {agent_session}",
                    from.id()
                ),
            );
        };
        intents.push(SessionSpaceMutation::BindProjectContext {
            binding: Box::new(SessionSpaceProjectContextBinding::new(
                project,
                context.clone(),
            )?),
        });
    }
    let mut carried = attachment.clone();
    carried
        .provenance
        .push(format!("dragged out of {}", from.id()));
    intents.push(SessionSpaceMutation::AttachAgentSession {
        attachment: carried,
    });
    if from.benched.contains(agent_session) {
        intents.push(SessionSpaceMutation::BenchAgentSession {
            agent_session: agent_session.clone(),
            benched: true,
        });
    }
    Ok(intents)
}

/// A bench names only a session the space holds.
fn bench_unattached(session: &ResourceRef, space: &SessionSpaceRef) -> AikitError {
    AikitError::new(
        "session_space.bench_unattached",
        format!(
            "{session} is not a session of {space}, so it cannot be benched there; attach it first"
        ),
    )
}

fn change(relation: &str, subject: impl Into<String>, value: &str) -> SessionSpaceChange {
    SessionSpaceChange {
        relation: relation.into(),
        subject: subject.into(),
        change: value.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservedRelationState {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceNativeObservation {
    pub reference: ResourceRef,
    pub state: ObservedRelationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Explicit proof supplied by the AgentSession owner. Transport/provider
/// reconnection is intentionally insufficient by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionContinuityEvidence {
    pub agent_session: ResourceRef,
    pub continuous: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReconstructionStatus {
    RestoredCanonical,
    Reobserved,
    Reestablished,
    Unavailable,
    Degraded,
    IrrecoverableProviderDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconstructionRelation {
    pub relation: String,
    pub reference: String,
    pub status: ReconstructionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceReconstructionReport {
    pub space: SessionSpaceRef,
    pub semantic_revision: u64,
    pub relations: Vec<ReconstructionRelation>,
    /// Provider window/pane/layout geometry is observation, never canonical.
    pub provider_native_detail: ReconstructionStatus,
}

pub fn reconstruct_session_space(
    authored: &SessionSpaceAuthoredState,
    runtime: Option<&SessionSpaceReadModel>,
    native_observations: &[SessionSpaceNativeObservation],
    continuity: &[AgentSessionContinuityEvidence],
) -> SessionSpaceReconstructionReport {
    let mut relations = Vec::new();

    for (project, context) in &authored.project_contexts {
        relations.push(ReconstructionRelation {
            relation: "project-context".into(),
            reference: format!("{} -> {}", project.as_str(), context.reference),
            status: ReconstructionStatus::RestoredCanonical,
            reason: Some("canonical authored Project/ContextResolution evidence restored".into()),
            evidence: context.provenance.clone(),
        });
    }

    let runtime_sessions: BTreeSet<_> = runtime
        .into_iter()
        .flat_map(|view| {
            view.agent_sessions
                .iter()
                .map(|binding| binding.agent_session.clone())
        })
        .collect();
    let continuity: BTreeMap<_, _> = continuity
        .iter()
        .map(|evidence| (evidence.agent_session.clone(), evidence))
        .collect();
    for session in authored.agent_sessions.keys() {
        let connected = runtime_sessions.contains(session);
        let proof = continuity.get(session);
        let status = if connected && proof.is_some_and(|proof| proof.continuous) {
            ReconstructionStatus::Reestablished
        } else if connected {
            ReconstructionStatus::Degraded
        } else {
            ReconstructionStatus::Unavailable
        };
        let reason = match status {
            ReconstructionStatus::Reestablished => {
                Some("AgentSession owner supplied explicit continuity evidence".into())
            }
            ReconstructionStatus::Degraded => Some(
                "transport/provider reconnection observed, but AgentSession continuity is unproven"
                    .into(),
            ),
            _ => {
                Some("authored AgentSession attachment intent restored; live session absent".into())
            }
        };
        relations.push(ReconstructionRelation {
            relation: "agent-session".into(),
            reference: session.to_string(),
            status,
            reason,
            evidence: proof
                .map(|proof| proof.provenance.clone())
                .unwrap_or_default(),
        });
    }

    let runtime_surfaces: BTreeSet<_> = runtime
        .into_iter()
        .flat_map(|view| view.surfaces.iter().map(|surface| surface.surface.clone()))
        .collect();
    for surface in authored.surfaces.keys() {
        let observed = runtime_surfaces.contains(surface);
        relations.push(ReconstructionRelation {
            relation: "surface".into(),
            reference: surface.to_string(),
            status: if observed {
                ReconstructionStatus::Reobserved
            } else {
                ReconstructionStatus::Unavailable
            },
            reason: Some(if observed {
                "canonical Surface identity was re-observed in provider runtime".into()
            } else {
                "Surface attachment intent restored; provider runtime has not re-observed it".into()
            }),
            evidence: Vec::new(),
        });
    }

    let observations: BTreeMap<_, _> = native_observations
        .iter()
        .map(|observation| (observation.reference.clone(), observation))
        .collect();
    for binding in authored.native_references.values() {
        let observation = observations.get(&binding.reference);
        let (status, reason) = match observation.map(|observation| observation.state) {
            Some(ObservedRelationState::Available) => (
                ReconstructionStatus::Reobserved,
                Some("same canonical native reference re-observed".into()),
            ),
            Some(ObservedRelationState::Degraded) => (
                ReconstructionStatus::Degraded,
                observation.and_then(|observation| observation.reason.clone()),
            ),
            Some(ObservedRelationState::Unavailable) => (
                ReconstructionStatus::Unavailable,
                observation.and_then(|observation| observation.reason.clone()),
            ),
            None => (
                ReconstructionStatus::Unavailable,
                Some("canonical reference restored; native provider/material not observed".into()),
            ),
        };
        relations.push(ReconstructionRelation {
            relation: format!("{:?}", binding.kind).to_ascii_lowercase(),
            reference: binding.reference.to_string(),
            status,
            reason,
            evidence: binding.provenance.clone(),
        });
    }

    SessionSpaceReconstructionReport {
        space: authored.id().clone(),
        semantic_revision: authored.revision,
        relations,
        provider_native_detail: ReconstructionStatus::IrrecoverableProviderDetail,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceExplanation {
    pub space: SessionSpaceRef,
    pub semantic_revision: u64,
    pub authored_projects: Vec<ProjectRef>,
    pub context_evidence: Vec<ContextResolutionEvidence>,
    pub authored_agent_sessions: Vec<ResourceRef>,
    pub authored_surfaces: Vec<ResourceRef>,
    pub native_references: Vec<SessionSpaceNativeReferenceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<SessionSpaceFocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconstruction: Option<SessionSpaceReconstructionReport>,
    pub authority: String,
    pub live_vs_persisted: String,
}

pub fn explain_session_space(
    authored: &SessionSpaceAuthoredState,
    reconstruction: Option<SessionSpaceReconstructionReport>,
) -> SessionSpaceExplanation {
    SessionSpaceExplanation {
        space: authored.id().clone(),
        semantic_revision: authored.revision,
        authored_projects: authored.project_contexts.keys().cloned().collect(),
        context_evidence: authored.project_contexts.values().cloned().collect(),
        authored_agent_sessions: authored.agent_sessions.keys().cloned().collect(),
        authored_surfaces: authored.surfaces.keys().cloned().collect(),
        native_references: authored.native_references.values().cloned().collect(),
        focus: authored.focus.clone(),
        reconstruction,
        authority: "AIKit owns SessionSpace semantic bindings; Project/Context resolver, AgentSession, Surface, provider, Host and Workcell/material identities remain native-owner identities".into(),
        live_vs_persisted: "persistent intent is not live provider truth; SessionSpaceRuntime/provider observations are evidence and are never authored by this explanation".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectBindingLocator, ProjectConstituentRef};

    fn project(name: &str) -> ProjectRef {
        ProjectRef::parse(&format!("project:{name}")).unwrap()
    }

    fn evidence(name: &str, origin: &str, hash: &str) -> ContextResolutionEvidence {
        let project = project(name);
        let binding = ProjectBinding::new(
            project,
            ProjectConstituentRef::parse("source:working-tree").unwrap(),
            ProjectBindingLocator::Remote {
                locator: format!("https://example.invalid/{name}"),
            },
        );
        let basis = ContextResolutionBasis {
            project_binding: binding,
            resolver_hash: hash.into(),
            catalog_revision: "catalog-1".into(),
            scopes: vec![ScopeResolution {
                kind: crate::scope::ScopeKind::Project,
                depth: 0,
                origin: origin.into(),
            }],
            context_sources: vec![ResourceRef::parse(&format!("context-source/{name}")).unwrap()],
            host: None,
            context_activations: Vec::new(),
            observed_source_resources: Vec::new(),
        };
        let bytes = serde_json::to_vec(&basis).unwrap();
        let digest = blake3::hash(&bytes).to_hex().to_string();
        ContextResolutionEvidence {
            reference: ContextResolutionRef(
                ResourceRef::parse(&format!("context-resolution/{}", &digest[..16])).unwrap(),
            ),
            basis,
            provenance: vec![origin.into()],
        }
    }

    /// A space of `project` holding `sessions`, applied mutation by mutation.
    fn rack(name: &str, sessions: &[&str]) -> SessionSpaceAuthoredState {
        let id = SessionSpaceRef::parse(&format!("session-space/{name}")).unwrap();
        let mut state = stage_session_space(
            None,
            SessionSpaceMutation::Create {
                id,
                label: Some(name.into()),
            },
        )
        .unwrap()
        .proposed;
        let binding =
            SessionSpaceProjectContextBinding::new(project("o-i"), evidence("o-i", "rack", "h1"))
                .unwrap();
        state = stage_session_space(
            Some(&state),
            SessionSpaceMutation::BindProjectContext {
                binding: Box::new(binding),
            },
        )
        .unwrap()
        .proposed;
        for session in sessions {
            let attachment = SessionSpaceAgentAttachmentIntent {
                agent_session: ResourceRef::parse(&format!("agent-session/{session}")).unwrap(),
                purpose: Some(format!("{session}'s chain")),
                provenance: vec!["seated".into()],
            };
            state = stage_session_space(
                Some(&state),
                SessionSpaceMutation::AttachAgentSession { attachment },
            )
            .unwrap()
            .proposed;
        }
        state
    }

    #[test]
    fn drag_out_seats_the_session_alone_in_a_space_of_its_own_in_the_same_project() {
        // O-I manual §19.15: "Drag a chain out of the list onto an empty track and it works there alone, chain and all."
        let from = rack("crew", &["ada", "cy"]);
        let ada = ResourceRef::parse("agent-session/ada").unwrap();
        let from = stage_session_space(
            Some(&from),
            SessionSpaceMutation::BenchAgentSession {
                agent_session: ada.clone(),
                benched: true,
            },
        )
        .unwrap()
        .proposed;
        let into = SessionSpaceRef::parse("session-space/ada-alone").unwrap();
        let intents =
            drag_out_intents(&from, &ada, into.clone(), Some("ada alone".into())).unwrap();
        let mut state: Option<SessionSpaceAuthoredState> = None;
        for intent in intents {
            state = Some(
                stage_session_space(state.as_ref(), intent)
                    .unwrap()
                    .proposed,
            );
        }
        let alone = state.unwrap();
        assert_eq!(alone.id(), &into);
        assert_eq!(alone.label.as_deref(), Some("ada alone"));
        assert_eq!(
            alone.agent_sessions.keys().collect::<Vec<_>>(),
            vec![&ada],
            "it works there alone"
        );
        assert_eq!(
            alone.agent_sessions[&ada].purpose.as_deref(),
            Some("ada's chain"),
            "chain and all"
        );
        assert!(alone.agent_sessions[&ada]
            .provenance
            .iter()
            .any(|p| p == "dragged out of session-space/crew"));
        assert!(
            alone.benched.contains(&ada),
            "a benched chain stays benched"
        );
        assert_eq!(
            alone.project_contexts, from.project_contexts,
            "the same Project, on the same evidence"
        );
        assert_eq!(alone.definition.projects, from.definition.projects);
    }

    #[test]
    fn drag_out_refuses_in_words_what_it_cannot_carry() {
        let from = rack("crew", &["ada", "cy"]);
        let into = || SessionSpaceRef::parse("session-space/out").unwrap();
        let nobody = ResourceRef::parse("agent-session/nobody").unwrap();
        let refused = drag_out_intents(&from, &nobody, into(), None).unwrap_err();
        assert_eq!(refused.code(), "session_space.drag_out_unattached");
        assert!(
            refused
                .to_string()
                .contains("agent-session/nobody is not a session of session-space/crew"),
            "{refused}"
        );
        let lone = rack("lone", &["ada"]);
        let ada = ResourceRef::parse("agent-session/ada").unwrap();
        let refused = drag_out_intents(&lone, &ada, into(), None).unwrap_err();
        assert_eq!(refused.code(), "session_space.drag_out_alone");
        let same = drag_out_intents(&from, &ada, from.id().clone(), None).unwrap_err();
        assert_eq!(same.code(), "session_space.drag_out_same_space");
        let mut bare = from.clone();
        bare.project_contexts.clear();
        let refused = drag_out_intents(&bare, &ada, into(), None).unwrap_err();
        assert_eq!(
            refused.code(),
            "session_space.drag_out_membership_without_context"
        );
    }

    #[test]
    fn two_projects_keep_independent_context_provenance() {
        let id = SessionSpaceRef::parse("session-space/two-projects").unwrap();
        let create = stage_session_space(
            None,
            SessionSpaceMutation::Create {
                id,
                label: Some("two projects".into()),
            },
        )
        .unwrap();
        let mut state = create.proposed;
        for (name, origin, hash) in [
            ("a", "/a/.aikit/profile.toml", "hash-a"),
            ("b", "/b/.aikit/profile.toml", "hash-b"),
        ] {
            let project = project(name);
            state = stage_session_space(
                Some(&state),
                SessionSpaceMutation::BindProjectContext {
                    binding: Box::new(
                        SessionSpaceProjectContextBinding::new(
                            project,
                            evidence(name, origin, hash),
                        )
                        .unwrap(),
                    ),
                },
            )
            .unwrap()
            .proposed;
        }
        assert_eq!(state.project_contexts.len(), 2);
        assert_ne!(
            state.project_contexts[&project("a")].reference,
            state.project_contexts[&project("b")].reference
        );
        assert_eq!(
            state.project_contexts[&project("a")].basis.scopes[0].origin,
            "/a/.aikit/profile.toml"
        );
        assert_eq!(
            state.project_contexts[&project("b")].basis.scopes[0].origin,
            "/b/.aikit/profile.toml"
        );
    }

    #[test]
    fn staging_is_write_free_and_basis_moves_only_in_proposed_state() {
        let id = SessionSpaceRef::parse("session-space/stage").unwrap();
        let current = stage_session_space(None, SessionSpaceMutation::Create { id, label: None })
            .unwrap()
            .proposed;
        let before = current.clone();
        let preview = stage_session_space(
            Some(&current),
            SessionSpaceMutation::Focus {
                focus: Some(SessionSpaceFocus {
                    target: ResourceRef::parse("project:a").unwrap(),
                    region: Some("editor".into()),
                    provenance: vec!["user".into()],
                }),
            },
        )
        .unwrap();
        assert_eq!(current, before);
        assert_eq!(preview.basis.unwrap().revision, 0);
        assert_eq!(preview.proposed.revision, 1);
    }

    #[test]
    fn provider_reconnect_does_not_prove_agent_session_continuity() {
        let id = SessionSpaceRef::parse("session-space/reconnect").unwrap();
        let mut state = SessionSpaceAuthoredState::new(id);
        let agent = ResourceRef::parse("agent-session/one").unwrap();
        state.agent_sessions.insert(
            agent.clone(),
            SessionSpaceAgentAttachmentIntent {
                agent_session: agent.clone(),
                purpose: None,
                provenance: vec!["authored".into()],
            },
        );
        let runtime = SessionSpaceReadModel {
            version: crate::session_space::SESSION_SPACE_VERSION.into(),
            id: state.id().clone(),
            lifecycle: crate::session_space::SessionSpaceLifecycle::Open,
            revision: 1,
            projects: Vec::new(),
            agent_sessions: vec![crate::session_space::SessionSpaceAgentSession {
                agent_session: agent.clone(),
                harness: ResourceRef::parse("harness/pi").unwrap(),
                native_session_id: Some("transport-returned".into()),
                provider: Some(ResourceRef::parse("provider/acp").unwrap()),
                provenance: vec!["provider reconnect".into()],
            }],
            components: Vec::new(),
            surfaces: Vec::new(),
            connections: Vec::new(),
            provenance: Vec::new(),
        };
        let report = reconstruct_session_space(&state, Some(&runtime), &[], &[]);
        let relation = report
            .relations
            .iter()
            .find(|relation| relation.reference == agent.as_str())
            .unwrap();
        assert_eq!(relation.status, ReconstructionStatus::Degraded);
        assert!(relation
            .reason
            .as_deref()
            .unwrap()
            .contains("continuity is unproven"));
    }

    #[test]
    fn same_canonical_native_reference_can_be_reobserved_without_identity_transfer() {
        let id = SessionSpaceRef::parse("session-space/native").unwrap();
        let mut state = SessionSpaceAuthoredState::new(id);
        let material = ResourceRef::parse("material/workcell-a/run-7").unwrap();
        state.native_references.insert(
            material.clone(),
            SessionSpaceNativeReferenceBinding {
                reference: material.clone(),
                kind: SessionSpaceNativeReferenceKind::Material,
                owner: Some(ResourceRef::parse("workcell/a").unwrap()),
                provider: None,
                host: Some(ResourceRef::parse("host/worker").unwrap()),
                purpose: Some("preview".into()),
                provenance: vec!["Workcell reading".into()],
            },
        );
        let report = reconstruct_session_space(
            &state,
            None,
            &[SessionSpaceNativeObservation {
                reference: material.clone(),
                state: ObservedRelationState::Available,
                provider: Some(ResourceRef::parse("provider/docker").unwrap()),
                reason: None,
            }],
            &[],
        );
        let relation = report
            .relations
            .iter()
            .find(|relation| relation.reference == material.as_str())
            .unwrap();
        assert_eq!(relation.status, ReconstructionStatus::Reobserved);
        assert_eq!(
            report.provider_native_detail,
            ReconstructionStatus::IrrecoverableProviderDetail
        );
    }
}
