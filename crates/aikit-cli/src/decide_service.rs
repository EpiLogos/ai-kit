//! The native lifecycle of a locally served decision model (Kev), owned by
//! AIKit and independent of Workcell.
//!
//! `decide.rs` elects a placement and speaks to it; it never started, stopped
//! or replaced a serving process. Until now that lifecycle existed only as
//! shell material (`scripts/decision-local/*.sh`) declared through Workcell's
//! declared-services path, so an installation without Workcell could only
//! point `endpoint` mode at a server somebody else already ran.
//!
//! This module supplies the same lifecycle at AIKit's own boundary, and labels
//! it honestly: the generated provider election is mode `endpoint` (a loopback
//! SystemOne-compatible service), the lifecycle owner is reported as `aikit`,
//! and no `workcell` executable is discovered, run or required by any verb.
//!
//! ```text
//! provision  pinned upstream checkout + environment + pinned artifacts,
//!            hashed into a material manifest, provider election written
//! start      adopt our own live process, or start one detached; ready means
//!            the pinned model card answers and one warm decision completed
//! status     process identity + live model card (+ optional material verify)
//! stop       identity-checked TERM then KILL; never an unrelated process
//! restart    stop then start
//! upgrade    apply a different pinned recipe; roll back to the previous one
//!            if the new cut cannot be provisioned or does not come up healthy
//! ```
//!
//! A service this module did not start (for example Workcell's own Kev on the
//! same port) is never adopted, stopped or restarted: ownership is the
//! recorded process identity (pid + start time + command line), not the port.

use crate::cli::{
    DecideServiceCommon, DecideServiceProvisionArgs, DecideServiceRestartArgs,
    DecideServiceStartArgs, DecideServiceStatusArgs, DecideServiceStopArgs,
    DecideServiceUpgradeArgs,
};
use crate::decide::{
    invoke_selected, DecisionModelIdentity, DecisionProviderConfig, DecisionProviderMode,
};
use crate::jev_now::{fail, minted_invocation_ref};
use aikit_adapters::decision_endpoint::{probe_models, DecisionEndpoint};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::jev::{DecisionLimits, JevRequest};
use aikit_core::{AikitError, Result};
use aikit_store::{AikitHome, ContextLock, LockOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const RECIPE_SCHEMA: &str = "aikit.decision-service-recipe/v1";
pub const SERVICE_SCHEMA: &str = "aikit.decision-service/v1";
pub const STATUS_SCHEMA: &str = "aikit.decision-service-status/v1";
pub const MANIFEST_SCHEMA: &str = "aikit.decision-material-manifest/v1";
const OWNER: &str = "aikit";
const DEFAULT_RECIPE: &str = "kev-0.8b";
const ADAPTER_FILES: [&str; 6] = [
    "adapter_config.json",
    "adapter_model.safetensors",
    "head.pt",
    "provenance.json",
    "tokenizer.json",
    "tokenizer_config.json",
];
/// Fetches both pinned artifacts and says where they landed. The revision is
/// the HF snapshot directory name, which the caller verifies against the pin.
const SNAPSHOT_SCRIPT: &str = "import json, sys\n\
from huggingface_hub import snapshot_download\n\
adapter_repo, adapter_rev, base_repo, base_rev = sys.argv[1:5]\n\
adapter = snapshot_download(adapter_repo, revision=adapter_rev)\n\
base = snapshot_download(base_repo, revision=base_rev)\n\
print(json.dumps({'adapter_dir': adapter, 'base_dir': base}))\n";

// ---------------------------------------------------------------------------
// The recipe: every pin that makes this the same service tomorrow.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRecipe {
    pub schema: String,
    pub recipe: String,
    pub family: String,
    pub upstream_repository: String,
    pub upstream_pin: String,
    pub adapter_repo: String,
    pub adapter_revision: String,
    pub base_repo: String,
    pub base_revision: String,
    pub python: String,
    pub extra: String,
    pub serve_module: String,
    /// The `--run` identity the server is started with.
    pub run: String,
    /// The model name the elected decision limits select.
    pub model: String,
    pub env: BTreeMap<String, String>,
    pub license: String,
    pub trained_state_tokens: u64,
    pub limits: DecisionLimits,
}

impl ServiceRecipe {
    /// The pinned Kev-0.8B cut. Upstream, base and adapter match the recipe
    /// the Workcell-declared scripts installed; the adapter revision is pinned
    /// here, where the scripts only named the repository.
    pub fn kev_0_8b() -> Self {
        Self {
            schema: RECIPE_SCHEMA.into(),
            recipe: DEFAULT_RECIPE.into(),
            family: "kev".into(),
            upstream_repository: "https://github.com/jaredpalmer/kev.git".into(),
            upstream_pin: "5920c5fe4ca8e0970ed4209ac2c9b8e18bea5109".into(),
            adapter_repo: "jaredpalmer/kev-0.8b".into(),
            adapter_revision: "9a45d25eb2ab761841196625383fa1dff0e56c1e".into(),
            base_repo: "Qwen/Qwen3.5-0.8B-Base".into(),
            base_revision: "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68".into(),
            python: "3.13".into(),
            extra: "serve".into(),
            serve_module: "kev.serve".into(),
            run: "jaredpalmer/kev-0.8b".into(),
            model: "kev-latest".into(),
            env: BTreeMap::from([
                ("HF_HUB_OFFLINE".into(), "1".into()),
                ("TRANSFORMERS_OFFLINE".into(), "1".into()),
                ("KEV_PREFIX_CACHE".into(), "8".into()),
            ]),
            license: "Apache-2.0".into(),
            trained_state_tokens: 7552,
            limits: DecisionLimits {
                timeout_ms: 120_000,
                max_attempts: 2,
                max_input_tokens_per_attempt: 16_384,
                max_output_tokens_per_attempt: 8_000,
                model: "kev-latest".into(),
            },
        }
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |what: &str| {
            fail(
                "decision_service.recipe_invalid",
                format!("service recipe: {what}"),
            )
        };
        if self.schema != RECIPE_SCHEMA {
            return Err(bad("unsupported schema"));
        }
        let hex40 = |s: &str| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
        for (name, value) in [
            ("upstream_pin", &self.upstream_pin),
            ("adapter_revision", &self.adapter_revision),
            ("base_revision", &self.base_revision),
        ] {
            if !hex40(value) {
                return Err(bad(&format!(
                    "{name} must be a full 40-hex revision; a branch or tag is not a pin"
                )));
            }
        }
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 200
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        };
        for (name, value) in [
            ("recipe", &self.recipe),
            ("family", &self.family),
            ("adapter_repo", &self.adapter_repo),
            ("base_repo", &self.base_repo),
            ("python", &self.python),
            ("extra", &self.extra),
            ("serve_module", &self.serve_module),
            ("run", &self.run),
            ("model", &self.model),
        ] {
            if !token(value) {
                return Err(bad(&format!("{name} must be a bounded plain token")));
            }
        }
        if !self.upstream_repository.starts_with("https://")
            || self.upstream_repository.contains(char::is_whitespace)
        {
            return Err(bad("upstream_repository must be an https URL"));
        }
        if self.limits.model != self.model {
            return Err(bad("limits.model must be the recipe's served model"));
        }
        for key in self.env.keys() {
            let lawful = !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                && !["AIKIT_", "CENTRAL_", "WORKCELL_", "LD_", "DYLD_"]
                    .iter()
                    .any(|prefix| key.starts_with(prefix));
            if !lawful {
                return Err(bad(&format!(
                    "env key {key} is not lawful for a service process"
                )));
            }
        }
        Ok(())
    }

    fn identity(&self, install_path: &Path, weights_bytes: Option<u64>) -> DecisionModelIdentity {
        DecisionModelIdentity {
            family: self.family.clone(),
            artifact: self.adapter_repo.clone(),
            base: self.base_repo.clone(),
            base_revision: self.base_revision.clone(),
            runtime: format!(
                "{} (uv, python {}, extra {}; upstream {})",
                self.serve_module, self.python, self.extra, self.upstream_pin
            ),
            backend: "auto (mlx on Apple Silicon; the live model card reports the actual backend)"
                .into(),
            precision: "as stored by the pinned artifacts".into(),
            calibration: "checkpoint-fitted temperature (reported live by GET /v1/models)".into(),
            license: self.license.clone(),
            weights_bytes,
            install_path: Some(install_path.display().to_string()),
            trained_state_tokens: Some(self.trained_state_tokens),
            notes: Some(format!(
                "served by AIKit's native local service lifecycle (`aikit decide service`); adapter revision {}",
                self.adapter_revision
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// State on disk.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provisioned {
    upstream_head: String,
    adapter_dir: String,
    base_dir: String,
    manifest_sha256: String,
    weights_bytes: u64,
    provisioned_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessRecord {
    pub(crate) pid: u32,
    /// `ps -o lstart=` at spawn: a reused pid cannot reproduce it.
    pub(crate) started_at: String,
    pub(crate) argv: Vec<String>,
    pub(crate) started_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceState {
    schema: String,
    owner: String,
    recipe: ServiceRecipe,
    port: u16,
    #[serde(default)]
    provisioned: Option<Provisioned>,
    #[serde(default)]
    process: Option<ProcessRecord>,
    /// The cut a failed upgrade returns to, kept until the next one succeeds.
    #[serde(default)]
    previous: Option<Box<PreviousCut>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousCut {
    recipe: ServiceRecipe,
    provisioned: Provisioned,
}

struct Layout {
    dir: PathBuf,
}

impl Layout {
    fn resolve(common: &DecideServiceCommon) -> Result<Self> {
        let dir = match &common.service_dir {
            Some(dir) => dir.clone(),
            None => AikitHome::discover()?
                .root()
                .join("services")
                .join("decision")
                .join(DEFAULT_RECIPE),
        };
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir()
                .map_err(|e| fail("decision_service.io", format!("working directory: {e}")))?
                .join(dir)
        };
        Ok(Self { dir })
    }
    fn state(&self) -> PathBuf {
        self.dir.join("service.json")
    }
    fn lock(&self) -> PathBuf {
        self.dir.join("service.lock")
    }
    fn log(&self) -> PathBuf {
        self.dir.join("service.log")
    }
    fn manifest(&self) -> PathBuf {
        self.dir.join("decision-material-manifest.json")
    }
    fn provider(&self) -> PathBuf {
        self.dir.join("decision-provider.json")
    }
    fn checkout(&self) -> PathBuf {
        self.dir.join("kev")
    }
    fn python(&self) -> PathBuf {
        self.checkout().join(".venv").join("bin").join("python")
    }
}

pub(crate) fn io_fail(what: &str, path: &Path, error: impl std::fmt::Display) -> AikitError {
    fail(
        "decision_service.io",
        format!("{what} {}: {error}", path.display()),
    )
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| io_fail("create", parent, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(|e| io_fail("write", &temporary, e))?;
    std::fs::rename(&temporary, path).map_err(|e| io_fail("replace", path, e))
}

fn load_state(layout: &Layout) -> Result<Option<ServiceState>> {
    let path = layout.state();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_fail("read", &path, e)),
    };
    let state: ServiceState = serde_json::from_slice(&bytes).map_err(|e| {
        fail(
            "decision_service.state_invalid",
            format!("{}: {e}", path.display()),
        )
    })?;
    if state.schema != SERVICE_SCHEMA || state.owner != OWNER {
        return Err(fail(
            "decision_service.state_invalid",
            format!(
                "{} is not an AIKit-owned decision service state (schema {}, owner {})",
                path.display(),
                state.schema,
                state.owner
            ),
        ));
    }
    state.recipe.validate()?;
    Ok(Some(state))
}

fn save_state(layout: &Layout, state: &ServiceState) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| fail("decision_service.encode", e.to_string()))?;
    write_atomic(&layout.state(), &bytes)
}

fn lock(layout: &Layout, purpose: &str) -> Result<ContextLock> {
    ContextLock::acquire_at(
        &layout.lock(),
        "decision-service",
        LockOptions::default()
            .with_timeout(Duration::from_secs(30))
            .with_purpose(purpose),
    )
}

fn load_recipe(path: Option<&Path>) -> Result<ServiceRecipe> {
    let recipe = match path {
        None => ServiceRecipe::kev_0_8b(),
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| io_fail("read recipe", path, e))?;
            serde_json::from_slice(&bytes).map_err(|e| {
                fail(
                    "decision_service.recipe_invalid",
                    format!("{}: {e}", path.display()),
                )
            })?
        }
    };
    recipe.validate()?;
    Ok(recipe)
}

fn check_port(port: u16) -> Result<()> {
    if port < 1024 {
        return Err(fail(
            "decision_service.port_invalid",
            "a local decision service binds an unprivileged loopback port (1024-65535)",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Process identity. Ownership is pid + start time + command line, never a port.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    /// The recorded process is alive and is the process we started.
    Ours,
    /// The recorded process is gone (or a zombie).
    Gone,
    /// The pid is alive but is not the process we recorded.
    Reused,
}

pub(crate) fn ps_field(pid: u32, field: &str) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", field])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn standing(record: &ProcessRecord, recipe: &ServiceRecipe, port: u16) -> Standing {
    process_standing(
        record,
        &[recipe.serve_module.clone(), format!("--port {port}")],
    )
}

/// Ownership of a recorded process: alive, same start time, and a command
/// line carrying every needle. Shared by every AIKit-owned local service.
pub(crate) fn process_standing(record: &ProcessRecord, needles: &[String]) -> Standing {
    // A zombie (we spawned it in this process and it has exited) is gone.
    match ps_field(record.pid, "stat=") {
        None => return Standing::Gone,
        Some(stat) if stat.starts_with('Z') => return Standing::Gone,
        Some(_) => {}
    }
    let started = ps_field(record.pid, "lstart=");
    let args = ps_field(record.pid, "args=").unwrap_or_default();
    let same_start = started.as_deref() == Some(record.started_at.as_str());
    let same_command = needles.iter().all(|needle| args.contains(needle.as_str()));
    if same_start && same_command {
        Standing::Ours
    } else {
        Standing::Reused
    }
}

pub(crate) fn signal(pid: u32, name: &str) -> bool {
    Command::new("kill")
        .args([name, &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub(crate) fn wait_until(
    deadline: Instant,
    interval: Duration,
    mut done: impl FnMut() -> bool,
) -> bool {
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(interval);
    }
}

pub(crate) fn log_tail(path: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

// ---------------------------------------------------------------------------
// Health: the pinned model card, not a bare port.
// ---------------------------------------------------------------------------

fn endpoint(port: u16) -> Result<DecisionEndpoint> {
    DecisionEndpoint::resolve(&format!("127.0.0.1:{port}"), false)
}

fn card_identity(card: &Value, recipe: &ServiceRecipe) -> std::result::Result<Value, String> {
    let entry = card["models"]
        .as_array()
        .and_then(|models| models.iter().find(|m| m["name"] == json!(recipe.model)))
        .ok_or_else(|| format!("the model card does not list `{}`", recipe.model))?;
    if entry["run"] != json!(recipe.run) {
        return Err(format!(
            "the model card serves run {} but the recipe pins {}",
            entry["run"], recipe.run
        ));
    }
    if entry["base"] != json!(recipe.base_repo) {
        return Err(format!(
            "the model card serves base {} but the recipe pins {}",
            entry["base"], recipe.base_repo
        ));
    }
    Ok(entry.clone())
}

fn read_card(curl: &Path, port: u16, timeout_ms: u64) -> Result<Value> {
    probe_models(curl.to_path_buf(), &endpoint(port)?, timeout_ms, None)
}

fn provider_config(
    recipe: &ServiceRecipe,
    port: u16,
    layout: &Layout,
    weights_bytes: Option<u64>,
) -> DecisionProviderConfig {
    DecisionProviderConfig {
        schema: crate::decide::DECISION_PROVIDER_SCHEMA.into(),
        // Honest placement: a loopback compatible endpoint. The lifecycle owner
        // is recorded in the service state, not smuggled into the placement.
        mode: DecisionProviderMode::Endpoint,
        address: Some(format!("127.0.0.1:{port}")),
        allow_remote: false,
        credential_ref: None,
        limits: Some(recipe.limits.clone()),
        jev_limits: None,
        decision_model: Some(recipe.identity(&layout.dir, weights_bytes)),
    }
}

fn warm_request(recipe: &ServiceRecipe) -> Result<JevRequest> {
    JevRequest::parse(
        &serde_json::to_vec(&json!({
            "model": recipe.model,
            "state": {"warmup": "Startup warmup: kernels compile on the first forward pass; this request is discarded."},
            "questions": {
                "warm_noul": {"type": "noul", "instructions": "Warmup probe: answer true."},
                "warm_choice": {"type": "choice", "instructions": "Warmup routing probe.",
                                "criteria": {"a": "first", "b": "second"}},
                "warm_score": {"type": "score", "instructions": "Warmup ordinal probe.",
                               "criteria": ["low", "high"]}
            }
        }))
        .expect("warm-up request encodes"),
    )
}

/// One real warm decision through the elected provider. Returns elapsed ms.
fn warm(config: &DecisionProviderConfig, recipe: &ServiceRecipe, curl: &Path) -> Result<u64> {
    let request = warm_request(recipe)?;
    let reference = minted_invocation_ref(&request)?;
    let started = Instant::now();
    let mut no_revalidate = || -> Result<()> { Ok(()) };
    let receipt = invoke_selected(
        config,
        &request,
        reference,
        Some(curl.to_path_buf()),
        false,
        &mut no_revalidate,
    )?;
    if !receipt.outcome_completed() {
        return Err(fail(
            "decision_service.warm_failed",
            format!(
                "the service answered its model card but the warm decision failed: {}",
                receipt
                    .failure_message()
                    .unwrap_or_else(|| "no answer".into())
            ),
        ));
    }
    Ok(started.elapsed().as_millis() as u64)
}

// ---------------------------------------------------------------------------
// Provisioning.
// ---------------------------------------------------------------------------

pub(crate) fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = std::fs::File::open(path).map_err(|e| io_fail("open", path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| io_fail("read", path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        total += n as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

fn run_step(runner: &dyn CommandRunner, argv: Vec<String>, code: &'static str) -> Result<String> {
    let output = runner.run(&argv).map_err(|e| {
        fail(
            "decision_service.step_unavailable",
            format!("`{}` could not run: {}", argv.join(" "), e.message()),
        )
    })?;
    output.require(&argv, code).map(|o| o.stdout)
}

fn s(value: impl Into<String>) -> String {
    value.into()
}

/// Make the pinned checkout, environment and artifacts, and hash them.
fn provision_material(
    layout: &Layout,
    recipe: &ServiceRecipe,
    runner: &dyn CommandRunner,
) -> Result<Provisioned> {
    std::fs::create_dir_all(&layout.dir).map_err(|e| io_fail("create", &layout.dir, e))?;
    let checkout = layout.checkout();
    let checkout_s = checkout.display().to_string();

    if !checkout.join(".git").exists() {
        run_step(
            runner,
            vec![
                s("git"),
                s("clone"),
                s("--quiet"),
                s("--no-checkout"),
                recipe.upstream_repository.clone(),
                checkout_s.clone(),
            ],
            "decision_service.clone_failed",
        )?;
    } else {
        let origin = run_step(
            runner,
            vec![
                s("git"),
                s("-C"),
                checkout_s.clone(),
                s("remote"),
                s("get-url"),
                s("origin"),
            ],
            "decision_service.checkout_foreign",
        )?;
        if origin.trim() != recipe.upstream_repository {
            return Err(fail(
                "decision_service.checkout_foreign",
                format!(
                    "{} has origin {} but the recipe pins {}; refusing to adopt another checkout",
                    checkout.display(),
                    origin.trim(),
                    recipe.upstream_repository
                ),
            ));
        }
    }
    let fetched = runner
        .run(&[
            s("git"),
            s("-C"),
            checkout_s.clone(),
            s("fetch"),
            s("--quiet"),
            s("origin"),
            recipe.upstream_pin.clone(),
        ])
        .map(|o| o.ok())
        .unwrap_or(false);
    if !fetched {
        run_step(
            runner,
            vec![
                s("git"),
                s("-C"),
                checkout_s.clone(),
                s("fetch"),
                s("--quiet"),
                s("origin"),
            ],
            "decision_service.fetch_failed",
        )?;
    }
    run_step(
        runner,
        vec![
            s("git"),
            s("-C"),
            checkout_s.clone(),
            s("checkout"),
            s("--quiet"),
            s("--detach"),
            recipe.upstream_pin.clone(),
        ],
        "decision_service.checkout_failed",
    )?;
    let head = run_step(
        runner,
        vec![
            s("git"),
            s("-C"),
            checkout_s.clone(),
            s("rev-parse"),
            s("HEAD"),
        ],
        "decision_service.checkout_failed",
    )?;
    let head = head.trim().to_string();
    if head != recipe.upstream_pin {
        return Err(fail(
            "decision_service.pin_mismatch",
            format!(
                "upstream HEAD {head} is not the pinned {}",
                recipe.upstream_pin
            ),
        ));
    }

    run_step(
        runner,
        vec![
            s("uv"),
            s("--directory"),
            checkout_s.clone(),
            s("sync"),
            s("--python"),
            recipe.python.clone(),
            s("--extra"),
            recipe.extra.clone(),
        ],
        "decision_service.environment_failed",
    )?;
    let python = layout.python();
    if !python.exists() {
        return Err(fail(
            "decision_service.environment_failed",
            format!("`uv sync` finished but {} does not exist", python.display()),
        ));
    }

    let stdout = run_step(
        runner,
        vec![
            python.display().to_string(),
            s("-c"),
            s(SNAPSHOT_SCRIPT),
            recipe.adapter_repo.clone(),
            recipe.adapter_revision.clone(),
            recipe.base_repo.clone(),
            recipe.base_revision.clone(),
        ],
        "decision_service.artifacts_failed",
    )?;
    let located: Value = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .ok_or_else(|| {
            fail(
                "decision_service.artifacts_failed",
                "the artifact fetch printed no location",
            )
        })?;
    let adapter_dir = PathBuf::from(located["adapter_dir"].as_str().unwrap_or_default());
    let base_dir = PathBuf::from(located["base_dir"].as_str().unwrap_or_default());
    for (what, dir, revision) in [
        ("adapter", &adapter_dir, &recipe.adapter_revision),
        ("base", &base_dir, &recipe.base_revision),
    ] {
        if dir.file_name().and_then(|n| n.to_str()) != Some(revision.as_str()) {
            return Err(fail(
                "decision_service.pin_mismatch",
                format!(
                    "the {what} snapshot {} is not the pinned revision {revision}",
                    dir.display()
                ),
            ));
        }
    }

    let mut artifacts = BTreeMap::new();
    let mut weights_bytes = 0u64;
    for name in ADAPTER_FILES {
        let path = adapter_dir.join(name);
        if path.exists() {
            let (sha, bytes) = sha256_file(&path)?;
            artifacts.insert(name.to_string(), json!({"sha256": sha, "bytes": bytes}));
        } else if name != "provenance.json" {
            return Err(fail(
                "decision_service.artifacts_failed",
                format!("the pinned adapter has no {name}"),
            ));
        }
    }
    let mut bases: Vec<PathBuf> = std::fs::read_dir(&base_dir)
        .map_err(|e| io_fail("list", &base_dir, e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("safetensors"))
        .collect();
    bases.sort();
    if bases.is_empty() {
        return Err(fail(
            "decision_service.artifacts_failed",
            "the pinned base has no safetensors weights",
        ));
    }
    for path in bases {
        let (sha, bytes) = sha256_file(&path)?;
        weights_bytes += bytes;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        artifacts.insert(
            format!("base/{name}"),
            json!({"sha256": sha, "bytes": bytes}),
        );
    }
    let manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "recipe": recipe.recipe,
        "upstream_repository": recipe.upstream_repository,
        "upstream_pin": recipe.upstream_pin,
        "upstream_head": head,
        "adapter_repo": recipe.adapter_repo,
        "adapter_revision": recipe.adapter_revision,
        "base_repo": recipe.base_repo,
        "base_revision": recipe.base_revision,
        "license": recipe.license,
        "runtime": {"python": recipe.python, "extra": recipe.extra},
        "adapter_dir": adapter_dir,
        "base_dir": base_dir,
        "artifacts": artifacts,
        "weights_bytes": weights_bytes,
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| fail("decision_service.encode", e.to_string()))?;
    write_atomic(&layout.manifest(), &manifest_bytes)?;
    Ok(Provisioned {
        upstream_head: head,
        adapter_dir: adapter_dir.display().to_string(),
        base_dir: base_dir.display().to_string(),
        manifest_sha256: format!("{:x}", Sha256::digest(&manifest_bytes)),
        weights_bytes,
        provisioned_unix_ms: now_ms(),
    })
}

fn write_provider(layout: &Layout, state: &ServiceState) -> Result<DecisionProviderConfig> {
    let config = provider_config(
        &state.recipe,
        state.port,
        layout,
        state.provisioned.as_ref().map(|p| p.weights_bytes),
    );
    config.validate()?;
    let bytes = serde_json::to_vec_pretty(&config)
        .map_err(|e| fail("decision_service.encode", e.to_string()))?;
    write_atomic(&layout.provider(), &bytes)?;
    Ok(config)
}

/// Re-hash every artifact the manifest names and say which differ.
fn verify_material(layout: &Layout) -> Value {
    let manifest: Value = match std::fs::read(layout.manifest())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(manifest) => manifest,
        None => return json!({"state": "missing", "reason": "no readable material manifest"}),
    };
    let adapter_dir = PathBuf::from(manifest["adapter_dir"].as_str().unwrap_or_default());
    let base_dir = PathBuf::from(manifest["base_dir"].as_str().unwrap_or_default());
    let mut mismatches = Vec::new();
    let mut checked = 0u64;
    for (name, entry) in manifest["artifacts"].as_object().into_iter().flatten() {
        let path = match name.strip_prefix("base/") {
            Some(file) => base_dir.join(file),
            None => adapter_dir.join(name),
        };
        checked += 1;
        match sha256_file(&path) {
            Ok((sha, bytes)) => {
                if json!(sha) != entry["sha256"] || json!(bytes) != entry["bytes"] {
                    mismatches.push(json!({"artifact": name, "reason": "hash or size differs"}));
                }
            }
            Err(e) => mismatches.push(json!({"artifact": name, "reason": e.message()})),
        }
    }
    json!({
        "state": if mismatches.is_empty() { "verified" } else { "corrupt" },
        "checked": checked,
        "mismatches": mismatches,
    })
}

// ---------------------------------------------------------------------------
// The lifecycle.
// ---------------------------------------------------------------------------

/// External collaborators, so tests can fake only the network-bound steps.
pub struct Tools {
    pub provision: Box<dyn CommandRunner>,
    pub curl: PathBuf,
}

impl Tools {
    fn system(step_timeout: Duration) -> Self {
        Self {
            // Provisioning is the one step that must reach the network.
            provision: Box::new(
                SystemRunner::new()
                    .with_timeout(step_timeout)
                    .with_env("HF_HUB_OFFLINE", "0")
                    .with_env("TRANSFORMERS_OFFLINE", "0"),
            ),
            curl: PathBuf::from("curl"),
        }
    }
}

fn require_state(layout: &Layout) -> Result<ServiceState> {
    load_state(layout)?.ok_or_else(|| {
        fail(
            "decision_service.not_provisioned",
            format!(
                "no decision service is provisioned at {}; run `aikit decide service provision`",
                layout.dir.display()
            ),
        )
    })
}

fn require_provisioned(state: &ServiceState) -> Result<&Provisioned> {
    state.provisioned.as_ref().ok_or_else(|| {
        fail(
            "decision_service.not_provisioned",
            "the service is declared but its material was never provisioned",
        )
    })
}

pub(crate) fn provision_with(
    layout_dir: &Path,
    port: u16,
    recipe: ServiceRecipe,
    tools: &Tools,
) -> Result<Value> {
    check_port(port)?;
    recipe.validate()?;
    let layout = Layout {
        dir: layout_dir.to_path_buf(),
    };
    std::fs::create_dir_all(&layout.dir).map_err(|e| io_fail("create", &layout.dir, e))?;
    let _lock = lock(&layout, "provision")?;
    let existing = load_state(&layout)?;
    if let Some(existing) = &existing {
        if let Some(process) = &existing.process {
            if standing(process, &existing.recipe, existing.port) == Standing::Ours {
                return Err(fail(
                    "decision_service.running",
                    "the service is running; stop it, or use `upgrade`, before provisioning again",
                ));
            }
        }
    }
    let provisioned = provision_material(&layout, &recipe, tools.provision.as_ref())?;
    let state = ServiceState {
        schema: SERVICE_SCHEMA.into(),
        owner: OWNER.into(),
        recipe,
        port,
        provisioned: Some(provisioned.clone()),
        process: None,
        previous: existing.and_then(|e| e.previous),
    };
    save_state(&layout, &state)?;
    let config = write_provider(&layout, &state)?;
    Ok(json!({
        "schema": STATUS_SCHEMA,
        "operation": "provision",
        "owner": OWNER,
        "state": "stopped",
        "service_dir": layout.dir,
        "recipe": state.recipe.recipe,
        "pins": {
            "upstream": state.recipe.upstream_pin,
            "adapter": format!("{}@{}", state.recipe.adapter_repo, state.recipe.adapter_revision),
            "base": format!("{}@{}", state.recipe.base_repo, state.recipe.base_revision),
        },
        "weights_bytes": provisioned.weights_bytes,
        "manifest": layout.manifest(),
        "manifest_sha256": provisioned.manifest_sha256,
        "provider_file": layout.provider(),
        "provider_mode": config.mode,
        "workcell": "not involved",
        "next": "aikit decide service start",
    }))
}

struct StartOptions {
    ready_timeout: Duration,
    warm: bool,
}

fn start_locked(
    layout: &Layout,
    state: &mut ServiceState,
    options: &StartOptions,
    tools: &Tools,
) -> Result<Value> {
    let provisioned = require_provisioned(state)?.clone();
    let python = layout.python();
    if !python.exists() {
        return Err(fail(
            "decision_service.not_provisioned",
            format!(
                "the service environment {} is missing; provision again",
                python.display()
            ),
        ));
    }
    let manifest_digest = std::fs::read(layout.manifest())
        .map(|b| format!("{:x}", Sha256::digest(&b)))
        .map_err(|e| io_fail("read", &layout.manifest(), e))?;
    if manifest_digest != provisioned.manifest_sha256 {
        return Err(fail(
            "decision_service.material_changed",
            "the material manifest differs from the one provisioned; provision or upgrade again",
        ));
    }
    let port = state.port;
    let recipe = state.recipe.clone();

    // Adopt our own live process; never another's.
    if let Some(record) = state.process.clone() {
        match standing(&record, &recipe, port) {
            Standing::Ours => {
                let health = read_card(&tools.curl, port, 5_000)
                    .map_err(|e| e.message().to_string())
                    .and_then(|card| card_identity(&card, &recipe));
                return Ok(json!({
                    "schema": STATUS_SCHEMA,
                    "operation": "start",
                    "outcome": "already-running",
                    "owner": OWNER,
                    "state": "running",
                    "pid": record.pid,
                    "healthy": health.is_ok(),
                    "health_reason": health.err(),
                    "address": format!("127.0.0.1:{port}"),
                    "provider_file": layout.provider(),
                    "workcell": "not involved",
                }));
            }
            Standing::Gone | Standing::Reused => {
                state.process = None;
                save_state(layout, state)?;
            }
        }
    }

    // A listener we do not own is a fact to report, never to adopt or replace.
    if let Ok(card) = read_card(&tools.curl, port, 1_500) {
        let compatible = card_identity(&card, &recipe).is_ok();
        return Err(fail(
            "decision_service.port_occupied",
            format!(
                "127.0.0.1:{port} already answers a model card ({}) and is not a process this service started; \
                 consume it with an `endpoint` election or provision this service on another --port",
                if compatible {
                    "it matches the pinned identity"
                } else {
                    "it does not match the pinned identity"
                }
            ),
        )
        .with("port", port.to_string()));
    }

    let mut argv = vec![
        python.display().to_string(),
        "-m".into(),
        recipe.serve_module.clone(),
        "--run".into(),
        recipe.run.clone(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
    ];
    let log_path = layout.log();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| io_fail("open", &log_path, e))?;
    let log_err = log
        .try_clone()
        .map_err(|e| io_fail("duplicate", &log_path, e))?;
    let program = argv.remove(0);
    let mut command = Command::new(&program);
    command
        .args(&argv)
        .current_dir(layout.checkout())
        .envs(&recipe.env)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    // The service must not inherit another owner's control authority.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("WORKCELL_") {
            command.env_remove(name);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a hang-up aimed at this command or the
        // terminal does not reach the service.
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        fail(
            "decision_service.spawn_failed",
            format!("could not start {program}: {e}"),
        )
    })?;
    let pid = child.id();
    argv.insert(0, program);
    let started_at = ps_field(pid, "lstart=").unwrap_or_default();
    let record = ProcessRecord {
        pid,
        started_at,
        argv: argv.clone(),
        started_unix_ms: now_ms(),
    };
    state.process = Some(record.clone());
    save_state(layout, state)?;

    // Ready means: the process is alive, the pinned model card answers, and
    // (unless declined) one real warm decision completed.
    let started = Instant::now();
    let deadline = started + options.ready_timeout;
    let mut exited = None;
    let mut identity = Err("the model card has not answered".to_string());
    let answered = wait_until(deadline, Duration::from_millis(250), || {
        if let Ok(Some(status)) = child.try_wait() {
            exited = Some(status);
            return true;
        }
        match read_card(&tools.curl, port, 2_000) {
            Ok(card) => {
                identity = card_identity(&card, &recipe);
                true
            }
            Err(e) => {
                identity = Err(e.message().to_string());
                false
            }
        }
    });
    if let Some(status) = exited {
        state.process = None;
        save_state(layout, state)?;
        return Err(fail(
            "decision_service.exited",
            format!(
                "the service process exited during startup ({status}); log tail:\n{}",
                log_tail(&log_path, 20)
            ),
        ));
    }
    if !answered {
        return Err(fail(
            "decision_service.not_ready",
            format!(
                "the pinned model card did not answer within {} s; the process (pid {pid}) is still recorded so `stop` or `status` can act on it; last probe: {}; log tail:\n{}",
                options.ready_timeout.as_secs(),
                identity.err().unwrap_or_default(),
                log_tail(&log_path, 20)
            ),
        ));
    }
    let served = identity.map_err(|reason| {
        fail(
            "decision_service.identity_mismatch",
            format!("the service answered but is not the pinned artifact: {reason}"),
        )
    })?;
    let ready_ms = started.elapsed().as_millis() as u64;
    let warm_ms = if options.warm {
        let config = write_provider(layout, state)?;
        Some(warm(&config, &recipe, &tools.curl)?)
    } else {
        None
    };
    // Detach: the child handle is dropped without waiting; the service
    // outlives this command.
    drop(child);
    Ok(json!({
        "schema": STATUS_SCHEMA,
        "operation": "start",
        "outcome": "started",
        "owner": OWNER,
        "state": "running",
        "pid": pid,
        "address": format!("127.0.0.1:{port}"),
        "served": served,
        "ready_ms": ready_ms,
        "warm_ms": warm_ms,
        "log": log_path,
        "provider_file": layout.provider(),
        "workcell": "not involved",
    }))
}

fn stop_locked(layout: &Layout, state: &mut ServiceState, grace: Duration) -> Result<Value> {
    let Some(record) = state.process.clone() else {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "not-running",
            "owner": OWNER, "state": "stopped", "workcell": "not involved",
        }));
    };
    match standing(&record, &state.recipe, state.port) {
        Standing::Gone => {
            state.process = None;
            save_state(layout, state)?;
            Ok(json!({
                "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "already-stopped",
                "owner": OWNER, "state": "stopped", "pid": record.pid, "workcell": "not involved",
            }))
        }
        Standing::Reused => Err(fail(
            "decision_service.identity_changed",
            format!(
                "pid {} is alive but is not the process this service started; refusing to signal an unrelated process",
                record.pid
            ),
        )),
        Standing::Ours => {
            signal(record.pid, "-TERM");
            let deadline = Instant::now() + grace;
            let gone = wait_until(deadline, Duration::from_millis(100), || {
                standing(&record, &state.recipe, state.port) != Standing::Ours
            });
            let mut escalated = false;
            if !gone {
                escalated = true;
                signal(record.pid, "-KILL");
                let gone = wait_until(
                    Instant::now() + Duration::from_secs(5),
                    Duration::from_millis(100),
                    || standing(&record, &state.recipe, state.port) != Standing::Ours,
                );
                if !gone {
                    return Err(fail(
                        "decision_service.stop_failed",
                        format!("pid {} survived SIGKILL", record.pid),
                    ));
                }
            }
            state.process = None;
            save_state(layout, state)?;
            Ok(json!({
                "schema": STATUS_SCHEMA, "operation": "stop", "outcome": "stopped",
                "owner": OWNER, "state": "stopped", "pid": record.pid,
                "escalated_to_kill": escalated, "workcell": "not involved",
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Verbs.
// ---------------------------------------------------------------------------

fn start_options(timeout_secs: u64, no_warm: bool) -> StartOptions {
    StartOptions {
        ready_timeout: Duration::from_secs(timeout_secs.max(1)),
        warm: !no_warm,
    }
}

pub fn service_provision(args: DecideServiceProvisionArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let recipe = load_recipe(args.recipe_file.as_deref())?;
    let tools = Tools::system(Duration::from_secs(args.step_timeout_secs.max(1)));
    provision_with(&layout.dir, args.port, recipe, &tools)
}

pub fn service_start(args: DecideServiceStartArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "start")?;
    let mut state = require_state(&layout)?;
    let tools = Tools::system(Duration::from_secs(60));
    start_locked(
        &layout,
        &mut state,
        &start_options(args.ready_timeout_secs, args.no_warm),
        &tools,
    )
}

pub fn service_stop(args: DecideServiceStopArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "stop")?;
    let mut state = require_state(&layout)?;
    stop_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.grace_secs.max(1)),
    )
}

pub fn service_restart(args: DecideServiceRestartArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "restart")?;
    let mut state = require_state(&layout)?;
    let tools = Tools::system(Duration::from_secs(60));
    let stopped = stop_locked(
        &layout,
        &mut state,
        Duration::from_secs(args.grace_secs.max(1)),
    )?;
    let started = start_locked(
        &layout,
        &mut state,
        &start_options(args.ready_timeout_secs, args.no_warm),
        &tools,
    )?;
    Ok(json!({
        "schema": STATUS_SCHEMA, "operation": "restart", "owner": OWNER,
        "stop": stopped, "start": started, "workcell": "not involved",
    }))
}

pub fn service_status(args: DecideServiceStatusArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let tools = Tools::system(Duration::from_secs(60));
    let Some(state) = load_state(&layout)? else {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "status", "owner": OWNER,
            "state": "not-provisioned", "service_dir": layout.dir,
            "next": "aikit decide service provision", "workcell": "not involved",
        }));
    };
    let mut status = json!({
        "schema": STATUS_SCHEMA, "operation": "status", "owner": OWNER,
        "lifecycle": "aikit decide service",
        "service_dir": layout.dir,
        "recipe": state.recipe.recipe,
        "address": format!("127.0.0.1:{}", state.port),
        "provider_file": layout.provider(),
        "provider_mode": "endpoint",
        "provisioned": state.provisioned,
        "workcell": "not involved",
    });
    let process_state = match &state.process {
        None => json!({"recorded": false}),
        Some(record) => {
            let standing = standing(record, &state.recipe, state.port);
            json!({
                "recorded": true, "pid": record.pid,
                "standing": match standing {
                    Standing::Ours => "ours",
                    Standing::Gone => "gone",
                    Standing::Reused => "pid-reused",
                },
                "started_unix_ms": record.started_unix_ms,
            })
        }
    };
    let ours = state
        .process
        .as_ref()
        .map(|r| standing(r, &state.recipe, state.port) == Standing::Ours)
        .unwrap_or(false);
    status["process"] = process_state;
    let card = read_card(&tools.curl, state.port, 5_000);
    let health = match &card {
        Ok(card) => match card_identity(card, &state.recipe) {
            Ok(served) => json!({"reachable": true, "identity": "pinned", "served": served}),
            Err(reason) => json!({"reachable": true, "identity": "mismatch", "reason": reason}),
        },
        Err(e) => json!({"reachable": false, "reason": e.message()}),
    };
    status["health"] = health;
    status["state"] = json!(match (ours, card.is_ok()) {
        (true, true) => "running",
        (true, false) => "running-unhealthy",
        (false, true) => "foreign-listener",
        (false, false) => {
            if state.provisioned.is_some() {
                "stopped"
            } else {
                "declared"
            }
        }
    });
    if args.verify_material {
        status["material"] = verify_material(&layout);
    }
    if args.probe {
        let config = provider_config(&state.recipe, state.port, &layout, None);
        status["diagnostic"] = match warm(&config, &state.recipe, &tools.curl) {
            Ok(ms) => json!({"outcome": "completed", "elapsed_ms": ms}),
            Err(e) => json!({"outcome": "failed", "reason": e.message()}),
        };
    }
    Ok(status)
}

pub fn service_upgrade(args: DecideServiceUpgradeArgs) -> Result<Value> {
    let layout = Layout::resolve(&args.common)?;
    let _lock = lock(&layout, "upgrade")?;
    let mut state = require_state(&layout)?;
    let target = load_recipe(args.recipe_file.as_deref())?;
    let tools = Tools::system(Duration::from_secs(args.step_timeout_secs.max(1)));
    if target == state.recipe && !args.force {
        return Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "upgrade", "outcome": "current",
            "owner": OWNER, "recipe": target.recipe,
            "pins": {"upstream": target.upstream_pin,
                     "adapter": target.adapter_revision, "base": target.base_revision},
            "workcell": "not involved",
        }));
    }
    let was_running = state
        .process
        .as_ref()
        .map(|r| standing(r, &state.recipe, state.port) == Standing::Ours)
        .unwrap_or(false);
    let previous = PreviousCut {
        recipe: state.recipe.clone(),
        provisioned: require_provisioned(&state)?.clone(),
    };
    let start = start_options(args.ready_timeout_secs, args.no_warm);
    let grace = Duration::from_secs(args.grace_secs.max(1));
    let stopped = stop_locked(&layout, &mut state, grace)?;

    let attempt = (|| -> Result<(Provisioned, Option<Value>)> {
        let provisioned = provision_material(&layout, &target, tools.provision.as_ref())?;
        state.recipe = target.clone();
        state.provisioned = Some(provisioned.clone());
        state.previous = Some(Box::new(previous.clone()));
        save_state(&layout, &state)?;
        write_provider(&layout, &state)?;
        let started = if was_running {
            Some(start_locked(&layout, &mut state, &start, &tools)?)
        } else {
            None
        };
        Ok((provisioned, started))
    })();

    match attempt {
        Ok((provisioned, started)) => Ok(json!({
            "schema": STATUS_SCHEMA, "operation": "upgrade", "outcome": "upgraded",
            "owner": OWNER, "stop": stopped, "start": started,
            "from": {"upstream": previous.recipe.upstream_pin,
                     "adapter": previous.recipe.adapter_revision,
                     "base": previous.recipe.base_revision},
            "to": {"upstream": target.upstream_pin,
                   "adapter": target.adapter_revision, "base": target.base_revision},
            "manifest_sha256": provisioned.manifest_sha256,
            "workcell": "not involved",
        })),
        Err(failure) => {
            // Return to the previous cut and say honestly whether that held.
            let _ = stop_locked(&layout, &mut state, grace);
            let rollback = provision_material(&layout, &previous.recipe, tools.provision.as_ref())
                .and_then(|restored| {
                    state.recipe = previous.recipe.clone();
                    state.provisioned = Some(restored);
                    state.previous = None;
                    save_state(&layout, &state)?;
                    write_provider(&layout, &state)?;
                    if was_running {
                        start_locked(&layout, &mut state, &start, &tools).map(|_| ())
                    } else {
                        Ok(())
                    }
                });
            let rollback_note = match rollback {
                Ok(()) => format!(
                    "rolled back to upstream {} / adapter {} and {}",
                    previous.recipe.upstream_pin,
                    previous.recipe.adapter_revision,
                    if was_running {
                        "restarted healthy"
                    } else {
                        "left stopped as before"
                    }
                ),
                Err(e) => format!("ROLLBACK ALSO FAILED ({}): {}", e.code(), e.message()),
            };
            Err(fail(
                "decision_service.upgrade_failed",
                format!(
                    "upgrade failed ({}): {}; {rollback_note}",
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
    use aikit_adapters::runner::Output;
    use std::sync::Mutex;

    /// Records argv and answers like the real tools would, creating the files
    /// the real steps create. Network-free.
    struct FakeProvisioner {
        calls: Mutex<Vec<Vec<String>>>,
        dir: PathBuf,
        hf: PathBuf,
        adapter_revision: String,
        base_revision: String,
        head: String,
        origin: String,
    }

    impl CommandRunner for FakeProvisioner {
        fn run(&self, argv: &[String]) -> Result<Output> {
            self.calls.lock().unwrap().push(argv.to_vec());
            let joined = argv.join(" ");
            if joined.contains("rev-parse HEAD") {
                return Ok(Output::success(format!("{}\n", self.head)));
            }
            if joined.contains("remote get-url origin") {
                return Ok(Output::success(format!("{}\n", self.origin)));
            }
            if argv[0] == "git" && argv.contains(&"clone".to_string()) {
                std::fs::create_dir_all(self.dir.join("kev/.git")).unwrap();
            }
            if argv[0] == "uv" {
                let bin = self.dir.join("kev/.venv/bin");
                std::fs::create_dir_all(&bin).unwrap();
                std::fs::write(bin.join("python"), "#!/bin/sh\n").unwrap();
            }
            if argv.iter().any(|a| a == "-c") {
                let adapter = self.hf.join("adapter").join(&self.adapter_revision);
                let base = self.hf.join("base").join(&self.base_revision);
                std::fs::create_dir_all(&adapter).unwrap();
                std::fs::create_dir_all(&base).unwrap();
                for name in ADAPTER_FILES {
                    std::fs::write(adapter.join(name), name.as_bytes()).unwrap();
                }
                std::fs::write(base.join("model.safetensors"), b"weights").unwrap();
                return Ok(Output::success(format!(
                    "progress noise\n{}\n",
                    json!({"adapter_dir": adapter, "base_dir": base})
                )));
            }
            Ok(Output::success(""))
        }
    }

    fn fake(dir: &Path, hf: &Path, recipe: &ServiceRecipe) -> FakeProvisioner {
        FakeProvisioner {
            calls: Mutex::new(Vec::new()),
            dir: dir.to_path_buf(),
            hf: hf.to_path_buf(),
            adapter_revision: recipe.adapter_revision.clone(),
            base_revision: recipe.base_revision.clone(),
            head: recipe.upstream_pin.clone(),
            origin: recipe.upstream_repository.clone(),
        }
    }

    #[test]
    fn the_default_recipe_is_pinned_and_lawful() {
        let recipe = ServiceRecipe::kev_0_8b();
        recipe.validate().unwrap();
        let mut floating = recipe.clone();
        floating.upstream_pin = "main".into();
        assert_eq!(
            floating.validate().unwrap_err().code(),
            "decision_service.recipe_invalid"
        );
        let mut workcell_env = recipe.clone();
        workcell_env
            .env
            .insert("WORKCELL_CONTROL_TOKEN".into(), "x".into());
        assert!(workcell_env.validate().is_err());
        let mut remote_repo = recipe;
        remote_repo.upstream_repository = "git@example.com:kev.git".into();
        assert!(remote_repo.validate().is_err());
    }

    #[test]
    fn provisioning_pins_every_step_and_never_names_workcell() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("svc");
        let recipe = ServiceRecipe::kev_0_8b();
        let runner = fake(&dir, &temp.path().join("hf"), &recipe);
        let tools = Tools {
            provision: Box::new(runner),
            curl: PathBuf::from("curl"),
        };
        let receipt = provision_with(&dir, 18019, recipe.clone(), &tools).unwrap();
        assert_eq!(receipt["state"], "stopped");
        assert_eq!(receipt["provider_mode"], "endpoint");
        assert_eq!(receipt["workcell"], "not involved");
        assert_eq!(receipt["weights_bytes"], 7);

        let state = load_state(&Layout { dir: dir.clone() }).unwrap().unwrap();
        assert_eq!(state.owner, "aikit");
        let provisioned = state.provisioned.unwrap();
        assert_eq!(provisioned.upstream_head, recipe.upstream_pin);
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(dir.join("decision-material-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["adapter_revision"], recipe.adapter_revision);
        assert_eq!(manifest["artifacts"]["base/model.safetensors"]["bytes"], 7);

        // The provider election it wrote is an ordinary endpoint election.
        let config: DecisionProviderConfig =
            serde_json::from_slice(&std::fs::read(dir.join("decision-provider.json")).unwrap())
                .unwrap();
        config.validate().unwrap();
        assert_eq!(config.mode, DecisionProviderMode::Endpoint);
        assert_eq!(config.address.as_deref(), Some("127.0.0.1:18019"));
    }

    #[test]
    fn a_snapshot_that_is_not_the_pinned_revision_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("svc");
        let recipe = ServiceRecipe::kev_0_8b();
        let mut runner = fake(&dir, &temp.path().join("hf"), &recipe);
        // The fetch lands a different adapter revision than the one pinned.
        runner.adapter_revision = "0".repeat(40);
        let tools = Tools {
            provision: Box::new(runner),
            curl: PathBuf::from("curl"),
        };
        let error = provision_with(&dir, 18019, recipe, &tools).unwrap_err();
        assert_eq!(error.code(), "decision_service.pin_mismatch");
    }

    #[test]
    fn a_foreign_checkout_is_never_adopted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("svc");
        std::fs::create_dir_all(dir.join("kev/.git")).unwrap();
        let recipe = ServiceRecipe::kev_0_8b();
        let mut runner = fake(&dir, &temp.path().join("hf"), &recipe);
        runner.origin = "https://example.com/other/kev.git".into();
        let tools = Tools {
            provision: Box::new(runner),
            curl: PathBuf::from("curl"),
        };
        let error = provision_with(&dir, 18019, recipe, &tools).unwrap_err();
        assert_eq!(error.code(), "decision_service.checkout_foreign");
    }

    #[test]
    fn the_model_card_must_be_the_pinned_artifact_not_just_any_listener() {
        let recipe = ServiceRecipe::kev_0_8b();
        let good = json!({"models": [{"name": "kev-latest", "run": recipe.run, "base": recipe.base_repo}]});
        assert!(card_identity(&good, &recipe).is_ok());
        let other = json!({"models": [{"name": "kev-latest", "run": "someone/else", "base": recipe.base_repo}]});
        assert!(card_identity(&other, &recipe).unwrap_err().contains("pins"));
        let absent = json!({"models": [{"name": "other"}]});
        assert!(card_identity(&absent, &recipe).is_err());
    }
}
