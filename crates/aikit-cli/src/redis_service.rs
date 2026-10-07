//! The native lifecycle of the local Redis that backs prepared NOW context,
//! owned by AIKit and independent of Workcell.
//!
//! AIKit connected to Redis but never started it: the process, its reference
//! configuration and its health came from Workcell's declared services
//! (`~/.workcell/services.json` `service:redis-now/personal-workcell`). An
//! installation without Workcell therefore had no Redis and no prepared
//! context. This module supplies the same lifecycle on the same core as
//! `decide_service` (recorded process identity, identity-checked stop,
//! foreign-listener refusal, rollback on upgrade) for the reference profile:
//! loopback, append-only persistence, a finite `maxmemory`, `noeviction`, and
//! at least the 8.10 series.
//!
//! No verb discovers, runs or requires a `workcell` or `factory` executable.
//! The `redis-server` binary itself is an ordinary prerequisite (named, version
//! checked and hashed), exactly as `uv` is for Kev. Nothing here ever flushes
//! or deletes data; `stop` keeps the data directory and `upgrade` reuses it.

use crate::cli::{
    NowServiceCommon, NowServiceProvisionArgs, NowServiceRestartArgs, NowServiceStartArgs,
    NowServiceStatusArgs, NowServiceStopArgs, NowServiceUpgradeArgs,
};
use crate::decide_service::{
    io_fail, log_tail, now_ms, process_standing, ps_field, sha256_file, signal, wait_until,
    write_atomic, ProcessRecord, Standing,
};
use crate::jev_now::fail;
use aikit_core::{AikitError, Result};
use aikit_store::{
    AikitHome, ContextLock, LockOptions, RedisNowConfig, RedisNowStore, NOW_REDIS_CONFIG_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const SERVICE_SCHEMA: &str = "aikit.redis-service/v1";
pub const STATUS_SCHEMA: &str = "aikit.redis-service-status/v1";
const OWNER: &str = "aikit";
/// The reference series the Redis NOW profile is written against.
pub const MINIMUM_SERIES: (u32, u32) = (8, 10);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Executable {
    path: String,
    version: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedisState {
    schema: String,
    owner: String,
    port: u16,
    maxmemory_bytes: u64,
    key_prefix: String,
    executable: Executable,
    #[serde(default)]
    process: Option<ProcessRecord>,
    /// The executable a failed upgrade returns to.
    #[serde(default)]
    previous: Option<Executable>,
}

struct Layout {
    dir: PathBuf,
}

impl Layout {
    fn resolve(common: &NowServiceCommon) -> Result<Self> {
        let dir = match &common.service_dir {
            Some(dir) => dir.clone(),
            None => AikitHome::discover()?
                .root()
                .join("services")
                .join("redis-now"),
        };
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir()
                .map_err(|e| fail("redis_service.io", format!("working directory: {e}")))?
                .join(dir)
        };
        Ok(Self { dir })
    }
    fn state(&self) -> PathBuf {
        self.dir.join("redis-service.json")
    }
    fn lock(&self) -> PathBuf {
        self.dir.join("service.lock")
    }
    fn log(&self) -> PathBuf {
        self.dir.join("redis-service.log")
    }
    fn conf(&self) -> PathBuf {
        self.dir.join("redis.conf")
    }
    fn data(&self) -> PathBuf {
        self.dir.join("data")
    }
    fn election(&self) -> PathBuf {
        self.dir.join("redis-now.json")
    }
}

fn lock(layout: &Layout, purpose: &str) -> Result<ContextLock> {
    ContextLock::acquire_at(
        &layout.lock(),
        "redis-service",
        LockOptions::default()
            .with_timeout(Duration::from_secs(30))
            .with_purpose(purpose),
    )
}

fn load_state(layout: &Layout) -> Result<Option<RedisState>> {
    let path = layout.state();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_fail("read", &path, e)),
    };
    let state: RedisState = serde_json::from_slice(&bytes).map_err(|e| {
        fail(
            "redis_service.state_invalid",
            format!("{}: {e}", path.display()),
        )
    })?;
    if state.schema != SERVICE_SCHEMA || state.owner != OWNER {
        return Err(fail(
            "redis_service.state_invalid",
            format!(
                "{} is not an AIKit-owned Redis service state",
                path.display()
            ),
        ));
    }
    Ok(Some(state))
}

fn save_state(layout: &Layout, state: &RedisState) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| fail("redis_service.encode", e.to_string()))?;
    write_atomic(&layout.state(), &bytes)
}

fn require_state(layout: &Layout) -> Result<RedisState> {
    load_state(layout)?.ok_or_else(|| {
        fail(
            "redis_service.not_provisioned",
            format!(
                "no Redis service is provisioned at {}; run `aikit now-context service provision`",
                layout.dir.display()
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// The executable: an ordinary prerequisite, named, version-checked and hashed.
// ---------------------------------------------------------------------------

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// `redis-server --version` prints `Redis server v=8.10.2 sha=…`.
fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace()
        .find_map(|token| token.strip_prefix("v="))
        .map(str::to_owned)
}

fn series(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn inspect_executable(requested: Option<&Path>) -> Result<(PathBuf, Executable)> {
    let path = match requested {
        Some(path) => {
            if !path.is_absolute() {
                return Err(fail(
                    "redis_service.executable_invalid",
                    "--redis-server must be an absolute path",
                ));
            }
            path.to_path_buf()
        }
        None => find_on_path("redis-server").ok_or_else(|| {
            fail(
                "redis_service.executable_missing",
                "no `redis-server` on PATH; install Redis ≥ 8.10 or pass --redis-server <absolute path>",
            )
        })?,
    };
    if !path.is_file() {
        return Err(fail(
            "redis_service.executable_invalid",
            format!("{} is not a file", path.display()),
        ));
    }
    let output = Command::new(&path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            fail(
                "redis_service.executable_invalid",
                format!("{} could not run: {e}", path.display()),
            )
        })?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let version = parse_version(&text).ok_or_else(|| {
        fail(
            "redis_service.executable_invalid",
            format!("{} did not report a version: {text:?}", path.display()),
        )
    })?;
    match series(&version) {
        Some(found) if found >= MINIMUM_SERIES => {}
        _ => {
            return Err(fail(
                "redis_service.version_unsupported",
                format!(
                    "{} is Redis {version}; the reference profile needs {}.{} or newer",
                    path.display(),
                    MINIMUM_SERIES.0,
                    MINIMUM_SERIES.1
                ),
            ))
        }
    }
    let (sha256, _) = sha256_file(&path)?;
    Ok((
        path.clone(),
        Executable {
            path: path.display().to_string(),
            version,
            sha256,
        },
    ))
}

// ---------------------------------------------------------------------------
// Configuration: the reference profile, generated and never hand-edited.
// ---------------------------------------------------------------------------

fn redis_conf(layout: &Layout, port: u16, maxmemory_bytes: u64) -> String {
    format!(
        "# Generated by `aikit now-context service provision`; edit by re-provisioning.\n\
         # Reference profile: loopback, append-only persistence, finite maxmemory, noeviction.\n\
         bind 127.0.0.1\n\
         port {port}\n\
         protected-mode yes\n\
         daemonize no\n\
         logfile \"\"\n\
         dir \"{}\"\n\
         appendonly yes\n\
         appendfsync everysec\n\
         save 3600 1 300 100 60 10000\n\
         maxmemory {maxmemory_bytes}\n\
         maxmemory-policy noeviction\n",
        layout.data().display()
    )
}

fn election_value(state: &RedisState, timeouts_ms: (u64, u64)) -> Value {
    json!({
        "schema": NOW_REDIS_CONFIG_SCHEMA,
        "address": format!("127.0.0.1:{}", state.port),
        "database": 0,
        "key_prefix": state.key_prefix,
        "username": null,
        "credential_ref": null,
        "allow_remote": false,
        "connect_timeout_ms": timeouts_ms.0,
        "io_timeout_ms": timeouts_ms.1,
        "prepared_ttl_seconds": 21600,
        "coordination_retention_seconds": 1209600
    })
}

fn store(state: &RedisState, connect_ms: u64) -> Result<RedisNowStore> {
    let config: RedisNowConfig = serde_json::from_value(election_value(state, (connect_ms, 1000)))
        .map_err(|e| fail("redis_service.config", e.to_string()))?;
    config.validate()?;
    RedisNowStore::new(config)
}

fn write_material(layout: &Layout, state: &RedisState) -> Result<()> {
    std::fs::create_dir_all(layout.data()).map_err(|e| io_fail("create", &layout.data(), e))?;
    write_atomic(
        &layout.conf(),
        redis_conf(layout, state.port, state.maxmemory_bytes).as_bytes(),
    )?;
    let election = serde_json::to_vec_pretty(&election_value(state, (1000, 1000)))
        .map_err(|e| fail("redis_service.encode", e.to_string()))?;
    write_atomic(&layout.election(), &election)?;
    save_state(layout, state)
}

// ---------------------------------------------------------------------------
// Lifecycle.
// ---------------------------------------------------------------------------

fn needles(state: &RedisState) -> Vec<String> {
    // Redis rewrites its process title to `redis-server 127.0.0.1:<port>`.
    vec!["redis-server".into(), format!("127.0.0.1:{}", state.port)]
}

fn ours(state: &RedisState) -> bool {
    state
        .process
        .as_ref()
        .is_some_and(|record| process_standing(record, &needles(state)) == Standing::Ours)
}

fn health(state: &RedisState) -> Value {
    let store = match store(state, 800) {
        Ok(store) => store,
        Err(e) => return json!({"reachable": false, "reason": e.message()}),
    };
    match store.status(None) {
        Err(e) => json!({"reachable": false, "reason": e.message()}),
        Ok(status) => match store.profile_reading(None) {
            Ok(reading) => {
                let violations = reading.violations(MINIMUM_SERIES);
                json!({
                    "reachable": true,
                    "redis_version": status.redis_version,
                    "profile": reading,
                    "profile_conforms": violations.is_empty(),
                    "violations": violations,
                })
            }
            Err(e) => json!({"reachable": true, "reason": e.message()}),
        },
    }
}

fn reachable(state: &RedisState) -> bool {
    store(state, 300)
        .and_then(|s| s.status(None))
        .map(|status| status.available)
        .unwrap_or(false)
}

fn verify_executable(state: &RedisState) -> Result<()> {
    let path = Path::new(&state.executable.path);
    let (sha, _) = sha256_file(path).map_err(|_| {
        fail(
            "redis_service.material_changed",
            format!("{} is missing; provision or upgrade again", path.display()),
        )
    })?;
    if sha != state.executable.sha256 {
        return Err(fail(
            "redis_service.material_changed",
            format!(
                "{} differs from the executable provisioned; use `upgrade` to adopt a new one",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn start_locked(layout: &Layout, state: &mut RedisState, ready_timeout: Duration) -> Result<Value> {
    verify_executable(state)?;
    if !layout.conf().exists() {
        return Err(fail(
            "redis_service.not_provisioned",
            "the generated redis.conf is missing; provision again",
        ));
    }
    if let Some(record) = state.process.clone() {
        if process_standing(&record, &needles(state)) == Standing::Ours {
            let h = health(state);
            return Ok(json!({
                "schema": STATUS_SCHEMA, "operation": "start", "outcome": "already-running",
                "owner": OWNER, "state": "running", "pid": record.pid,
                "healthy": h["profile_conforms"] == json!(true), "health": h,
                "election_file": layout.election(), "workcell": "not involved",
            }));
        }
        state.process = None;
        save_state(layout, state)?;
    }
    // A listener we did not start is a fact to report, never to adopt.
    if reachable(state) {
        return Err(fail(
            "redis_service.port_occupied",
            format!(
                "127.0.0.1:{} already answers as Redis and is not a process this service started; \
                 elect it with its own aikit.redis-now-config/v1 or provision this service on another --port",
                state.port
            ),
        )
        .with("port", state.port.to_string()));
    }
    let log_path = layout.log();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| io_fail("open", &log_path, e))?;
    let log_err = log
        .try_clone()
        .map_err(|e| io_fail("duplicate", &log_path, e))?;
    let argv = vec![
        state.executable.path.clone(),
        layout.conf().display().to_string(),
    ];
    let mut command = Command::new(&argv[0]);
    command
        .arg(&argv[1])
        .current_dir(&layout.dir)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("WORKCELL_") {
            command.env_remove(name);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        fail(
            "redis_service.spawn_failed",
            format!("could not start {}: {e}", argv[0]),
        )
    })?;
    let pid = child.id();
    state.process = Some(ProcessRecord {
        pid,
        started_at: ps_field(pid, "lstart=").unwrap_or_default(),
        argv,
        started_unix_ms: now_ms(),
    });
    save_state(layout, state)?;

    let started = Instant::now();
    let mut exited = None;
    let mut last = String::from("Redis has not answered");
    let answered = wait_until(started + ready_timeout, Duration::from_millis(100), || {
        if let Ok(Some(status)) = child.try_wait() {
            exited = Some(status);
            return true;
        }
        match store(state, 300).and_then(|s| s.status(None)) {
            Ok(_) => true,
            Err(e) => {
                last = e.message().to_string();
                false
            }
        }
    });
    if let Some(status) = exited {
        state.process = None;
        save_state(layout, state)?;
        return Err(fail(
            "redis_service.exited",
            format!(
                "redis-server exited during startup ({status}); log tail:\n{}",
                log_tail(&log_path, 20)
            ),
        ));
    }
    if !answered {
        return Err(fail(
            "redis_service.not_ready",
            format!(
                "Redis did not answer within {} s (pid {pid} stays recorded for `stop`); last probe: {last}; log tail:\n{}",
                ready_timeout.as_secs(),
                log_tail(&log_path, 20)
            ),
        ));
    }
    let h = health(state);
    if h["profile_conforms"] != json!(true) {
        return Err(fail(
            "redis_service.profile_violation",
            format!(
                "Redis answered but does not conform to the reference profile: {} (pid {pid} stays recorded for `stop`)",
                h["violations"]
            ),
        ));
    }
    drop(child);
    Ok(json!({
        "schema": STATUS_SCHEMA, "operation": "start", "outcome": "started",
        "owner": OWNER, "state": "running", "pid": pid,
        "address": format!("127.0.0.1:{}", state.port),
        "ready_ms": started.elapsed().as_millis() as u64,
        "health": h, "log": log_path,
        "election_file": layout.election(), "workcell": "not involved",
    }))
}

fn stop_locked(layout: &Layout, state: &mut RedisState, grace: Duration) -> Result<Value> {
    let Some(record) = state.process.clone() else {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "not-running",
            "owner": OWNER, "state": "stopped", "workcell": "not involved",
        }));
    };
    let needles = needles(state);
    match process_standing(&record, &needles) {
        Standing::Gone => {
            state.process = None;
            save_state(layout, state)?;
            Ok(json!({
                "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "already-stopped",
                "owner": OWNER, "state": "stopped", "pid": record.pid, "workcell": "not involved",
            }))
        }
        Standing::Reused => Err(fail(
            "redis_service.identity_changed",
            format!(
                "pid {} is alive but is not the process this service started; refusing to signal an unrelated process",
                record.pid
            ),
        )),
        Standing::Ours => {
            // TERM lets Redis flush and fsync its append-only file.
            signal(record.pid, "-TERM");
            let gone = wait_until(Instant::now() + grace, Duration::from_millis(100), || {
                process_standing(&record, &needles) != Standing::Ours
            });
            let mut escalated = false;
            if !gone {
                escalated = true;
                signal(record.pid, "-KILL");
                if !wait_until(
                    Instant::now() + Duration::from_secs(5),
                    Duration::from_millis(100),
                    || process_standing(&record, &needles) != Standing::Ours,
                ) {
                    return Err(fail(
                        "redis_service.stop_failed",
                        format!("pid {} survived SIGKILL", record.pid),
                    ));
                }
            }
            state.process = None;
            save_state(layout, state)?;
            Ok(json!({
                "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "stopped",
                "owner": OWNER, "state": "stopped", "pid": record.pid,
                "escalated_to_kill": escalated, "data_kept": layout.data(),
                "workcell": "not involved",
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Verbs.
// ---------------------------------------------------------------------------

pub fn service_provision(args: NowServiceProvisionArgs) -> Result<Value> {
    if args.port < 1024 {
        return Err(fail(
            "redis_service.port_invalid",
            "a local Redis binds an unprivileged loopback port (1024-65535)",
        ));
    }
    if !(16..=1_048_576).contains(&args.maxmemory_mb) {
        return Err(fail(
            "redis_service.maxmemory_invalid",
            "maxmemory must be finite: 16 MiB to 1 TiB",
        ));
    }
    if args.key_prefix.is_empty()
        || args.key_prefix.len() > 128
        || !args
            .key_prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(fail(
            "redis_service.key_prefix_invalid",
            "key prefix must be a bounded plain token",
        ));
    }
    let layout = Layout::resolve(&args.common)?;
    let (_, executable) = inspect_executable(args.redis_server.as_deref())?;
    std::fs::create_dir_all(&layout.dir).map_err(|e| io_fail("create", &layout.dir, e))?;
    let _lock = lock(&layout, "provision")?;
    let existing = load_state(&layout)?;
    if existing.as_ref().is_some_and(ours) {
        return Err(fail(
            "redis_service.running",
            "the service is running; stop it, or use `upgrade`, before provisioning again",
        ));
    }
    let state = RedisState {
        schema: SERVICE_SCHEMA.into(),
        owner: OWNER.into(),
        port: args.port,
        maxmemory_bytes: args.maxmemory_mb * 1024 * 1024,
        key_prefix: args.key_prefix,
        executable,
        process: None,
        previous: existing.and_then(|e| e.previous),
    };
    write_material(&layout, &state)?;
    Ok(json!({
        "schema": STATUS_SCHEMA, "operation": "provision", "owner": OWNER,
        "state": "stopped", "service_dir": layout.dir,
        "executable": state.executable,
        "profile": {"bind": "127.0.0.1", "appendonly": "yes",
                    "maxmemory_bytes": state.maxmemory_bytes, "maxmemory_policy": "noeviction"},
        "conf": layout.conf(), "election_file": layout.election(),
        "workcell": "not involved", "next": "aikit now-context service start",
    }))
}

pub fn service_start(args: NowServiceStartArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "start")?;
    let mut state = require_state(&layout)?;
    start_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.ready_timeout_secs.max(1)),
    )
}

pub fn service_stop(args: NowServiceStopArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "stop")?;
    let mut state = require_state(&layout)?;
    stop_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.grace_secs.max(1)),
    )
}

pub fn service_restart(args: NowServiceRestartArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "restart")?;
    let mut state = require_state(&layout)?;
    let stopped = stop_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.grace_secs.max(1)),
    )?;
    let started = start_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.ready_timeout_secs.max(1)),
    )?;
    Ok(json!({
        "schema": STATUS_SCHEMA, "operation": "restart", "owner": OWNER,
        "stop": stopped, "start": started, "workcell": "not involved",
    }))
}

pub fn service_status(args: NowServiceStatusArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let Some(state) = load_state(&layout)? else {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "status", "owner": OWNER,
            "state": "not-provisioned", "service_dir": layout.dir,
            "next": "aikit now-context service provision", "workcell": "not involved",
        }));
    };
    let h = health(&state);
    let is_ours = ours(&state);
    let up = h["reachable"] == json!(true);
    let process = match &state.process {
        None => json!({"recorded": false}),
        Some(record) => json!({
            "recorded": true, "pid": record.pid,
            "standing": match process_standing(record, &needles(&state)) {
                Standing::Ours => "ours", Standing::Gone => "gone", Standing::Reused => "pid-reused",
            },
        }),
    };
    let conforms = h["profile_conforms"] == json!(true);
    Ok(json!({
        "schema": STATUS_SCHEMA, "operation": "status", "owner": OWNER,
        "lifecycle": "aikit now-context service",
        "service_dir": layout.dir,
        "address": format!("127.0.0.1:{}", state.port),
        "election_file": layout.election(),
        "executable": state.executable,
        "process": process, "health": h,
        "state": match (is_ours, up, conforms) {
            (true, true, true) => "running",
            (true, _, _) => "running-unhealthy",
            (false, true, _) => "foreign-listener",
            (false, false, _) => "stopped",
        },
        "workcell": "not involved",
    }))
}

pub fn service_upgrade(args: NowServiceUpgradeArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "upgrade")?;
    let mut state = require_state(&layout)?;
    // Refuse an unusable target before touching the running service.
    let (_, target) = inspect_executable(Some(&args.redis_server))?;
    if target == state.executable {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "upgrade", "outcome": "current",
            "owner": OWNER, "executable": target, "workcell": "not involved",
        }));
    }
    let was_running = ours(&state);
    let previous = state.executable.clone();
    let grace = Duration::from_secs(args.grace_secs.max(1));
    let ready = Duration::from_secs(args.ready_timeout_secs.max(1));
    let stopped = stop_locked(&layout, &mut state, grace)?;
    state.executable = target.clone();
    state.previous = Some(previous.clone());
    let attempt = save_state(&layout, &state).and_then(|_| {
        if was_running {
            start_locked(&layout, &mut state, ready).map(Some)
        } else {
            Ok(None)
        }
    });
    match attempt {
        Ok(started) => Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "upgrade", "outcome": "upgraded",
            "owner": OWNER, "stop": stopped, "start": started,
            "from": previous, "to": target, "data_kept": layout.data(),
            "workcell": "not involved",
        })),
        Err(failure) => {
            let _ = stop_locked(&layout, &mut state, grace);
            state.executable = previous.clone();
            state.previous = None;
            let rollback = save_state(&layout, &state).and_then(|_| {
                if was_running {
                    start_locked(&layout, &mut state, ready).map(|_| ())
                } else {
                    Ok(())
                }
            });
            let note = match rollback {
                Ok(()) => format!(
                    "rolled back to {} {} and {}",
                    previous.path,
                    previous.version,
                    if was_running {
                        "restarted conforming"
                    } else {
                        "left stopped as before"
                    }
                ),
                Err(e) => format!("ROLLBACK ALSO FAILED ({}): {}", e.code(), e.message()),
            };
            Err(AikitError::new(
                "redis_service.upgrade_failed",
                format!(
                    "upgrade failed ({}): {}; {note}",
                    failure.code(),
                    failure.message()
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_profile_is_what_the_generated_configuration_says() {
        let layout = Layout {
            dir: PathBuf::from("/tmp/redis-svc"),
        };
        let conf = redis_conf(&layout, 6390, 268_435_456);
        for line in [
            "bind 127.0.0.1",
            "port 6390",
            "appendonly yes",
            "maxmemory 268435456",
            "maxmemory-policy noeviction",
            "daemonize no",
        ] {
            assert!(conf.lines().any(|l| l == line), "missing {line}\n{conf}");
        }
        assert!(!conf.contains("flush"), "nothing here ever flushes data");
    }

    #[test]
    fn versions_parse_and_old_series_are_refused() {
        assert_eq!(
            parse_version("Redis server v=8.10.2 sha=00000000:1 malloc=libc"),
            Some("8.10.2".into())
        );
        assert_eq!(series("8.10.2"), Some((8, 10)));
        assert!(series("7.4.1").unwrap() < MINIMUM_SERIES);
        assert!(series("8.9.9").unwrap() < MINIMUM_SERIES);
        assert!(series("9.0.0").unwrap() >= MINIMUM_SERIES);
    }

    #[test]
    fn a_running_redis_that_departs_from_the_profile_is_named_not_accepted() {
        use aikit_store::RedisProfileReading;
        let good = RedisProfileReading {
            redis_version: Some("8.10.2".into()),
            aof_enabled: true,
            maxmemory: 1 << 28,
            maxmemory_policy: "noeviction".into(),
        };
        assert!(good.violations(MINIMUM_SERIES).is_empty());
        let bad = RedisProfileReading {
            redis_version: Some("8.9.0".into()),
            aof_enabled: false,
            maxmemory: 0,
            maxmemory_policy: "allkeys-lru".into(),
        };
        assert_eq!(bad.violations(MINIMUM_SERIES).len(), 4);
    }
}
