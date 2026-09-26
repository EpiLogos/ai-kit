//! The Agent-work lifecycle stage machine behind Compose's Enter-work step
//! (convergence B2; `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §5.1 Enter
//! work, and the E0 operation/owner matrix #3-#8).
//!
//! The state distinctions are law and are what these tests pin: saved is
//! not accepted; accepted is not prepared; prepared is not a started
//! provider; none of them is running. A compound Save-and-start walks the
//! native stages in order and lands exactly where the owner operations
//! carried it; a failed stage preserves every earlier one and a retry
//! resumes *that stage only*; "Saved; not running" is the explicit outcome
//! of a save-only intent; a running session is never launched twice.
//!
//! The backend here is a test double — its receipts carry the same shapes
//! the real owner operations return (Central profile save/accept, O-I world
//! readiness, folded `agent-session-prepare`, encounter launch), but it is
//! a fixture for the surface's stage semantics, not evidence that any
//! provider ran. Runtime success claims belong to the real operations
//! Worker A is binding, exercised in the integrated slice.

mod common;
use aikit_core::capsule::Capsule;
use aikit_core::Result;
use aikit_core::catalog::MemoryCatalog;
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::resolve::{resolve, ResolveRequest, ResolvedView};
use aikit_core::scope::ScopeKind;
use aikit_core::trust::MemoryTrust;
use aikit_tui::application::{AgentWorkStage, ComposeIntent, WorkStageName};
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::backend::{
    AgentProfileAcceptReceipt, AgentProfileSaveReceipt, AgentSessionPreparation, EncounterLaunch,
    JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle, WorldReadiness,
};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::world_entry::AgentWorkBindings;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt_down() -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT))
}

/// What the double's owner operations do. `true` operations return real
/// receipt-shaped values; `false` ones return the owner's refusal, exactly
/// like an unbound or refusing operation would.
#[derive(Default)]
struct Behaviour {
    save_ok: bool,
    accept_ok: bool,
    readiness_ok: bool,
    prepare_ok: bool,
    prepare_reports_provider_started: bool,
    launch_ok: bool,
}

/// A backend with every lifecycle operation BOUND, whose behaviour the test
/// script controls. All non-lifecycle trait methods are inert defaults —
/// this test never searches, stages or previews packages.
struct LifecycleBackend {
    context: ContextDescriptor,
    view: ResolvedView,
    behaviour: Behaviour,
    saves: std::cell::Cell<usize>,
    accepts: std::cell::Cell<usize>,
    readiness_checks: std::cell::Cell<usize>,
    prepares: std::cell::Cell<usize>,
    launches: std::cell::Cell<usize>,
}

impl LifecycleBackend {
    fn new(behaviour: Behaviour) -> Self {
        let context = ContextDescriptor::for_project("/work/aikit");
        let view = resolve(
            &MemoryCatalog::default(),
            &MemoryTrust::default(),
            &ResolveRequest {
                context: context.clone(),
                layers: Vec::new(),
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap();
        Self {
            context,
            view,
            behaviour,
            saves: std::cell::Cell::new(0),
            accepts: std::cell::Cell::new(0),
            readiness_checks: std::cell::Cell::new(0),
            prepares: std::cell::Cell::new(0),
            launches: std::cell::Cell::new(0),
        }
    }
}

impl PaletteBackend for LifecycleBackend {
    fn context(&self) -> &ContextDescriptor {
        &self.context
    }

    fn view(&self) -> &ResolvedView {
        &self.view
    }

    fn documents(&self) -> Vec<aikit_core::search::SearchDoc> {
        Vec::new()
    }

    fn capsule(&self, _id: &CapsuleId) -> Option<&Capsule> {
        None
    }

    fn preview(&self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<Projected> {
        Err(aikit_core::AikitError::new("test.preview", "unused"))
    }

    fn apply(&mut self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<GenerationId> {
        Err(aikit_core::AikitError::new("test.apply", "unused"))
    }

    fn start(&mut self, _intent: &RunIntent) -> Result<JobOutput> {
        Err(aikit_core::AikitError::new("test.start", "unused"))
    }

    fn recent(&self) -> Vec<RunIntent> {
        Vec::new()
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        Vec::new()
    }

    fn promote(&mut self, _draft: &PromotionDraft) -> Result<CapsuleId> {
        Err(aikit_core::AikitError::new("test.promote", "unused"))
    }

    fn agent_work_bindings(&self) -> AgentWorkBindings {
        AgentWorkBindings {
            save: true,
            accept: true,
            readiness: true,
            prepare: true,
            launch: true,
        }
    }

    fn save_agent_profile(
        &mut self,
        _purpose: &str,
        _name: Option<&str>,
    ) -> Result<AgentProfileSaveReceipt> {
        self.saves.set(self.saves.get() + 1);
        if !self.behaviour.save_ok {
            return Err(aikit_core::AikitError::new(
                "test.save_refused",
                "the owner refused the save",
            ));
        }
        Ok(AgentProfileSaveReceipt {
            profile_ref: "agent-profile/command-encounter-probe".into(),
            agent_ref: "agent/command-encounter-probe".into(),
            revision: "rev-1".into(),
            content_digest: Some("digest-1".into()),
        })
    }

    fn accept_agent_profile(
        &mut self,
        _expected_revision: &str,
        _expected_content_digest: Option<&str>,
    ) -> Result<AgentProfileAcceptReceipt> {
        self.accepts.set(self.accepts.get() + 1);
        if !self.behaviour.accept_ok {
            return Err(aikit_core::AikitError::new(
                "test.accept_refused",
                "the owner refused the acceptance",
            ));
        }
        Ok(AgentProfileAcceptReceipt {
            profile_ref: "agent-profile/command-encounter-probe".into(),
            revision: "rev-1".into(),
            content_digest: "digest-1".into(),
        })
    }

    fn world_readiness(&self) -> Result<WorldReadiness> {
        self.readiness_checks.set(self.readiness_checks.get() + 1);
        if !self.behaviour.readiness_ok {
            return Ok(WorldReadiness {
                ready: false,
                reason: Some("no provider credential is configured".into()),
                suggested_action: Some("configure the provider credential".into()),
            });
        }
        Ok(WorldReadiness {
            ready: true,
            reason: None,
            suggested_action: None,
        })
    }

    fn prepare_agent_session(
        &mut self,
        _profile_ref: &str,
    ) -> Result<AgentSessionPreparation> {
        self.prepares.set(self.prepares.get() + 1);
        if !self.behaviour.prepare_ok {
            return Err(aikit_core::AikitError::new(
                "test.prepare_refused",
                "the owner refused the preparation",
            ));
        }
        Ok(AgentSessionPreparation {
            agent_session: "agent-session/probe-1".into(),
            space: Some("session-space/dev".into()),
            provider_started: self.behaviour.prepare_reports_provider_started,
        })
    }

    fn start_encounter(&mut self, _agent_session: &str) -> Result<EncounterLaunch> {
        self.launches.set(self.launches.get() + 1);
        if !self.behaviour.launch_ok {
            return Err(aikit_core::AikitError::new(
                "test.launch_refused",
                "the owner refused the launch",
            ));
        }
        Ok(EncounterLaunch {
            agent_session: "agent-session/probe-1".into(),
            carrier: Some("carrier/terminal".into()),
        })
    }
}

/// Walk a fresh surface to the Enter-work step with the purpose and name
/// authored exactly as the §1.3 path prescribes. The walk itself needs no
/// lifecycle operation, so the returned backend starts with every counter
/// at zero and the behaviour the test scripted.
fn enter_work_surface(
    behaviour: Behaviour,
) -> (ApplicationSurfaceController, LifecycleBackend) {
    let mut seed = LifecycleBackend::new(Behaviour::default());
    let mut surface = ApplicationSurfaceController::new(
        &mut seed,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup),
    )
    .unwrap();
    surface.handle(&mut seed, key(KeyCode::Char('4'))).unwrap();
    for _ in 0..9 {
        surface.handle(&mut seed, alt_down()).unwrap();
    }
    surface.handle(&mut seed, key(KeyCode::Enter)).unwrap();
    for character in "Prove the command-encounter convergence slice: orient, find practice, and return evidence for O:I issue 527.".chars() {
        surface.handle(&mut seed, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut seed, key(KeyCode::Enter)).unwrap();
    for character in "command-encounter-probe".chars() {
        surface.handle(&mut seed, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut seed, key(KeyCode::Enter)).unwrap();
    (surface, LifecycleBackend::new(behaviour))
}

/// The full happy path: Save and start Direct work walks save -> accept ->
/// readiness -> prepare (provider NOT started) -> launch, and lands
/// Running, with the live session named. Every stage ran exactly once.
#[test]
fn the_compound_walks_every_stage_and_lands_running() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        accept_ok: true,
        readiness_ok: true,
        prepare_ok: true,
        prepare_reports_provider_started: false,
        launch_ok: true,
    });

    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();

    assert_eq!(backend.saves.get(), 1);
    assert_eq!(backend.accepts.get(), 1);
    assert_eq!(backend.readiness_checks.get(), 1);
    assert_eq!(backend.prepares.get(), 1);
    assert_eq!(backend.launches.get(), 1, "the compound launched exactly once");
    assert_eq!(
        surface.semantic().agent_work,
        AgentWorkStage::Running {
            agent_session: "agent-session/probe-1".into(),
        }
    );
    let status = surface.semantic().status.as_ref().unwrap();
    assert!(status.message.contains("Direct work running"), "{}", status.message);
    assert!(status.message.contains("agent-session/probe-1"), "{}", status.message);
    assert_eq!(surface.semantic().compose_intent, None);
}

/// Save-only ends at saved and says so: "Saved; not running" is the
/// success message, and nothing downstream ran.
#[test]
fn save_only_ends_at_saved_and_says_not_running() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        ..Behaviour::default()
    });

    surface.handle(&mut backend, key(KeyCode::Char('1'))).unwrap();

    assert_eq!(backend.saves.get(), 1);
    assert_eq!(backend.accepts.get(), 0, "save-only never accepts");
    assert_eq!(backend.prepares.get(), 0);
    assert_eq!(backend.launches.get(), 0);
    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Saved { .. }
    ));
    let status = surface.semantic().status.as_ref().unwrap();
    assert_eq!(status.message, "Saved; not running");
    assert_eq!(surface.semantic().compose_intent, None);
}

/// A launch failure after a landed save reports the failing stage and
/// preserves the accepted source; the retry resumes the launch stage only —
/// save and accept are not replayed.
#[test]
fn launch_failure_preserves_saved_source_and_resume_does_not_replay_earlier_stages() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        accept_ok: true,
        readiness_ok: true,
        prepare_ok: true,
        prepare_reports_provider_started: false,
        launch_ok: false,
    });

    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();
    assert_eq!(backend.saves.get(), 1);
    assert_eq!(backend.accepts.get(), 1);
    assert_eq!(backend.launches.get(), 1);
    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Failed {
            failed: WorkStageName::Launch,
            ..
        }
    ));
    let status = surface.semantic().status.as_ref().unwrap();
    assert!(
        status.message.contains("stage launch failed"),
        "{}",
        status.message
    );
    assert!(
        status.message.contains("earlier stages stand"),
        "{}",
        status.message
    );

    // The failed launch fell back to the prepared stage: preparation had
    // landed (provider not started), and that is exactly what stands.
    assert!(matches!(
        surface.semantic().agent_work.stable(),
        AgentWorkStage::Prepared {
            provider_started: false,
            ..
        }
    ));

    // Repair the condition and retry: only the launch runs again.
    backend.behaviour.launch_ok = true;
    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();
    assert_eq!(backend.saves.get(), 1, "the retry must not replay the save");
    assert_eq!(backend.accepts.get(), 1, "the retry must not replay the accept");
    assert_eq!(
        backend.prepares.get(), 1,
        "the retry must not replay a landed preparation"
    );
    assert_eq!(backend.launches.get(), 2, "the retry re-ran the launch only");
    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Running { .. }
    ));
}

/// World readiness that answers not-ready is a semantic stage failure
/// naming its reason and action — never a pass, never a transport crash.
#[test]
fn a_not_ready_world_names_its_reason_and_action() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        accept_ok: true,
        readiness_ok: false,
        ..Behaviour::default()
    });

    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();

    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Failed {
            failed: WorkStageName::Readiness,
            ..
        }
    ));
    let status = surface.semantic().status.as_ref().unwrap();
    assert!(
        status.message.contains("no provider credential is configured"),
        "{}",
        status.message
    );
    assert!(
        status.message.contains("configure the provider credential"),
        "{}",
        status.message
    );
    assert_eq!(
        backend.prepares.get(), 0,
        "preparation must not run when the world is not ready"
    );
}

/// Preparation that reports `provider_started: true` fails the prepare
/// stage: the contract says preparation must not start the provider, and an
/// owner claiming otherwise is recorded, not papered over.
#[test]
fn preparation_reporting_a_started_provider_fails_the_contract() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        accept_ok: true,
        readiness_ok: true,
        prepare_ok: true,
        prepare_reports_provider_started: true,
        launch_ok: true,
    });

    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();

    assert_eq!(
        backend.launches.get(), 0,
        "a contract-violating preparation must not launch"
    );
    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Failed {
            failed: WorkStageName::Prepare,
            ..
        }
    ));
    let status = surface.semantic().status.as_ref().unwrap();
    assert!(
        status.message.contains("provider_started=true"),
        "{}",
        status.message
    );
}

/// An already-running session is never launched twice: pressing the
/// compound again names the running session and dispatches nothing.
#[test]
fn a_running_session_is_never_launched_twice() {
    let (mut surface, mut backend) = enter_work_surface(Behaviour {
        save_ok: true,
        accept_ok: true,
        readiness_ok: true,
        prepare_ok: true,
        prepare_reports_provider_started: false,
        launch_ok: true,
    });
    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();
    assert!(matches!(
        surface.semantic().agent_work,
        AgentWorkStage::Running { .. }
    ));
    assert_eq!(backend.launches.get(), 1);

    surface.handle(&mut backend, key(KeyCode::Char('2'))).unwrap();
    assert_eq!(backend.launches.get(), 1, "no duplicate launch");
    let status = surface.semantic().status.as_ref().unwrap();
    assert!(
        status.message.contains("already running"),
        "{}",
        status.message
    );
    assert!(
        status.message.contains("agent-session/probe-1"),
        "{}",
        status.message
    );
}

/// Reducer-level state semantics: an unaccepted save is not a Start; the
/// stage ladder only advances through owner receipts; and authoring a new
/// purpose invalidates a staged preview (a source change never silently
/// rides an old review).
#[test]
fn reducer_state_semantics() {
    use aikit_tui::{reduce_tui, CompositionPreview, StagedChanges, UiAction};
    use aikit_core::scope::ScopeKind;

    // Saved is not accepted: Start-style receipt ladders cannot skip stages.
    let reduction = reduce_tui(
        aikit_tui::TuiState::default(),
        UiAction::AgentProfileSaved {
            profile_ref: "agent-profile/p".into(),
            agent_ref: "agent/p".into(),
            revision: "r".into(),
            content_digest: Some("d".into()),
        },
    );
    assert!(matches!(
        reduction.state.agent_work,
        AgentWorkStage::Saved { .. }
    ));
    assert!(reduction.effects.is_empty(), "no intent, no downstream stage");

    // An accepted receipt with no compound intent does not prepare.
    let reduction = reduce_tui(
        aikit_tui::TuiState::default(),
        UiAction::AgentProfileAccepted {
            profile_ref: "agent-profile/p".into(),
            revision: "r".into(),
            content_digest: "d".into(),
        },
    );
    assert!(matches!(
        reduction.state.agent_work,
        AgentWorkStage::Accepted { .. }
    ));
    assert!(reduction.effects.is_empty());

    // Authoring a purpose invalidates any staged preview.
    let state = aikit_tui::TuiState {
        preview: Some(CompositionPreview {
            revision: "rev".into(),
            scope: ScopeKind::Session,
            staged: StagedChanges::default(),
            summary: "old review".into(),
        }),
        ..aikit_tui::TuiState::default()
    };
    let reduction = reduce_tui(state, UiAction::SetComposePurpose("new exact purpose".into()));
    assert_eq!(reduction.state.compose_purpose, "new exact purpose");
    assert!(
        reduction.state.preview.is_none(),
        "a source change invalidates the previous preview"
    );

    // The save-only intent is recorded when Save Agent is dispatched with a
    // purpose, and its completion clears it with "Saved; not running".
    let state = aikit_tui::TuiState {
        compose_purpose: "exact purpose".into(),
        ..aikit_tui::TuiState::default()
    };
    let reduction = reduce_tui(state, UiAction::ComposeSaveAgent);
    assert_eq!(
        reduction.state.compose_intent,
        Some(ComposeIntent::SaveOnly)
    );
    assert!(matches!(
        reduction.effects.as_slice(),
        [aikit_tui::UiEffect::SaveAgentProfile { .. }]
    ));
}

