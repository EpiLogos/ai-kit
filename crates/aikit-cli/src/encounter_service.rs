//! Resident, provider-neutral ACP encounters. The UI reads cursor pages and
//! submits owner actions; disconnecting an IPC client never drops a provider.
use aikit_adapters::{
    agent_connection::{
        ConnectionDescriptor, ConnectionSignalKind, NativePermissionRequest, SessionOpenMode,
    },
    agent_session_host::{
        AgentSessionHost, AgentSessionHostLimits, HostEvent, SessionEventJournal, SessionLane,
    },
    interactive_connection::{AcpStableConnectionAdapter, PermissionDecision},
};
use aikit_core::context_activation::ContextActivationReceipt;
use aikit_core::harness_admission::HarnessActivationObservation;
use aikit_core::projection::ProjectionPlan;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use aikit_store::encounter::context::{
    ContextExpectation, ContextOperation, ContextRequest, ContextScope,
};
use aikit_store::now_context::RedisNowConfig;
use aikit_store::{encounter::EncounterStore, AikitHome, SessionSpaceApplicationStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

#[path = "encounter_agency.rs"]
mod agency;
pub use agency::mint::{
    mint_from_cli, mint_per_project_agency, mint_request_document, REQUIRED_MINTED_ACTIONS,
};
pub use agency::model::EncounterModelOpen;
pub use agency::{
    EncounterA2aFraming, EncounterAddressedTurn, EncounterAgencyBinding, EncounterContextPacket,
    EncounterGroupRecipient,
};

#[path = "encounter_addressing.rs"]
mod encounter_addressing;
pub use encounter_addressing::{
    EncounterAddressableParticipant, EncounterAddressableParticipantsReading,
    EncounterAddressableParticipantsRequest,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EncounterProtocol {
    #[default]
    Acp,
    PiRpc,
    PrimeRpc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterNowContextConfig {
    pub redis: RedisNowConfig,
    /// Optional owner-authored preparation request. When selected, a missing
    /// view (or a fresh AgentSession for the same participant) is prepared
    /// synchronously before the first provider turn through the same native
    /// `now-context prepare` implementation. Warm reads never invoke Jev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare_request: Option<PathBuf>,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_external_provider")]
    pub external_provider: bool,
}
fn default_external_provider() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterProvider {
    #[serde(default)]
    pub protocol: EncounterProtocol,
    pub id: String,
    pub label: String,
    /// Explicit argv of a freeform provider. A profile-derived provider
    /// (`from_profile`) omits it — the profile's `[sessions.connect]` argv is
    /// the one source; an explicit argv alongside `from_profile` is a
    /// contradiction refused at configuration and resolution.
    #[serde(default)]
    pub argv: Vec<String>,
    /// Optional resolved acting-body identity. This is provider configuration
    /// provenance, not Agent identity; consumers may require an exact body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_ref: Option<String>,
    /// Revision of the acting-body implementation when body_ref is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_revision: Option<String>,
    /// The embedded harness profile slug this provider's connection facts are
    /// derived from at load time (`crate::encounter_profile_provider`). A
    /// profile-derived provider carries an empty `argv` here; an explicit
    /// argv alongside `from_profile` is a contradiction and refuses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_profile: Option<String>,
    /// Older-release argv variants that put the same harness into the same
    /// protocol mode, tried in declared order when the primary argv fails
    /// before any ACP initialize response. Declared, never discovered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv_fallback: Vec<Vec<String>>,
    /// Launch environment facts. Nothing can carry them to the child yet (the
    /// scrubbed final-child environment is credential-only); a provider that
    /// declares any refuses at open rather than being accepted and dropped.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Working-directory fact. The encounter open supplies the project-bound
    /// working directory; a provider-declared cwd refuses at open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Explicit owner-configured admission basis. Absence preserves optional context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_context: Option<EncounterContextAdmission>,
    /// A pinned scoped policy. It selects a catalogue route, not an Agent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<EncounterRequiredSource>,
    /// Optional Redis-backed NOW delivery selected for this provider. The
    /// credential is a native SecretRef; material never appears in config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_context: Option<EncounterNowContextConfig>,
}

/// Pins existing source, not a copy of its content or a grant of semantic authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterRequiredSource {
    pub source: ResourceRef,
    pub revision: SourceRevision,
    pub path: PathBuf,
    /// Material byte binding, separate from the source owner's opaque revision.
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterContextAdmission {
    pub sources: Vec<EncounterRequiredSource>,
    #[serde(default)]
    pub source_activations: Vec<ContextActivationReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<ProjectionPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<HarnessActivationObservation>,
}

impl EncounterContextAdmission {
    /// Verify required material before a provider effect. Historical activation
    /// evidence remains distinct: this check never asserts fresh runtime loading.
    pub fn verify(&self) -> Result<()> {
        if self.sources.is_empty() || self.sources.len() > 128 {
            return Err(AikitError::new(
                "encounter.context_invalid",
                "Required context must name 1 to 128 sources",
            ));
        }
        let mut identities = std::collections::BTreeSet::new();
        for source in &self.sources {
            ResourceRef::parse(source.source.as_str())?;
            SourceRevision::parse(source.revision.as_str())?;
            if !source.path.is_absolute() || !identities.insert(source.source.clone()) {
                return Err(AikitError::new(
                    "encounter.context_invalid",
                    "Required source needs an absolute material locator and unique source identity",
                ));
            }
            let expected = source.content_digest.strip_prefix("blake3:").filter(|value| {
                value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }).ok_or_else(|| AikitError::new("encounter.context_invalid", "Required source digest must be blake3 followed by 64 lowercase hexadecimal digits"))?;
            let path_metadata = std::fs::metadata(&source.path).map_err(|e| {
                AikitError::new(
                    "encounter.context_unavailable",
                    format!(
                        "Required source {} at revision {} is unavailable: {e}",
                        source.source, source.revision
                    ),
                )
            })?;
            if !path_metadata.is_file() {
                return Err(AikitError::new(
                    "encounter.context_invalid",
                    "Required source must be a regular file",
                ));
            }
            let mut file = std::fs::File::open(&source.path).map_err(|e| {
                AikitError::new(
                    "encounter.context_unavailable",
                    format!(
                        "Required source {} at revision {} is unreadable: {e}",
                        source.source, source.revision
                    ),
                )
            })?;
            let metadata = file.metadata().map_err(error)?;
            const MAX_BYTES: u64 = 4 * 1024 * 1024;
            if !metadata.is_file() || metadata.len() > MAX_BYTES {
                return Err(AikitError::new(
                    "encounter.context_invalid",
                    "Required source must be a regular file no larger than 4 MiB",
                ));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            if bytes.len() as u64 > MAX_BYTES || blake3::hash(&bytes).to_hex().as_str() != expected
            {
                return Err(AikitError::new(
                    "encounter.context_stale",
                    format!(
                        "Required source {} no longer matches the admitted material for revision {}",
                        source.source, source.revision
                    ),
                ));
            }
        }
        for activation in &self.source_activations {
            activation.validate()?;
            if !identities.contains(&activation.source) {
                return Err(AikitError::new(
                    "encounter.context_invalid",
                    "Source activation evidence names a source outside the admitted basis",
                ));
            }
        }
        match (&self.projection, &self.activation) {
            (Some(plan), Some(activation)) => {
                activation.validate_against(plan)?;
                if self
                    .source_activations
                    .iter()
                    .any(|source| source.target != plan.target)
                {
                    return Err(AikitError::new(
                        "encounter.context_invalid",
                        "Source activation target differs from the admitted projection",
                    ));
                }
            }
            (None, None) => {}
            _ => {
                return Err(AikitError::new(
                    "encounter.context_invalid",
                    "Projection and activation evidence must be supplied together",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum EncounterRequest {
    Context {
        request: ContextRequest,
    },
    PromptContext {
        agent_session: ResourceRef,
        draft_revision: u64,
        context: ContextExpectation,
    },
    /// Select a catalogue-backed, scoped, already configured native body.
    OpenModel {
        request: Box<EncounterModelOpen>,
    },
    Health,
    /// Stop only this explicitly identified owner after its provider processes
    /// have been shut down. This is never an ordinary view-detach operation.
    Shutdown {
        expected_pid: u32,
    },
    Permission {
        agent_session: ResourceRef,
        request_id: String,
        decision: PermissionDecision,
    },
    Providers,
    /// Read only the exact model selector observed from an already-resident
    /// native provider session. This does not claim catalogue availability.
    ModelRead {
        agent_session: ResourceRef,
    },
    /// Reclassify only a causally proven legacy native-load replay projection.
    ClassifyLegacyLoadReplay {
        agent_session: ResourceRef,
    },
    /// Read-only addressed-delivery preflight: which candidate sessions this
    /// sender may currently address with these exact sources. It opens no
    /// provider, writes no membership and reserves no delivery; the send path
    /// repeats this admission before any transport.
    AddressableParticipants {
        request: Box<EncounterAddressableParticipantsRequest>,
    },
    /// Request an exact provider-advertised model from the resident native
    /// session. Durable model policy/Agency selection stays outside this route.
    ModelSelect {
        agent_session: ResourceRef,
        provider_model_id: String,
        /// Optional exact provider-advertised execution budget. It is applied
        /// only after model confirmation and never exposes arbitrary config.
        #[serde(default)]
        provider_reasoning_effort: Option<String>,
        /// Optional for old clients; new consumers bind a write to their read.
        #[serde(default)]
        expected_native_session_id: Option<String>,
    },
    /// Read the permission modes the resident native session advertises and
    /// which one is current. Provider disclosure, not an AIKit policy.
    ModeRead {
        agent_session: ResourceRef,
    },
    /// Switch the resident native session to one provider-advertised
    /// permission mode. The harness decides what the mode allows; it applies
    /// to the next action.
    ModeSelect {
        agent_session: ResourceRef,
        provider_mode_id: String,
        /// Binds this write to the session the caller read.
        #[serde(default)]
        expected_native_session_id: Option<String>,
    },
    Send {
        agent_session: ResourceRef,
        turn: EncounterAddressedTurn,
    },
    SendGroup {
        delivery_ref: ResourceRef,
        sender: ResourceRef,
        packet: EncounterContextPacket,
        recipients: Vec<EncounterGroupRecipient>,
    },
    Delivery {
        agent_session: ResourceRef,
        delivery_ref: ResourceRef,
    },
    /// Resume the actually recorded native session; never silently create a new one.
    Reconnect {
        space: SessionSpaceRef,
        agent_session: ResourceRef,
        provider: String,
        cwd: PathBuf,
    },
    View {
        agent_session: ResourceRef,
        before: Option<u64>,
    },
    Open {
        space: SessionSpaceRef,
        agent_session: ResourceRef,
        provider: String,
        cwd: PathBuf,
    },
    Read {
        agent_session: ResourceRef,
        after: u64,
        limit: usize,
    },
    Draft {
        agent_session: ResourceRef,
        basis: u64,
        text: String,
    },
    Prompt {
        agent_session: ResourceRef,
        draft_revision: u64,
    },
    Cancel {
        agent_session: ResourceRef,
        reason: Option<String>,
    },
    Status {
        agent_session: ResourceRef,
    },
}
type PendingPermissions =
    Arc<Mutex<BTreeMap<ResourceRef, BTreeMap<String, NativePermissionRequest>>>>;
struct Journal(Arc<EncounterStore>, PendingPermissions, String);
impl SessionEventJournal for Journal {
    fn append(&self, session: &ResourceRef, event: &HostEvent) -> Result<()> {
        if let HostEvent::Signal(signal) = event {
            if let ConnectionSignalKind::HistoryReplay { update } = &signal.kind {
                // ACP v1 load replay has no provider-history-item identifier.
                // Retain raw update evidence in exact observed order, but keep it
                // out of the live transcript projection: comparing text/chunks
                // would collapse distinct messages or duplicate a re-chunked one.
                self.0.append(
                    session,
                    &json!({
                        "kind":"provider-history-replay",
                        "standing":"unreconciled-no-provider-history-event-id",
                        "connection_generation":self.2,
                        "native_session_id":signal.native_session_id,
                        "sequence":signal.sequence,
                        "update":update,
                    }),
                )?;
                return Ok(());
            }
        }
        self.0.append(
            session,
            &json!({"kind":"provider","event":event,"connection_generation":self.2}),
        )?;
        let mut pending = self.1.lock().map_err(error)?;
        match event {
            HostEvent::Signal(signal) => {
                if let ConnectionSignalKind::PermissionRequested { request } = &signal.kind {
                    let requests = pending.entry(session.clone()).or_default();
                    if requests.len() >= 128 {
                        return Err(AikitError::new(
                            "encounter.permission_capacity",
                            "Provider exceeded concurrent pending permission capacity",
                        ));
                    }
                    requests.insert(request.native_request_id.clone(), request.clone());
                }
            }
            HostEvent::TurnEnded(_) => {
                pending.remove(session);
            }
        }
        Ok(())
    }
}
struct Resident {
    host: AgentSessionHost,
    lane: SessionLane,
    space: SessionSpaceRef,
    provider: String,
    provider_label: String,
    operations: Mutex<()>,
    required_context: Option<EncounterContextAdmission>,
    body_ref: Option<String>,
    body_revision: Option<String>,
    protocol: EncounterProtocol,
    generation: String,
    cwd: PathBuf,
    argv: Vec<String>,
    model: Option<agency::model::PreparedModel>,
    now_context: Option<EncounterNowContextConfig>,
}
impl Resident {
    /// The resident provider as a view may describe it: identity, the owner's
    /// label, the acting-body pin and the displayable launch facts.
    fn provider_view(&self) -> Value {
        let mut view = provider_launch_facts(self.protocol, &self.argv);
        view["id"] = json!(self.provider);
        view["label"] = json!(self.provider_label);
        view["body_ref"] = json!(self.body_ref);
        view["body_revision"] = json!(self.body_revision);
        view
    }

    fn prompt_payload(&self, text: &str) -> Value {
        match self.protocol {
            EncounterProtocol::Acp => json!([{"type":"text","text":text}]),
            EncounterProtocol::PiRpc | EncounterProtocol::PrimeRpc => json!(text),
        }
    }
}
enum Lifecycle {
    Running,
    Closed(Value),
    Failed(String),
}
pub struct EncounterService {
    home: AikitHome,
    lifecycle: RwLock<Lifecycle>,
    shutdown_requested: std::sync::atomic::AtomicBool,
    store: Arc<EncounterStore>,
    residents: Mutex<BTreeMap<ResourceRef, Arc<Resident>>>,
    permissions: PendingPermissions,
}
/// How a provider's harness is launched, reduced to what may be shown: the
/// protocol, the program's file name and, for interpreters, the script's file
/// name. Never another argv element, an environment value or a full path.
///
/// A definition that launches through macOS `sandbox-exec` declares its own
/// confinement in argv (there is no separate field); it reads as
/// `sandboxed: true` and names the confined program, not the wrapper.
pub fn provider_launch_facts(protocol: EncounterProtocol, argv: &[String]) -> Value {
    fn base(value: &str) -> Option<String> {
        Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    }
    let mut program = argv;
    let mut sandboxed = false;
    if argv
        .first()
        .and_then(|first| base(first))
        .is_some_and(|name| name == "sandbox-exec")
    {
        // sandbox-exec [-f profile-file | -p profile | -n name] [-D key=value ...] command ...
        let mut at = 1;
        while at < argv.len() && argv[at].starts_with('-') {
            at += if matches!(argv[at].as_str(), "-f" | "-p" | "-n" | "-D") {
                2
            } else {
                1
            };
        }
        if at < argv.len() {
            program = &argv[at..];
            sandboxed = true;
        }
    }
    let command = program.first().and_then(|first| base(first));
    let entry = program.iter().skip(1).find_map(|argument| {
        [".js", ".mjs", ".cjs", ".ts", ".py"]
            .iter()
            .any(|suffix| argument.ends_with(suffix))
            .then(|| base(argument))
            .flatten()
    });
    json!({
        "protocol": protocol,
        "command": command,
        "entry": entry,
        "sandboxed": sandboxed,
    })
}

const NATIVE_MODE_AUTHORITY: &str = "provider-advertised-session-mode; applies-to-the-next-action";

fn observation_native(
    host: &AgentSessionHost,
    session: &ResourceRef,
    lane: &SessionLane,
) -> String {
    host.identity(session)
        .map(|identity| identity.binding.native_session_id)
        .unwrap_or_else(|_| lane.binding().native_session_id.clone())
}

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.runtime", message.to_string())
}

/// One failed launch attempt, journaled verbatim: which declared variant was
/// tried, why it failed, whether a provider process existed and its cleanup
/// is confirmed, and whether another declared variant follows.
struct FailedLaunchAttempt<'a> {
    reconnect: bool,
    index: usize,
    variant: &'a [String],
    failure: &'a AikitError,
    /// `None`: spawn failed, no process existed. `Some(ok)`: a process existed
    /// and shutdown was attempted with the given confirmation.
    cleanup: Option<bool>,
    more_variants: bool,
}
impl EncounterService {
    pub fn new(home: AikitHome) -> Result<Self> {
        Ok(Self {
            lifecycle: RwLock::new(Lifecycle::Running),
            shutdown_requested: std::sync::atomic::AtomicBool::new(false),
            store: Arc::new(EncounterStore::open(&home)?),
            home,
            residents: Mutex::new(BTreeMap::new()),
            permissions: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }
    pub fn configure(home: &AikitHome, provider: EncounterProvider) -> Result<()> {
        if provider.id.len() > 128
            || provider.id.is_empty()
            || !provider
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || provider.body_ref.as_ref().is_some_and(|value| {
                value.trim().is_empty() || aikit_core::ResourceRef::parse(value).is_err()
            })
            || provider
                .body_revision
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || provider.body_ref.is_some() != provider.body_revision.is_some()
        {
            return Err(error(
                "Provider requires a safe id, explicit native argv, and either both body_ref/body_revision or neither",
            ));
        }
        if let Some(now) = &provider.now_context {
            now.redis.validate()?;
        }
        if provider.from_profile.is_some() {
            // A profile-derived provider stores its slug and resolves its
            // connection facts at load time; fail fast here when the profile
            // cannot derive (unknown slug, contradiction, unreachable facts)
            // instead of storing a provider no open can launch.
            crate::encounter_profile_provider::resolve_provider(provider.clone())?;
        } else if provider.argv.is_empty() || provider.argv[0].is_empty() {
            return Err(error(
                "Provider requires a safe id and explicit native argv",
            ));
        } else {
            // Freeform providers get the same honesty gate: connection env/cwd
            // facts that cannot reach the child are refused at configuration,
            // never stored to be accepted and dropped at open.
            crate::encounter_profile_provider::ensure_connection_facts_reachable(&provider)?;
        }
        let root = home.state().join("encounter-providers");
        std::fs::create_dir_all(&root).map_err(error)?;
        let path = root.join(format!("{}.json", provider.id));
        // Configuration is an explicit native operation, never an IPC request.
        let content = serde_json::to_vec_pretty(&provider).map_err(error)?;
        let temp = root.join(format!(".{}.{}.tmp", provider.id, ulid::Ulid::generate()));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(error)?;
        use std::io::Write;
        let result = (|| {
            file.write_all(&content)?;
            file.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result.map_err(error)
    }
    fn providers(&self) -> Result<Vec<EncounterProvider>> {
        let root = self.home.state().join("encounter-providers");
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for entry in std::fs::read_dir(root).map_err(error)? {
            let path = entry.map_err(error)?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let bytes = std::fs::read(&path).map_err(error)?;
                let provider: EncounterProvider = serde_json::from_slice(&bytes).map_err(error)?;
                // Profile-derived providers resolve their connection facts
                // here, at load, from the embedded profile — never a
                // hand-copied snapshot. A provider that cannot resolve
                // refuses naming its file.
                rows.push(
                    crate::encounter_profile_provider::resolve_provider(provider).map_err(
                        |failure| {
                            AikitError::new(
                                failure.code(),
                                format!("{}: {failure}", path.display()),
                            )
                        },
                    )?,
                );
            }
        }
        Ok(rows)
    }
    fn resident(&self, session: &ResourceRef) -> Result<Arc<Resident>> {
        self.residents.lock().map_err(error)?.get(session).cloned().ok_or_else(||error("This canonical encounter has no resident native session; explicit owner open is required"))
    }
    fn require_attached(&self, session: &ResourceRef) -> Result<()> {
        if SessionSpaceApplicationStore::new(self.home.clone())
            .list()?
            .iter()
            .any(|space| space.agent_sessions.contains_key(session))
        {
            Ok(())
        } else {
            Err(error(
                "Canonical AgentSession is not attached to any retained SessionSpace",
            ))
        }
    }
    fn check_context_scope(&self, scope: &ContextScope) -> Result<()> {
        let project = ResourceRef::parse(&scope.project)?;
        if let Some(session) = &scope.agent_session {
            self.require_attached(session)?;
            if !SessionSpaceApplicationStore::new(self.home.clone())
                .list()?
                .iter()
                .any(|space| {
                    space.definition.projects.contains(&project)
                        && space.agent_sessions.contains_key(session)
                })
            {
                return Err(AikitError::new(
                    "encounter.context_scope",
                    "Session is not attached to the selected Project",
                ));
            }
        }
        Ok(())
    }
    fn check_context(
        &self,
        session: &ResourceRef,
        provider: &str,
        phase: &str,
        context: Option<&EncounterContextAdmission>,
    ) -> Result<()> {
        let Some(context) = context else {
            return Ok(());
        };
        match context.verify() {
            Ok(()) => {
                self.store.append(session, &json!({"kind":"context-admission-checked", "provider":provider, "phase":phase, "sources":context.sources, "source_activations":context.source_activations, "activation":context.activation, "verification":"required-source-material", "fresh_runtime_loading_observed":false}))?;
                Ok(())
            }
            Err(failure) => {
                self.store.append(session, &json!({"kind":"context-admission-refused", "provider":provider, "phase":phase, "code":failure.code(), "reason":failure.to_string()}))?;
                Err(failure)
            }
        }
    }

    fn check_resident_context(
        &self,
        session: &ResourceRef,
        resident: &Resident,
        phase: &str,
    ) -> Result<()> {
        crate::direct_agent_session::check(&self.home, session, &resident.cwd)?;
        let current = self.providers()?.into_iter().find(|p| p.id == resident.provider)
            .ok_or_else(|| AikitError::new("encounter.provider_removed", "The resident provider configuration was removed; reopen explicitly before further effects"))?;
        if current.required_context != resident.required_context
            || current.protocol != resident.protocol
            || current.argv != resident.argv
            || current.body_ref != resident.body_ref
            || current.body_revision != resident.body_revision
        {
            let failure = AikitError::new(
                "encounter.context_changed",
                "Required context configuration changed; recompose and reopen instead of silently updating a resident encounter",
            );
            self.store.append(session, &json!({"kind":"context-admission-refused", "provider":resident.provider, "phase":phase, "code":failure.code(), "reason":failure.to_string()}))?;
            return Err(failure);
        }
        self.check_context(
            session,
            &resident.provider,
            phase,
            resident.required_context.as_ref(),
        )?;
        let model = agency::model::prepare(&self.home, session, &current)?;
        if model != resident.model {
            return Err(error(
                "Resident model policy/catalogue/credential/Agency basis changed; explicit re-resolution is required",
            ));
        }
        if let Some(model) = &model {
            if resident.host.identity(session)?.state != aikit_adapters::SessionLaneState::Resident
            {
                return Err(error(
                    "Model-selected resident already has a turn in flight; no overlapping model readmission",
                ));
            }
            // Pi get_state is a native read. The adapter rejects changed native
            // provider/model/session before another prompt can be submitted.
            resident.host.initialize()?;
            self.store.append(
                session,
                &json!({"kind":"model-admission-checked", "phase":phase,
                "selection":model, "native_model_state_checked":true, "inference_observed":false}),
            )?;
        }
        Ok(())
    }

    /// Exclusive shutdown waits for already admitted owner operations, then
    /// stops all residents before acknowledging. A failed cleanup is retained
    /// as failure, never converted into a later empty successful shutdown.
    fn shutdown(&self, expected_pid: u32) -> Result<Value> {
        if expected_pid != std::process::id() {
            return Err(AikitError::new(
                "encounter.owner_changed",
                "Shutdown PID does not identify this encounter owner",
            ));
        }
        // Refuse new read leases immediately so a busy stream of callers cannot
        // starve the exclusive shutdown lease.
        self.shutdown_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut lifecycle = self.lifecycle.write().map_err(error)?;
        match &*lifecycle {
            Lifecycle::Closed(receipt) => return Ok(receipt.clone()),
            Lifecycle::Failed(reason) => {
                return Err(AikitError::new("encounter.shutdown_failed", reason.clone()));
            }
            Lifecycle::Running => {}
        }
        let residents = std::mem::take(&mut *self.residents.lock().map_err(error)?);
        let mut stopped = Vec::new();
        let mut failures = Vec::new();
        for (session, resident) in residents {
            // apply() holds a read lease throughout each operation. With the
            // exclusive lease here, only the map owns these Resident values.
            match Arc::try_unwrap(resident) {
                Ok(resident) => {
                    let native = resident.lane.binding().native_session_id.clone();
                    // Journal failure must not prevent material cleanup.
                    if let Err(error) = self.store.append(&session, &json!({"kind":"owner-shutdown-requested","native_session_id":native,"reason":"explicit owner lifecycle operation"})) {
                        failures.push(format!("{session}: shutdown request journal: {error}"));
                    }
                    match resident.host.shutdown() {
                        Ok(status) => {
                            let receipt = json!({"agent_session":session,"native_session_id":native,"process_status":status.map(|s|s.to_string()),"process_stopped":true});
                            if let Err(error) = self.store.append(
                                &session,
                                &json!({"kind":"owner-shutdown-completed","receipt":receipt}),
                            ) {
                                failures
                                    .push(format!("{session}: shutdown receipt journal: {error}"));
                            }
                            stopped.push(receipt);
                        }
                        Err(error) => failures.push(format!("{session}: {error}")),
                    }
                }
                Err(resident) => {
                    // Preserve the still-owned resident for diagnostics. No
                    // successful ACK may claim this process has stopped.
                    self.residents
                        .lock()
                        .map_err(error)?
                        .insert(session.clone(), resident);
                    failures.push(format!(
                        "{session}: resident still borrowed during exclusive shutdown"
                    ));
                }
            }
        }
        self.permissions.lock().map_err(error)?.clear();
        if !failures.is_empty() {
            let reason = failures.join("; ");
            *lifecycle = Lifecycle::Failed(reason.clone());
            return Err(AikitError::new("encounter.shutdown_failed", reason));
        }
        let receipt = json!({"protocol":"aikit-encounter-v1","pid":std::process::id(),"shutdown":true,"stopped":stopped,"canonical_sessions_retained":true});
        *lifecycle = Lifecycle::Closed(receipt.clone());
        Ok(receipt)
    }

    /// Ask the resident harness for one advertised permission mode and keep
    /// the owner's record of it: the request before any wire effect, the
    /// provider's confirmation after. A confirmation that cannot be recorded
    /// is uncertain, never silently resent.
    fn configure_native_mode(
        &self,
        agent_session: &ResourceRef,
        lane: &SessionLane,
        provider: &str,
        native_session_id: &str,
        provider_mode_id: &str,
        origin: Option<Value>,
    ) -> Result<aikit_adapters::ModeConfigurationReceipt> {
        let mut requested = json!({
            "kind":"native-mode-configuration-requested",
            "agent_session":agent_session,
            "native_session_id":native_session_id,
            "provider":provider,
            "requested_provider_mode_id":provider_mode_id,
            "authority":NATIVE_MODE_AUTHORITY
        });
        if let Some(origin) = &origin {
            requested["origin"] = origin.clone();
        }
        self.store.append(agent_session, &requested)?;
        let receipt = lane.set_mode(provider_mode_id)?;
        let mut confirmed = json!({
            "kind":"native-mode-configuration-confirmed",
            "receipt":receipt,
            "authority":NATIVE_MODE_AUTHORITY
        });
        if let Some(origin) = origin {
            confirmed["origin"] = origin;
        }
        self.store.append(agent_session, &confirmed).map_err(|e| {
            AikitError::new(
                "encounter.mode_configuration_uncertain",
                format!("Provider confirmed the mode change but receipt persistence failed; do not resend automatically: {e}"),
            )
        })?;
        Ok(receipt)
    }

    /// The owner's configured default permission mode for a new native
    /// session (`ai-kit:permissions:permissions.default-mode`). Applied only
    /// when the harness advertises that mode; every outcome is recorded and
    /// none of them fails the open.
    fn apply_default_mode(
        &self,
        agent_session: &ResourceRef,
        host: &AgentSessionHost,
        lane: &SessionLane,
        provider: &str,
        argv: &[String],
    ) {
        let setting_ref = crate::permission_defaults::SETTING_REF;
        let modes = match crate::permission_defaults::read(&self.home) {
            Ok(modes) => modes,
            Err(failure) => {
                let _ = self.store.append(agent_session, &json!({
                    "kind":"native-mode-default-not-applied",
                    "setting_ref":setting_ref,
                    "reason":format!("The configured default permission modes could not be read: {}", failure.message())
                }));
                return;
            }
        };
        let Some((harness, mode)) = crate::permission_defaults::lookup(&modes, provider, argv)
        else {
            return;
        };
        let origin = json!({"setting_ref":setting_ref,"harness":harness});
        let observation = match host.identity(agent_session) {
            Ok(identity) => identity.binding.mode_observation,
            Err(_) => None,
        };
        let not_applied = |reason: String| {
            let _ = self.store.append(
                agent_session,
                &json!({
                    "kind":"native-mode-default-not-applied",
                    "setting_ref":setting_ref,
                    "harness":harness,
                    "requested_provider_mode_id":mode,
                    "reason":reason
                }),
            );
        };
        let Some(observation) = observation else {
            not_applied("The harness advertised no session permission modes".into());
            return;
        };
        if observation.current_mode_id == mode {
            let _ = self.store.append(
                agent_session,
                &json!({
                    "kind":"native-mode-default-already-current",
                    "setting_ref":setting_ref,
                    "harness":harness,
                    "mode_observation":observation
                }),
            );
            return;
        }
        if !observation.advertises(&mode) {
            not_applied(format!(
                "The harness does not advertise mode {mode}; it offers {}",
                observation
                    .available_modes
                    .iter()
                    .map(|option| option.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            return;
        }
        let native = observation_native(host, agent_session, lane);
        if let Err(failure) =
            self.configure_native_mode(agent_session, lane, provider, &native, &mode, Some(origin))
        {
            let _ = self.store.append(
                agent_session,
                &json!({
                    "kind":"native-mode-default-failed",
                    "setting_ref":setting_ref,
                    "harness":harness,
                    "requested_provider_mode_id":mode,
                    "error_code":failure.code(),
                    "reason":failure.message()
                }),
            );
        }
    }

    fn open_native(
        &self,
        space: SessionSpaceRef,
        agent_session: ResourceRef,
        provider: String,
        cwd: PathBuf,
        reconnect: bool,
        model_target: Option<&EncounterModelOpen>,
    ) -> Result<Value> {
        let authored = SessionSpaceApplicationStore::new(self.home.clone()).load(&space)?;
        if !authored.agent_sessions.contains_key(&agent_session) {
            return Err(error(
                "AgentSession is not attached to this canonical SessionSpace",
            ));
        }
        let cwd = std::fs::canonicalize(&cwd).map_err(error)?;
        if !cwd.is_dir() {
            return Err(error("Encounter working directory is not a directory"));
        }
        // Project membership is semantic authority. A view cannot move
        // an attached project encounter into an unrelated directory.
        if !authored.definition.projects.is_empty()
            && !authored.project_contexts.values().any(|context| {
                match &context.basis.project_binding.locator {
                    aikit_core::project::ProjectBindingLocator::LocalDirectory { path } => {
                        std::fs::canonicalize(path).is_ok_and(|root| cwd.starts_with(root))
                    }
                    _ => false,
                }
            })
        {
            return Err(error(
                "Encounter directory is outside its authored local Project context; resolve and attach current native context first",
            ));
        }
        // Held for the whole launch; released just before the queued-delivery
        // drain, which takes the same agency lock itself.
        let agency_lock = self.lock_agency(&agent_session)?;
        self.check_agency(&agent_session)?;
        crate::direct_agent_session::check(&self.home, &agent_session, &cwd)?;
        let previous = self.store.last_native_binding(&agent_session)?;
        let mut residents = self.residents.lock().map_err(error)?;
        if let Some(held) = residents.get(&agent_session) {
            if held.space != space || held.provider != provider || held.cwd != cwd {
                return Err(error(
                    "Canonical encounter is already bound to another space/provider",
                ));
            }
            self.check_resident_context(&agent_session, held, "resident-open")?;
            let receipt = json!({"agent_session":agent_session,"native_session_id":held.lane.binding().native_session_id,"model_observation":held.lane.binding().model_observation,"model_selection":held.model,"resident":true});
            drop(residents);
            drop(agency_lock);
            // An already-resident open is a readiness moment too: queued
            // durable deliveries may now be deliverable.
            return self.open_receipt_with_drain(agent_session, receipt);
        }
        if !reconnect && previous.is_some() {
            return Err(AikitError::new(
                "encounter.resume_required",
                "A prior native binding exists; use explicit reconnect, or create a new canonical session for a fresh/forked encounter",
            ));
        }
        if reconnect
            && previous.as_ref().is_none_or(|p| {
                p["provider"].as_str() != Some(provider.as_str())
                    || p["space"].as_str() != Some(space.to_string().as_str())
            })
        {
            return Err(AikitError::new(
                "encounter.reconnect_basis",
                "Reconnect requires the actual recorded provider/space/native session binding",
            ));
        }
        let configured = self
            .providers()?
            .into_iter()
            .find(|p| p.id == provider)
            .ok_or_else(|| error("ACP provider is not configured in AIKit"))?;
        // A declared connection env/cwd cannot reach the provider child today;
        // launching anyway would accept and silently drop them. Refuse naming
        // the facts instead.
        crate::encounter_profile_provider::ensure_connection_facts_reachable(&configured)?;
        if reconnect
            && previous.as_ref().is_some_and(|p| {
                p["cwd"] != json!(cwd)
                    || p["protocol"] != json!(configured.protocol)
                    || p["provider_argv_digest"]
                        != json!(blake3::hash(
                            serde_json::to_string(&configured.argv)
                                .expect("argv JSON")
                                .as_bytes()
                        )
                        .to_hex()
                        .to_string())
                    || p["now_context_config_digest"]
                        != json!(blake3::hash(
                            serde_json::to_vec(&configured.now_context)
                                .expect("NOW config JSON")
                                .as_slice()
                        )
                        .to_hex()
                        .to_string())
            })
        {
            return Err(AikitError::new(
                "encounter.reconnect_basis",
                "The recorded cwd/protocol/provider command changed or is unpinned; do not silently resume on a replacement body",
            ));
        }
        self.check_context(
            &agent_session,
            &provider,
            "before-provider-start",
            configured.required_context.as_ref(),
        )?;
        if let Some(target) = model_target {
            agency::model::validate_target(&self.home, &agent_session, &configured, target)?;
        }
        self.check_task_launch(&agent_session, &configured, &cwd)?;
        let connection = ResourceRef::parse(format!(
            "connection/encounter-{}",
            blake3::hash(agent_session.as_str().as_bytes()).to_hex()
        ))
        .map_err(error)?;
        if reconnect && configured.protocol != EncounterProtocol::Acp {
            return Err(AikitError::new(
                "encounter.reconnect_unsupported",
                "This native provider does not publish a supported resume operation; ACP reconnect rides the capability-gated session/resume, and this protocol has no reconnect route through the encounter service; no replacement session was created",
            ));
        }
        // Resolve the composed tool surface before any provider process exists,
        // so a composition failure cannot orphan a native provider. Whether the
        // resolved entries are carried is decided by the negotiated capability
        // after the provider handshake below.
        let mcp_entries = match configured.protocol {
            EncounterProtocol::Acp => {
                crate::encounter_mcp::active_tool_source_entries(&self.home, &cwd)?
            }
            EncounterProtocol::PiRpc | EncounterProtocol::PrimeRpc => Vec::new(),
        };
        let generation = ulid::Ulid::generate().to_string();
        let journal: Option<Arc<dyn SessionEventJournal>> = Some(Arc::new(Journal(
            self.store.clone(),
            self.permissions.clone(),
            generation.clone(),
        )));
        let model = agency::model::prepare(&self.home, &agent_session, &configured)?;
        let task_bound = self.is_task_bound(&agent_session)?;
        if configured.protocol == EncounterProtocol::PrimeRpc
            && (configured.body_ref.is_none() || configured.body_revision.is_none())
        {
            return Err(AikitError::new(
                "encounter.prime_body_unresolved",
                "Prime RPC providers must name an exact body_ref and body_revision",
            ));
        }
        let mut launch_argv = if let Some(model) = &model {
            if task_bound {
                configured.argv.clone()
            } else {
                agency::model::direct_launcher(&agent_session, &configured, model)?
            }
        } else {
            configured.argv.clone()
        };
        // The bounded child-to-parent message channel: a session-scoped
        // directory the adapter hands the launcher and drains into this
        // journal. It carries words, never effects; each file is one bounded
        // record the child wrote through its inherited skill, correlated to
        // the child's locus digest like every faculty receipt.
        let child_message_dir = if configured.protocol == EncounterProtocol::PrimeRpc {
            launch_argv.extend(["--agent-session".into(), agent_session.to_string()]);
            let dir = self.home.state().join("encounter-child-messages").join(
                blake3::hash(agent_session.as_str().as_bytes())
                    .to_hex()
                    .to_string(),
            );
            std::fs::create_dir_all(&dir).map_err(error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(error)?;
            }
            launch_argv.extend(["--child-message-dir".into(), dir.display().to_string()]);
            Some(dir)
        } else {
            None
        };
        // Profile-declared key delivery rides the direct provider launch: the
        // child is the real harness, so the declared key is injected into the
        // scrubbed final-child environment here. The re-exec launchers
        // (selected-model, task boundary) re-materialise at their final exec
        // and take no environment from this spawn.
        let launch_environment = if model.is_none() && !task_bound {
            agency::model::profile_environment(&self.home, &agent_session, &configured)?
        } else {
            None
        };
        let provenance = vec![format!("native encounter provider {provider}")];
        // Launch the primary argv, then the declared fallback variants. A
        // variant is only retried when its failure happened before any ACP
        // initialize response — a spawn failure, or a connection that closed
        // before the handshake completed (`agent_session_host.handshake_failed`).
        // A semantic refusal (an initialize response arrived) is the protocol
        // speaking, not a wrong argv variant, and is never retried: there is
        // no silent retry loop here. The model direct-launcher resolves the
        // declared variants by the same rule at its own final exec
        // (journaled as `native-model-launch-variant-selected`), so a
        // model-policy-bound open falls back on an unresolvable primary too.
        let direct_provider_launch = model.is_none() || task_bound;
        let mut launch_variants = vec![launch_argv.clone()];
        if direct_provider_launch {
            launch_variants.extend(configured.argv_fallback.clone());
        }
        let single_variant = launch_variants.len() == 1;
        let mut launched: Option<(AgentSessionHost, ConnectionDescriptor)> = None;
        let mut failed_attempts: Vec<String> = Vec::new();
        for (index, variant) in launch_variants.iter().enumerate() {
            let last_variant = index + 1 == launch_variants.len();
            let attempt_host = match configured.protocol {
                EncounterProtocol::Acp => AgentSessionHost::launch_with_journal_and_environment(
                    AcpStableConnectionAdapter::new(connection.clone(), provenance.clone()),
                    variant,
                    Some(&cwd),
                    AgentSessionHostLimits::default(),
                    journal.clone(),
                    launch_environment.as_ref(),
                ),
                EncounterProtocol::PiRpc => AgentSessionHost::launch_with_journal_and_environment(
                    {
                        let adapter =
                            aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter::new(
                                connection.clone(),
                                cwd.to_string_lossy().into_owned(),
                                provenance.clone(),
                            );
                        match &model {
                            Some(model) => adapter.with_selected_model(
                                &model.policy.native_provider,
                                &model.policy.provider_native_id,
                            )?,
                            None => adapter,
                        }
                    },
                    variant,
                    Some(&cwd),
                    AgentSessionHostLimits::default(),
                    journal.clone(),
                    launch_environment.as_ref(),
                ),
                EncounterProtocol::PrimeRpc => {
                    AgentSessionHost::launch_with_journal_and_environment(
                        {
                            let adapter =
                            aikit_adapters::prime_rpc_connection::PrimeRpcConnectionAdapter::new(
                                connection.clone(),
                                cwd.to_string_lossy().into_owned(),
                                provenance.clone(),
                            );
                            match &model {
                                Some(model) => adapter.with_selected_model(
                                    &model.policy.native_provider,
                                    &model.policy.provider_native_id,
                                )?,
                                None => adapter,
                            }
                        },
                        variant,
                        Some(&cwd),
                        AgentSessionHostLimits::default(),
                        journal.clone(),
                        launch_environment.as_ref(),
                    )
                }
            };
            let attempt_host = match attempt_host {
                Ok(host) => host,
                Err(failure) => {
                    // Spawn failure: no provider process of ours to clean up.
                    self.record_failed_launch_attempt(
                        &agent_session,
                        FailedLaunchAttempt {
                            reconnect,
                            index,
                            variant,
                            failure: &failure,
                            cleanup: None,
                            more_variants: !last_variant,
                        },
                    )?;
                    failed_attempts.push(format!(
                        "`{}` failed to spawn: {}",
                        variant.join(" "),
                        failure.message()
                    ));
                    if single_variant {
                        return Err(failure);
                    }
                    if last_variant {
                        return Err(self.launch_variants_exhausted(&failed_attempts));
                    }
                    continue;
                }
            };
            match attempt_host.initialize() {
                Ok(descriptor) => {
                    launched = Some((attempt_host, descriptor));
                    break;
                }
                Err(failure) => {
                    let cleanup = attempt_host.shutdown();
                    if cleanup.is_err() {
                        // Refuse later effects if this body may still be live.
                        self.shutdown_requested
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    self.record_failed_launch_attempt(
                        &agent_session,
                        FailedLaunchAttempt {
                            reconnect,
                            index,
                            variant,
                            failure: &failure,
                            cleanup: Some(cleanup.is_ok()),
                            more_variants: !last_variant,
                        },
                    )?;
                    failed_attempts.push(format!(
                        "`{}` failed before a usable initialize response: {}",
                        variant.join(" "),
                        failure.message()
                    ));
                    if single_variant {
                        return Err(failure);
                    }
                    // A non-handshake failure means the harness spoke: the
                    // refusal is semantic, never a wrong argv variant.
                    let fallback_eligible = failure.code() == "agent_session_host.handshake_failed";
                    if !fallback_eligible || last_variant {
                        return Err(self.launch_variants_exhausted(&failed_attempts));
                    }
                }
            }
        }
        let (host, negotiated) = launched.expect("a launch variant succeeded or returned");
        let protocol_name = match configured.protocol {
            EncounterProtocol::Acp => "acp",
            EncounterProtocol::PiRpc => "pi-rpc",
            EncounterProtocol::PrimeRpc => "prime-rpc",
        };
        let mcp_resolution = crate::encounter_mcp::session_mcp_resolution(
            protocol_name,
            negotiated.capabilities.mcp_servers,
            mcp_entries.clone(),
        )?;
        // The wire carries none of the composed tool surface only when the
        // resolution says so; record the honest route (the harness's own
        // native MCP configuration seam) in the journal and in the open
        // outcome below, so a composed tool set is never silently dropped.
        let mcp_native_fallback = match &mcp_resolution {
            crate::encounter_mcp::SessionMcpResolution::NativeProjectionFallback { reason } => {
                self.store.append(
                    &agent_session,
                    &json!({
                        "kind":"native-mcp-native-projection-fallback",
                        "provider":provider,
                        "reason":reason,
                        "composed_tools_route":"harness-native-mcp-config-seam-not-the-session-wire"
                    }),
                )?;
                // Execute the route the record names: project the composed
                // capsules into the harness's own managed tools seam when its
                // profile declares one — through the same WorldEdit + Inverse
                // + Procedure pipeline `aikit apply` uses, never a bare file
                // write. The write is a projection, never a session
                // precondition: a failure journals its error event and the
                // open proceeds; a harness without a declared seam records
                // the boundary instead of writing anything.
                let machine_home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                let projection =
                    crate::encounter_native_projection::project_composed_tools_to_native_seam(
                        &self.home,
                        &cwd,
                        &machine_home,
                        configured.from_profile.as_deref(),
                        &mcp_entries,
                    );
                self.store.append(
                    &agent_session,
                    &projection.journal_event(&provider, configured.from_profile.as_deref()),
                )?;
                Some((reason.clone(), projection))
            }
            _ => None,
        };
        let mcp = mcp_resolution;
        // ACP reconnect rides the capability-gated `session/resume` (stabilized
        // 2026-04-23, no history replay): the adapter routes Resume there and
        // refuses naming `agentCapabilities.sessionCapabilities.resume` when
        // the target did not advertise it — the stale "ACP has no generic
        // attach" refusal is gone. The refusal surfaces through the
        // native-open-refused journal event below.
        let lane = match host.open_session(crate::encounter_mcp::build_session_open_request(
            if reconnect {
                SessionOpenMode::Resume
            } else if matches!(
                configured.protocol,
                EncounterProtocol::PiRpc | EncounterProtocol::PrimeRpc
            ) {
                SessionOpenMode::Attach
            } else {
                SessionOpenMode::Create
            },
            if reconnect {
                Some(
                    previous
                        .as_ref()
                        .and_then(|p| p["native_session_id"].as_str())
                        .ok_or_else(|| error("Prior native session identity is missing"))?
                        .to_owned(),
                )
            } else {
                None
            },
            &cwd.to_string_lossy(),
            mcp,
            Some(agent_session.clone()),
        )) {
            Ok(lane) => lane,
            Err(failure) => {
                // The adapter can reject session/resume before a SessionOpened
                // binding exists. Retain that actual failure and confirmed
                // cleanup without inventing a successful native continuation.
                let cleanup = host.shutdown();
                if cleanup.is_err() {
                    self.shutdown_requested
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                self.store.append(
                    &agent_session,
                    &json!({
                        "kind":"native-open-refused",
                        "continuation_requested":reconnect,
                        "error_code":failure.code(),
                        "cleanup_confirmed":cleanup.is_ok(),
                        "binding_recorded":false,
                        "turn_replayed":false
                    }),
                )?;
                return Err(failure);
            }
        };
        let native = lane.binding().native_session_id.clone();
        if reconnect
            && previous
                .as_ref()
                .and_then(|p| p["native_session_id"].as_str())
                != Some(native.as_str())
        {
            let cleanup = host.shutdown();
            if cleanup.is_err() {
                // Refuse later effects if this body may still be live.
                self.shutdown_requested
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            self.store.append(&agent_session, &json!({"kind":"native-reconnect-identity-refused", "cleanup_confirmed":cleanup.is_ok(), "turn_replayed":false}))?;
            return Err(AikitError::new(
                "encounter.native_identity_changed",
                "The harness returned another native identity to session/resume; no binding or successful continuation was recorded",
            ));
        }
        // A bound policy is delivered, never assumed. Pi RPC carried its
        // selection into the session open through the adapter; an ACP
        // resident receives it now, through the native session's own model
        // configuration: the adapter refuses a harness that advertises no
        // model selector or a model outside its advertised list, and
        // confirms the readback — the readback semantics the old
        // protocol-only gate said were unassumed are proven here, or the
        // open fails.
        let selected_configuration = match (&model, configured.protocol) {
            (Some(model), EncounterProtocol::Acp) => {
                let receipt = lane.set_model(&model.policy.provider_native_id)?;
                Some((model.dispatch.clone(), receipt))
            }
            _ => None,
        };
        let model_observation = lane.binding().model_observation.clone();
        let model_reading = serde_json::to_value(&model).map_err(error)?;
        let body_ref = configured.body_ref.clone();
        let body_revision = configured.body_revision.clone();
        if let Some((dispatch, receipt)) = &selected_configuration {
            self.store.append(&agent_session,&json!({"kind":"selected-model-configured","agent_session":agent_session,"native_session_id":receipt.native_session_id,"provider":provider,"dispatch":dispatch,"previous_model_observation":receipt.previous,"model_observation":receipt.current,"standing":"provider-confirmed-session-configuration-under-the-durable-model-policy"}))?;
        }
        let opened_mode_observation = lane.binding().mode_observation.clone();
        self.store.append(&agent_session,&json!({"kind":"binding","space":space,"provider":provider,"protocol":configured.protocol,"body_ref":body_ref,"body_revision":body_revision,"cwd":cwd,"provider_argv_digest":blake3::hash(serde_json::to_string(&configured.argv).expect("argv JSON").as_bytes()).to_hex().to_string(),"now_context_config_digest":blake3::hash(serde_json::to_vec(&configured.now_context).expect("NOW config JSON").as_slice()).to_hex().to_string(),"native_session_id":native,"model_observation":model_observation,"mode_observation":opened_mode_observation,"model_selection":model_reading,"effective_launch_argv":launch_argv,"continuation":if reconnect {"native-resume"} else {"new-native-session"},"composed_tools_route":if mcp_native_fallback.is_some() {"harness-native-mcp-config-seam"} else {"session-wire-or-none"},"mcp_native_fallback_reason":mcp_native_fallback.as_ref().map(|(reason, _)| reason.clone())}))?;
        // The owner drains transport delivery; durable cursor readers are
        // independent views of the same canonical journal.
        let drain = lane.clone();
        std::thread::spawn(move || while drain.recv().is_some() {});
        // Drain the bounded child-to-parent message channel into the same
        // journal: one file is one record the child wrote through its
        // inherited skill, removed only after it is journalled. A file that
        // cannot be journalled is renamed aside and named, never silently
        // dropped. The channel carries words, never effects.
        if let Some(dir) = child_message_dir {
            let store = Arc::clone(&self.store);
            let session = agent_session.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    return;
                };
                let mut files: Vec<std::path::PathBuf> = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                    .collect();
                files.sort();
                for path in files {
                    let journaled = (|| -> std::result::Result<(), AikitError> {
                        let bytes = std::fs::read(&path).map_err(error)?;
                        if bytes.len() > 64 * 1024 {
                            return Err(error("child message exceeds the 64 KiB channel bound"));
                        }
                        let value: serde_json::Value =
                            serde_json::from_slice(&bytes).map_err(error)?;
                        if value.get("schema").and_then(|s| s.as_str())
                            != Some("actuation.child-message/v1")
                        {
                            return Err(error("unrecognised child message schema"));
                        }
                        let cursor = store
                            .append(
                                &session,
                                &json!({
                                    "kind":"child-message",
                                    "from":value.get("from").cloned().unwrap_or(serde_json::Value::Null),
                                    "receiver_role":value.get("receiver_role").cloned().unwrap_or(serde_json::Value::Null),
                                    "text":value.get("text").cloned().unwrap_or(serde_json::Value::Null),
                                    "text_sha256":value.get("text_sha256").cloned().unwrap_or(serde_json::Value::Null),
                                    "file":path.file_name().map(|name| name.to_string_lossy().to_string()),
                                }),
                            )
                            .map_err(|e| error(e.to_string()))?;
                        let _ = cursor;
                        Ok(())
                    })();
                    match journaled {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&path);
                        }
                        Err(failure) => {
                            let _ = std::fs::rename(&path, path.with_extension("json.rejected"));
                            let _ = store.append(
                                &session,
                                &json!({
                                    "kind":"child-message-rejected",
                                    "file":path.file_name().map(|name| name.to_string_lossy().to_string()),
                                    "reason":failure.to_string(),
                                }),
                            );
                        }
                    }
                }
            });
        }
        // A new session starts in the owner's configured default permission
        // mode when the harness advertises it. A continued (loaded) session
        // keeps whatever mode it was left in: nothing is re-imposed on it.
        if !reconnect {
            self.apply_default_mode(&agent_session, &host, &lane, &provider, &configured.argv);
        }
        let mode_observation = host
            .identity(&agent_session)
            .ok()
            .and_then(|identity| identity.binding.mode_observation);
        residents.insert(
            agent_session.clone(),
            Arc::new(Resident {
                host,
                lane,
                space,
                provider,
                provider_label: configured.label,
                operations: Mutex::new(()),
                required_context: configured.required_context,
                body_ref: body_ref.clone(),
                body_revision: body_revision.clone(),
                protocol: configured.protocol,
                generation,
                cwd,
                argv: configured.argv,
                model,
                now_context: configured.now_context,
            }),
        );
        drop(residents);
        drop(agency_lock);
        // The resident just became ready: this is the moment queued durable
        // deliveries wait for. Drain before answering the open.
        let mut receipt = json!({"agent_session":agent_session,"native_session_id":native,"model_observation":model_observation,"mode_observation":mode_observation,"model_selection":model_reading,"body_ref":configured.body_ref,"body_revision":configured.body_revision,"resident":true,"inference_observed":false});
        if let Some((reason, projection)) = &mcp_native_fallback {
            // The composed tool surface does not ride this session's wire: the
            // open outcome names the harness's native MCP configuration seam
            // as the route that carries it, and records what that route
            // actually did — the executed write receipt, the named boundary
            // when the harness declares no managed seam, or the failure.
            // Never silent, never a promise the write did not keep.
            receipt["composed_tools"] = json!({
                "carried_on_wire":false,
                "route":"harness-native-mcp-config-seam",
                "reason":reason,
                "execution":projection.execution_receipt(configured.from_profile.as_deref()),
            });
        }
        self.open_receipt_with_drain(agent_session, receipt)
    }

    /// Journal one failed launch attempt, following the native-open-refused
    /// event pattern: what was attempted, why it failed, whether a process
    /// existed and its cleanup is confirmed, and whether another declared
    /// variant follows. Never silent, never a bare retry.
    fn record_failed_launch_attempt(
        &self,
        agent_session: &ResourceRef,
        attempt: FailedLaunchAttempt<'_>,
    ) -> Result<()> {
        self.store
            .append(
                agent_session,
                &json!({
                    "kind":"native-launch-attempt-failed",
                    "continuation_requested":attempt.reconnect,
                    "attempt":attempt.index,
                    "argv":attempt.variant,
                    "error_code":attempt.failure.code(),
                    "reason":attempt.failure.message(),
                    "process_started":attempt.cleanup.is_some(),
                    "cleanup_confirmed":attempt.cleanup,
                    "fallback":if attempt.more_variants {"next-declared-variant"} else {"none"}
                }),
            )
            .map(|_| ())
    }

    /// The error for a launch whose variants are done being tried: it names
    /// every variant attempted and each one's failure reason.
    fn launch_variants_exhausted(&self, failed_attempts: &[String]) -> AikitError {
        AikitError::new(
            "encounter.launch_variants_exhausted",
            format!(
                "every declared launch variant failed: {}",
                failed_attempts.join("; ")
            ),
        )
    }

    /// An open/reconnect makes the resident ready — the moment queued durable
    /// deliveries wait for. Drain before answering so the caller sees what the
    /// readiness admitted. Drain trouble never fails the open itself.
    fn open_receipt_with_drain(
        &self,
        agent_session: ResourceRef,
        mut receipt: Value,
    ) -> Result<Value> {
        match self.drain_queued_deliveries(&agent_session) {
            Ok(drain) => {
                let drained =
                    |value: &Value| value.as_array().is_some_and(|entries| !entries.is_empty());
                if drained(&drain["delivered"]) || drained(&drain["refused"]) {
                    receipt["queued_drain"] = drain;
                }
            }
            Err(failure) => {
                let _ = self.store.append(
                    &agent_session,
                    &json!({"kind":"queued-drain-error","code":failure.code(),"reason":failure.message()}),
                );
            }
        }
        Ok(receipt)
    }

    /// Reconnect a failed body under the exclusive owner lease. A view-only
    /// reconnect cannot stop another session, replay a turn or mint a replacement.
    fn reconnect_native(
        &self,
        space: SessionSpaceRef,
        agent_session: ResourceRef,
        provider: String,
        cwd: PathBuf,
    ) -> Result<Value> {
        let mut lifecycle = self.lifecycle.write().map_err(error)?;
        if self
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
            || !matches!(*lifecycle, Lifecycle::Running)
        {
            return Err(AikitError::new(
                "encounter.owner_stopped",
                "Owner is stopping or requires cleanup repair",
            ));
        }
        let cwd = std::fs::canonicalize(cwd).map_err(error)?;
        self.require_attached(&agent_session)?;
        crate::direct_agent_session::check(&self.home, &agent_session, &cwd)?;
        let mut residents = self.residents.lock().map_err(error)?;
        if let Some(held) = residents.get(&agent_session) {
            if held.space != space || held.provider != provider || held.cwd != cwd {
                return Err(AikitError::new(
                    "encounter.reconnect_basis",
                    "Reconnect cannot change the native session's Project, Space or provider",
                ));
            }
            if held.host.transport_error().is_none() {
                drop(residents);
                return self.open_native(space, agent_session, provider, cwd, true, None);
            }
            if !held
                .host
                .descriptor()?
                .capabilities
                .supports(SessionOpenMode::Resume)
            {
                return Err(AikitError::new(
                    "encounter.resume_unsupported",
                    "This harness did not advertise agentCapabilities.sessionCapabilities.resume; ACP reconnect rides the capability-gated session/resume, and without it the failed session remains inspectable but cannot be resumed",
                ));
            }
            self.store.append(&agent_session, &json!({"kind":"native-reconnect-requested", "native_session_id":held.lane.binding().native_session_id, "operation":"session/resume", "previous_turn_outcome":"unknown; not-replayed"}))?;
            let removed = residents.remove(&agent_session).expect("held resident");
            let removed = match Arc::try_unwrap(removed) {
                Ok(resident) => resident,
                Err(held) => {
                    residents.insert(agent_session.clone(), held);
                    return Err(AikitError::new(
                        "encounter.resident_in_use",
                        "The failed body is still borrowed; inspect and explicitly retry after it settles",
                    ));
                }
            };
            if let Err(failure) = removed.host.shutdown() {
                let reason = format!("Failed body cleanup is uncertain: {failure}");
                *lifecycle = Lifecycle::Failed(reason.clone());
                let _ = self.store.append(
                    &agent_session,
                    &json!({"kind":"native-reconnect-cleanup-uncertain","reason":reason}),
                );
                return Err(AikitError::new("encounter.cleanup_uncertain", reason));
            }
            self.permissions
                .lock()
                .map_err(error)?
                .remove(&agent_session);
        }
        drop(residents);
        self.open_native(space, agent_session, provider, cwd, true, None)
    }

    pub fn apply(&self, request: EncounterRequest) -> Result<Value> {
        if let EncounterRequest::Reconnect {
            space,
            agent_session,
            provider,
            cwd,
        } = request
        {
            return self.reconnect_native(space, agent_session, provider, cwd);
        }
        if let EncounterRequest::Shutdown { expected_pid } = &request {
            return self.shutdown(*expected_pid);
        }
        // This lease prevents a new launch or effect from racing with shutdown.
        if self
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(AikitError::new(
                "encounter.owner_stopped",
                "The encounter owner is shutting down or stopped",
            ));
        }
        let lifecycle = self.lifecycle.read().map_err(error)?;
        if self
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
            || !matches!(*lifecycle, Lifecycle::Running)
        {
            return Err(AikitError::new(
                "encounter.owner_stopped",
                "The encounter owner is shutting down or stopped",
            ));
        }
        match request {
            EncounterRequest::Context { request } => {
                self.check_context_scope(&request.scope)?;
                let data = match request.request {
                    ContextOperation::Read => self.store.prepared_context(&request.scope)?,
                    ContextOperation::Edit { basis, mutation } => {
                        self.store.edit_context(&request.scope, basis, *mutation)?
                    }
                    ContextOperation::Adopt {
                        basis,
                        project_basis,
                    } => self
                        .store
                        .adopt_context(&request.scope, basis, project_basis)?,
                };
                Ok(json!(data))
            }
            EncounterRequest::PromptContext {
                agent_session,
                draft_revision,
                context,
            } => {
                self.check_context_scope(&context.scope)?;
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                let _agency_lock = self.lock_agency(&agent_session)?;
                self.check_resident_context(&agent_session, &resident, "before-prompt")?;
                let mut now_context = None;
                let cleared = self.store.submit_context(
                    &agent_session,
                    draft_revision,
                    Some(&context),
                    |text| {
                        let text = self.prepare_agency_text(&agent_session, text)?;
                        let prepared = self.prepare_now_context(&agent_session, text)?;
                        let handle = resident
                            .lane
                            .prompt(resident.prompt_payload(&prepared.text))?;
                        drop(handle);
                        now_context = Some(prepared);
                        Ok(())
                    },
                )?;
                if let Some(prepared) = now_context {
                    self.finish_now_context(&agent_session, prepared)?;
                }
                Ok(
                    json!({"accepted":true,"draft":cleared,"context_revision":context.revision,"context_digest":context.digest}),
                )
            }
            EncounterRequest::OpenModel { request } => self.open_model(*request),
            request @ (EncounterRequest::Send { .. }
            | EncounterRequest::SendGroup { .. }
            | EncounterRequest::Delivery { .. }) => self.agency_request(request),
            // Reconnect never reaches this match: apply() routes it through
            // reconnect_native before the read lease is taken, so the
            // lifecycle guard always holds for reconnects.
            EncounterRequest::Reconnect { .. } | EncounterRequest::Shutdown { .. } => {
                unreachable!("handled before acquiring read lease")
            }
            EncounterRequest::Permission {
                agent_session,
                request_id,
                decision,
            } => {
                self.require_attached(&agent_session)?;
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                let _agency_lock = self.lock_agency(&agent_session)?;
                let request = self
                    .permissions
                    .lock()
                    .map_err(error)?
                    .get(&agent_session)
                    .and_then(|requests| requests.get(&request_id))
                    .cloned()
                    .ok_or_else(|| {
                        AikitError::new(
                            "encounter.permission_stale",
                            "Provider permission is no longer pending",
                        )
                    })?;
                if let PermissionDecision::Selected { option_id } = &decision {
                    let rejecting = request
                        .choices
                        .iter()
                        .find(|choice| &choice.option_id == option_id)
                        .and_then(|choice| choice.kind.as_deref())
                        .is_some_and(|kind| matches!(kind, "reject_once" | "reject_always"));
                    if !rejecting {
                        self.check_agency(&agent_session)?;
                        self.check_resident_context(
                            &agent_session,
                            &resident,
                            "provider-permission",
                        )?;
                    }
                }
                // The actual adapter validates native request identity and the offered
                // option. This is provider transport consent, never an Actuation grant.
                self.store.append(&agent_session,&json!({"kind":"provider-permission-response-requested","request":request,"decision":decision,"authority":"native-provider-consent"}))?;
                resident
                    .lane
                    .respond_permission(&request, decision.clone())?;
                self.store.append(&agent_session,&json!({"kind":"provider-permission-response-sent","request_id":request_id,"decision":decision,"authority":"native-provider-consent"})).map_err(|e|AikitError::new("encounter.permission_response_uncertain",format!("Provider consent response was sent but receipt failed; do not resend: {e}")))?;
                if let Some(requests) = self
                    .permissions
                    .lock()
                    .map_err(error)?
                    .get_mut(&agent_session)
                {
                    requests.remove(&request_id);
                }
                Ok(json!({"sent":true,"request_id":request_id}))
            }
            EncounterRequest::Health => {
                Ok(json!({"protocol":"aikit-encounter-v1","pid":std::process::id()}))
            }
            EncounterRequest::View {
                agent_session,
                before,
            } => {
                self.require_attached(&agent_session)?;
                let mut view = self.store.view(&agent_session, before)?;
                let resident = self
                    .residents
                    .lock()
                    .map_err(error)?
                    .get(&agent_session)
                    .cloned();
                let (connection, ready) = match resident {
                    Some(resident) => {
                        let identity = resident.host.identity(&agent_session)?;
                        let fault = resident.host.transport_error();
                        let state = format!("{:?}", identity.state);
                        let ready = state == "Resident" && fault.is_none();
                        (
                            json!({"resident":true,"native_session_id":identity.binding.native_session_id,"state":state,"error":fault,"provider":resident.provider_view()}),
                            ready,
                        )
                    }
                    None => (
                        json!({"resident":false,"state":"Disconnected","error":null}),
                        false,
                    ),
                };
                let active = connection["state"] == "TurnInFlight";
                view["schema"] = json!("aikit.encounter-view/v1");
                view["connection"] = connection;
                view["permissions"] = json!(self
                    .permissions
                    .lock()
                    .map_err(error)?
                    .get(&agent_session)
                    .map(|r| r.values().cloned().collect::<Vec<_>>())
                    .unwrap_or_default());
                view["permission_authority"] = json!("native-provider-consent");
                view["history_reclassifications"] =
                    json!(self.store.legacy_load_reclassifications(&agent_session)?);
                let can_open = !view["connection"]["resident"].as_bool().unwrap_or(false)
                    && !self.providers()?.is_empty();
                view["actions"] = json!([
                    {"ref":"aikit.encounter.open","enabled":can_open,"reason":if can_open{None}else{Some("An encounter requires a configured provider and no existing resident connection")}},
                    {"ref":"aikit.encounter.draft","enabled":true,"reason":null},
                    {"ref":"aikit.encounter.context","enabled":true,"reason":null},
                    {"ref":"aikit.encounter.prompt","enabled":ready,"reason":if ready{None}else{Some("A ready resident provider is required")}},
                    {"ref":"aikit.encounter.cancel","enabled":active,"reason":if active{None}else{Some("There is no active provider turn")}},
                    {"ref":"aikit.encounter.permission","enabled":!view["permissions"].as_array().is_none_or(|r|r.is_empty()),"reason":"Only an actual pending provider consent request can be answered; this does not confer Actuation authority"},
                    {"ref":"aikit.encounter.model-read","enabled":ready,"reason":if ready{None}else{Some("A ready resident provider is required")}},
                    {"ref":"aikit.encounter.mode-read","enabled":ready,"reason":if ready{None}else{Some("A ready resident provider is required")}}
                ]);
                Ok(view)
            }
            EncounterRequest::Providers => Ok(json!(self
                .providers()?
                .into_iter()
                .map(|p| {
                    let mut row = provider_launch_facts(p.protocol, &p.argv);
                    row["id"] = json!(p.id);
                    row["label"] = json!(p.label);
                    row
                })
                .collect::<Vec<_>>())),
            EncounterRequest::ClassifyLegacyLoadReplay { agent_session } => {
                self.require_attached(&agent_session)?;
                let classification = self.store.classify_legacy_load_replay(&agent_session)?;
                self.store.append(&agent_session, &json!({"kind":"legacy-native-load-replay-classification","classification":classification}))?;
                Ok(classification)
            }
            EncounterRequest::AddressableParticipants { request } => {
                self.addressable_participants(*request)
            }
            EncounterRequest::ModelRead { agent_session } => {
                self.require_attached(&agent_session)?;
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                self.check_resident_context(&agent_session, &resident, "native-model-read")?;
                let identity = resident.host.identity(&agent_session)?;
                Ok(json!({
                    "agent_session":agent_session,
                    "native_session_id":identity.binding.native_session_id,
                    "model_observation":identity.binding.model_observation,
                    "model_controls":match resident.host.transport_error() {
                        Some(reason) => aikit_adapters::interactive_connection::NativeModelControls::unavailable(reason),
                        None => resident.lane.model_controls()?,
                    },
                    "pinned_model_id":resident.model.as_ref().map(|model| &model.policy.provider_native_id),
                    "standing":"provider-reported-configuration-not-independent-selection-or-inference-proof"
                }))
            }
            EncounterRequest::ModelSelect {
                agent_session,
                provider_model_id,
                provider_reasoning_effort,
                expected_native_session_id,
            } => {
                self.require_attached(&agent_session)?;
                if provider_model_id.trim().is_empty() || provider_model_id.len() > 256 {
                    return Err(AikitError::new(
                        "encounter.invalid_provider_model_id",
                        "Provider model id must be a non-empty bounded native identifier",
                    ));
                }
                if provider_reasoning_effort
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
                {
                    return Err(AikitError::new(
                        "encounter.invalid_provider_reasoning_effort",
                        "Provider reasoning effort must be a non-empty bounded native identifier",
                    ));
                }
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                self.check_resident_context(&agent_session, &resident, "native-model-select")?;
                let observed = resident.host.identity(&agent_session)?;
                if expected_native_session_id
                    .as_ref()
                    .is_some_and(|expected| expected != &observed.binding.native_session_id)
                {
                    return Err(AikitError::new("encounter.stale_native_session", "The native session changed after the model read; read its controls again before selecting"));
                }
                if resident
                    .model
                    .as_ref()
                    .is_some_and(|model| model.policy.provider_native_id != provider_model_id)
                {
                    return Err(AikitError::new(
                        "encounter.model_policy_conflict",
                        "The requested provider model conflicts with this resident's explicit durable model policy; reopen through the policy owner",
                    ));
                }
                let before = resident.host.identity(&agent_session)?;
                self.store.append(&agent_session, &json!({
                    "kind":"native-model-configuration-requested",
                    "agent_session":agent_session,
                    "native_session_id":before.binding.native_session_id,
                    "provider":resident.provider,
                    "requested_provider_model_id":provider_model_id,
                    "requested_provider_reasoning_effort":provider_reasoning_effort,
                    "authority":"provider-advertised-session-config; not-durable-model-policy-or-agency"
                }))?;
                let mut receipt = resident.lane.set_model(&provider_model_id)?;
                if let Some(provider_reasoning_effort) = provider_reasoning_effort.as_deref() {
                    receipt = resident
                        .lane
                        .set_reasoning_effort(provider_reasoning_effort)?;
                }
                self.store.append(&agent_session, &json!({
                    "kind":"native-model-configuration-confirmed",
                    "receipt":receipt,
                    "authority":"provider-confirmed-session-config; not-durable-model-policy-or-agency"
                })).map_err(|e| AikitError::new("encounter.model_configuration_uncertain", format!("Provider confirmed configuration but receipt persistence failed; do not resend automatically: {e}")))?;
                Ok(json!({
                    "agent_session":receipt.agent_session,
                    "native_session_id":receipt.native_session_id,
                    "previous_model_observation":receipt.previous,
                    "model_observation":receipt.current,
                    "selected":true,
                    "inference_observed":false,
                    "standing":"provider-confirmed-native-session-configuration; durable-model-policy-and-agency-unchanged"
                }))
            }
            EncounterRequest::ModeRead { agent_session } => {
                self.require_attached(&agent_session)?;
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                self.check_resident_context(&agent_session, &resident, "native-mode-read")?;
                let identity = resident.host.identity(&agent_session)?;
                Ok(json!({
                    "agent_session":agent_session,
                    "native_session_id":identity.binding.native_session_id,
                    "mode_observation":identity.binding.mode_observation,
                    "mode_controls":match resident.host.transport_error() {
                        Some(reason) => aikit_adapters::interactive_connection::NativeModeControls::unavailable(reason),
                        None => resident.lane.mode_controls()?,
                    },
                    "standing":"provider-reported-configuration-not-independent-selection-or-inference-proof"
                }))
            }
            EncounterRequest::ModeSelect {
                agent_session,
                provider_mode_id,
                expected_native_session_id,
            } => {
                self.require_attached(&agent_session)?;
                if provider_mode_id.trim().is_empty() || provider_mode_id.len() > 128 {
                    return Err(AikitError::new(
                        "encounter.invalid_provider_mode_id",
                        "Provider mode id must be a non-empty bounded native identifier",
                    ));
                }
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                self.check_resident_context(&agent_session, &resident, "native-mode-select")?;
                let observed = resident.host.identity(&agent_session)?;
                if expected_native_session_id
                    .as_ref()
                    .is_some_and(|expected| expected != &observed.binding.native_session_id)
                {
                    return Err(AikitError::new("encounter.stale_native_session", "The native session changed after the mode read; read its modes again before selecting"));
                }
                // Refuse before anything is journaled or sent: only an idle
                // session that advertised this exact mode can be asked.
                let controls = resident.lane.mode_controls()?;
                if !controls.mode_selection {
                    return Err(AikitError::new(
                        "encounter.mode_selection_unavailable",
                        controls.reason.unwrap_or_else(|| {
                            "This session offers no permission-mode selection".into()
                        }),
                    ));
                }
                if !observed
                    .binding
                    .mode_observation
                    .as_ref()
                    .is_some_and(|modes| modes.advertises(&provider_mode_id))
                {
                    return Err(AikitError::new(
                        "encounter.mode_not_advertised",
                        format!("The harness did not advertise mode {provider_mode_id} for this session; read its modes again"),
                    ));
                }
                let receipt = self.configure_native_mode(
                    &agent_session,
                    &resident.lane,
                    &resident.provider,
                    &observed.binding.native_session_id,
                    &provider_mode_id,
                    None,
                )?;
                Ok(json!({
                    "agent_session":receipt.agent_session,
                    "native_session_id":receipt.native_session_id,
                    "previous_mode_observation":receipt.previous,
                    "mode_observation":receipt.current,
                    "selected":true,
                    "standing":"provider-confirmed-native-session-mode; applies-to-the-next-action"
                }))
            }
            EncounterRequest::Open {
                space,
                agent_session,
                provider,
                cwd,
            } => self.open_native(space, agent_session, provider, cwd, false, None),
            EncounterRequest::Read {
                agent_session,
                after,
                limit,
            } => {
                self.require_attached(&agent_session)?;
                Ok(json!(self.store.events(&agent_session, after, limit)?))
            }
            EncounterRequest::Draft {
                agent_session,
                basis,
                text,
            } => {
                self.require_attached(&agent_session)?;
                Ok(json!(self.store.set_draft(&agent_session, basis, &text)?))
            }
            EncounterRequest::Prompt {
                agent_session,
                draft_revision,
            } => {
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
                let _agency_lock = self.lock_agency(&agent_session)?;
                self.check_resident_context(&agent_session, &resident, "before-prompt")?;
                let mut context_evidence = None;
                let mut now_context = None;
                let cleared = self.store.submit(&agent_session, draft_revision, |text| {
                    let text = self.prepare_agency_text(&agent_session, text)?;
                    let prepared = self.prepare_now_context(&agent_session, text)?;
                    let (text, evidence) = crate::direct_agent_session::prompt(
                        &self.home,
                        &agent_session,
                        &prepared.text,
                    )?;
                    let handle = resident.lane.prompt(resident.prompt_payload(&text))?;
                    context_evidence = evidence;
                    now_context = Some(prepared);
                    drop(handle);
                    Ok(())
                })?;
                // submit holds the journal transaction. Append only after
                // commit; a failed post-dispatch receipt is never replayable.
                if let Some(mut evidence) = context_evidence {
                    evidence["draft_revision"] = json!(draft_revision);
                    evidence["native_session_id"] =
                        json!(resident.lane.binding().native_session_id);
                    self.store.append(&agent_session, &evidence).map_err(|_| AikitError::new("encounter.submission_uncertain", "Native prompt was submitted but context receipt failed; reread, do not replay"))?;
                }
                if let Some(prepared) = now_context {
                    self.finish_now_context(&agent_session, prepared)?;
                }
                Ok(json!({"accepted":true,"draft":cleared}))
            }
            EncounterRequest::Cancel {
                agent_session,
                reason,
            } => {
                let resident = self.resident(&agent_session)?;
                let receipt = resident.lane.interrupt(reason)?;
                Ok(
                    json!({"commands":receipt.commands,"requested_at_sequence":receipt.requested_at_sequence}),
                )
            }
            EncounterRequest::Status { agent_session } => {
                let resident = self.resident(&agent_session)?;
                let identity = resident.host.identity(&agent_session)?;
                // The status reading names the acting body exactly as the open
                // receipt did: the configured provider id/label, the pinned
                // body_ref/body_revision (a body-less ordinary provider leaves
                // them null) and the observed model facts. Consumers that must
                // require an exact body read them from here, never from a
                // persisted active flag.
                let binding = resident.lane.binding();
                Ok(
                    json!({"agent_session":agent_session,"native_session_id":identity.binding.native_session_id,"state":format!("{:?}",identity.state),"error":resident.host.transport_error(),"provider":resident.provider_view(),"model_observation":binding.model_observation,"permissions":self.permissions.lock().map_err(error)?.get(&agent_session).map(|r|r.values().cloned().collect::<Vec<_>>()).unwrap_or_default(),"permission_authority":"native-provider-consent"}),
                )
            }
        }
    }
}

/// Short, home-specific local IPC path also works for long native home paths.
pub fn socket_path(home: &AikitHome) -> PathBuf {
    let key = blake3::hash(home.state().as_os_str().as_encoded_bytes()).to_hex();
    std::env::temp_dir()
        .join(format!("aikit-{}", &key[..16]))
        .join("owner.sock")
}

#[cfg(unix)]
pub fn serve(home: AikitHome, socket: &Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    if let Some(parent) = socket.parent() {
        use std::os::unix::fs::DirBuilderExt;
        if !parent.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(error)?;
        }
        let metadata = std::fs::symlink_metadata(parent).map_err(error)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(error("Encounter socket requires a private owner directory"));
        }
    }
    // One owner per home, even when clients supply another socket path.
    // Kernel advisory lock survives client exit and is released on owner death.
    let _resident_lock = aikit_store::ContextLock::acquire(
        &home,
        "encounter-resident",
        aikit_store::LockOptions::default()
            .with_timeout(std::time::Duration::ZERO)
            .with_purpose("resident canonical ACP encounters"),
    )?;
    if socket.exists() {
        use std::os::unix::fs::FileTypeExt;
        let metadata = std::fs::symlink_metadata(socket).map_err(error)?;
        if !metadata.file_type().is_socket() {
            return Err(error("Refusing to replace a non-socket encounter path"));
        }
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Err(error("Encounter socket is already active"));
        }
        std::fs::remove_file(socket).map_err(error)?;
    }
    let listener = UnixListener::bind(socket).map_err(error)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).map_err(error)?;
    let service = Arc::new(EncounterService::new(home)?);
    let clients = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    listener.set_nonblocking(true).map_err(error)?;
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    while !stop.load(Ordering::Acquire) {
        // Retire completed threads rather than accumulating one handle per IPC.
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let _ = workers.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
            Err(failure) => return Err(error(failure)),
        };
        // Accepted sockets inherit O_NONBLOCK on macOS. Workers use bounded
        // blocking reads/writes, so restore that mode before receiving a frame
        // or writing a response larger than the socket's immediate capacity.
        stream.set_nonblocking(false).map_err(error)?;
        if clients.fetch_add(1, Ordering::AcqRel) >= 32 {
            clients.fetch_sub(1, Ordering::AcqRel);
            let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
            let _ = stream.write_all(b"{\"error\":\"encounter client capacity reached\"}\n");
            continue;
        }
        let service = service.clone();
        let clients = clients.clone();
        let stop = stop.clone();
        workers.push(std::thread::spawn(move || {
            let mut shutdown = false;
            let result = (|| -> Result<Value> {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .map_err(error)?;
                let mut bytes = Vec::new();
                use std::io::Read;
                BufReader::new(&stream)
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes)
                    .map_err(error)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(error("encounter request exceeds byte limit"));
                }
                let request = serde_json::from_slice(&bytes).map_err(error)?;
                shutdown = matches!(request, EncounterRequest::Shutdown { .. });
                service.apply(request)
            })();
            let shutdown_succeeded = shutdown && result.is_ok();
            let response = match result {
                Ok(data) => json!({"ok":true,"data":data}),
                Err(error) => {
                    json!({"ok":false,"error":{"code":error.code(),"message":error.message()}})
                }
            };
            if let Ok(mut bytes) = serde_json::to_vec(&response) {
                bytes.push(b'\n');
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
                let _ = stream.write_all(&bytes);
            }
            // The process must not exit before the successful response has been
            // sent (or the requesting client has disconnected).
            if shutdown_succeeded {
                stop.store(true, Ordering::Release);
            }
            clients.fetch_sub(1, Ordering::AcqRel);
        }));
    }
    drop(listener);
    for worker in workers {
        let _ = worker.join();
    }
    std::fs::remove_file(socket).map_err(error)?;
    Ok(())
}
#[cfg(unix)]
pub fn request(socket: &Path, request: &EncounterRequest) -> Result<Value> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).map_err(error)?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(120)))
        .map_err(error)?;
    let mut bytes = serde_json::to_vec(request).map_err(error)?;
    bytes.push(b'\n');
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(error)?;
    stream.write_all(&bytes).map_err(error)?;
    let mut response = Vec::new();
    BufReader::new(stream)
        .take(4 * 1024 * 1024 + 1)
        .read_until(b'\n', &mut response)
        .map_err(error)?;
    if response.len() > 4 * 1024 * 1024 {
        return Err(error("Encounter response exceeds transport byte capacity"));
    }
    serde_json::from_slice(&response).map_err(error)
}

/// Start the generic native owner once. No provider is opened by startup.
#[cfg(unix)]
pub fn start(home: &AikitHome, cwd: &Path) -> Result<Value> {
    let socket = socket_path(home);
    let _starting = aikit_store::ContextLock::acquire(
        home,
        "encounter-start",
        aikit_store::LockOptions::default().with_purpose("start resident encounter owner"),
    )?;
    if let Ok(health) = request(&socket, &EncounterRequest::Health) {
        return Ok(health);
    }
    std::fs::create_dir_all(home.state()).map_err(error)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.state().join("encounter-owner.log"))
        .map_err(error)?;
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(std::env::current_exe().map_err(error)?)
        .arg("-C")
        .arg(cwd)
        .args(crate::session_space_verb_prefix())
        .arg("encounter-serve")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(error)?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map_err(error)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Ok(health) = request(&socket, &EncounterRequest::Health) {
            return Ok(health);
        }
        if let Some(status) = child.try_wait().map_err(error)? {
            return Err(error(format!(
                "Encounter owner exited ({status}); see encounter-owner.log"
            )));
        }
        if std::time::Instant::now() >= deadline {
            return Err(error(
                "Encounter owner startup is still unconfirmed; inspect native owner log before retrying",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_home() -> AikitHome {
        let temp = TempDir::new().unwrap();
        let home = AikitHome::at(temp.path().join("home"));
        home.ensure_layout().unwrap();
        // Keep the tempdir alive for the whole test by leaking it; each test
        // needs exactly one home.
        std::mem::forget(temp);
        home
    }

    fn profile_provider(id: &str, slug: &str) -> EncounterProvider {
        EncounterProvider {
            protocol: EncounterProtocol::Acp,
            id: id.to_owned(),
            label: format!("{id} via profile"),
            argv: Vec::new(),
            from_profile: Some(slug.to_owned()),
            argv_fallback: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            now_context: None,
        }
    }

    #[test]
    fn a_configured_from_profile_provider_resolves_its_connection_facts_at_load_time() {
        let home = test_home();
        EncounterService::configure(&home, profile_provider("gemini-acp", "gemini")).unwrap();
        let service = EncounterService::new(home).unwrap();

        let providers = service.providers().unwrap();
        let provider = providers
            .iter()
            .find(|p| p.id == "gemini-acp")
            .expect("configured provider resolves");

        assert_eq!(provider.argv, ["gemini", "--acp"]);
        assert_eq!(
            provider.argv_fallback,
            [["gemini", "--experimental-acp"]],
            "the derived fallback variants ride the resolved provider into every open"
        );
        assert_eq!(provider.from_profile.as_deref(), Some("gemini"));
    }

    #[test]
    fn a_from_profile_provider_with_an_explicit_argv_refuses_at_configuration() {
        let home = test_home();
        let mut provider = profile_provider("gemini-acp", "gemini");
        provider.argv = vec![
            "/opt/homebrew/bin/gemini".into(),
            "--experimental-acp".into(),
        ];

        let failure = EncounterService::configure(&home, provider).unwrap_err();

        assert_eq!(failure.code(), "encounter.from_profile_argv_conflict");
    }

    #[test]
    fn an_unknown_from_profile_slug_refuses_at_configuration_not_at_first_open() {
        let home = test_home();

        let failure =
            EncounterService::configure(&home, profile_provider("ghost", "ghost")).unwrap_err();

        assert_eq!(failure.code(), "encounter.from_profile_unknown");
    }

    #[test]
    fn a_plain_provider_without_argv_still_refuses_configuration() {
        let home = test_home();
        let mut provider = profile_provider("hand", "gemini");
        provider.from_profile = None;

        let failure = EncounterService::configure(&home, provider).unwrap_err();

        assert_eq!(failure.code(), "encounter.runtime");
        assert!(
            failure.to_string().contains("explicit native argv"),
            "freeform providers still need their own argv: {failure}"
        );
    }

    #[test]
    fn a_provider_declaring_unreachable_env_or_cwd_refuses_before_any_launch() {
        let home = test_home();
        let mut provider = profile_provider("hand", "gemini");
        provider.from_profile = None;
        provider.argv = vec!["gemini".into(), "--acp".into()];
        provider
            .env
            .insert("GEMINI_API_BASE".into(), "https://api.example".into());

        let failure = EncounterService::configure(&home, provider).unwrap_err();

        // The refusal happens at configuration, before any open can accept
        // and silently drop the declared facts.
        assert_eq!(failure.code(), "encounter.connect_facts_unreachable");
    }
}
