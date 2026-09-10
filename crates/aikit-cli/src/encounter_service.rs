//! Resident, provider-neutral ACP encounters. The UI reads cursor pages and
//! submits owner actions; disconnecting an IPC client never drops a provider.
use aikit_adapters::{
    agent_connection::{
        ConnectionSignalKind, NativePermissionRequest, SessionOpenMode, SessionOpenRequest,
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
pub use agency::{
    EncounterAddressedTurn, EncounterAgencyBinding, EncounterContextPacket, EncounterGroupRecipient,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EncounterProtocol {
    #[default]
    Acp,
    PiRpc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterProvider {
    #[serde(default)]
    pub protocol: EncounterProtocol,
    pub id: String,
    pub label: String,
    pub argv: Vec<String>,
    /// Explicit owner-configured admission basis. Absence preserves optional context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_context: Option<EncounterContextAdmission>,
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
                return Err(AikitError::new("encounter.context_stale", format!("Required source {} no longer matches the admitted material for revision {}", source.source, source.revision)));
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
                ))
            }
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum EncounterRequest {
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
    protocol: EncounterProtocol,
    generation: String,
    cwd: PathBuf,
    argv: Vec<String>,
}
impl Resident {
    fn prompt_payload(&self, text: &str) -> Value {
        match self.protocol {
            EncounterProtocol::Acp => json!([{"type":"text","text":text}]),
            EncounterProtocol::PiRpc => json!(text),
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
fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.runtime", message.to_string())
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
            || provider.argv.is_empty()
            || provider.argv[0].is_empty()
        {
            return Err(error(
                "Provider requires a safe id and explicit native argv",
            ));
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
                rows.push(
                    serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?,
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
        let current = self.providers()?.into_iter().find(|p| p.id == resident.provider)
            .ok_or_else(|| AikitError::new("encounter.provider_removed", "The resident provider configuration was removed; reopen explicitly before further effects"))?;
        if current.required_context != resident.required_context
            || current.protocol != resident.protocol
            || current.argv != resident.argv
        {
            let failure = AikitError::new("encounter.context_changed", "Required context configuration changed; recompose and reopen instead of silently updating a resident encounter");
            self.store.append(session, &json!({"kind":"context-admission-refused", "provider":resident.provider, "phase":phase, "code":failure.code(), "reason":failure.to_string()}))?;
            return Err(failure);
        }
        self.check_context(
            session,
            &resident.provider,
            phase,
            resident.required_context.as_ref(),
        )
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
                return Err(AikitError::new("encounter.shutdown_failed", reason.clone()))
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

    fn open_native(
        &self,
        space: SessionSpaceRef,
        agent_session: ResourceRef,
        provider: String,
        cwd: PathBuf,
        reconnect: bool,
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
            return Err(error("Encounter directory is outside its authored local Project context; resolve and attach current native context first"));
        }
        let _agency_lock = self.lock_agency(&agent_session)?;
        self.check_agency(&agent_session)?;
        let previous = self.store.last_native_binding(&agent_session)?;
        let mut residents = self.residents.lock().map_err(error)?;
        if let Some(held) = residents.get(&agent_session) {
            if held.space != space || held.provider != provider || held.cwd != cwd {
                return Err(error(
                    "Canonical encounter is already bound to another space/provider",
                ));
            }
            self.check_resident_context(&agent_session, held, "resident-open")?;
            return Ok(
                json!({"agent_session":agent_session,"native_session_id":held.lane.binding().native_session_id,"model_observation":held.lane.binding().model_observation,"resident":true}),
            );
        }
        if !reconnect && previous.is_some() {
            return Err(AikitError::new("encounter.resume_required","A prior native binding exists; use explicit reconnect, or create a new canonical session for a fresh/forked encounter"));
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
            })
        {
            return Err(AikitError::new("encounter.reconnect_basis", "The recorded cwd/protocol/provider command changed or is unpinned; do not silently resume on a replacement body"));
        }
        self.check_context(
            &agent_session,
            &provider,
            "before-provider-start",
            configured.required_context.as_ref(),
        )?;
        let connection = ResourceRef::parse(format!(
            "connection/encounter-{}",
            blake3::hash(agent_session.as_str().as_bytes()).to_hex()
        ))
        .map_err(error)?;
        if reconnect && configured.protocol != EncounterProtocol::Acp {
            return Err(AikitError::new("encounter.reconnect_unsupported","This native provider does not publish a supported load/resume operation; no replacement session was created"));
        }
        let generation = ulid::Ulid::generate().to_string();
        let journal: Option<Arc<dyn SessionEventJournal>> = Some(Arc::new(Journal(
            self.store.clone(),
            self.permissions.clone(),
            generation.clone(),
        )));
        let provenance = vec![format!("native encounter provider {provider}")];
        let host = match configured.protocol {
            EncounterProtocol::Acp => AgentSessionHost::launch_with_journal(
                AcpStableConnectionAdapter::new(connection, provenance),
                &configured.argv,
                Some(&cwd),
                AgentSessionHostLimits::default(),
                journal,
            ),
            EncounterProtocol::PiRpc => AgentSessionHost::launch_with_journal(
                aikit_adapters::pi_rpc_connection::PiRpcConnectionAdapter::new(
                    connection,
                    cwd.to_string_lossy().into_owned(),
                    provenance,
                ),
                &configured.argv,
                Some(&cwd),
                AgentSessionHostLimits::default(),
                journal,
            ),
        }?;
        host.initialize()?;
        let lane = host.open_session(SessionOpenRequest {
            mode: if reconnect {
                SessionOpenMode::Load
            } else if configured.protocol == EncounterProtocol::PiRpc {
                SessionOpenMode::Attach
            } else {
                SessionOpenMode::Create
            },
            native_session_id: if reconnect {
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
            cwd: cwd.to_string_lossy().into_owned(),
            additional_directories: vec![],
            mcp_servers: vec![],
            agent_session: Some(agent_session.clone()),
        })?;
        let native = lane.binding().native_session_id.clone();
        let model_observation = lane.binding().model_observation.clone();
        self.store.append(&agent_session,&json!({"kind":"binding","space":space,"provider":provider,"protocol":configured.protocol,"cwd":cwd,"provider_argv_digest":blake3::hash(serde_json::to_string(&configured.argv).expect("argv JSON").as_bytes()).to_hex().to_string(),"native_session_id":native,"model_observation":model_observation,"continuation":if reconnect {"native-load"} else {"new-native-session"}}))?;
        // The owner drains transport delivery; durable cursor readers are
        // independent views of the same canonical journal.
        let drain = lane.clone();
        std::thread::spawn(move || while drain.recv().is_some() {});
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
                protocol: configured.protocol,
                generation,
                cwd,
                argv: configured.argv,
            }),
        );
        Ok(
            json!({"agent_session":agent_session,"native_session_id":native,"model_observation":model_observation,"resident":true}),
        )
    }

    pub fn apply(&self, request: EncounterRequest) -> Result<Value> {
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
            request @ (EncounterRequest::Send { .. }
            | EncounterRequest::SendGroup { .. }
            | EncounterRequest::Delivery { .. }) => self.agency_request(request),
            EncounterRequest::Reconnect {
                space,
                agent_session,
                provider,
                cwd,
            } => self.open_native(space, agent_session, provider, cwd, true),
            EncounterRequest::Shutdown { .. } => {
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
                            json!({"resident":true,"native_session_id":identity.binding.native_session_id,"state":state,"error":fault,"provider":{"id":resident.provider,"label":resident.provider_label}}),
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
                let can_open = !view["connection"]["resident"].as_bool().unwrap_or(false)
                    && !self.providers()?.is_empty();
                view["actions"] = json!([
                    {"ref":"aikit.encounter.open","enabled":can_open,"reason":if can_open{None}else{Some("An encounter requires a configured provider and no existing resident connection")}},
                    {"ref":"aikit.encounter.draft","enabled":true,"reason":null},
                    {"ref":"aikit.encounter.prompt","enabled":ready,"reason":if ready{None}else{Some("A ready resident provider is required")}},
                    {"ref":"aikit.encounter.cancel","enabled":active,"reason":if active{None}else{Some("There is no active provider turn")}},
                    {"ref":"aikit.encounter.permission","enabled":!view["permissions"].as_array().is_none_or(|r|r.is_empty()),"reason":"Only an actual pending provider consent request can be answered; this does not confer Actuation authority"}
                ]);
                Ok(view)
            }
            EncounterRequest::Providers => Ok(json!(self
                .providers()?
                .into_iter()
                .map(|p| json!({"id":p.id,"label":p.label}))
                .collect::<Vec<_>>())),
            EncounterRequest::Open {
                space,
                agent_session,
                provider,
                cwd,
            } => self.open_native(space, agent_session, provider, cwd, false),
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
                let cleared = self.store.submit(&agent_session, draft_revision, |text| {
                    let text = self.prepare_agency_text(&agent_session, text)?;
                    let handle = resident.lane.prompt(resident.prompt_payload(&text))?;
                    drop(handle);
                    Ok(())
                })?;
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
                Ok(
                    json!({"agent_session":agent_session,"native_session_id":identity.binding.native_session_id,"state":format!("{:?}",identity.state),"error":resident.host.transport_error(),"permissions":self.permissions.lock().map_err(error)?.get(&agent_session).map(|r|r.values().cloned().collect::<Vec<_>>()).unwrap_or_default(),"permission_authority":"native-provider-consent"}),
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
            return Err(error("Encounter owner startup is still unconfirmed; inspect native owner log before retrying"));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
