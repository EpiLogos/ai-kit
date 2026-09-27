//! Hosting AIKit's terminal application surface.
//!
//! Placement remains a CLI concern; semantic operation belongs to the shared V2
//! application service. Every interactive CLI entry point delegates to the same
//! reducer-native ApplicationSurface. No CLI path instantiates the retired
//! Palette/Tree semantic controllers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::platform::MuxKind;
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::{
    NavigationEvidence, NavigationEvidenceClass, ResourceDescriptor, ResourceKind, ResourceRecord,
    ResourceRef, ResourceSearchIndex,
};
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};

use aikit_store::{
    familiarity_observation_event, replay_familiarity, AikitHome, EventRecorder, FamiliarityReplay,
};

use aikit_tui::application::RelationView;
use aikit_tui::application_surface::ApplicationSurfaceRequest;
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::host::{TerminalProfile, UiHost};
use aikit_tui::PaletteOutcome;

use crate::app::Service;

/// V2 surface decorator that gives the shared Service two application-boundary
/// responsibilities which must not leak into the deterministic capsule resolver:
/// rebuilding learned navigation evidence and advertising real compositional
/// Resources already selected by the loaded Project/scope stack.
///
/// It delegates every resolver/package/runtime operation to the existing Service;
/// this is not a second application service or semantic store.
struct V2SurfaceService<'a> {
    service: &'a mut Service,
    /// Correlation retained from this backend's own save stage: the accept CAS
    /// names the exact profile the save landed. The surface stage machine
    /// always saves before accepting, so this is the reviewed source — never a
    /// renderer-selected or roster-guessed identity.
    saved_profile_ref: Option<String>,
}

impl<'a> V2SurfaceService<'a> {
    fn new(service: &'a mut Service) -> Self {
        Self {
            service,
            saved_profile_ref: None,
        }
    }

    fn composition_navigation_index(&self) -> ResourceSearchIndex {
        let mut index = <Service as PaletteBackend>::navigation_index(self.service);

        // Profiles are already part of the authoritative ordered scope stack.
        // Advertising them as V2 Resources makes that existing composition intent
        // selectable/searchable without turning ProfileId into a CapsuleId.
        let mut profiles = BTreeSet::new();
        if let Some(layers) = <Service as PaletteBackend>::scope_layers(self.service) {
            for layer in layers {
                profiles.extend(layer.patch.profiles.iter().cloned());
                profiles.extend(layer.patch.uses.iter().map(|used| used.profile.clone()));
            }
        }
        for profile in profiles {
            if let Ok(id) = ResourceRef::parse(profile.to_string()) {
                let record = ResourceRecord::new(ResourceDescriptor::new(
                    id,
                    ResourceKind::Profile,
                    profile.to_string(),
                    "Profile selected by the resolved scope composition",
                ));
                index.insert_resource(
                    record,
                    vec![
                        NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                            .with_detail("profile selected by the active scope stack"),
                    ],
                );
            }
        }

        // Project Skill Sets are authored project composition, not hidden package
        // activation. Keep their own Resource identity so human and agent views can
        // inspect the same selection without manufacturing Capability/Capsule ids.
        for name in self.service.project_skill_sets() {
            let Ok(id) = ResourceRef::parse(format!("skill-set/{name}")) else {
                continue;
            };
            let record = ResourceRecord::new(ResourceDescriptor::new(
                id,
                ResourceKind::SkillSet,
                name.clone(),
                "Skill Set selected by the current Project",
            ));
            index.insert_resource(
                record,
                vec![
                    NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                        .with_detail("skill set selected by the current Project"),
                ],
            );
        }

        index
    }
}

impl PaletteBackend for V2SurfaceService<'_> {
    fn context(&self) -> &aikit_core::ContextDescriptor {
        <Service as PaletteBackend>::context(self.service)
    }

    fn view(&self) -> &ResolvedView {
        <Service as PaletteBackend>::view(self.service)
    }

    /// Forwarded so the interactive surface can see the same native owner
    /// identity the CLI's own commands already resolve through `Service`.
    /// Before this forward existed the trait default (`Ok(None)`) answered
    /// here instead, which is indistinguishable from "no ProjectCentral
    /// identity is bound" — a silent downgrade this decorator must not
    /// introduce.
    fn project_binding(&self) -> Result<Option<aikit_core::project::ProjectBinding>> {
        <Service as PaletteBackend>::project_binding(self.service)
    }

    /// Forwarded for the same reason as `project_binding`: `Service` already
    /// observes real Git material through `NativeGitProvider`, and this
    /// decorator's job is to carry every resolver/package/runtime answer
    /// through unchanged, not to re-decide which of them the interactive
    /// surface is allowed to see. Leaving this one unforwarded is exactly the
    /// wiring gap that made the Worlds pane's Git section permanently absent.
    fn versioned_world(&self) -> Result<Option<aikit_core::resource::VersionedProjectWorld>> {
        <Service as PaletteBackend>::versioned_world(self.service)
    }

    /// Forwarded for the same reason as `versioned_world`: `Service` is the
    /// layer that can observe the OS secure store and the world's bindings, and
    /// this decorator must carry that observation through unchanged. Leaving it
    /// on the trait default (`Ok(None)`) is exactly the wiring gap that made the
    /// System pane's Credentials/Providers rows permanently "not attempted".
    fn credential_world(
        &self,
    ) -> Result<Option<aikit_core::credential_world::CredentialWorldDisclosure>> {
        <Service as PaletteBackend>::credential_world(self.service)
    }

    /// Forwarded for the same reason as `credential_world`: the health checks
    /// run on `Service`, and this decorator must carry that reading through so
    /// the System pane the TUI opens through sees it rather than the trait
    /// default of `Ok(None)`.
    fn doctor_world(&self) -> Result<Option<aikit_core::doctor_world::DoctorDisclosure>> {
        <Service as PaletteBackend>::doctor_world(self.service)
    }

    /// Forwarded for the same reason as `doctor_world`: the Workcell
    /// observation runs on `Service`, and the System pane sees it only if the
    /// decorator carries it through.
    fn workcell_world(&self) -> Result<Option<aikit_core::workcell_world::WorkcellDisclosure>> {
        <Service as PaletteBackend>::workcell_world(self.service)
    }

    /// Forwarded like the others: the roster is composed on `Service`, and the
    /// palette's roster overlay reaches it only through this decorator.
    fn model_roster(&self) -> Result<Option<aikit_core::resource::ModelRoster>> {
        <Service as PaletteBackend>::model_roster(self.service)
    }

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        <Service as PaletteBackend>::scope_layers(self.service)
    }

    fn application_home(&self) -> Option<&AikitHome> {
        Some(self.service.home())
    }

    fn documents(&self) -> Vec<SearchDoc> {
        <Service as PaletteBackend>::documents(self.service)
    }

    fn context_resource_records(&self) -> Result<Vec<ResourceRecord>> {
        self.service.context_resource_records()
    }

    fn navigation_index(&self) -> ResourceSearchIndex {
        self.composition_navigation_index()
    }

    fn familiarity(&self) -> Result<Option<FamiliarityStore>> {
        match replay_familiarity(self.service.index())? {
            FamiliarityReplay::Loaded { store, .. } => Ok(Some(store)),
            FamiliarityReplay::Invalidated { .. } => Ok(None),
        }
    }

    fn record_familiarity(&mut self, observation: FamiliarityObservation) -> Result<()> {
        let event = familiarity_observation_event(observation)?;
        EventRecorder::new(self.service.index(), self.service.home().event_log()).record(&event)
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&aikit_core::Capsule> {
        <Service as PaletteBackend>::capsule(self.service, id)
    }

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected> {
        <Service as PaletteBackend>::preview(self.service, scope, toggles)
    }

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId> {
        <Service as PaletteBackend>::apply(self.service, scope, toggles)
    }

    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput> {
        <Service as PaletteBackend>::start(self.service, intent)
    }

    fn recent(&self) -> Vec<RunIntent> {
        <Service as PaletteBackend>::recent(self.service)
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        <Service as PaletteBackend>::promotion_drafts(self.service)
    }

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId> {
        <Service as PaletteBackend>::promote(self.service, draft)
    }

    fn open_source(&mut self, id: &CapsuleId) -> Result<PathBuf> {
        <Service as PaletteBackend>::open_source(self.service, id)
    }

    fn working_environments(
        &self,
    ) -> Result<Option<Vec<aikit_core::working_environment::WorkingEnvironmentObservation>>> {
        <Service as PaletteBackend>::working_environments(self.service)
    }

    fn working_environment_subjects(&self) -> Result<Vec<aikit_core::resource::ResourceRef>> {
        <Service as PaletteBackend>::working_environment_subjects(self.service)
    }

    fn act_in_working_environment(
        &mut self,
        provider: &aikit_core::resource::ResourceRef,
        subject: &aikit_core::resource::ResourceRef,
        operation: aikit_tui::live_field::WorkingEnvironmentOperation,
    ) -> Result<aikit_tui::live_field::WorkingEnvironmentOutcome> {
        <Service as PaletteBackend>::act_in_working_environment(
            self.service,
            provider,
            subject,
            operation,
        )
    }

    fn factory_work_entry(&self) -> aikit_tui::backend::FactoryWorkEntry {
        <Service as PaletteBackend>::factory_work_entry(self.service)
    }

    fn start_factory_work(&mut self) -> Result<aikit_tui::backend::FactoryWorkStartReceipt> {
        <Service as PaletteBackend>::start_factory_work(self.service)
    }

    fn agent_work_bindings(&self) -> aikit_tui::world_entry::AgentWorkBindings {
        // Read once at surface construction (the trait's contract): the
        // operations are bound exactly when the invocation stands in a native
        // Central root or Work Project. Launch routes through the encounter
        // owner, which is always constructible for this backend.
        let bound = agent_work::central_scope(self.service).is_ok();
        aikit_tui::world_entry::AgentWorkBindings {
            save: bound,
            accept: bound,
            readiness: bound,
            prepare: bound,
            launch: bound,
        }
    }

    fn save_agent_profile(
        &mut self,
        purpose: &str,
        name: Option<&str>,
        skill_sets: &[String],
    ) -> Result<aikit_tui::backend::AgentProfileSaveReceipt> {
        let scope = agent_work::central_scope(self.service)?;
        let world_ref = agent_work::expected_world_ref(&scope)?;
        let mut input = serde_json::json!({
            "scope": scope.scope,
            "intent_expression": purpose,
            "purpose": purpose,
            "world_ref": world_ref,
            "ratified_world_refs": [world_ref],
            "skill_refs": [],
            "skill_set_refs": skill_sets,
        });
        if let Some(project) = &scope.project {
            input["project"] = serde_json::json!(project);
        }
        if let Some(name) = name {
            input["name"] = serde_json::json!(name);
        }
        let runner = aikit_adapters::runner::SystemRunner::new();
        let expressed = agent_work::run_central_action(
            &runner,
            &scope,
            "agent-profile.express",
            input,
        )?;
        let profile_ref = expressed["allocation"]["profile_ref"]
            .as_str()
            .or_else(|| expressed["profile"]["ref"].as_str())
            .ok_or_else(|| {
                aikit_core::AikitError::new(
                    "agent_profile.save_invalid",
                    "Central's express answer names no profile reference",
                )
            })?
            .to_owned();
        // Revision and content digest come from the owner's own review
        // reading, never derived here: these are the exact CAS values a later
        // accept must name.
        let review = agent_work::run_central_action(
            &runner,
            &scope,
            "agent-profile.review",
            agent_work::review_input(&scope, &profile_ref),
        )?;
        let receipt = agent_work::save_receipt(&review)?;
        self.saved_profile_ref = Some(receipt.profile_ref.clone());
        Ok(receipt)
    }

    fn accept_agent_profile(
        &mut self,
        expected_revision: &str,
        expected_content_digest: Option<&str>,
    ) -> Result<aikit_tui::backend::AgentProfileAcceptReceipt> {
        let profile_ref = self.saved_profile_ref.clone().ok_or_else(|| {
            aikit_core::AikitError::new(
                "agent_profile.accept_without_save",
                "accept names the profile this surface saved; no save is held here",
            )
        })?;
        let expected_content_digest = expected_content_digest.ok_or_else(|| {
            aikit_core::AikitError::new(
                "agent_profile.accept_without_digest",
                "accept is a CAS on the exact reviewed source; its content digest is required",
            )
        })?;
        let scope = agent_work::central_scope(self.service)?;
        let runner = aikit_adapters::runner::SystemRunner::new();
        agent_work::run_central_action(
            &runner,
            &scope,
            "agent-profile.accept",
            serde_json::json!({
                "scope": scope.scope,
                "profile_ref": profile_ref,
                "expected_revision": expected_revision,
                "expected_content_digest": expected_content_digest,
            }),
        )?;
        // A write acknowledgement is not acceptance evidence: reread the
        // review and confirm the exact source before the stage advances.
        let review = agent_work::run_central_action(
            &runner,
            &scope,
            "agent-profile.review",
            agent_work::review_input(&scope, &profile_ref),
        )?;
        if review["accepted"] != serde_json::json!(true)
            || review["profile"]["revision"] != serde_json::json!(expected_revision)
            || review["content_digest"] != serde_json::json!(expected_content_digest)
        {
            return Err(aikit_core::AikitError::new(
                "agent_profile.accept_stale",
                "Central does not show the exact source as accepted; reread the roster before retrying",
            ));
        }
        Ok(aikit_tui::backend::AgentProfileAcceptReceipt {
            profile_ref,
            revision: expected_revision.to_owned(),
            content_digest: expected_content_digest.to_owned(),
        })
    }

    fn world_readiness(&self) -> Result<aikit_tui::backend::WorldReadiness> {
        let reading = crate::direct_agent_session::scope(self.service)?;
        let readiness = &reading["world_readiness"];
        Ok(aikit_tui::backend::WorldReadiness {
            ready: readiness["ready"] == serde_json::json!(true),
            reason: readiness["reason"].as_str().map(str::to_owned),
            suggested_action: readiness["action"].as_str().map(str::to_owned),
        })
    }

    fn prepare_agent_session(
        &mut self,
        profile_ref: &str,
    ) -> Result<aikit_tui::backend::AgentSessionPreparation> {
        let scope = agent_work::central_scope(self.service)?;
        let runner = aikit_adapters::runner::SystemRunner::new();
        // Acceptance evidence comes from the owner's review reading — never
        // from renderer state — and `prepare` revalidates it natively.
        let review = agent_work::run_central_action(
            &runner,
            &scope,
            "agent-profile.review",
            agent_work::review_input(&scope, profile_ref),
        )?;
        if review["accepted"] != serde_json::json!(true) {
            return Err(aikit_core::AikitError::new(
                "agent_profile.not_accepted",
                "preparation needs the exact accepted source; accept the definition in Central first",
            ));
        }
        let request = crate::direct_agent_session::PrepareRequest {
            request_id: format!("tui-prepare-{}", ulid::Ulid::generate()),
            profile_ref: profile_ref.to_owned(),
            expected_revision: agent_work::text_field(&review["profile"]["revision"], "revision")?,
            expected_content_digest: agent_work::text_field(
                &review["content_digest"],
                "content digest",
            )?,
            expected_acceptance_ref: agent_work::text_field(
                &review["acceptance"]["acceptance_ref"],
                "acceptance ref",
            )?,
        };
        let reading = crate::direct_agent_session::prepare(self.service, request)?;
        if reading["prepared"] != serde_json::json!(true) {
            let session = reading["agent_session"].as_str().unwrap_or(profile_ref);
            return Err(aikit_core::AikitError::new(
                "session_space.preparation_incomplete",
                format!(
                    "preparation is incomplete; session {session} stays resumable through the same retained request"
                ),
            ));
        }
        Ok(aikit_tui::backend::AgentSessionPreparation {
            agent_session: agent_work::text_field(&reading["agent_session"], "agent session")?,
            space: reading["space"].as_str().map(str::to_owned),
            provider_started: reading["provider_started"].as_bool().unwrap_or(false),
        })
    }

    fn start_encounter(
        &mut self,
        agent_session: &str,
    ) -> Result<aikit_tui::backend::EncounterLaunch> {
        agent_work::start_encounter(self.service, agent_session)
    }
}

/// Production backing for the V2 surface's native Agent-work lifecycle: each
/// stage routes to the existing owner operation (Central agent-profile
/// express/review/accept, the folded direct-agent-session preparation, and the
/// encounter owner daemon). Nothing here manufactures a receipt; a stage's
/// outcome is the owner's own answer.
mod agent_work {
    use super::Service;
    use aikit_adapters::runner::CommandRunner;
    use aikit_core::{AikitError, ResourceRef, Result};
    use serde_json::{json, Value};
    use std::path::PathBuf;

    pub(crate) struct CentralScope {
        pub cwd: PathBuf,
        pub central: PathBuf,
        pub scope: &'static str,
        pub project: Option<String>,
    }

    fn refusal(code: &'static str, message: impl Into<String>) -> AikitError {
        AikitError::new(code, message)
    }

    /// The native Central encounter scope of this invocation: the canonical
    /// Central root, whether the invocation stands at the root or in exactly
    /// one Work Project. The same derivation the folded preparation applies.
    pub(crate) fn central_scope(service: &Service) -> Result<CentralScope> {
        let cwd = std::fs::canonicalize(service.invocation_cwd())
            .map_err(|error| refusal("direct_agent.source_io", error.to_string()))?;
        let central = crate::temporal::central_root_enclosing(Some(&cwd)).ok_or_else(|| {
            refusal(
                "direct_agent.central_unbound",
                "The disclosed location is not in Central; select the Central root or a native Work Project",
            )
        })?;
        let central = std::fs::canonicalize(central)
            .map_err(|error| refusal("direct_agent.source_io", error.to_string()))?;
        let (scope, project) = if cwd == central {
            ("root", None)
        } else {
            let relative = cwd.strip_prefix(central.join("Work")).map_err(|_| {
                refusal(
                    "direct_agent.scope_invalid",
                    "Direct Agent scope must be the Central root or an exact Work member",
                )
            })?;
            if relative.components().count() != 1
                || !cwd.join("ProjectCentral/project.json").is_file()
            {
                return Err(refusal(
                    "direct_agent.scope_invalid",
                    "Select the exact native Project root",
                ));
            }
            (
                "project",
                Some(
                    relative
                        .to_str()
                        .ok_or_else(|| refusal("direct_agent.source_io", "Project name is not UTF-8"))?
                        .to_owned(),
                ),
            )
        };
        Ok(CentralScope {
            cwd,
            central,
            scope,
            project,
        })
    }

    /// The canonical World ref of the scope: `control:root`, or the Project's
    /// own native identity read from its authored manifest — the same
    /// expected-scope derivation the native review check applies.
    pub(crate) fn expected_world_ref(scope: &CentralScope) -> Result<String> {
        if scope.scope == "root" {
            return Ok("control:root".to_owned());
        }
        scope.project.as_deref().ok_or_else(|| {
            refusal("direct_agent.scope_invalid", "project scope names no Project")
        })?;
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(scope.cwd.join("ProjectCentral/project.json")).map_err(|error| {
                refusal("direct_agent.source_io", error.to_string())
            })?,
        )
        .map_err(|error| refusal("direct_agent.source_io", error.to_string()))?;
        let id = manifest["project_id"].as_str().ok_or_else(|| {
            refusal(
                "direct_agent.project_invalid",
                "Native Project source identity is missing",
            )
        })?;
        Ok(format!("project:{id}"))
    }

    pub(crate) fn review_input(scope: &CentralScope, profile_ref: &str) -> Value {
        let mut input = json!({"scope": scope.scope, "profile_ref": profile_ref});
        if let Some(project) = &scope.project {
            input["project"] = json!(project);
        }
        input
    }

    /// One bounded native Central Action invocation through the owner's own
    /// CLI: resolved executable, explicit argv vector, JSON envelope checked.
    pub(crate) fn run_central_action<R: CommandRunner>(
        runner: &R,
        scope: &CentralScope,
        action: &str,
        input: Value,
    ) -> Result<Value> {
        let executable = std::env::var("CENTRAL_CTRL_BIN").unwrap_or_else(|_| "ctrl".into());
        let argv = vec![
            executable,
            "--json".into(),
            "--root".into(),
            scope.central.display().to_string(),
            "action".into(),
            "run".into(),
            action.into(),
            input.to_string(),
        ];
        let output = runner.run(&argv)?;
        if output.status != 0 {
            return Err(refusal(
                "central.action_unavailable",
                format!("Central could not answer `{action}`; repair System → Central and reread"),
            ));
        }
        if output.stdout.len() > 1024 * 1024 {
            return Err(refusal(
                "central.action_oversized",
                format!("Central's `{action}` answer is oversized"),
            ));
        }
        let envelope: Value = serde_json::from_str(&output.stdout)
            .map_err(|error| refusal("central.action_invalid", error.to_string()))?;
        if envelope["ok"] != json!(true) {
            return Err(refusal(
                "central.action_refused",
                format!("Central refused `{action}`: {}", envelope["error"]["message"].as_str().unwrap_or("no reason given")),
            ));
        }
        Ok(envelope["data"].clone())
    }

    pub(crate) fn text_field(value: &Value, name: &str) -> Result<String> {
        value
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                refusal(
                    "central.action_invalid",
                    format!("Central's answer carries no {name}"),
                )
            })
    }

    pub(crate) fn save_receipt(
        review: &Value,
    ) -> Result<aikit_tui::backend::AgentProfileSaveReceipt> {
        let profile = &review["profile"];
        Ok(aikit_tui::backend::AgentProfileSaveReceipt {
            profile_ref: text_field(&profile["ref"], "profile ref")?,
            agent_ref: text_field(&profile["agent_ref"], "agent ref")?,
            revision: text_field(&profile["revision"], "revision")?,
            content_digest: review["content_digest"].as_str().map(str::to_owned),
        })
    }

    /// Launch the encounter through the one native owner daemon: the session
    /// must already be prepared and attached, the provider is the configured
    /// encounter provider, and the owner's own receipt is the answer.
    #[cfg(unix)]
    pub(crate) fn start_encounter(service: &mut Service, agent_session: &str) -> Result<aikit_tui::backend::EncounterLaunch> {
        let session = ResourceRef::parse(agent_session)
            .map_err(|error| refusal("encounter.session_invalid", error.to_string()))?;
        let Some(binding) = crate::direct_agent_session::read(service.home(), &session)? else {
            return Err(refusal(
                "encounter.session_unprepared",
                format!("no prepared Direct session named {agent_session}; prepare it before launch"),
            ));
        };
        let encounter = crate::encounter_service::EncounterService::new(service.home().clone())?;
        let providers = encounter.providers()?;
        let provider = match providers.as_slice() {
            [one] => one.id.clone(),
            [] => {
                return Err(refusal(
                    "encounter.provider_unconfigured",
                    "no encounter provider is configured; configure the harness provider before launching",
                ))
            }
            many => {
                let names = many
                    .iter()
                    .map(|provider| provider.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(refusal(
                    "encounter.provider_ambiguous",
                    format!("several encounter providers are configured ({names}); keep one before launching from the surface"),
                ));
            }
        };
        crate::encounter_service::start(service.home(), &binding.cwd)?;
        let receipt = crate::encounter_service::request(
            &crate::encounter_service::socket_path(service.home()),
            &crate::encounter_service::EncounterRequest::Open {
                space: binding.space.clone(),
                agent_session: session,
                provider,
                cwd: binding.cwd.clone(),
            },
        )?;
        Ok(aikit_tui::backend::EncounterLaunch {
            agent_session: receipt["agent_session"]
                .as_str()
                .unwrap_or(agent_session)
                .to_owned(),
            carrier: receipt["provider"].as_str().map(str::to_owned),
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn start_encounter(
        _service: &mut Service,
        _agent_session: &str,
    ) -> Result<aikit_tui::backend::EncounterLaunch> {
        Err(refusal(
            "encounter.start_unavailable",
            "the encounter owner transport is not available on this platform",
        ))
    }
}

/// Build a terminal profile from an environment lookup and the `--fullscreen`
/// flag.
pub fn terminal_profile<F>(env: F, fullscreen: bool) -> TerminalProfile
where
    F: Fn(&str) -> Option<String>,
{
    let (cols, rows) = terminal_size(&env);
    let mut profile = TerminalProfile::new(cols, rows);

    if env("TMUX").is_some() {
        profile = profile.in_mux(MuxKind::Tmux);
    } else if env("CMUX").is_some() || env("CMUX_SURFACE").is_some() {
        profile = profile.in_mux(MuxKind::Cmux);
    }

    if fullscreen {
        profile = profile.requested(UiHost::Fullscreen);
    }
    profile
}

fn terminal_size<F>(env: &F) -> (u16, u16)
where
    F: Fn(&str) -> Option<String>,
{
    let parse = |key: &str, default: u16| {
        env(key)
            .and_then(|value| value.trim().parse::<u16>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default)
    };
    (parse("COLUMNS", 80), parse("LINES", 24))
}

/// Open the final V2 application surface over one live service.
///
/// `opening_tree` is retained as a user-facing CLI option, but its meaning is
/// "open Knowledge with the Tree projection of the one relation read model". It
/// does not instantiate TreeState or a separate tree controller.
pub fn run_surface(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
    opening_tree: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = ApplicationSurfaceRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    if opening_tree {
        request = request.opening_relations(RelationView::Tree);
    }
    let mut backend = V2SurfaceService::new(service);
    aikit_tui::application_surface::run_on_terminal(&mut backend, request)
}

/// Compatibility helper for callers that historically asked to "open the
/// palette". The behavior is now exactly the final ApplicationSurface.
pub fn run(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    run_surface(service, query, fullscreen, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Mutex;

    /// The native owner resolution reads process environment variables
    /// (`CENTRAL_ROOT`, `CENTRAL_CTRL_BIN`); the env-mutating tests serialise
    /// on this lock so parallel tests never observe each other's world.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git is available in the test environment");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A real repository with a real ProjectCentral identity, the same
    /// minimal fixture `versioned_world_wiring.rs` uses for `Service` itself
    /// -- `.aikit` is what makes the service recognise a Project at all, and
    /// `ProjectCentral/project.json` is what gives it a native owner identity
    /// `versioned_world`/`project_binding` refuse to observe without.
    fn project(root: &Path) {
        std::fs::create_dir_all(root.join(".aikit")).unwrap();
        std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
        std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
        std::fs::write(
            root.join("ProjectCentral/project.json"),
            r#"{"schema":"central.project/v1","project_id":"project:surface-probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
        )
        .unwrap();
        git(root, &["init", "--initial-branch=trunk"]);
        git(root, &["config", "user.email", "probe@example.invalid"]);
        git(root, &["config", "user.name", "probe"]);
        std::fs::write(root.join("README.md"), "probe\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "first"]);
    }

    fn service(home: &Path, root: &Path) -> Service {
        let mut env = BTreeMap::new();
        env.insert(
            "AIKIT_CONTEXT_ID".to_owned(),
            aikit_core::ContextId::generate().to_string(),
        );
        Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
    }

    /// The wiring proof this module exists to guard: every interactive entry
    /// point opens the terminal application through `V2SurfaceService`, not
    /// through `Service` directly, so a forward that only exists on
    /// `Service` never reaches the TUI. Before `project_binding` and
    /// `versioned_world` were added to this `impl PaletteBackend for
    /// V2SurfaceService`, calling them on the decorator silently ran the
    /// `PaletteBackend` trait default (`Ok(None)`) instead of `Service`'s
    /// real observation -- indistinguishable, at the type level, from "this
    /// Project has no Git material". This test drives the decorator itself
    /// against a real repository made by real `git`, the same discipline
    /// `versioned_world_wiring.rs` uses one layer down for `Service` alone.
    #[test]
    fn the_surface_decorator_forwards_real_git_material_not_the_trait_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);

        let observed = backend
            .versioned_world()
            .expect("observation does not fail on a real worktree");
        let versioned = observed.expect(
            "the decorator must forward Service's real observation, not the \
             PaletteBackend trait default of None",
        );
        assert_eq!(versioned.repository.branch.as_deref(), Some("trunk"));
        assert!(
            versioned.working.is_clean(),
            "a fresh commit leaves a clean tree"
        );

        let binding = backend
            .project_binding()
            .expect("binding observation does not fail")
            .expect(
                "the decorator must forward Service's real ProjectBinding, not the \
                 PaletteBackend trait default of None",
            );
        assert_eq!(binding.project.as_str(), "project:surface-probe");
    }

    /// The credential-world forward has the same failure mode as the git one:
    /// the TUI opens through `V2SurfaceService`, so a producer that exists only
    /// on `Service` never reaches the System pane unless the decorator forwards
    /// it. Left on the trait default (`Ok(None)`) the pane would read
    /// `not attempted` forever — the exact defect W7 fixes. This drives the
    /// decorator itself and asserts it carries `Service`'s real reading through.
    #[test]
    fn the_surface_decorator_forwards_the_real_credential_reading_not_the_trait_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);

        let disclosure = backend
            .credential_world()
            .expect("composing the credential world does not fail")
            .expect(
                "the decorator must forward Service's real reading, not the \
                 PaletteBackend trait default of None",
            );
        // Observed, not the `Unknown` roster the `not_attempted` default carries.
        assert!(
            disclosure.providers.is_known(),
            "the decorator carries a real observed roster: {:?}",
            disclosure.providers
        );
        assert!(
            !disclosure.credentials.is_empty(),
            "the seed catalogue's hosted Models declare credential needs the \
             decorator's reading must carry"
        );
    }

    /// The doctor forward has the same failure mode: the TUI opens through the
    /// decorator, so the health checks that run on `Service` reach the System
    /// pane only if the decorator forwards them. Left on the trait default the
    /// pane would read `not attempted` forever.
    #[test]
    fn the_surface_decorator_forwards_the_real_health_reading_not_the_trait_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);

        let disclosure = backend
            .doctor_world()
            .expect("running the checks does not fail")
            .expect(
                "the decorator must forward Service's real reading, not the \
                 PaletteBackend trait default of None",
            );
        assert!(
            disclosure.was_attempted(),
            "the decorator carries an observed health reading, not not-attempted"
        );
    }

    /// The Workcell forward, same failure mode as the others.
    #[test]
    fn the_surface_decorator_forwards_the_real_workcell_reading_not_the_trait_default() {
        use aikit_core::workcell_world::WorkcellKnowledge;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);

        let disclosure = backend
            .workcell_world()
            .expect("observing does not fail")
            .expect("the decorator must forward Service's real reading, not None");
        // A real observation, not the not-attempted default.
        assert!(
            !matches!(disclosure.knowledge, WorkcellKnowledge::NotAttempted { .. }),
            "the decorator carries a real observation"
        );
    }

    /// The roster forward: composed on `Service`, reaching the palette overlay
    /// only through the decorator.
    #[test]
    fn the_surface_decorator_forwards_the_model_roster() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);
        assert!(
            backend.model_roster().unwrap().is_some(),
            "the decorator carries Service's composed roster, not the trait default"
        );
    }

    /// The native Agent-work lifecycle wiring: before the production
    /// implementations existed on `V2SurfaceService`, every stage silently ran
    /// the refusing `PaletteBackend` trait default, so the TUI could not save,
    /// accept, prepare or launch anything no matter what the machine had. The
    /// guard drives the decorator itself against a fake Central owner and
    /// asserts the exact owner argv, the CAS values, and the receipt mapping —
    /// the stage machine is proven separately in `aikit-tui`'s lifecycle tests.
    fn central_world() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("central");
        std::fs::create_dir_all(&root).unwrap();
        // The same minimal native identity `project()` builds: the folded
        // readiness reading requires a real Project binding at the cwd, so the
        // fixture root carries one.
        project(&root);
        (tmp, root)
    }

    fn fake_ctrl(dir: &Path, log: &Path) -> PathBuf {
        let path = dir.join("ctrl");
        let dir = dir.display().to_string();
        let log = log.display().to_string();
        script(
            &path,
            &format!(
                r#"echo "$*" >> "{log}"
case "$6" in
  agent-profile.express) echo '{{"ok":true,"data":{{"allocation":{{"profile_ref":"agent-profile:expressed-fake","agent_ref":"agent:expressed-fake","revision":"r1","recognition":"unrecognised"}},"profile":{{"ref":"agent-profile:expressed-fake","agent_ref":"agent:expressed-fake","revision":"r1"}}}}}}' ;;
  agent-profile.accept) touch "{dir}/accepted" ; echo '{{"ok":true,"data":{{}}}}' ;;
  agent-profile.review)
    if [ -f "{dir}/accepted" ]; then accepted=true; else accepted=false; fi
    echo "{{\"ok\":true,\"data\":{{\"schema\":\"central.agent-profile-review/v1\",\"scope_ref\":\"control:root\",\"accepted\":$accepted,\"content_digest\":\"sha256:fake\",\"profile\":{{\"ref\":\"agent-profile:expressed-fake\",\"agent_ref\":\"agent:expressed-fake\",\"revision\":\"r1\"}},\"acceptance\":{{\"schema\":\"central.agent-profile-acceptance/v1\",\"acceptance_ref\":\"acceptance:fake\",\"profile_ref\":\"agent-profile:expressed-fake\",\"profile_revision\":\"r1\",\"content_digest\":\"sha256:fake\",\"scope_ref\":\"control:root\"}}}}}}" ;;
  central.world.effective-sources) echo '{{"ok":true,"data":{{"world_ref":"control:root","sources":[]}}}}' ;;
  *) echo '{{"ok":false,"error":{{"code":"invalid_input","message":"Unknown Action"}}}}'; exit 2 ;;
esac"#
            ),
        );
        path
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn the_surface_binds_the_agent_work_lifecycle_over_a_central_root_only() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (tmp, root) = central_world();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let canonical_root = std::fs::canonicalize(&root).unwrap();
        std::env::set_var("CENTRAL_ROOT", &canonical_root);
        let mut env = BTreeMap::new();
        env.insert(
            "AIKIT_CONTEXT_ID".to_owned(),
            aikit_core::ContextId::generate().to_string(),
        );
        let mut svc =
            Service::open(aikit_store::AikitHome::at(&home), &root, |key| env.get(key).cloned())
                .unwrap();
        let backend = V2SurfaceService::new(&mut svc);
        let bindings = backend.agent_work_bindings();
        assert!(
            bindings.save && bindings.accept && bindings.readiness && bindings.prepare,
            "standing in Central binds every native Agent-work stage: {bindings:?}"
        );

        // Outside Central nothing is bound: the honest unavailability the
        // world-entry rows name, not a stage that fails halfway through.
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut svc =
            Service::open(aikit_store::AikitHome::at(&home), &elsewhere, |key| {
                env.get(key).cloned()
            })
            .unwrap();
        let backend = V2SurfaceService::new(&mut svc);
        let bindings = backend.agent_work_bindings();
        assert!(
            !bindings.save && !bindings.accept && !bindings.readiness && !bindings.prepare,
            "outside Central no stage is bound: {bindings:?}"
        );
        std::env::remove_var("CENTRAL_ROOT");
    }

    #[test]
    fn save_and_accept_route_through_central_with_exact_cas_and_review_readback() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (tmp, root) = central_world();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let log = tmp.path().join("ctrl.log");
        let ctrl = fake_ctrl(tmp.path(), &log);
        std::env::set_var("CENTRAL_ROOT", std::fs::canonicalize(&root).unwrap());
        std::env::set_var("CENTRAL_CTRL_BIN", &ctrl);
        let mut env = BTreeMap::new();
        env.insert(
            "AIKIT_CONTEXT_ID".to_owned(),
            aikit_core::ContextId::generate().to_string(),
        );
        let mut svc =
            Service::open(aikit_store::AikitHome::at(&home), &root, |key| env.get(key).cloned())
                .unwrap();
        let mut backend = V2SurfaceService::new(&mut svc);

        let saved = backend
            .save_agent_profile("Guard the day's close", Some("Daykeeper"), &[])
            .expect("save routes through the Central owner");
        assert_eq!(saved.profile_ref, "agent-profile:expressed-fake");
        assert_eq!(saved.agent_ref, "agent:expressed-fake");
        assert_eq!(saved.revision, "r1");
        assert_eq!(saved.content_digest.as_deref(), Some("sha256:fake"));
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(
            calls.contains("agent-profile.express"),
            "save goes through the express authoring Action: {calls}"
        );
        assert!(
            calls.contains("\"intent_expression\":\"Guard the day's close\""),
            "the purpose is carried verbatim: {calls}"
        );
        assert!(
            calls.contains("\"world_ref\":\"control:root\""),
            "the composed World ref is carried: {calls}"
        );
        assert!(
            calls.contains("agent-profile.review"),
            "revision and digest come from the owner's review reading: {calls}"
        );

        // Accept is a CAS on the exact reviewed source: the owner receives the
        // exact revision and digest, and the stage reads acceptance back from
        // the review before reporting success.
        let accepted = backend
            .accept_agent_profile("r1", Some("sha256:fake"))
            .expect("accept routes through the Central owner with the exact CAS");
        assert_eq!(accepted.profile_ref, "agent-profile:expressed-fake");
        assert_eq!(accepted.revision, "r1");
        assert_eq!(accepted.content_digest, "sha256:fake");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(
            calls.contains("\"expected_revision\":\"r1\"")
                && calls.contains("\"expected_content_digest\":\"sha256:fake\""),
            "the accept call carries the exact CAS values: {calls}"
        );

        // The world readiness reading is the folded native scope answer.
        let readiness = backend.world_readiness().expect("readiness reads natively");
        assert!(readiness.ready, "the fake answers a bound world: {readiness:?}");

        // A fresh surface holds no saved correlation: accept without this
        // surface's own save refuses instead of guessing a profile.
        let mut svc = Service::open(aikit_store::AikitHome::at(&home), &root, |key| {
            env.get(key).cloned()
        })
        .unwrap();
        let mut backend = V2SurfaceService::new(&mut svc);
        let error = backend
            .accept_agent_profile("r1", Some("sha256:fake"))
            .expect_err("accept without this surface's save refuses");
        assert_eq!(error.code(), "agent_profile.accept_without_save");
        std::env::remove_var("CENTRAL_ROOT");
        std::env::remove_var("CENTRAL_CTRL_BIN");
    }
}
