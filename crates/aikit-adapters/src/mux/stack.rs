//! Hybrid multiplexer stacks: an outer presentation host showing an inner
//! topology owner.
//!
//! The configuration that motivates this module is a cmux workspace holding an
//! ssh session attached to a tmux on a build box. Two multiplexers are live, and
//! treating either one as "the" multiplexer produces a different wrong answer:
//! split a cmux workspace and the new pane appears next to the ssh session rather
//! than beside the code; ask tmux for a workspace pill and nothing happens.
//!
//! So the stack is modelled explicitly, innermost first, and each operation is
//! routed by what it *means*:
//!
//! * **topology** — sessions, panes, focus, close — goes to the **innermost**
//!   mux. That is the one whose surfaces the user is looking at.
//! * **the palette** goes to the innermost layer that has a real popup, which in
//!   practice means tmux wins whenever a tmux is anywhere in the stack.
//! * **status** fans **outwards**: a workspace pill lives on the presentation
//!   host, and a tmux status line lives on the inner one, and both are useful.
//! * `--mux` **overrides** detection and collapses the stack to the named layer.
//!
//! ## The remote boundary
//!
//! When the inner mux is on another host, its registry is the one in play. Local
//! and remote catalogues are never merged: a capsule id means different content
//! on two machines, and a resolution that silently drew from both would be
//! unexplainable and unreproducible. [`combine_registries`] refuses the mix by
//! construction rather than by convention.

use aikit_core::platform::MuxKind;
use aikit_core::session::SessionPlan;
use aikit_core::{AikitError, Result};

use super::{
    MuxAdapter, MuxCapabilities, MuxLocation, MuxPresence, MuxTarget, Notification, PaletteRequest,
    ReconcileMode, SessionBinding, SpawnRequest, SpawnedTarget, StatusUpdate, UiHost,
};

// ---------------------------------------------------------------------------
// Registries
// ---------------------------------------------------------------------------

/// Which machine's registry a context resolves against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveRegistry {
    Local,
    Remote { host: String },
}

impl EffectiveRegistry {
    pub fn is_remote(&self) -> bool {
        matches!(self, EffectiveRegistry::Remote { .. })
    }

    pub fn host(&self) -> Option<&str> {
        match self {
            EffectiveRegistry::Local => None,
            EffectiveRegistry::Remote { host } => Some(host),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            EffectiveRegistry::Local => "the local registry".to_string(),
            EffectiveRegistry::Remote { host } => format!("the registry on {host}"),
        }
    }
}

/// Two registry scopes, or a refusal.
///
/// There is deliberately no "merge" behaviour. A capsule id identifies different
/// content on two machines, so a view assembled from both would be neither
/// explainable nor reproducible — and the failure would show up as an agent
/// behaving oddly, long after the mixing happened.
pub fn combine_registries(
    a: &EffectiveRegistry,
    b: &EffectiveRegistry,
) -> Result<EffectiveRegistry> {
    if a == b {
        return Ok(a.clone());
    }
    let describe = |r: &EffectiveRegistry| match r {
        EffectiveRegistry::Local => "local".to_string(),
        EffectiveRegistry::Remote { host } => host.clone(),
    };
    Err(AikitError::new(
        "registry.cross_host_mix",
        format!(
            "the {} registry and the {} registry cannot be combined; a capsule id means \
             different content on different machines, so one host's registry has to be chosen",
            describe(a),
            describe(b)
        ),
    )
    .with("left", describe(a))
    .with("right", describe(b)))
}

/// The inner mux is somewhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBoundary {
    pub host: String,
}

impl RemoteBoundary {
    pub fn new(host: impl Into<String>) -> Self {
        Self { host: host.into() }
    }
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

/// One multiplexer in the stack.
pub struct StackLayer {
    pub adapter: Box<dyn MuxAdapter>,
    pub presence: MuxPresence,
    pub capabilities: MuxCapabilities,
}

impl StackLayer {
    pub fn kind(&self) -> MuxKind {
        self.presence.kind
    }
}

/// What became of one layer's share of a fanned-out status update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusDelivery {
    pub kind: MuxKind,
    pub delivered: bool,
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// The stack
// ---------------------------------------------------------------------------

/// The multiplexers in play, innermost first.
pub struct MuxStack {
    layers: Vec<StackLayer>,
    remote: Option<RemoteBoundary>,
    project: Option<String>,
}

/// A `MuxAdapter` cannot be `Debug` without every adapter opting in, so the
/// stack renders the part that is actually informative: which multiplexers it
/// found, in which order, and whether they are somewhere else.
impl std::fmt::Debug for MuxStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuxStack")
            .field("layers", &self.kinds())
            .field("remote", &self.remote.as_ref().map(|r| r.host.as_str()))
            .field("project", &self.project)
            .finish()
    }
}

impl MuxStack {
    /// Work out which of the candidate multiplexers we are actually inside.
    ///
    /// `candidates` are supplied outermost-first (cmux before tmux), because that
    /// is the order they nest in; the stack stores them the other way round, so
    /// that "innermost" is `layers[0]` everywhere below.
    ///
    /// `forced` is `--mux`. It collapses the stack to the named layer, which is
    /// the point of the flag: the user is saying which one they mean.
    pub fn detect(candidates: Vec<Box<dyn MuxAdapter>>, forced: Option<MuxKind>) -> Result<Self> {
        let mut inside: Vec<StackLayer> = Vec::new();
        let mut available: Vec<StackLayer> = Vec::new();

        for adapter in candidates {
            let presence = adapter.detect()?;
            let capabilities = adapter.capabilities();
            let layer = StackLayer {
                adapter,
                presence,
                capabilities,
            };
            if layer.presence.inside {
                inside.push(layer);
            } else if layer.presence.is_usable() {
                available.push(layer);
            }
        }

        if let Some(kind) = forced {
            let chosen = inside
                .into_iter()
                .chain(available)
                .find(|l| l.kind() == kind)
                .ok_or_else(|| {
                    AikitError::new(
                        "mux.not_available",
                        format!(
                            "`--mux {kind}` was asked for, but no usable {kind} was found here"
                        ),
                    )
                    .with("mux", kind.as_str())
                })?;
            return Ok(Self {
                layers: vec![chosen],
                remote: None,
                project: None,
            });
        }

        // Plain is the fallback terminal we are already in, not a real layer
        // wrapped around tmux/cmux. Keeping it beside an active mux makes it look
        // like a presentation host and fans status into a terminal that owns no
        // such surface.
        if inside.iter().any(|layer| layer.kind() != MuxKind::Plain) {
            inside.retain(|layer| layer.kind() != MuxKind::Plain);
        }

        if inside.is_empty() {
            // Nothing said we were inside it. A plain terminal is the honest
            // answer, and a caller that supplied one gets it.
            let fallback = available
                .into_iter()
                .find(|l| l.kind() == MuxKind::Plain)
                .ok_or_else(|| {
                    AikitError::new(
                        "mux.not_available",
                        "no multiplexer reported that this process is inside it, and no plain \
                         terminal adapter was offered as a fallback",
                    )
                })?;
            return Ok(Self {
                layers: vec![fallback],
                remote: None,
                project: None,
            });
        }

        // Candidates arrive outermost-first; the stack is indexed innermost-first.
        inside.reverse();
        Ok(Self {
            layers: inside,
            remote: None,
            project: None,
        })
    }

    #[must_use]
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// The same stack, but reached across a remote boundary.
    #[must_use]
    pub fn across(mut self, boundary: RemoteBoundary) -> Self {
        self.remote = Some(boundary);
        self
    }

    // -----------------------------------------------------------------------
    // Shape
    // -----------------------------------------------------------------------

    pub fn layers(&self) -> &[StackLayer] {
        &self.layers
    }

    /// Innermost first.
    pub fn kinds(&self) -> Vec<MuxKind> {
        self.layers.iter().map(StackLayer::kind).collect()
    }

    /// The multiplexer whose panes the user is looking at.
    pub fn topology(&self) -> &dyn MuxAdapter {
        // `detect` never produces an empty stack: every path either finds a layer
        // or returns an error.
        &*self.layers[0].adapter
    }

    pub fn topology_kind(&self) -> MuxKind {
        self.layers[0].kind()
    }

    /// The outermost host, when it is not also the topology owner.
    pub fn presentation(&self) -> Option<&dyn MuxAdapter> {
        (self.layers.len() > 1).then(|| &*self.layers[self.layers.len() - 1].adapter)
    }

    pub fn presentation_kind(&self) -> Option<MuxKind> {
        (self.layers.len() > 1).then(|| self.layers[self.layers.len() - 1].kind())
    }

    pub fn is_hybrid(&self) -> bool {
        self.layers.len() > 1
    }

    /// `payments · staging-box · tmux · presented by cmux`.
    ///
    /// The host segment appears only when the session is somewhere else, and it
    /// appears *before* the multiplexer, because "which machine" is the question
    /// a person needs answered before "which multiplexer".
    pub fn describe_location(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(project) = &self.project {
            parts.push(project.clone());
        }
        if let Some(remote) = &self.remote {
            parts.push(remote.host.clone());
        }
        parts.push(self.topology_kind().to_string());
        if let Some(outer) = self.presentation_kind() {
            parts.push(format!("presented by {outer}"));
        }
        parts.join(" · ")
    }

    pub fn effective_registry(&self) -> EffectiveRegistry {
        match &self.remote {
            Some(boundary) => EffectiveRegistry::Remote {
                host: boundary.host.clone(),
            },
            None => EffectiveRegistry::Local,
        }
    }

    /// What the user should be told about this stack before they act on it.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(remote) = &self.remote {
            out.push(format!(
                "this session's topology lives on {}, so capabilities resolve against that \
                 machine's registry; the local registry is not in play here",
                remote.host
            ));
        }
        out
    }

    pub fn current_location(&self) -> Result<MuxLocation> {
        let mut location = self.topology().current_location()?;
        location.project = self.project.clone();
        if let Some(remote) = &self.remote {
            location.host = remote.host.clone();
            location.remote = true;
        }
        Ok(location)
    }

    pub fn session_exists(&self, plan: &SessionPlan) -> Result<bool> {
        self.topology().session_exists(plan)
    }

    pub fn inspect_session(&self, plan: &SessionPlan) -> Result<SessionBinding> {
        let mut binding = self.topology().inspect_session(plan)?;
        binding.warnings.extend(self.warnings());
        Ok(binding)
    }

    // -----------------------------------------------------------------------
    // Topology goes inwards
    // -----------------------------------------------------------------------

    pub fn ensure_session(
        &self,
        plan: &SessionPlan,
        mode: ReconcileMode,
    ) -> Result<SessionBinding> {
        let mut binding = self.topology().ensure_session(plan, mode)?;
        binding.warnings.extend(self.warnings());
        Ok(binding)
    }

    pub fn spawn(&self, request: SpawnRequest) -> Result<SpawnedTarget> {
        self.topology().spawn(request)
    }

    pub fn focus(&self, target: &MuxTarget) -> Result<()> {
        self.topology().focus(target)
    }

    pub fn close(&self, target: &MuxTarget) -> Result<()> {
        self.topology().close(target)
    }

    // -----------------------------------------------------------------------
    // The palette goes to the best host
    // -----------------------------------------------------------------------

    /// Open the palette in the innermost layer that has a real popup.
    ///
    /// Inside a tmux inside a cmux this is tmux's `display-popup`: a genuine
    /// overlay over the pane the user is in beats a new surface elsewhere, even
    /// though the outer host is the one drawing the pixels.
    pub fn open_palette(&self, request: PaletteRequest) -> Result<UiHost> {
        match self.layers.iter().find(|l| l.capabilities.true_popup) {
            Some(layer) => layer.adapter.open_palette(request),
            None => self.topology().open_palette(request),
        }
    }

    // -----------------------------------------------------------------------
    // Status fans outwards
    // -----------------------------------------------------------------------

    /// Tell every layer that has somewhere to put it.
    ///
    /// Returns one entry per layer, including the ones that could not take it: a
    /// caller that cannot tell where its status went cannot tell the user either.
    pub fn set_status(&self, status: StatusUpdate) -> Result<Vec<StatusDelivery>> {
        let mut out = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            if !layer.capabilities.status_metadata {
                out.push(StatusDelivery {
                    kind: layer.kind(),
                    delivered: false,
                    note: Some(format!(
                        "{} has no status surface to put this on",
                        layer.kind()
                    )),
                });
                continue;
            }
            layer.adapter.set_status(status.clone())?;
            out.push(StatusDelivery {
                kind: layer.kind(),
                delivered: true,
                note: None,
            });
        }
        Ok(out)
    }

    /// Notify through the outermost layer that can, falling inwards.
    ///
    /// The opposite direction from topology on purpose: a notification is for the
    /// person, and the person is looking at the outermost surface.
    pub fn notify(&self, notification: Notification) -> Result<()> {
        for layer in self.layers.iter().rev() {
            if layer.capabilities.notifications {
                return layer.adapter.notify(notification);
            }
        }
        self.topology().notify(notification)
    }
}
