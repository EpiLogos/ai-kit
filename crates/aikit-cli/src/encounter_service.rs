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
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{encounter::EncounterStore, AikitHome, SessionSpaceApplicationStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterProvider {
    pub id: String,
    pub label: String,
    pub argv: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum EncounterRequest {
    Health,
    Permission {
        agent_session: ResourceRef,
        request_id: String,
        decision: PermissionDecision,
    },
    Providers,
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
struct Journal(Arc<EncounterStore>, PendingPermissions);
impl SessionEventJournal for Journal {
    fn append(&self, session: &ResourceRef, event: &HostEvent) -> Result<()> {
        self.0
            .append(session, &json!({"kind":"provider","event":event}))?;
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
}
pub struct EncounterService {
    home: AikitHome,
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
    pub fn apply(&self, request: EncounterRequest) -> Result<Value> {
        match request {
            EncounterRequest::Permission {
                agent_session,
                request_id,
                decision,
            } => {
                self.require_attached(&agent_session)?;
                let resident = self.resident(&agent_session)?;
                let _operation = resident.operations.lock().map_err(error)?;
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
            } => {
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
                let mut residents = self.residents.lock().map_err(error)?;
                if let Some(held) = residents.get(&agent_session) {
                    if held.space != space || held.provider != provider {
                        return Err(error(
                            "Canonical encounter is already bound to another space/provider",
                        ));
                    }
                    return Ok(
                        json!({"agent_session":agent_session,"native_session_id":held.lane.binding().native_session_id,"resident":true}),
                    );
                }
                let configured = self
                    .providers()?
                    .into_iter()
                    .find(|p| p.id == provider)
                    .ok_or_else(|| error("ACP provider is not configured in AIKit"))?;
                let connection = ResourceRef::parse(format!(
                    "connection/encounter-{}",
                    blake3::hash(agent_session.as_str().as_bytes()).to_hex()
                ))
                .map_err(error)?;
                let host = AgentSessionHost::launch_with_journal(
                    AcpStableConnectionAdapter::new(
                        connection,
                        vec![format!("native encounter provider {provider}")],
                    ),
                    &configured.argv,
                    Some(&cwd),
                    AgentSessionHostLimits::default(),
                    Some(Arc::new(Journal(
                        self.store.clone(),
                        self.permissions.clone(),
                    ))),
                )?;
                host.initialize()?;
                let lane = host.open_session(SessionOpenRequest {
                    mode: SessionOpenMode::Create,
                    native_session_id: None,
                    cwd: cwd.to_string_lossy().into_owned(),
                    additional_directories: vec![],
                    mcp_servers: vec![],
                    agent_session: Some(agent_session.clone()),
                })?;
                let native = lane.binding().native_session_id.clone();
                self.store.append(&agent_session,&json!({"kind":"binding","space":space,"provider":provider,"native_session_id":native}))?;
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
                    }),
                );
                Ok(
                    json!({"agent_session":agent_session,"native_session_id":native,"resident":true}),
                )
            }
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
                let cleared = self.store.submit(&agent_session, draft_revision, |text| {
                    let handle = resident.lane.prompt(json!([{"type":"text","text":text}]))?;
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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
    for stream in listener.incoming() {
        let mut stream = stream.map_err(error)?;
        if clients.fetch_add(1, Ordering::AcqRel) >= 32 {
            clients.fetch_sub(1, Ordering::AcqRel);
            let _ = stream.write_all(b"{\"error\":\"encounter client capacity reached\"}\n");
            continue;
        }
        let service = service.clone();
        let clients = clients.clone();
        std::thread::spawn(move || {
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
                service.apply(serde_json::from_slice(&bytes).map_err(error)?)
            })();
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
            clients.fetch_sub(1, Ordering::AcqRel);
        });
    }
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
