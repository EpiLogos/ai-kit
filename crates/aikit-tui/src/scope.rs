//! The mutation scope: where a toggle would be written.
//!
//! Two rules, both of them core's, and neither of them re-decided here.
//!
//! * The starting scope is [`ContextDescriptor::default_mutation_scope`]. Inside
//!   a task, the task overlay; inside a session, the session overlay; inside a
//!   project with no session, the private project-local file; outside a project,
//!   the global profile.
//! * `Tab` cycles [`ContextDescriptor::permitted_scopes`] and nothing else. A
//!   palette that offered `Project` in a directory that is not a repository would
//!   be offering to create a file nobody asked for.
//!
//! ## Why the confirmation cannot be a preference
//!
//! `<repo>/.aikit/profile.toml` is committed. Writing it changes what every
//! colleague's palette resolves, and it does so through a file review will see
//! later and the user will not. A session overlay is the opposite: private,
//! disposable, gone when the session is. Making both cost one keystroke would
//! make them the same scope with different names, so
//! [`ScopeSelector::confirmation`] returns `Some` for exactly the scopes core
//! marks [`ScopeKind::requires_confirmation_to_write`], and the reducer has no
//! path from a staged set to an apply at those scopes that does not pass through
//! it.

use aikit_core::context::ContextDescriptor;
use aikit_core::error::AikitError;
use aikit_core::scope::ScopeKind;
use aikit_core::Result;

/// The scope the palette will write to, and the ones `Tab` can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSelector {
    current: ScopeKind,
    permitted: Vec<ScopeKind>,
}

impl ScopeSelector {
    /// Start where this context says changes belong.
    pub fn for_context(descriptor: &ContextDescriptor) -> Self {
        let permitted = descriptor.permitted_scopes();
        let current = descriptor.default_mutation_scope();
        // `default_mutation_scope` is derived from the same fields as
        // `permitted_scopes`, so the two agree; falling back to the first
        // permitted scope keeps a future divergence from producing a selector
        // pointing somewhere `Tab` cannot reach.
        let current = if permitted.contains(&current) {
            current
        } else {
            permitted.first().copied().unwrap_or(ScopeKind::Global)
        };
        Self { current, permitted }
    }

    /// Start at an explicitly requested scope.
    pub fn with_scope(descriptor: &ContextDescriptor, scope: ScopeKind) -> Result<Self> {
        let permitted = descriptor.permitted_scopes();
        if !permitted.contains(&scope) {
            let available = permitted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AikitError::new(
                "scope.unavailable_in_context",
                format!(
                    "there is no {scope} scope in this context; available scopes are {available}"
                ),
            )
            .with("scope", scope.as_str())
            .with("available", available));
        }
        Ok(Self {
            current: scope,
            permitted,
        })
    }

    pub fn current(&self) -> ScopeKind {
        self.current
    }

    pub fn permitted(&self) -> &[ScopeKind] {
        &self.permitted
    }

    /// `Tab`. Wraps, and never leaves the permitted set.
    pub fn cycle(&mut self) {
        if self.permitted.is_empty() {
            return;
        }
        let index = self
            .permitted
            .iter()
            .position(|s| *s == self.current)
            .unwrap_or(0);
        self.current = self.permitted[(index + 1) % self.permitted.len()];
    }

    pub fn requires_confirmation(&self) -> bool {
        self.current.requires_confirmation_to_write()
    }

    /// The confirmation this scope demands before `changes` are written.
    ///
    /// `None` for the cheap scopes. There is no argument that turns a `Some` into
    /// a `None`.
    pub fn confirmation(&self, changes: usize) -> Option<WriteConfirmation> {
        if !self.requires_confirmation() {
            return None;
        }
        let noun = if changes == 1 { "change" } else { "changes" };
        Some(WriteConfirmation {
            scope: self.current,
            prompt: format!(
                "Write {changes} {noun} to the {} profile?",
                self.current.as_str()
            ),
            detail: detail_for(self.current).to_string(),
        })
    }
}

/// A confirmation the user must answer before a durable, shared write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteConfirmation {
    pub scope: ScopeKind,
    pub prompt: String,
    /// Which file changes, and who else sees it.
    pub detail: String,
}

fn detail_for(scope: ScopeKind) -> &'static str {
    match scope {
        ScopeKind::Project => {
            "<repo>/.aikit/profile.toml is committed: everyone working on this repository \
             resolves it. Use project-local or the session overlay for a private change."
        }
        ScopeKind::Global => {
            "~/.aikit/profiles applies to every project on this machine, including ones you \
             have not opened yet."
        }
        // Unreachable while `requires_confirmation_to_write` names only the two
        // durable shared scopes; kept total rather than panicking if it grows.
        other => match other {
            ScopeKind::Host => "this host's profile applies to every project on this machine.",
            _ => "this change is durable.",
        },
    }
}
