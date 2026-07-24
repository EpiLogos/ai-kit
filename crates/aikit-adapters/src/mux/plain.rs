//! No multiplexer: the terminal AIKit was invoked in, and nothing else.
//!
//! This adapter exists so that "AIKit works without tmux" is a tested property
//! rather than an aspiration. Its whole design principle is that it refuses
//! loudly. Every operation it cannot perform returns `mux.unsupported` with a
//! sentence naming what *would* work — a silent no-op would leave the user
//! staring at a palette that claimed to have opened a pane somewhere.

use std::sync::Mutex;

use aikit_core::platform::MuxKind;
use aikit_core::session::{Placement, SessionPlan};
use aikit_core::{AikitError, Result};

use super::{
    MuxAdapter, MuxCapabilities, MuxLocation, MuxPresence, MuxTarget, Notification, PaletteRequest,
    ReconcileMode, SessionBinding, SpawnRequest, SpawnedTarget, StatusUpdate, UiHost,
};

/// The one surface a bare terminal has.
pub const CURRENT_TERMINAL: &str = "current-terminal";

#[derive(Default)]
pub struct Plain {
    /// A plain terminal has no status surface, so what AIKit was asked to display
    /// is kept for the caller to render rather than dropped on the floor.
    status: Mutex<Vec<(String, String)>>,
    notifications: Mutex<Vec<Notification>>,
}

impl Plain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Status values the caller still has to render itself.
    pub fn pending_status(&self) -> Vec<(String, String)> {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn pending_notifications(&self) -> Vec<Notification> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn unsupported(what: &str) -> AikitError {
        AikitError::new(
            "mux.unsupported",
            format!(
                "{what} needs a multiplexer, and this is a plain terminal; start the session \
                 under tmux or cmux, or run the command here instead"
            ),
        )
        .with("mux", MuxKind::Plain.as_str())
    }
}

impl MuxAdapter for Plain {
    fn kind(&self) -> MuxKind {
        MuxKind::Plain
    }

    fn capabilities(&self) -> MuxCapabilities {
        // Deliberately identical to the all-false default: a bare terminal has
        // none of these, and the honest way to say so is to claim nothing.
        MuxCapabilities::default()
    }

    fn detect(&self) -> Result<MuxPresence> {
        Ok(MuxPresence {
            kind: MuxKind::Plain,
            installed: true,
            version: None,
            server_running: true,
            // We are, by definition, already in it.
            inside: true,
            detail: None,
        })
    }

    fn current_location(&self) -> Result<MuxLocation> {
        Ok(MuxLocation {
            kind: MuxKind::Plain,
            host: String::new(),
            remote: false,
            project: None,
            session: None,
            view: None,
            surface: Some(CURRENT_TERMINAL.to_string()),
        })
    }

    fn ensure_session(&self, plan: &SessionPlan, _mode: ReconcileMode) -> Result<SessionBinding> {
        let Some(first_view) = plan.views.first() else {
            return Err(AikitError::new(
                "mux.empty_plan",
                format!("session plan `{}` declares no views", plan.id),
            ));
        };

        let mut binding = SessionBinding::new(MuxKind::Plain, plan.id.clone());
        // Nothing was created: the terminal was already here. `attach =
        // if-created` depends on this being honest.
        binding.created = false;
        binding.warnings.extend(plan.warnings.clone());

        let root = &first_view.steps[0];
        binding.views.insert(first_view.id.clone(), first_view.id.clone());
        binding.surfaces.insert(
            format!("{}/{}", first_view.id, root.pane),
            CURRENT_TERMINAL.to_string(),
        );
        binding.preserve("the current terminal is the session's only surface");

        for view in &plan.views {
            for step in &view.steps {
                if view.id == first_view.id && step.pane == root.pane {
                    continue;
                }
                binding.warnings.push(format!(
                    "pane `{}/{}` was not created: there is no multiplexer here, so the session \
                     has exactly one surface",
                    view.id, step.pane
                ));
            }
        }

        Ok(binding)
    }

    fn spawn(&self, request: SpawnRequest) -> Result<SpawnedTarget> {
        match request.placement {
            Placement::Current => Ok(SpawnedTarget {
                target: MuxTarget::surface(MuxKind::Plain, CURRENT_TERMINAL),
                created: false,
                note: Some(
                    "there is no multiplexer here, so the command runs in this terminal and \
                     replaces what you are looking at"
                        .to_string(),
                ),
            }),
            Placement::NewPane => Err(Self::unsupported("opening a new pane")),
            Placement::NewView => Err(Self::unsupported("opening a new window")),
            Placement::Background => Err(Self::unsupported("running a job in the background")),
        }
    }

    fn focus(&self, _target: &MuxTarget) -> Result<()> {
        // The only surface is already focused. Asking is not an error.
        Ok(())
    }

    fn close(&self, _target: &MuxTarget) -> Result<()> {
        Err(AikitError::new(
            "mux.unsupported",
            "the only surface here is the terminal AIKit is running in, and closing it is not \
             something AIKit will do to you",
        ))
    }

    fn open_palette(&self, _request: PaletteRequest) -> Result<UiHost> {
        Ok(UiHost::InlineCurrentTerminal)
    }

    fn set_status(&self, status: StatusUpdate) -> Result<()> {
        let mut kept = self.status.lock().unwrap_or_else(|e| e.into_inner());
        for (key, value) in status.values {
            kept.retain(|(k, _)| k != &key);
            kept.push((key, value));
        }
        kept.sort();
        Ok(())
    }

    fn notify(&self, notification: Notification) -> Result<()> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(notification);
        Ok(())
    }
}
