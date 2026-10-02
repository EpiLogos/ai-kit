//! Genuine Central Task preparation, Workcell confinement and installed npm.
//! These cases are controlled test Tasks; no Factory/worker/model Return claim.
use super::*;
use aikit_adapters::connection_process::ModelEnvironment;
use sha2::{Digest, Sha256};

const NPM_TOKEN: &str = "controlled-native-npm-task-allocation";

fn prepare_native_codex_task(evidence: &Path) -> (World, Value) {
    let mut world = World::new(true);
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
    request["selected_directories"] = json!([]);
    request["prepared_run_scope"] = Value::Null;
    request["material_host"] = Value::Null;
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
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

fn bounded(command: &mut Command, evidence: &Path, label: &str) -> std::process::Output {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let (stdin_parent, stdin_child) = UnixStream::pair().unwrap();
    let (mut stdout, stdout_child) = UnixStream::pair().unwrap();
    let (mut stderr, stderr_child) = UnixStream::pair().unwrap();
    stdout.set_nonblocking(true).unwrap();
    stderr.set_nonblocking(true).unwrap();
    let argv = json!({"program":command.get_program().to_string_lossy(),
        "args":command.get_args().map(|a|a.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "stdio":"owned Unix sockets; stdin EOF without a prompt; finite nonblocking output drain"});
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
    drop(stdin_parent);
    let deadline = Instant::now() + Duration::from_secs(60);
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
    let output = bounded(&mut acp_version, &evidence, "actual-public-codex-acp-version");
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
        format!("{} {}", package["name"].as_str().unwrap(), package["version"].as_str().unwrap()),
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
    assert_eq!(fs::read(task_path(&world)).unwrap(), before);
    assert_eq!(
        fs::read(world.root.join("Work/demo/src/partial.txt")).unwrap(),
        b"NATIVE_SOURCE_PARTIAL_UNCHANGED"
    );
    assert!(world.child.is_none());
}
