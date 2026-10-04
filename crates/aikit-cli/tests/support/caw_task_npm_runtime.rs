//! Genuine Central Task preparation, Workcell confinement and installed npm.
//! These cases are controlled test Tasks; no Factory/worker/model Return claim.
use super::*;
use aikit_adapters::connection_process::ModelEnvironment;
use sha2::{Digest, Sha256};

const NPM_TOKEN: &str = "controlled-native-npm-task-allocation";

fn prepare_native_codex_task(evidence: &Path) -> (World, Value) {
    prepare_native_codex_task_with_selection(evidence, false)
}

fn prepare_native_codex_task_with_selection(evidence: &Path, selected: bool) -> (World, Value) {
    let mut world = if selected {
        World::with_model_action_and_driver(
            true,
            true,
            Some(selected_native_session_space_driver(evidence)),
        )
    } else {
        World::new(true)
    };
    retain_world(&mut world, evidence);
    let authority_path = world.root.join("Control/user/native-action-authority.json");
    fs::write(&authority_path, json!({"schema":"central.native-action-authority/v1",
        "scope_ref":"control:root", "grants":[{"principal_ref":"agent:existing-1",
            "actor_kind":"agent", "token_sha256":format!("{:x}",Sha256::digest(NPM_TOKEN.as_bytes())),
            "scope_refs":["control:root"], "actions":["central.now.allocate"],
            "expires_at_unix_seconds":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+300}]}).to_string()).unwrap();
    let relation_path = world.root.join("Control/relations/source-relations.json");
    let mut relations: Value = serde_json::from_slice(&fs::read(&relation_path).unwrap()).unwrap();
    relations["relations"].as_array_mut().unwrap().push(json!({
        "ref":"central:source:control:root:Control/user/native-action-authority.json",
        "path":"Control/user/native-action-authority.json", "roles":["native-action-authority"],
        "provenance":"human-adopted", "standing":"architecture-contract",
        "treatment":"projectcentral-user", "recognition":"controlled-test-only", "recorded_at_unix_seconds":1
    }));
    fs::write(&relation_path, relations.to_string()).unwrap();
    fs::write(
        world.root.join("Work/demo/src/partial.txt"),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED",
    )
    .unwrap();
    let mut request = world.prepare_input();
    // Replace the legacy test provider before any preparation or launch. Only
    // the actual embedded Codex/npx connection is admitted by these cases.
    request["provider"] = json!({"id":"native-codex-npm-runtime",
        "label":"Actual embedded Codex ACP runtime", "protocol":"acp", "from_profile":"codex", "argv":[]});
    if selected {
        request["provider"]["model_policy"] = native_codex_selection_policy(&world);
    }
    request["selected_directories"] = json!([]);
    request["prepared_run_scope"] = Value::Null;
    request["material_host"] = Value::Null;
    let mut command = Command::new(world.native_driver());
    command
        .env("AIKIT_HOME", world.home.root())
        .env(
            "OI_ACTUATION_BIN",
            std::env::var_os("AIKIT_CAW_ACTUATION_BIN").unwrap(),
        )
        .env("CENTRAL_NATIVE_TOKEN", NPM_TOKEN)
        .arg("-C")
        .arg(&world.root)
        .args([
            "encounter-task-configure",
            "--agent-session",
            "agent-session/task",
            "--request-json",
        ])
        .arg(request.to_string());
    let output = bounded(&mut command, evidence, "actual-native-task-prepare");
    assert!(
        output.status.success(),
        "actual native preparation refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prepared: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(prepared["ready"], true);
    assert_eq!(prepared["request"], request);
    (world, prepared)
}

// Existing scripts select genuine copied native drivers through this variable.
// Only the new selected-account case uses it; old World preparations retain
// their compile-time driver and default behavior.
fn selected_native_session_space_driver(evidence: &Path) -> PathBuf {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    let path = PathBuf::from(
        std::env::var_os("AIKIT_SESSION_SPACE_BINARY")
            .expect("select the genuine qualified aikit-session-space compiler image"),
    );
    assert!(path.is_absolute() && path.canonicalize().unwrap() == path);
    let named_before = fs::symlink_metadata(&path).unwrap();
    assert!(
        named_before.is_file()
            && !named_before.file_type().is_symlink()
            && named_before.permissions().mode() & 0o111 != 0
    );
    // Same finite image limit as the genuine native material custody caller.
    let byte_limit = 629_145_600u64;
    assert!(named_before.len() > 0 && named_before.len() <= byte_limit);
    // Native open flags for the two supported acceptance machines. Refuse
    // another platform before opening; never block on a substituted FIFO.
    let flags = if cfg!(target_os = "linux") {
        0o400000 | 0o4000
    } else if cfg!(target_os = "macos") {
        0x100 | 0x4
    } else {
        panic!("held native driver admission requires Linux or macOS")
    };
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(&path)
        .unwrap();
    let metadata = file.metadata().unwrap();
    let basis = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.mode(),
            m.nlink(),
        )
    };
    assert!(metadata.is_file());
    assert_eq!(basis(&metadata), basis(&named_before));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65_536];
    let mut bytes = 0u64;
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "native driver digest deadline"
        );
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        bytes = bytes.checked_add(count as u64).unwrap();
        assert!(bytes <= metadata.len() && bytes <= byte_limit);
        digest.update(&buffer[..count]);
    }
    assert_eq!(bytes, metadata.len());
    assert_eq!(basis(&file.metadata().unwrap()), basis(&metadata));
    assert_eq!(
        basis(&fs::symlink_metadata(&path).unwrap()),
        basis(&metadata)
    );
    fs::write(evidence.join("selected-native-session-space-driver.json"),
        serde_json::to_vec_pretty(&json!({"selector":"AIKIT_SESSION_SPACE_BINARY",
            "path":path,"sha256":format!("{:x}",digest.finalize()),
            "bytes":bytes,"byteLimit":byte_limit,"bufferBytes":buffer.len(),
            "device":metadata.dev(),"inode":metadata.ino(),"heldNamedBeforeAfterEqual":true,
            "standing":"actual caller-selected compiler image; Source qualification supplied separately by caller, not inferred from pathname"})).unwrap()).unwrap();
    path
}

// This catalogue and finite policy are genuine controlled native inputs.
// Requiring the OpenAI credential route with no API binding makes the owner
// verify Codex's actual ChatGPT login and bind its selected executable. Bare
// EOF/refusal cases keep their original no-policy preparation unchanged.
fn native_codex_selection_policy(world: &World) -> Value {
    let entry = ModelCatalogueEntry {
        model: r("model:gpt-5.5"),
        name: "GPT 5.5".into(),
        description: "Actual Codex own-login ACP session creation without inference".into(),
        superseded_refs: Default::default(),
        routes: vec![DeclaredRoute {
            provider: ProviderRef::parse("provider:openai").unwrap(),
            kind: ModelRouteKind::ProviderNative,
            provider_native_ids: ["gpt-5.5".to_string()].into(),
            endpoint: None,
            credential: CredentialCondition::Required {
                hint: "Actual native Codex ChatGPT own-login; no copied or fabricated credential"
                    .into(),
            },
        }],
        source: SourceRef::parse("source/actual-codex-task-selection-test").unwrap(),
        freshness: None,
        book: None,
    };
    let catalogue = world
        .home
        .root()
        .join(aikit_store::model_catalogue::MODEL_CATALOGUE_DIR);
    fs::create_dir_all(&catalogue).unwrap();
    fs::write(
        catalogue.join("actual-codex-task.json"),
        serde_json::to_vec(&vec![&entry]).unwrap(),
    )
    .unwrap();
    let (resolved, problems) = aikit_store::model_catalogue::resolved_catalogue(&world.home);
    assert!(
        problems.is_empty(),
        "actual controlled catalogue refused: {problems:?}"
    );
    assert_eq!(resolved.get(&entry.model), Some(&entry));
    let policy = json!({
        "schema":"aikit.model-dispatch-policy/v1",
        "agent_ref":"agent:existing-1", "world_ref":"control:root",
        "authority_ref":"authority:project:delegation", "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:gpt-5.5", "provider_ref":"provider:openai",
        "native_provider":"openai", "provider_native_id":"gpt-5.5",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()+300_000,
        "credential":null,
    });
    let path = world.root.join("actual-codex-task-policy.json");
    let bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&path, &bytes).unwrap();
    json!({"source":"source/actual-codex-task-policy",
        "revision":"rev/actual-codex-task-policy-1", "path":path,
        "content_digest":format!("blake3:{}",blake3::hash(&bytes).to_hex())})
}

fn task_path(world: &World) -> PathBuf {
    world.home.state().join("encounter-tasks").join(format!(
        "{}.json",
        blake3::hash(b"agent-session/task").to_hex()
    ))
}

fn actual_npm() -> PathBuf {
    let npm = PathBuf::from(
        std::env::var_os("AIKIT_CAW_NPM_BIN")
            .expect("supply the actual installed npm executable; never a protocol double"),
    );
    assert!(npm.is_absolute() && npm.is_file());
    assert_eq!(npm.file_name().and_then(|name| name.to_str()), Some("npm"));
    npm
}

fn evidence_directory(name: &str) -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("AIKIT_CAW_NATIVE_NPM_EVIDENCE_DIR")
            .expect("an allocated absolute native evidence directory is mandatory"),
    );
    assert!(root.is_absolute() && root.is_dir());
    let path = root.join(format!(
        "{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

struct OwnedSubprocess {
    child: Child,
    reaped: bool,
    cleanup_attempted: bool,
}
impl OwnedSubprocess {
    fn stop_before(&mut self, deadline: Instant) -> Result<std::process::ExitStatus, String> {
        self.cleanup_attempted = true;
        let stop_error = self.child.kill().err();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    return Ok(status);
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    return Err(format!(
                        "owned reap deadline expired; stop error: {stop_error:?}"
                    ))
                }
                Err(error) => {
                    return Err(format!(
                        "owned reap failed: {error}; stop error: {stop_error:?}"
                    ))
                }
            }
        }
    }
}
impl Drop for OwnedSubprocess {
    fn drop(&mut self) {
        if !self.reaped && !self.cleanup_attempted {
            let _ = self.stop_before(Instant::now() + Duration::from_secs(5));
        }
    }
}

fn drain_available(
    reader: &mut std::os::unix::net::UnixStream,
    retained: &mut Vec<u8>,
    observed: &mut usize,
) -> std::io::Result<bool> {
    use std::io::Read;
    let mut bytes = [0u8; 8192];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                *observed = observed.saturating_add(count);
                let remaining = (1024 * 1024usize).saturating_sub(retained.len());
                retained.extend_from_slice(&bytes[..count.min(remaining)]);
                if *observed > 1024 * 1024 {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Exercise the actual ACP handshake. Responses originate only from the native
/// selected provider; this client supplies no prompt, MCP server or tool reply.
struct NativeAcpSession {
    cwd: PathBuf,
    pending: Vec<u8>,
    written: usize,
    parsed: usize,
    initialized: bool,
    session_id: Option<String>,
    requests: Vec<Value>,
}
impl NativeAcpSession {
    fn new(cwd: &Path) -> Self {
        let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":1,"clientCapabilities":{},
                "clientInfo":{"name":"AIKit-native-account-regression","version":env!("CARGO_PKG_VERSION")}}});
        Self {
            cwd: cwd.to_path_buf(),
            pending: format!("{request}\n").into_bytes(),
            written: 0,
            parsed: 0,
            initialized: false,
            session_id: None,
            requests: vec![request],
        }
    }
    fn advance(
        &mut self,
        stdout: &[u8],
        stdin: &mut std::os::unix::net::UnixStream,
    ) -> Result<(), String> {
        use std::io::Write;
        while let Some(end) = stdout[self.parsed..].iter().position(|byte| *byte == b'\n') {
            let end = self.parsed + end;
            let line = &stdout[self.parsed..end];
            self.parsed = end + 1;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let reply: Value = serde_json::from_slice(line)
                .map_err(|error| format!("actual ACP stdout is not a JSON frame: {error}"))?;
            if reply.get("method").is_some() {
                if reply.get("id").is_some() {
                    return Err("actual ACP requested an unadvertised client operation; no synthetic reply supplied".into());
                }
                continue;
            }
            let Some(id) = reply.get("id").and_then(Value::as_u64) else {
                return Err(
                    "actual ACP response lacks this client's numeric request identity".into(),
                );
            };
            if reply.get("error").is_some() {
                return Err(format!(
                    "actual ACP request {id} failed: {}",
                    reply["error"]
                ));
            }
            if id == 1 && !self.initialized && self.written == self.pending.len() {
                if reply["result"]["protocolVersion"] != 1 {
                    return Err("actual ACP initialization did not negotiate version 1".into());
                }
                self.initialized = true;
                let request = json!({"jsonrpc":"2.0","id":2,"method":"session/new",
                    "params":{"cwd":self.cwd,"mcpServers":[]}});
                self.pending = format!("{request}\n").into_bytes();
                self.written = 0;
                self.requests.push(request);
            } else if id == 2
                && self.initialized
                && self.written == self.pending.len()
                && self.session_id.is_none()
            {
                let session = reply["result"]["sessionId"]
                    .as_str()
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        "actual ACP session/new has no provider-originated sessionId".to_owned()
                    })?;
                self.session_id = Some(session.to_owned());
            } else {
                return Err(format!(
                    "actual ACP response {id} does not match the pending native request"
                ));
            }
        }
        while self.written < self.pending.len() {
            match stdin.write(&self.pending[self.written..]) {
                Ok(0) => return Err("actual ACP input closed before the complete request".into()),
                Ok(count) => self.written += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("actual ACP input write failed: {error}")),
            }
        }
        Ok(())
    }
}

fn bounded(command: &mut Command, evidence: &Path, label: &str) -> std::process::Output {
    bounded_with_session(command, evidence, label, None)
}

fn bounded_with_session(
    command: &mut Command,
    evidence: &Path,
    label: &str,
    acp_cwd: Option<&Path>,
) -> std::process::Output {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let (stdin_parent, stdin_child) = UnixStream::pair().unwrap();
    let (mut stdout, stdout_child) = UnixStream::pair().unwrap();
    let (mut stderr, stderr_child) = UnixStream::pair().unwrap();
    stdout.set_nonblocking(true).unwrap();
    stderr.set_nonblocking(true).unwrap();
    let argv = json!({"program":command.get_program().to_string_lossy(),
        "args":command.get_args().map(|a|a.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "stdio":if acp_cwd.is_some() { "owned Unix sockets; actual initialize then session/new, zero MCP and no prompt, then EOF" } else { "owned Unix sockets; stdin EOF without a prompt; finite nonblocking output drain" }});
    fs::write(
        evidence.join(format!("{label}-argv.json")),
        serde_json::to_vec_pretty(&argv).unwrap(),
    )
    .unwrap();
    let mut owned = OwnedSubprocess {
        child: command
            .stdin(Stdio::from(OwnedFd::from(stdin_child)))
            .stdout(Stdio::from(OwnedFd::from(stdout_child)))
            .stderr(Stdio::from(OwnedFd::from(stderr_child)))
            .spawn()
            .unwrap(),
        reaped: false,
        cleanup_attempted: false,
    };
    // Release the Command's parent-held copies of the child endpoints so an
    // actual child exit can produce EOF. This does not change spawned stdio.
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // Actual Workcell protocol boundaries require pipe/socket fd0 and fd1.
    // Dropping our input endpoint delivers EOF; no test provider/prompt exists.
    let mut stdin_parent = Some(stdin_parent);
    let mut acp = acp_cwd.map(NativeAcpSession::new);
    if acp.is_some() {
        stdin_parent
            .as_ref()
            .unwrap()
            .set_nonblocking(true)
            .unwrap();
    } else {
        drop(stdin_parent.take());
    }
    let deadline = Instant::now() + Duration::from_secs(if acp.is_some() { 90 } else { 60 });
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_observed = 0;
    let mut err_observed = 0;
    let mut out_eof = false;
    let mut err_eof = false;
    let mut status = None;
    let mut failure = None;
    loop {
        if !out_eof {
            match drain_available(&mut stdout, &mut out, &mut out_observed) {
                Ok(eof) => out_eof = eof,
                Err(error) => failure = Some(format!("stdout drain failed: {error}")),
            }
        }
        if !err_eof {
            match drain_available(&mut stderr, &mut err, &mut err_observed) {
                Ok(eof) => err_eof = eof,
                Err(error) => failure = Some(format!("stderr drain failed: {error}")),
            }
        }
        if status.is_none() {
            match owned.child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    owned.reaped = true;
                }
                Ok(None) => {}
                Err(error) => failure = Some(format!("owned process observation failed: {error}")),
            }
        }
        if out_observed > 1024 * 1024 || err_observed > 1024 * 1024 {
            failure = Some("actual subprocess output exceeded its retained 1MiB bound".into());
        }
        if let Some(protocol) = acp.as_mut() {
            if protocol.session_id.is_none() && failure.is_none() {
                if let Err(error) = protocol.advance(&out, stdin_parent.as_mut().unwrap()) {
                    failure = Some(error);
                }
                if protocol.session_id.is_some() {
                    // Native session/new replied successfully. EOF now asks
                    // the actual adapter to close its own app-server normally.
                    drop(stdin_parent.take());
                }
            }
            if status.is_some() && protocol.session_id.is_none() && failure.is_none() {
                failure = Some(
                    "actual ACP process exited without a successful native session/new response"
                        .into(),
                );
            }
        }
        if status.is_some() && out_eof && err_eof && failure.is_none() {
            break;
        }
        if Instant::now() >= deadline && failure.is_none() {
            failure = Some("actual subprocess/pipe EOF deadline expired".into());
        }
        if failure.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(stdin_parent.take());
    if let Some(protocol) = &acp {
        fs::write(evidence.join(format!("{label}-acp.json")),
            serde_json::to_vec_pretty(&json!({"initialized":protocol.initialized,
                "nativeSessionId":protocol.session_id,"requests":protocol.requests,
                "noPrompt":true,"mcpServers":[],"automaticRetry":false,
                "standing":"actual provider-originated initialization/session response; no synthetic protocol reply"})).unwrap()).unwrap();
    }
    let mut forced_stop = false;
    let mut cleanup_error = None;
    if !owned.reaped {
        forced_stop = true;
        match owned.stop_before(Instant::now() + Duration::from_secs(5)) {
            Ok(exit) => status = Some(exit),
            Err(error) => cleanup_error = Some(error),
        }
    }
    // The socket endpoints close on every path; no reader thread survives an
    // uncertain descendant retaining fd1/fd2. Only this owned child is reaped.
    drop(stdout);
    drop(stderr);
    fs::write(evidence.join(format!("{label}.stdout")), &out).unwrap();
    fs::write(evidence.join(format!("{label}.stderr")), &err).unwrap();
    fs::write(evidence.join(format!("{label}-outcome.json")), json!({"exitCode":status.as_ref().and_then(|s|s.code()),
        "ownedPid":owned.child.id(), "ownedProcessReaped":owned.reaped, "forcedStop":forced_stop,
        "failure":failure,"cleanupError":cleanup_error,"stdoutEof":out_eof,"stderrEof":err_eof,
        "stdoutObservedBytes":out_observed,"stderrObservedBytes":err_observed,
        "stdoutSha256":format!("{:x}",Sha256::digest(&out)), "stderrSha256":format!("{:x}",Sha256::digest(&err)),
        "standing":"finite actual subprocess outcome; no model/session/descendant-quiescence claim"}).to_string()).unwrap();
    assert!(failure.is_none() && cleanup_error.is_none() && owned.reaped,
        "actual failure/partial bytes retained; no success claim: {failure:?}; cleanup: {cleanup_error:?}");
    std::process::Output {
        status: status.unwrap(),
        stdout: out,
        stderr: err,
    }
}

fn retain_world(world: &mut World, evidence: &Path) {
    // Keep the actual controlled owner state on every outcome. An uncertain
    // descendant lifetime must never cause its Task directories to disappear.
    let allocated = std::mem::replace(&mut world._temp, tempfile::tempdir().unwrap());
    let retained_root = allocated.keep();
    assert_eq!(retained_root.canonicalize().unwrap(), world.root);
    fs::write(evidence.join("retained-native-owner-root.json"), json!({"root":world.root,
        "retention":"preserve on success and failure", "standing":"controlled native test Root; not personal ground or the commissioned Run"}).to_string()).unwrap();
}

fn retain_native_basis(world: &World, prepared: &Value, evidence: &Path) {
    fs::write(
        evidence.join("native-task-reading.json"),
        serde_json::to_vec_pretty(prepared).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("native-task-owner-bytes.json"),
        fs::read(task_path(world)).unwrap(),
    )
    .unwrap();
    let names = [
        "AIKIT_CAW_CTRL_BIN",
        "AIKIT_CAW_WORKCELL_BOUNDARY_BIN",
        "AIKIT_CAW_ACTUATION_BIN",
    ];
    let binaries = names
        .iter()
        .map(|name| {
            let path = PathBuf::from(std::env::var_os(name).unwrap());
            let hash = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
            json!({"selector":name, "path":path, "sha256":hash})
        })
        .collect::<Vec<_>>();
    fs::write(
        evidence.join("native-owner-provenance.json"),
        serde_json::to_vec_pretty(&binaries).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires actual native Central/Workcell/Actuation and installed npm; no model or credentials"]
fn native_workcell_scrubbed_npm_writes_cache_only_inside_actual_task_t() {
    let evidence = evidence_directory("native-npm-write-aperture");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let cache = now.join("runtime/npm-cache");
    assert!(prepared["requirements"]["writable_paths"]
        .as_array()
        .unwrap()
        .contains(&json!(now)));
    let npm = actual_npm();
    let npm_hash = format!("{:x}", Sha256::digest(fs::read(&npm).unwrap()));
    fs::write(
        evidence.join("actual-npm-provenance.json"),
        json!({"path":npm,"sha256":npm_hash}).to_string(),
    )
    .unwrap();
    let requirements = world.root.join("exact-native-npm-requirements.json");
    fs::write(&requirements, prepared["requirements"].to_string()).unwrap();
    let boundary = PathBuf::from(std::env::var_os("AIKIT_CAW_WORKCELL_BOUNDARY_BIN").unwrap());
    for (label, args) in [
        ("version", vec!["--version"]),
        ("cache-reading", vec!["config", "get", "cache"]),
        ("cache-write", vec!["cache", "verify"]),
    ] {
        let mut command = Command::new(&boundary);
        command
            .current_dir(&world.root)
            .args([
                "exec",
                requirements.to_str().unwrap(),
                prepared["requirements"]["policy_revision"]
                    .as_str()
                    .unwrap(),
                prepared["inspection"]["requirements_digest"]
                    .as_str()
                    .unwrap(),
                "--",
            ])
            .arg(&npm)
            .args(args);
        // Exercise the real scrub and then the same owner-derived cache
        // ordering. This credential-free case proves native filesystem/npm
        // behaviour; the production TaskExec join is the separate case below.
        ModelEnvironment::new().apply(&mut command);
        command.env("npm_config_cache", &cache);
        let output = bounded(&mut command, &evidence, label);
        assert!(
            output.status.success(),
            "actual npm/Workcell refused: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if label == "cache-reading" {
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                cache.to_str().unwrap()
            );
        }
    }
    let npx = PathBuf::from(
        std::env::var_os("AIKIT_CAW_NPX_BIN")
            .expect("supply actual installed npx for the public ACP --version case"),
    );
    assert!(npx.is_absolute() && npx.is_file());
    assert_eq!(npx.file_name().and_then(|name| name.to_str()), Some("npx"));
    fs::write(
        evidence.join("actual-npx-provenance.json"),
        json!({"path":npx,
        "sha256":format!("{:x}",Sha256::digest(fs::read(&npx).unwrap()))})
        .to_string(),
    )
    .unwrap();
    let mut acp_version = Command::new(&boundary);
    acp_version
        .current_dir(&world.root)
        .args([
            "exec",
            requirements.to_str().unwrap(),
            prepared["requirements"]["policy_revision"]
                .as_str()
                .unwrap(),
            prepared["inspection"]["requirements_digest"]
                .as_str()
                .unwrap(),
            "--",
        ])
        .arg(&npx)
        .args(["-y", "@agentclientprotocol/codex-acp", "--version"]);
    ModelEnvironment::new().apply(&mut acp_version);
    acp_version.env("npm_config_cache", &cache);
    let output = bounded(
        &mut acp_version,
        &evidence,
        "actual-public-codex-acp-version",
    );
    assert!(
        output.status.success(),
        "actual public ACP version probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stdout.is_empty(),
        "actual adapter version must produce its native package identity"
    );
    let reported_version = String::from_utf8(output.stdout).unwrap();
    let packages = fs::read_dir(cache.join("_npx"))
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .join("node_modules/@agentclientprotocol/codex-acp/package.json")
        })
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "fresh native Task cache must retain one actual downloaded ACP package"
    );
    let package_path = &packages[0];
    let package_bytes = fs::read(package_path).unwrap();
    let package: Value = serde_json::from_slice(&package_bytes).unwrap();
    assert_eq!(package["name"], "@agentclientprotocol/codex-acp");
    assert!(package["version"].as_str().is_some_and(|v| !v.is_empty()));
    assert_eq!(
        reported_version.trim(),
        format!(
            "{} {}",
            package["name"].as_str().unwrap(),
            package["version"].as_str().unwrap()
        ),
        "the actual executed public adapter must identify the downloaded package"
    );
    let package_root = package_path.parent().unwrap().canonicalize().unwrap();
    assert!(package_root.starts_with(cache.canonicalize().unwrap()));
    let binaries = match &package["bin"] {
        Value::String(path) => vec![path.as_str()],
        Value::Object(paths) => paths.values().map(|p| p.as_str().unwrap()).collect(),
        _ => panic!("actual downloaded ACP package has no native bin declaration"),
    };
    let entries = binaries.into_iter().map(|relative| {
        let path = package_root.join(relative).canonicalize().unwrap();
        assert!(path.starts_with(&package_root), "actual package bin must belong to its downloaded package");
        let bytes = fs::read(&path).unwrap();
        json!({"relativePath":relative,"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()})
    }).collect::<Vec<_>>();
    fs::write(
        evidence.join("actual-downloaded-acp-package.json"),
        &package_bytes,
    )
    .unwrap();
    fs::write(evidence.join("actual-public-acp-package-provenance.json"), json!({
        "name":package["name"],"version":package["version"],"packagePath":package_path,
        "packageSha256":format!("{:x}",Sha256::digest(&package_bytes)),"cachePath":cache,
        "entryPoints":entries,"standing":"actual public package version; no model prompt, native selection or worker Return"}).to_string()).unwrap();
    assert!(
        cache.join("_cacache").is_dir(),
        "actual npm cache verify must write cache bytes"
    );
    assert_eq!(cache.canonicalize().unwrap(), cache);
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(
        fs::read(world.root.join("Work/demo/src/partial.txt")).unwrap(),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED"
    );
    assert_eq!(
        fs::read(world.root.join("Control/user/human.md")).unwrap(),
        b"HUMAN_UNCHANGED"
    );
    assert!(world.child.is_none(), "no encounter resident was created");
}

#[test]
#[ignore = "requires actual native owners and installed Codex/npx connection; refuses before provider start"]
fn native_codex_task_exec_refuses_redirected_runtime_cache_without_task_write() {
    let evidence = evidence_directory("native-npm-redirected-runtime");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let outside = world.root.join("outside-native-task-cache");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, now.join("runtime")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let output = bounded(&mut command, &evidence, "actual-task-exec-refusal");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Codex runtime cache ancestors must be real canonical directories"),
        "must reach the actual owner cache guard, not another failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(
        fs::read(world.root.join("Work/demo/src/partial.txt")).unwrap(),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED"
    );
    assert!(world.child.is_none());
}

#[cfg(feature = "codex-account-native")]
#[test]
#[ignore = "requires actual Codex own-login and installed embedded ACP/npx package; no prompt"]
fn actual_codex_task_exec_eof_uses_allocated_npm_cache_and_preserves_native_task() {
    let evidence = evidence_directory("native-codex-task-eof-cache");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let codex = PathBuf::from(
        std::env::var_os("AIKIT_CAW_CODEX_BIN")
            .expect("provide the actual installed native Codex executable; no fake login"),
    );
    assert!(codex.is_absolute() && codex.is_file());
    let mut login = Command::new(&codex);
    login.args(["login", "status"]);
    let account = bounded(&mut login, &evidence, "actual-native-codex-login-status");
    assert!(
        account.status.success(),
        "genuine native own-login prerequisite unavailable"
    );
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let cache = now.join("runtime/npm-cache");
    let sqlite_home = now.join("runtime/codex-sqlite");
    let ambient_sqlite = world.root.join("ungranted-ambient-sqlite");
    fs::create_dir(&ambient_sqlite).unwrap();
    fs::write(
        ambient_sqlite.join("retained.txt"),
        b"AMBIENT_SQLITE_UNCHANGED",
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    // The native Task route must replace an ungranted ambient runtime path
    // after the model-env scrub. This is a real directory, never an owner reply.
    command.env("CODEX_SQLITE_HOME", &ambient_sqlite);
    let input_home = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .unwrap()
        .canonicalize()
        .unwrap();
    // Metadata only: no credential or actual private history bytes are copied
    // into this account gate's evidence. Original Factory session replay is a
    // separate owning activity, never inferred from this EOF case.
    use std::os::unix::fs::MetadataExt;
    let input_basis = ["auth.json", "config.toml", "installation_id"]
        .into_iter()
        .map(|name| {
            let basis = match fs::symlink_metadata(input_home.join(name)) {
                Ok(m) => Some((
                    m.dev(),
                    m.ino(),
                    m.len(),
                    m.mode(),
                    m.mtime(),
                    m.mtime_nsec(),
                )),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => panic!("actual input metadata unavailable: {e}"),
            };
            (name, basis)
        })
        .collect::<Vec<_>>();
    let output = bounded(&mut command, &evidence, "actual-native-task-exec-eof");
    assert!(
        output.status.success(),
        "actual embedded ACP/npx failed without a prompt: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        cache.join("_cacache").is_dir(),
        "production TaskExec must route actual npm writes inside T"
    );
    assert_eq!(cache.canonicalize().unwrap(), cache);
    assert_eq!(sqlite_home.canonicalize().unwrap(), sqlite_home);
    let sqlite_entries = fs::read_dir(&sqlite_home)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert!(
        sqlite_entries.iter().any(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("state_") && name.ends_with(".sqlite"))
        }),
        "actual native state DB must exist inside the admitted Task T"
    );
    assert_eq!(
        fs::read(ambient_sqlite.join("retained.txt")).unwrap(),
        b"AMBIENT_SQLITE_UNCHANGED"
    );
    assert_eq!(fs::read_dir(&ambient_sqlite).unwrap().count(), 1);
    fs::write(
        evidence.join("actual-native-sqlite-material-paths.json"),
        serde_json::to_vec_pretty(&json!({"sqlite_home":sqlite_home,
            "entries":sqlite_entries,"task_revision":prepared["revision"],
            "standing":"actual SQLite material after no-prompt Task exec; no app-server readiness/session continuity claim"}))
        .unwrap(),
    )
    .unwrap();
    let runtime_id_path =
        now.join("native-codex-runtime/files/installation_id/upper/installation_id");
    let runtime_id = fs::read_to_string(&runtime_id_path).unwrap();
    let id = runtime_id.trim();
    assert_eq!(id.len(),36,"actual native app-server must write its runtime UUID; an empty Workcell placeholder is not startup evidence");
    assert!(id
        .bytes()
        .enumerate()
        .all(|(n, b)| if [8, 13, 18, 23].contains(&n) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }));
    // This explicit second launch follows a successful first EOF outcome on
    // the SAME Task. It is not automatic replay after an uncertain failure.
    let reentered = bounded(&mut command, &evidence, "actual-native-task-reentry-eof");
    assert!(
        reentered.status.success(),
        "same Task re-entry failed: {}",
        String::from_utf8_lossy(&reentered.stderr)
    );
    assert_eq!(
        fs::read_to_string(&runtime_id_path).unwrap(),
        runtime_id,
        "same Task keeps its native runtime material, not a replacement semantic Session"
    );
    for (name, before) in input_basis {
        let after = match fs::symlink_metadata(input_home.join(name)) {
            Ok(m) => Some((
                m.dev(),
                m.ino(),
                m.len(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("actual retained input metadata unavailable: {e}"),
        };
        assert_eq!(
            after, before,
            "native original {name} input metadata changed"
        );
    }
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(
        fs::read(world.root.join("Work/demo/src/partial.txt")).unwrap(),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED"
    );
    assert!(world.child.is_none());
}

#[test]
#[ignore = "requires actual native Central/Workcell/Actuation and embedded Codex profile; refuses before provider start"]
fn native_codex_task_exec_refuses_redirected_sqlite_before_any_provider_write() {
    use std::os::unix::fs::MetadataExt;

    let evidence = evidence_directory("native-codex-sqlite-redirect");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    fs::create_dir(now.join("runtime")).unwrap();
    let outside = world.root.join("ungranted-sqlite-owner");
    fs::create_dir(&outside).unwrap();
    let retained = outside.join("retained.txt");
    fs::write(&retained, b"FOREIGN_SQLITE_UNCHANGED").unwrap();
    let identity = fs::metadata(&retained).unwrap();
    std::os::unix::fs::symlink(&outside, now.join("runtime/codex-sqlite")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let output = bounded(&mut command, &evidence, "actual-redirected-sqlite-refusal");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Codex runtime cache ancestors must be real canonical directories"));
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(fs::read(&retained).unwrap(), b"FOREIGN_SQLITE_UNCHANGED");
    let after = fs::metadata(&retained).unwrap();
    assert_eq!(
        (after.dev(), after.ino(), after.mtime(), after.mtime_nsec()),
        (
            identity.dev(),
            identity.ino(),
            identity.mtime(),
            identity.mtime_nsec()
        )
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
    assert!(!now.join("runtime/npm-cache").exists());
    assert!(fs::symlink_metadata(now.join("runtime/codex-sqlite"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
#[ignore = "requires actual native Central/Workcell/Actuation and embedded Codex profile; refuses before provider start"]
fn native_codex_task_exec_preserves_non_directory_sqlite_material_on_refusal() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let evidence = evidence_directory("native-codex-sqlite-not-directory");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    fs::create_dir(now.join("runtime")).unwrap();
    let obstruction = now.join("runtime/codex-sqlite");
    fs::write(&obstruction, b"EXISTING_SQLITE_MATERIAL_UNCHANGED").unwrap();
    fs::set_permissions(&obstruction, fs::Permissions::from_mode(0o444)).unwrap();
    let identity = fs::metadata(&obstruction).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let output = bounded(
        &mut command,
        &evidence,
        "actual-non-directory-sqlite-refusal",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Codex runtime cache ancestors must be real canonical directories"));
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(
        fs::read(&obstruction).unwrap(),
        b"EXISTING_SQLITE_MATERIAL_UNCHANGED"
    );
    let after = fs::metadata(&obstruction).unwrap();
    assert_eq!(
        (
            after.dev(),
            after.ino(),
            after.mode(),
            after.mtime(),
            after.mtime_nsec()
        ),
        (
            identity.dev(),
            identity.ino(),
            identity.mode(),
            identity.mtime(),
            identity.mtime_nsec()
        )
    );
    assert!(!now.join("runtime/npm-cache").exists());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires actual current native Task and Workcell runtime projection operation; no provider start"]
fn native_codex_task_projection_redirect_retains_foreign_material_and_task_revision() {
    use std::os::unix::fs::MetadataExt;
    let evidence = evidence_directory("native-codex-projection-redirect");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let input = world.root.join("controlled-codex-input");
    fs::create_dir(&input).unwrap();
    let foreign = world.root.join("foreign-native-runtime");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("retained"), b"FOREIGN_RUNTIME_UNCHANGED").unwrap();
    let basis = fs::metadata(foreign.join("retained")).unwrap();
    std::os::unix::fs::symlink(&foreign, now.join("native-codex-runtime")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .env("CODEX_HOME", &input)
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let output = bounded(&mut command, &evidence, "actual-projection-refusal");
    assert!(!output.status.success());
    let native: Value =
        serde_json::from_slice(&output.stderr).expect("exact current Workcell structured refusal");
    assert_eq!(native["runtime_projection"]["phase"], "material-setup");
    assert_eq!(native["runtime_projection"]["executed"], false);
    assert_eq!(
        fs::read(foreign.join("retained")).unwrap(),
        b"FOREIGN_RUNTIME_UNCHANGED"
    );
    let after = fs::metadata(foreign.join("retained")).unwrap();
    assert_eq!(
        (basis.dev(), basis.ino(), basis.mtime(), basis.mtime_nsec()),
        (after.dev(), after.ino(), after.mtime(), after.mtime_nsec())
    );
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert!(!now.join("runtime/npm-cache").exists());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires actual current native Task and Workcell input admission; no credentials/model/provider"]
fn native_codex_task_projection_auth_redirect_refuses_before_any_runtime_material() {
    let evidence = evidence_directory("native-codex-auth-input-redirect");
    let (world, prepared) = prepare_native_codex_task(&evidence);
    retain_native_basis(&world, &prepared, &evidence);
    let before = fs::read(task_path(&world)).unwrap();
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let input = world.root.join("controlled-codex-input");
    fs::create_dir(&input).unwrap();
    let foreign = world.root.join("controlled-input-not-credential");
    fs::write(&foreign, b"CONTROLLED_FOREIGN_UNCHANGED").unwrap();
    std::os::unix::fs::symlink(&foreign, input.join("auth.json")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command
        .env("AIKIT_HOME", world.home.root())
        .env("CODEX_HOME", &input)
        .current_dir(world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let output = bounded(&mut command, &evidence, "actual-auth-input-refusal");
    assert!(!output.status.success());
    let native: Value =
        serde_json::from_slice(&output.stderr).expect("exact current Workcell structured refusal");
    assert_eq!(native["runtime_projection"]["phase"], "origin-admission");
    assert_eq!(
        native["runtime_projection"]["material_setup_started"],
        false
    );
    assert_eq!(fs::read(&foreign).unwrap(), b"CONTROLLED_FOREIGN_UNCHANGED");
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert!(!now.join("native-codex-runtime").exists());
    assert!(!now.join("runtime/npm-cache").exists());
}

#[cfg(feature = "codex-account-native")]
fn selected_native_codex_program_basis(expected: &Path) -> Value {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let profile = aikit_adapters::profiles::for_slug("codex").unwrap();
    let program = profile
        .presence
        .as_ref()
        .unwrap()
        .executables
        .first()
        .unwrap();
    // This is the same production resolver used by codex_login_basis. PATH
    // and semantic HOME are inherited unchanged; the test never supplies a
    // replacement CODEX_PATH. The native Model owner delivers that binding.
    let selected = aikit_cli::probe::which(program).expect("declared native Codex is not on PATH");
    assert!(
        selected.is_absolute(),
        "selected native PATH program must be absolute"
    );
    let canonical = selected.canonicalize().unwrap();
    assert_eq!(
        canonical,
        expected.canonicalize().unwrap(),
        "the Model owner's native PATH selection differs from AIKIT_CAW_CODEX_BIN"
    );
    let named = fs::symlink_metadata(&canonical).unwrap();
    assert!(named.is_file() && !named.file_type().is_symlink());
    let physical = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    let mut held = fs::File::open(&canonical).unwrap();
    assert_eq!(physical(&held.metadata().unwrap()), physical(&named));
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut digest = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        assert!(
            Instant::now() < deadline,
            "selected native executable hash deadline"
        );
        let n = held.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        count += n as u64;
        assert!(
            count <= 536_870_912,
            "selected native executable exceeds retained hash bound"
        );
        digest.update(&buffer[..n]);
    }
    assert_eq!(count, named.len());
    assert_eq!(physical(&held.metadata().unwrap()), physical(&named));
    assert_eq!(
        physical(&fs::symlink_metadata(&canonical).unwrap()),
        physical(&named)
    );
    assert_eq!(selected.canonicalize().unwrap(), canonical);
    json!({"profile":profile.slug,"declaredExecutable":program,"selectedPath":selected,
        "canonicalPath":canonical,"device":named.dev(),"inode":named.ino(),"bytes":count,
        "mode":named.mode(),"mtime":named.mtime(),"mtimeNsec":named.mtime_nsec(),
        "ctime":named.ctime(),"ctimeNsec":named.ctime_nsec(),"sha256":format!("{:x}",digest.finalize()),
        "standing":"actual unchanged-PATH native selection and stable held/named executable; Model owner verifies login and supplies CODEX_PATH"})
}

#[cfg(feature = "codex-account-native")]
fn native_input_metadata(
    home: &Path,
) -> std::collections::BTreeMap<PathBuf, Option<(u64, u64, u64, u32, i64, i64, i64, i64)>> {
    use std::os::unix::fs::MetadataExt;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut pending = [
        "auth.json",
        ".credentials.json",
        "config.toml",
        "config.d",
        "managed_config.toml",
        "installation_id",
        "thread-writer-locks",
        "sessions",
        "archived_sessions",
        "history.jsonl",
        "session_index.jsonl",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    let mut observed = std::collections::BTreeMap::new();
    while let Some(relative) = pending.pop() {
        assert!(
            Instant::now() < deadline && observed.len() < 100_000,
            "actual original metadata census exceeded its explicit time/entry bound"
        );
        let path = home.join(&relative);
        match fs::symlink_metadata(&path) {
            Ok(m) => {
                if m.is_dir() && !m.file_type().is_symlink() {
                    for entry in fs::read_dir(&path).unwrap() {
                        assert!(
                            pending.len() + observed.len() < 100_000,
                            "actual original metadata census exceeds its entry bound"
                        );
                        pending.push(relative.join(entry.unwrap().file_name()));
                    }
                }
                observed.insert(
                    relative,
                    Some((
                        m.dev(),
                        m.ino(),
                        m.len(),
                        m.mode(),
                        m.mtime(),
                        m.mtime_nsec(),
                        m.ctime(),
                        m.ctime_nsec(),
                    )),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                observed.insert(relative, None);
            }
            Err(error) => panic!("actual original metadata cannot be inspected: {error}"),
        }
    }
    observed
}

#[cfg(feature = "codex-account-native")]
fn native_task_session_files(directory: &Path) -> Vec<PathBuf> {
    let mut pending = vec![directory.to_path_buf()];
    let mut files = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while let Some(path) = pending.pop() {
        assert!(
            Instant::now() < deadline && pending.len() + files.len() < 10_000,
            "actual new Task session material exceeded its metadata census bound"
        );
        for entry in fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(
                !metadata.file_type().is_symlink(),
                "new Task session material must not redirect outside T"
            );
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}

#[cfg(feature = "codex-account-native")]
#[test]
#[ignore = "requires actual Codex own-login and native Central/Workcell/Actuation/embedded ACP/npx; initialize and session/new only, no prompt"]
fn actual_codex_task_acp_session_creation_and_reentry_keep_thread_locks_in_task_t() {
    use std::os::unix::fs::MetadataExt;
    let evidence = evidence_directory("native-codex-task-acp-session-locks");
    let (world, prepared) = prepare_native_codex_task_with_selection(&evidence, true);
    retain_native_basis(&world, &prepared, &evidence);
    let codex = PathBuf::from(
        std::env::var_os("AIKIT_CAW_CODEX_BIN")
            .expect("provide actual installed native Codex; no fabricated login"),
    );
    assert!(codex.is_absolute() && codex.is_file());
    let input_home = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .unwrap()
        .canonicalize()
        .unwrap();
    let original = native_input_metadata(&input_home);
    let semantic_home = std::env::var_os("HOME");
    let semantic_codex_home = std::env::var_os("CODEX_HOME");
    let native_program = selected_native_codex_program_basis(&codex);
    let semantic_path = std::env::var_os("PATH");
    let mut version = Command::new(&codex);
    version.arg("--version");
    assert!(bounded(
        &mut version,
        &evidence,
        "actual-session-lock-native-version"
    )
    .status
    .success());
    let mut login = Command::new(&codex);
    login.args(["login", "status"]);
    let login = bounded(&mut login, &evidence, "actual-session-lock-login-status");
    let login_stdout = String::from_utf8_lossy(&login.stdout);
    let login_stderr = String::from_utf8_lossy(&login.stderr);
    assert!(
        login.status.success()
            && ((login_stdout.trim() == "Logged in using ChatGPT"
                && login_stderr.trim().is_empty())
                || (login_stderr.trim() == "Logged in using ChatGPT"
                    && login_stdout.trim().is_empty())),
        "actual native Codex must affirm its own ChatGPT login, not merely exit successfully"
    );
    assert_eq!(native_input_metadata(&input_home), original);
    assert_eq!(selected_native_codex_program_basis(&codex), native_program);
    let policy_source = prepared["request"]["provider"]["model_policy"].clone();
    let policy_path = PathBuf::from(policy_source["path"].as_str().unwrap());
    let policy_bytes = fs::read(&policy_path).unwrap();
    assert_eq!(
        policy_source["content_digest"],
        format!("blake3:{}", blake3::hash(&policy_bytes).to_hex())
    );
    let policy: Value = serde_json::from_slice(&policy_bytes).unwrap();
    assert_eq!(policy["model_ref"], "model:gpt-5.5");
    assert_eq!(policy["provider_ref"], "provider:openai");
    assert_eq!(policy["provider_native_id"], "gpt-5.5");
    assert!(policy["credential"].is_null());
    let catalogue_path = world
        .home
        .root()
        .join(aikit_store::model_catalogue::MODEL_CATALOGUE_DIR)
        .join("actual-codex-task.json");
    let catalogue_bytes = fs::read(&catalogue_path).unwrap();
    fs::write(evidence.join("actual-selected-native-model-inputs.json"),
        serde_json::to_vec_pretty(&json!({"policySource":policy_source,"policy":policy,
            "catalogueSHA256":format!("{:x}",Sha256::digest(&catalogue_bytes)),"nativeProgram":native_program,
            "standing":"actual controlled policy/catalogue inputs before native Task exec; no fabricated PreparedModel snapshot or inference claim"})).unwrap()).unwrap();
    let task_before = fs::read(task_path(&world)).unwrap();
    let task_record: Value = serde_json::from_slice(&task_before).unwrap();
    assert_eq!(
        task_record["request"]["provider"]["model_policy"],
        policy_source
    );
    let now = PathBuf::from(
        prepared["allocation"]["allocation"]["writable_destination"]
            .as_str()
            .unwrap(),
    );
    let runtime = now.join("native-codex-runtime");
    let locks = runtime.join("directories/thread-writer-locks/upper");
    let sessions = runtime.join("directories/sessions/upper");
    let cwd = world.root.join("Work/demo");
    let mut command = Command::new(world.native_driver());
    command
        .env("AIKIT_HOME", world.home.root())
        .current_dir(&cwd)
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            prepared["revision"].as_str().unwrap(),
        ]);
    let mut first_native_session = None;
    let mut first_coordination_inode = None;
    let mut first_session_paths = Vec::new();
    for label in ["actual-task-acp-first", "actual-task-acp-reentry"] {
        // Each second launch follows a successful, fully reaped EOF outcome.
        // It creates an actual provider session in the SAME unchanged Task,
        // rather than retrying an uncertain failure or claiming Session reuse.
        assert_eq!(std::env::var_os("HOME"), semantic_home);
        assert_eq!(std::env::var_os("CODEX_HOME"), semantic_codex_home);
        assert_eq!(std::env::var_os("PATH"), semantic_path);
        assert_eq!(selected_native_codex_program_basis(&codex), native_program);
        assert_eq!(fs::read(&policy_path).unwrap(), policy_bytes);
        assert_eq!(fs::read(&catalogue_path).unwrap(), catalogue_bytes);
        let output = bounded_with_session(&mut command, &evidence, label, Some(&cwd));
        assert!(
            output.status.success(),
            "actual native ACP session creation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let account: Value = serde_json::from_slice(
            &fs::read(evidence.join(format!("{label}-outcome.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            account["forcedStop"], false,
            "successful native session must close normally after client EOF"
        );
        assert_eq!(account["stdoutEof"], true);
        assert_eq!(account["stderrEof"], true);
        assert_eq!(account["ownedProcessReaped"], true);
        let handshake: Value =
            serde_json::from_slice(&fs::read(evidence.join(format!("{label}-acp.json"))).unwrap())
                .unwrap();
        let native_session = handshake["nativeSessionId"].as_str().unwrap().to_owned();
        assert!(handshake["initialized"] == true && !native_session.is_empty());
        assert_eq!(handshake["requests"][0]["method"], "initialize");
        assert_eq!(handshake["requests"][1]["method"], "session/new");
        assert_eq!(handshake["requests"].as_array().unwrap().len(), 2);
        assert_eq!(handshake["requests"][1]["params"]["mcpServers"], json!([]));
        assert_eq!(locks.canonicalize().unwrap(), locks);
        assert_eq!(sessions.canonicalize().unwrap(), sessions);
        assert!(locks.starts_with(&now) && sessions.starts_with(&now));
        let lock = locks.join(".coordination.lock");
        let metadata = fs::symlink_metadata(&lock).unwrap();
        assert!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "real session/new must create its native coordination lock in Task COW material"
        );
        let paths = native_task_session_files(&sessions);
        assert!(
            paths
                .iter()
                .any(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl")),
            "actual native session/new must retain its rollout material inside Task T"
        );
        if label == "actual-task-acp-first" {
            first_native_session = Some(native_session.clone());
            first_coordination_inode = Some((metadata.dev(), metadata.ino()));
            first_session_paths = paths.clone();
        } else {
            assert_ne!(Some(native_session.clone()),first_native_session,
                "explicit session/new reentry creates a new actual provider session, not a manufactured continuity id");
            assert_eq!(
                Some((metadata.dev(), metadata.ino())),
                first_coordination_inode,
                "same Task retains its existing native lock material"
            );
            assert!(
                first_session_paths.iter().all(|p| paths.contains(p)),
                "same Task reentry must retain earlier partial session material"
            );
        }
        assert_eq!(selected_native_codex_program_basis(&codex), native_program);
        assert_eq!(fs::read(&policy_path).unwrap(), policy_bytes);
        assert_eq!(fs::read(&catalogue_path).unwrap(), catalogue_bytes);
        assert_eq!(
            native_input_metadata(&input_home),
            original,
            "actual original auth/config/ambient lock/history objects changed"
        );
        assert_eq!(
            fs::read(task_path(&world)).unwrap(),
            task_before,
            "actual Task CAS or authority bytes changed during protocol execution"
        );
        fs::write(evidence.join(format!("{label}-task-cow-material.json")),
            serde_json::to_vec_pretty(&json!({"taskRevision":prepared["revision"],"taskT":now,
                "lockPath":lock,"lockDevice":metadata.dev(),"lockInode":metadata.ino(),
                "sessionPaths":paths,"nativeSessionId":native_session,
                "originalMetadataEntryCount":original.len(),"originalMetadataUnchanged":true,
                "policySource":policy_source,"nativeProgram":native_program,
                "standing":"actual native no-prompt ACP session material; controlled test Task, no Original Run or worker Return"})).unwrap()).unwrap();
    }
    assert_eq!(
        fs::read(world.root.join("Work/demo/src/partial.txt")).unwrap(),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED"
    );
    assert_eq!(
        fs::read(world.root.join("Control/user/human.md")).unwrap(),
        b"HUMAN_UNCHANGED"
    );
    assert!(world.child.is_none(), "no encounter resident was created");
}
