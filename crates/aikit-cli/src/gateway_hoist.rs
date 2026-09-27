//! `aikit gateway hoist` — place this gateway's posture on another Workcell,
//! address a gateway that lives there, or receive one that arrived.
//!
//! One verb, two phases, honest everywhere:
//!
//! - **plan** (the default) packs the posture — connector entries (token by
//!   LOCATION, never value), the semantic gateway state (bindings, stream
//!   journals, Communiques), the coexistence document, and the resolved
//!   agent-provider entries the connectors' `agent_backing` names — and
//!   prints exactly what would move, what the target must re-resolve (token
//!   files, provider argv), and what identity each thing keeps. Bindings,
//!   streams and Communiques keep their refs; the gateway's own ref and
//!   Workcell become the target's (`agency-gateway/omarchy` on
//!   `workcell:omarchy`): material moves, semantics hold, and the plan says
//!   so.
//! - **apply** (`--apply`) reaches the Workcell the posture is declared for
//!   (`gateway remote add` had to happen first) and stages the posture as a
//!   versioned bundle into the target home's `state/`, where the target's own
//!   `aikit gateway hoist --receive` unpacks it. With `--ssh TARGET` the
//!   staging (and, with `--yes`, the receive and the service install) run
//!   over that channel; without it the bundle is staged locally beside the
//!   exact operator commands that carry it over. Every step prints; the first
//!   refusal stops the apply with the exact remedy.
//! - **receive** (`--receive`) unpacks a staged bundle into this home,
//!   refusing to clobber an existing posture without `--force`, under the
//!   gateway state lock, validating the snapshot through the kernel's own
//!   restore law before anything is written.
//!
//! Laws kept here:
//!
//! - **Credentials never move through this code.** Token LOCATIONS move; the
//!   token files themselves are the operator's to stage on the target. The
//!   one exception is explicit: with `--ssh`, `--yes` AND `--include-tokens`
//!   the token FILE at each packed `file:` location is copied from this
//!   machine to the same absolute path on the target, and this machine's
//!   declared copy of the target's own gateway token is placed at the
//!   target's conventional `gateway.token` path — every such copy is a
//!   printed step.
//! - **Workcell owns material lifecycle; Gateway owns contact semantics.**
//!   The bundle carries the semantic state; the hoisted gateway answers for
//!   the target Workcell from the moment its service installs there.
//! - **Deterministic first.** Apply's remote steps sit behind
//!   [`TargetChannel`]; the fixture channel in the tests runs the same
//!   ordered steps the ssh channel runs in reality.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    acquire_gateway_state_lock, load_coexistence, load_gateway_connectors, store_coexistence,
    store_gateway_connectors, AgencyGateway, CoexistenceDocument, GatewayConnectorEntry,
    GatewayConnectorsFile, GatewaySnapshot, GATEWAY_CONNECTORS_SCHEMA,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::encounter_profile_provider::resolve_provider;
use crate::gateway_contact::{load_remotes, now_unix_ms, three_part, GatewayRemote, WORKCELL_ENV};
use crate::secret_location::SecretLocation;

/// The staged posture bundle's schema.
pub const GATEWAY_HOIST_SCHEMA: &str = "aikit.gateway-hoist/v1";
/// Where apply stages the bundle and receive finds it, inside the target
/// home's `state/` directory.
pub const STAGED_BUNDLE_NAME: &str = "gateway-hoist-pending.json";
/// Where apply leaves the bundle when no ssh channel was given, inside the
/// source home's `state/` directory.
pub const LOCAL_BUNDLE_DIR: &str = "gateway-hoist";
/// How long receive waits for the gateway state lock before refusing.
const RECEIVE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// What `aikit gateway hoist` was asked to do.
#[derive(Debug, Clone, Default)]
pub struct HoistArgs {
    /// The Workcell the posture moves to (`workcell:omarchy`).
    pub to: Option<String>,
    /// Stage the packed posture (plan only without it).
    pub apply: bool,
    /// Unpack a staged bundle into this home.
    pub receive: bool,
    /// Receive over an existing posture.
    pub force: bool,
    /// Execute the remote steps after staging (needs the ssh channel).
    pub yes: bool,
    /// The ssh target the channel reaches (`user@host`). Never invented: the
    /// operator names it.
    pub ssh: Option<String>,
    /// Copy each packed `file:` token file to the target. Explicit opt-in.
    pub include_tokens: bool,
    /// The target gateway ref; derived from the Workcell when omitted.
    pub gateway_ref: Option<String>,
}

// ---------------------------------------------------------------------------
// The packed posture.
// ---------------------------------------------------------------------------

/// The posture as it travels: semantic state and locations, never credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackedPosture {
    pub schema: String,
    pub packed_at_unix_ms: u64,
    pub source_gateway_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_workcell_ref: Option<String>,
    pub target_workcell_ref: String,
    pub target_gateway_ref: String,
    #[serde(default)]
    pub connectors: Vec<GatewayConnectorEntry>,
    pub gateway_state: GatewaySnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coexistence: Option<CoexistenceDocument>,
    #[serde(default)]
    pub agent_providers: Vec<Value>,
    /// The target's gateway WebSocket endpoint as the source declares it, so
    /// receive can print the exact install command. Applied when the remote
    /// declaration is consulted; absent from a bare pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_declared_bind: Option<String>,
}

fn workcell_slug(workcell_ref: &str) -> Result<&str> {
    ResourceRef::parse(workcell_ref).map_err(|error| {
        AikitError::new(
            "gateway.hoist_workcell_invalid",
            format!("parse workcell ref {workcell_ref}: {error}"),
        )
    })?;
    workcell_ref
        .strip_prefix("workcell:")
        .filter(|slug| !slug.is_empty())
        .ok_or_else(|| {
            AikitError::new(
                "gateway.hoist_workcell_invalid",
                format!(
                    "{workcell_ref} is not a Workcell ref; hoisting takes --to workcell:<name>"
                ),
            )
        })
}

/// Pack the posture of this AIKit home for `--to`: the pure half of the verb.
/// Nothing here resolves a token value; locations are carried as declared.
pub fn pack(home: &AikitHome, args: &HoistArgs) -> Result<PackedPosture> {
    let to = args.to.as_deref().ok_or_else(|| {
        AikitError::new(
            "cli.usage",
            "hoisting takes --to workcell:<name> (or --receive)",
        )
    })?;
    let slug = workcell_slug(to)?.to_owned();
    let target_gateway_ref = match &args.gateway_ref {
        Some(raw) => {
            ResourceRef::parse(raw).map_err(|error| {
                AikitError::new(
                    "gateway.hoist_gateway_ref_invalid",
                    format!("parse gateway ref {raw}: {error}"),
                )
            })?;
            raw.clone()
        }
        None => format!("agency-gateway/{slug}"),
    };
    ResourceRef::parse(&target_gateway_ref).map_err(|error| {
        AikitError::new(
            "gateway.hoist_gateway_ref_invalid",
            format!("parse gateway ref {target_gateway_ref}: {error}"),
        )
    })?;

    let connectors =
        load_gateway_connectors(&crate::gateway_connectors::connectors_path(home))?.connectors;

    // The semantic state: restored through the kernel's own law, re-snapshotted
    // under the target's identity. A home that never ran a gateway packs an
    // empty posture, which is honest.
    let source_ref = std::env::var("AIKIT_GATEWAY_REF")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "agency-gateway/local".into());
    let mut gateway = AgencyGateway::new(ResourceRef::parse(&source_ref).map_err(|error| {
        AikitError::new(
            "gateway.hoist_source_ref_invalid",
            format!("parse gateway ref {source_ref}: {error}"),
        )
    })?);
    let state_file = home.gateway_state();
    if state_file.exists() {
        gateway = aikit_adapters::restore_gateway_state(gateway, Some(&state_file))?;
    }
    let mut gateway_state = gateway.snapshot();
    gateway_state.gateway_ref = ResourceRef::parse(&target_gateway_ref).map_err(|error| {
        AikitError::new(
            "gateway.hoist_gateway_ref_invalid",
            format!("parse gateway ref {target_gateway_ref}: {error}"),
        )
    })?;

    let coexistence_path = crate::gateway_ops::coexistence_path(home);
    let coexistence = coexistence_path
        .exists()
        .then(|| load_coexistence(&coexistence_path))
        .transpose()?;

    // The resolved agent-provider entries the connectors' backings need. A
    // declared backing no provider answers refuses here, at pack time, the
    // same law serve applies at startup: the gateway must not move carrying a
    // conversation it could never answer.
    let mut agent_providers = Vec::new();
    let backings: Vec<&str> = connectors
        .iter()
        .filter_map(|entry| entry.agent_backing.as_deref())
        .collect();
    if !backings.is_empty() {
        let providers = load_resolved_providers(home)?;
        for backing in backings {
            let provider = providers
                .iter()
                .find(|provider| provider.id == backing)
                .ok_or_else(|| {
                    three_part(
                        "gateway.hoist_backing_unresolved",
                        format!(
                            "Connector backing {backing:?} has no encounter provider declared in {}.",
                            home.state().join("encounter-providers").display()
                        ),
                        "The posture was not packed; nothing moved.",
                        "Declare the provider with `aikit encounter providers add` (or remove \
                         --agent-backing), then hoist again."
                            .to_owned(),
                    )
                })?;
            agent_providers.push(serde_json::to_value(provider).map_err(|error| {
                AikitError::new(
                    "gateway.hoist_provider_encode",
                    format!("encode provider {}: {error}", provider.id),
                )
            })?);
        }
    }

    Ok(PackedPosture {
        schema: GATEWAY_HOIST_SCHEMA.into(),
        packed_at_unix_ms: now_unix_ms(),
        source_gateway_ref: source_ref,
        source_workcell_ref: std::env::var(WORKCELL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty()),
        target_workcell_ref: to.to_owned(),
        target_gateway_ref,
        connectors,
        gateway_state,
        coexistence,
        agent_providers,
        target_declared_bind: None,
    })
}

/// The encounter plane's own provider load for the posture: every
/// `encounter-providers/*.json` of this home, resolved from embedded profiles
/// at load, exactly as the serve path resolves them.
fn load_resolved_providers(
    home: &AikitHome,
) -> Result<Vec<crate::encounter_service::EncounterProvider>> {
    let root = home.state().join("encounter-providers");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|error| {
        AikitError::new(
            "gateway.hoist_providers_unreadable",
            format!("read {}: {error}", root.display()),
        )
    })? {
        let path = entry
            .map_err(|error| {
                AikitError::new(
                    "gateway.hoist_providers_unreadable",
                    format!("read {}: {error}", root.display()),
                )
            })?
            .path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let provider: crate::encounter_service::EncounterProvider =
            serde_json::from_slice(&std::fs::read(&path).map_err(|error| {
                AikitError::new(
                    "gateway.hoist_providers_unreadable",
                    format!("read {}: {error}", path.display()),
                )
            })?)
            .map_err(|error| {
                AikitError::new(
                    "gateway.hoist_providers_unreadable",
                    format!("decode {}: {error}", path.display()),
                )
            })?;
        rows.push(resolve_provider(provider).map_err(|failure| {
            AikitError::new(failure.code(), format!("{}: {failure}", path.display()))
        })?);
    }
    Ok(rows)
}

/// The `file:` token locations the posture names, with whether the file
/// exists (mode only — never its contents) on this machine.
fn file_token_locations(posture: &PackedPosture) -> Vec<(String, String, bool)> {
    let mut rows = Vec::new();
    for entry in &posture.connectors {
        if let Some(raw) = &entry.token_location {
            if let Ok(location) = SecretLocation::parse(raw) {
                if let SecretLocation::File(path) = &location {
                    rows.push((
                        entry.connector_ref.clone(),
                        location.render(),
                        path.exists(),
                    ));
                }
            }
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------

/// The plan: what moves, what the target re-resolves, what identity keeps.
/// Everything the apply would do, printed before anything is done.
pub fn plan_value(posture: &PackedPosture) -> Result<Value> {
    let journals = posture.gateway_state.streams.len();
    let events: usize = posture
        .gateway_state
        .streams
        .iter()
        .map(|stream| stream.events.len())
        .sum();
    let providers: Vec<Value> = posture
        .agent_providers
        .iter()
        .map(|provider| {
            json!({
                "id": provider.get("id"),
                "protocol": provider.get("protocol"),
                "argv": provider.get("argv"),
                "cwd_note": "the launch working directory re-resolves on the target at serve time unless the provider names one",
            })
        })
        .collect();
    let re_resolved: Vec<Value> = file_token_locations(posture)
        .into_iter()
        .map(|(connector_ref, location, exists_here)| {
            json!({
                "what": "connector token",
                "connector_ref": connector_ref,
                "location": location,
                "exists_on_this_machine": exists_here,
                "expectation": format!(
                    "an owner-only token file must exist at {location} on {} for the connector to build; \
                     stage it there, or re-run apply with --include-tokens to copy this machine's file",
                    posture.target_workcell_ref
                ),
            })
        })
        .collect();
    Ok(json!({
        "schema": "aikit.gateway-hoist-plan/v1",
        "verb": "plan",
        "from": {
            "gateway_ref": posture.source_gateway_ref,
            "workcell_ref": posture.source_workcell_ref,
        },
        "to": {
            "workcell_ref": posture.target_workcell_ref,
            "gateway_ref": posture.target_gateway_ref,
        },
        "moves": {
            "connectors": posture.connectors.iter().map(|entry| json!({
                "connector_ref": entry.connector_ref,
                "platform": entry.platform,
                "implementation": entry.implementation,
                "enabled": entry.enabled,
                "agent_backing": entry.agent_backing,
                "token_location": entry.token_location,
                "keeps": "its connector ref, platform and connector identity",
            })).collect::<Vec<_>>(),
            "bindings": posture.gateway_state.bindings.len(),
            "stream_journals": journals,
            "journal_events": events,
            "communiques": posture.gateway_state.communiques.len(),
            "pending_deliveries": posture.gateway_state.pending_deliveries.len(),
            "coexistence_policy": posture.coexistence.as_ref().map(|document| document.policy.as_str()),
            "agent_providers": providers,
        },
        "re_resolved_on_target": re_resolved,
        "identity": {
            "kept": format!(
                "binding refs, stream refs and journal sequence, and every Communique ref stay \
                 exactly as they are ({} bindings, {journals} journals, {} Communiques)",
                posture.gateway_state.bindings.len(),
                posture.gateway_state.communiques.len()
            ),
            "becomes": format!(
                "the gateway answers as {} on {} once its service installs there",
                posture.target_gateway_ref, posture.target_workcell_ref
            ),
        },
        "next": format!(
            "stage it with `aikit gateway hoist --to {} --apply [--ssh user@host]`; the target \
             unpacks with `aikit gateway hoist --receive`",
            posture.target_workcell_ref
        ),
    }))
}

// ---------------------------------------------------------------------------
// apply — the ordered steps behind a channel.
// ---------------------------------------------------------------------------

/// One reach into the target: a command run there, with optional stdin,
/// answering stdout. The fixture channel in the tests and the ssh channel in
/// reality run the same ordered steps.
pub trait TargetChannel {
    fn run(&self, step: &'static str, argv: &[String], stdin: Option<&[u8]>) -> Result<Vec<u8>>;
    /// How the channel names itself in the plan and the output.
    fn describe(&self) -> String;
}

/// The real channel: `ssh <target> -- <argv…>`. The target is the operator's
/// word, never an invention of this code.
pub struct SshChannel {
    pub target: String,
}

impl TargetChannel for SshChannel {
    fn run(&self, step: &'static str, argv: &[String], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("ssh")
            .arg(&self.target)
            .arg("--")
            .args(argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                step_failure(
                    step,
                    &self.describe(),
                    format!("ssh could not start: {error}"),
                )
            })?;
        if let Some(bytes) = stdin {
            child
                .stdin
                .take()
                .ok_or_else(|| {
                    step_failure(step, &self.describe(), "ssh stdin unavailable".to_owned())
                })?
                .write_all(bytes)
                .map_err(|error| {
                    step_failure(step, &self.describe(), format!("write stdin: {error}"))
                })?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| step_failure(step, &self.describe(), error.to_string()))?;
        if !output.status.success() {
            return Err(step_failure(
                step,
                &self.describe(),
                format!(
                    "exit {:?}: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(output.stdout)
    }

    fn describe(&self) -> String {
        format!("ssh {}", self.target)
    }
}

fn step_failure(step: &str, channel: &str, detail: String) -> AikitError {
    AikitError::new(
        "gateway.hoist_step_failed",
        format!("hoist step {step:?} over {channel} failed: {detail}"),
    )
}

/// Stage the posture. With `--ssh` every step runs over the channel; without
/// it the bundle is staged locally and the operator commands are printed.
/// Every step prints; the first refusal stops the apply with the remedy.
pub fn apply(home: &AikitHome, args: &HoistArgs) -> Result<Value> {
    match &args.ssh {
        Some(target) => apply_over_channel(
            home,
            args,
            &SshChannel {
                target: target.clone(),
            },
        ),
        None => apply_local(home, args),
    }
}

/// The declared remote for `--to`; apply refuses to invent a target.
fn declared_remote(home: &AikitHome, workcell_ref: &str) -> Result<GatewayRemote> {
    load_remotes(home)?
        .remotes
        .into_iter()
        .find(|remote| remote.workcell_ref == workcell_ref)
        .ok_or_else(|| {
            three_part(
                "gateway.remote_undeclared",
                format!(
                    "No gateway endpoint is declared for {workcell_ref}, so there is no fabric \
                     fact to hoist against."
                ),
                "Nothing was staged.",
                format!(
                    "Declare the endpoint first: aikit gateway remote add --workcell \
                     {workcell_ref} --ws HOST:PORT --token-location file:/ABSOLUTE/PATH"
                ),
            )
        })
}

/// The token files `--include-tokens` would copy, verified to resolve here
/// before any step runs: first this machine's declared copy of the target's
/// own gateway token, then each connector's `file:` token.
fn include_token_sources(
    posture: &PackedPosture,
    remote: &GatewayRemote,
) -> Result<Vec<(String, PathBuf, Vec<u8>, Option<String>)>> {
    let mut rows: Vec<(String, PathBuf, Vec<u8>, Option<String>)> = Vec::new();
    let mut push = |what: String, location: &str, target_name: Option<String>| -> Result<()> {
        let secret = SecretLocation::parse(location)
            .and_then(|parsed| parsed.resolve())
            .map_err(|error| {
                three_part(
                    "gateway.hoist_token_missing",
                    format!("The token file for {what} cannot be read from {location}: {error}."),
                    "Nothing was staged; no token moved.",
                    format!(
                        "Make it an owner-only, non-empty file (chmod 600 {location}), or apply \
                         without --include-tokens and stage the token on the target by hand."
                    ),
                )
            })?;
        let path = match SecretLocation::parse(location)? {
            SecretLocation::File(path) => path,
            SecretLocation::Declared(_) => return Ok(()),
        };
        rows.push((what, path, secret.expose().as_bytes().to_vec(), target_name));
        Ok(())
    };
    // The declared token is this machine's copy of the target gateway's own
    // bearer token; placed at the target's conventional path it is exactly
    // what the target's install-service checks peers against.
    if let SecretLocation::File(_) = SecretLocation::parse(&remote.token_location)? {
        push(
            format!(
                "the declared copy of {}'s gateway token",
                remote.workcell_ref
            ),
            &remote.token_location,
            Some("gateway.token".to_owned()),
        )?;
    } else {
        return Err(three_part(
            "gateway.hoist_token_missing",
            format!(
                "The declared token for {} lives at {} (a non-file location), so --include-tokens \
                 has no file to copy.",
                remote.workcell_ref, remote.token_location
            ),
            "Nothing was staged; no token moved.",
            "Stage the token on the target by hand, then apply without --include-tokens.",
        ));
    }
    for entry in &posture.connectors {
        if let Some(raw) = &entry.token_location {
            if matches!(SecretLocation::parse(raw)?, SecretLocation::File(_)) {
                push(format!("connector {}", entry.connector_ref), raw, None)?;
            }
        }
    }
    Ok(rows)
}

/// The ordered ssh-channel apply: resolve the target home, stage the bundle
/// (and, explicitly, tokens), and — with `--yes` — receive and install there.
pub fn apply_over_channel(
    home: &AikitHome,
    args: &HoistArgs,
    channel: &dyn TargetChannel,
) -> Result<Value> {
    let to = args
        .to
        .as_deref()
        .ok_or_else(|| AikitError::new("cli.usage", "apply takes --to workcell:<name>"))?;
    let remote = declared_remote(home, to)?;
    let mut posture = pack(home, args)?;
    posture.target_declared_bind = Some(remote.websocket_bind.clone());
    let bundle = serde_json::to_vec(&posture).map_err(|error| {
        AikitError::new(
            "gateway.hoist_bundle_encode",
            format!("encode bundle: {error}"),
        )
    })?;

    let mut steps: Vec<Value> = Vec::new();
    let record = |steps: &mut Vec<Value>, step: &'static str, argv: &[String], detail: String| {
        steps.push(json!({
            "step": step,
            "argv": argv,
            "detail": detail,
        }));
    };

    // Tokens first, before anything is staged: a missing source token file is
    // a refusal with no half-staged posture behind it.
    let tokens = if args.include_tokens {
        if !args.yes {
            return Err(AikitError::new(
                "cli.usage",
                "--include-tokens copies token files over the ssh channel; it needs --yes",
            ));
        }
        include_token_sources(&posture, &remote)?
    } else {
        Vec::new()
    };

    // 1. The target home.
    let home_out = channel.run(
        "resolve-target-home",
        &["printenv".into(), "HOME".into()],
        None,
    )?;
    let target_home = String::from_utf8_lossy(&home_out).trim().to_owned();
    if !target_home.starts_with('/') {
        return Err(step_failure(
            "resolve-target-home",
            &channel.describe(),
            format!("the target answered HOME={target_home:?}; an absolute home is required"),
        ));
    }
    record(
        &mut steps,
        "resolve-target-home",
        &["printenv".into(), "HOME".into()],
        format!("target home is {target_home}"),
    );

    // 2. The bundle into the target's state directory.
    let bundle_path = format!("{target_home}/.aikit/state/{STAGED_BUNDLE_NAME}");
    let stage_argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("umask 077; mkdir -p '{target_home}/.aikit/state' && cat > '{bundle_path}'"),
    ];
    channel.run("stage-bundle", &stage_argv, Some(&bundle))?;
    record(
        &mut steps,
        "stage-bundle",
        &stage_argv,
        format!("{} bytes staged at {bundle_path}", bundle.len()),
    );

    // 3. Token files, each its own printed step. A token lands at the
    // target's own .aikit path — the source machine's absolute path has no
    // meaning there, and receive rewrites the posture's locations to match.
    // The target gateway's own bearer token lands at the conventional
    // gateway.token path its install-service checks.
    for (what, path, bytes, target_name) in &tokens {
        let file_name = match target_name {
            Some(name) => name.clone(),
            None => path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .ok_or_else(|| {
                    step_failure(
                        "stage-token",
                        &channel.describe(),
                        format!("token path {} has no file name", path.display()),
                    )
                })?,
        };
        let (target_dir, target_path) = match target_name {
            Some(_) => (
                format!("{target_home}/.aikit"),
                format!("{target_home}/.aikit/{file_name}"),
            ),
            None => (
                format!("{target_home}/.aikit/credentials"),
                format!("{target_home}/.aikit/credentials/{file_name}"),
            ),
        };
        let token_argv = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("umask 077; mkdir -p '{target_dir}' && cat > '{target_path}'"),
        ];
        channel.run("stage-token", &token_argv, Some(bytes))?;
        record(
            &mut steps,
            "stage-token",
            &token_argv,
            format!(
                "{what}: {} bytes from {} to {}",
                bytes.len(),
                path.display(),
                target_path
            ),
        );
    }

    let gateway_token_location = format!("{target_home}/.aikit/gateway.token");
    let receive_command = if args.force {
        "aikit gateway hoist --receive --force".to_owned()
    } else {
        "aikit gateway hoist --receive".to_owned()
    };
    let install_command = format!(
        "aikit gateway install-service --ws {} --ws-token-location file:{gateway_token_location} \
         --workcell-ref {} --gateway-ref {}",
        remote.websocket_bind, posture.target_workcell_ref, posture.target_gateway_ref
    );

    // 4–5. With --yes the remote steps execute over the same channel.
    let (receive_done, install_done) = if args.yes {
        let mut receive_argv = vec![
            "aikit".to_owned(),
            "gateway".to_owned(),
            "hoist".to_owned(),
            "--receive".to_owned(),
        ];
        if args.force {
            receive_argv.push("--force".to_owned());
        }
        let out = channel.run("receive-on-target", &receive_argv, None)?;
        record(
            &mut steps,
            "receive-on-target",
            &receive_argv,
            String::from_utf8_lossy(&out).trim().to_owned(),
        );
        let install_argv = vec![
            "aikit".to_owned(),
            "gateway".to_owned(),
            "install-service".to_owned(),
            "--ws".to_owned(),
            remote.websocket_bind.clone(),
            "--ws-token-location".to_owned(),
            format!("file:{gateway_token_location}"),
            "--workcell-ref".to_owned(),
            posture.target_workcell_ref.clone(),
            "--gateway-ref".to_owned(),
            posture.target_gateway_ref.clone(),
        ];
        let out = channel.run("install-service-on-target", &install_argv, None)?;
        record(
            &mut steps,
            "install-service-on-target",
            &install_argv,
            String::from_utf8_lossy(&out).trim().to_owned(),
        );
        (true, true)
    } else {
        (false, false)
    };

    let swap_command = format!(
        "aikit gateway remote add --workcell {} --ws {} --token-location {}",
        posture.target_workcell_ref, remote.websocket_bind, remote.token_location
    );
    Ok(json!({
        "schema": "aikit.gateway-hoist-apply/v1",
        "verb": "apply",
        "channel": channel.describe(),
        "target": {
            "workcell_ref": posture.target_workcell_ref,
            "gateway_ref": posture.target_gateway_ref,
            "home": target_home,
            "bundle": bundle_path,
        },
        "steps": steps,
        "flips": {
            "receive_on_target": {
                "command": receive_command,
                "where": format!("on {}", posture.target_workcell_ref),
                "executed": receive_done,
            },
            "install_service_on_target": {
                "command": install_command,
                "where": format!("on {}", posture.target_workcell_ref),
                "executed": install_done,
                "note": "the token file must exist owner-only at the location named; stage it or re-run with --include-tokens",
            },
            "remote_add_swap": {
                "command": swap_command,
                "where": "on every machine whose gateway declarations should reach the hoisted gateway (this machine already declares it)",
                "executed": false,
                "note": "the token location is each machine's own copy of the target gateway's token",
            },
        },
        "final": format!(
            "when {} answers (`aikit gateway --at {} status`), retire the gateway here: \
             `aikit gateway uninstall-service`",
            posture.target_workcell_ref, posture.target_workcell_ref
        ),
    }))
}

/// The no-channel apply: the bundle is staged locally, and the exact operator
/// commands carry it over. Nothing reaches for a host this code was not given.
fn apply_local(home: &AikitHome, args: &HoistArgs) -> Result<Value> {
    let to = args
        .to
        .as_deref()
        .ok_or_else(|| AikitError::new("cli.usage", "apply takes --to workcell:<name>"))?;
    let remote = declared_remote(home, to)?;
    let mut posture = pack(home, args)?;
    posture.target_declared_bind = Some(remote.websocket_bind.clone());
    if args.include_tokens {
        return Err(AikitError::new(
            "cli.usage",
            "--include-tokens copies token files over the ssh channel; it needs --ssh and --yes",
        ));
    }
    let slug = workcell_slug(to)?;
    let dir = home.state().join(LOCAL_BUNDLE_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| {
        AikitError::new(
            "gateway.hoist_stage_local",
            format!("create {}: {error}", dir.display()),
        )
    })?;
    let bundle_path = dir.join(format!("{slug}.json"));
    let bundle = serde_json::to_vec_pretty(&posture).map_err(|error| {
        AikitError::new(
            "gateway.hoist_bundle_encode",
            format!("encode bundle: {error}"),
        )
    })?;
    let tmp = bundle_path.with_extension("json.tmp");
    std::fs::write(&tmp, &bundle).map_err(|error| {
        AikitError::new(
            "gateway.hoist_stage_local",
            format!("write {}: {error}", tmp.display()),
        )
    })?;
    std::fs::rename(&tmp, &bundle_path).map_err(|error| {
        AikitError::new(
            "gateway.hoist_stage_local",
            format!("write {}: {error}", bundle_path.display()),
        )
    })?;

    let receive_command = "aikit gateway hoist --receive".to_owned();
    let install_command = format!(
        "aikit gateway install-service --ws {} --ws-token-location file:$HOME/.aikit/gateway.token \
         --workcell-ref {} --gateway-ref {}",
        remote.websocket_bind, posture.target_workcell_ref, posture.target_gateway_ref
    );
    let swap_command = format!(
        "aikit gateway remote add --workcell {} --ws {} --token-location {}",
        posture.target_workcell_ref, remote.websocket_bind, remote.token_location
    );
    Ok(json!({
        "schema": "aikit.gateway-hoist-apply/v1",
        "verb": "apply",
        "channel": "local staging (no --ssh given; nothing left this machine)",
        "target": {
            "workcell_ref": posture.target_workcell_ref,
            "gateway_ref": posture.target_gateway_ref,
            "bundle": bundle_path.display().to_string(),
        },
        "steps": [ { "step": "stage-bundle", "detail": format!("{} bytes staged at {}", bundle.len(), bundle_path.display()) } ],
        "carry_over": [
            "ssh TARGET 'mkdir -p ~/.aikit/state'".to_owned(),
            format!("scp {} TARGET:.aikit/state/{STAGED_BUNDLE_NAME}", bundle_path.display()),
        ],
        "flips": {
            "receive_on_target": {
                "command": receive_command,
                "where": format!("on {}", posture.target_workcell_ref),
                "executed": false,
            },
            "install_service_on_target": {
                "command": install_command,
                "where": format!("on {}", posture.target_workcell_ref),
                "executed": false,
                "note": "the token file must exist owner-only at the location named; stage it by hand",
            },
            "remote_add_swap": {
                "command": swap_command,
                "where": "on every machine whose gateway declarations should reach the hoisted gateway (this machine already declares it)",
                "executed": false,
            },
        },
        "final": format!(
            "when {} answers (`aikit gateway --at {} status`), retire the gateway here: \
             `aikit gateway uninstall-service`",
            posture.target_workcell_ref, posture.target_workcell_ref
        ),
    }))
}

// ---------------------------------------------------------------------------
// receive
// ---------------------------------------------------------------------------

/// Unpack a staged bundle into this home. Existing posture is never clobbered
/// without --force; the state file is written under the gateway state lock,
/// so a running service is a refusal, not a race.
pub fn receive(home: &AikitHome, force: bool) -> Result<Value> {
    let staged = home.state().join(STAGED_BUNDLE_NAME);
    let bytes = std::fs::read(&staged).map_err(|error| {
        three_part(
            "gateway.hoist_receive_no_bundle",
            format!(
                "No hoist bundle is staged at {} ({error}).",
                staged.display()
            ),
            "Nothing was unpacked; this home's posture is unchanged.",
            format!(
                "Stage one from the source machine (`aikit gateway hoist --to <workcell> --apply \
                 --ssh user@host`, or --apply and carry the bundle to {}).",
                staged.display()
            ),
        )
    })?;
    let posture: PackedPosture = serde_json::from_slice(&bytes).map_err(|error| {
        AikitError::new(
            "gateway.hoist_bundle_invalid",
            format!(
                "{} is not a valid {GATEWAY_HOIST_SCHEMA} bundle: {error}",
                staged.display()
            ),
        )
    })?;
    if posture.schema != GATEWAY_HOIST_SCHEMA {
        return Err(AikitError::new(
            "gateway.hoist_bundle_invalid",
            format!("{} has schema {}", staged.display(), posture.schema),
        ));
    }

    // Clobber law: existing posture is an owner fact, not an obstacle.
    let existing = existing_posture(home)?;
    if !existing.is_empty() && !force {
        return Err(three_part(
            "gateway.hoist_receive_clobber",
            format!(
                "This home already holds gateway posture: {}.",
                existing.join("; ")
            ),
            "Nothing was unpacked.",
            "Uninstall or clear it first, or receive over it with --force.",
        ));
    }

    // The kernel's own restore law validates identity and sequence before
    // anything is written.
    let gateway = AgencyGateway::from_snapshot(posture.gateway_state.clone())?;

    let state_file = home.gateway_state();
    let _lock = acquire_gateway_state_lock(
        &state_file,
        RECEIVE_LOCK_TIMEOUT,
        "gateway hoist receive",
    )
    .map_err(|error| {
        three_part(
            "gateway.hoist_receive_state_held",
            format!("The gateway state {} cannot be taken: {error}.", state_file.display()),
            "Nothing was unpacked; a running gateway must not be written around.",
            "Stop the gateway service on this machine (`aikit gateway uninstall-service`), then receive.",
        )
    })?;

    write_atomic(
        &state_file,
        &serde_json::to_vec_pretty(&gateway.snapshot())
            .map_err(|error| AikitError::new("gateway.hoist_receive_write", error.to_string()))?,
    )?;

    let mut landed = vec![format!(
        "gateway state {} ({} bindings, {} stream journals, {} Communiques) as {}",
        state_file.display(),
        posture.gateway_state.bindings.len(),
        posture.gateway_state.streams.len(),
        posture.gateway_state.communiques.len(),
        posture.target_gateway_ref,
    )];

    if !posture.connectors.is_empty() {
        let connectors_path = crate::gateway_connectors::connectors_path(home);
        // Token locations are material facts: an .aikit-relative file:
        // location re-resolves to this home, so the declared posture lands
        // valid here instead of inheriting the source machine's paths.
        let target_home = home.root().display().to_string();
        let mut connectors = posture.connectors.clone();
        let mut relocations: Vec<String> = Vec::new();
        for entry in &mut connectors {
            if let Some(location) = &entry.token_location {
                if let Some(rest) = location.strip_prefix("file:") {
                    if let Some(index) = rest.find("/.aikit/") {
                        let after = &rest[index + "/.aikit/".len()..];
                        let relocated = format!("file:{target_home}/.aikit/{after}");
                        if relocated != *location {
                            relocations.push(format!("{} -> {}", location, relocated));
                            entry.token_location = Some(relocated);
                        }
                    }
                }
            }
        }
        store_gateway_connectors(
            &connectors_path,
            &GatewayConnectorsFile {
                schema: GATEWAY_CONNECTORS_SCHEMA.into(),
                connectors,
            },
        )?;
        landed.push(format!(
            "{} connector declarations at {}",
            posture.connectors.len(),
            connectors_path.display()
        ));
        for relocation in relocations {
            landed.push(format!("token location re-resolved: {}", relocation));
        }
    }
    if let Some(document) = &posture.coexistence {
        let path = crate::gateway_ops::coexistence_path(home);
        store_coexistence(&path, document)?;
        landed.push(format!(
            "coexistence policy {} at {}",
            document.policy.as_str(),
            path.display()
        ));
    }
    if !posture.agent_providers.is_empty() {
        let root = home.state().join("encounter-providers");
        std::fs::create_dir_all(&root).map_err(|error| {
            AikitError::new(
                "gateway.hoist_receive_write",
                format!("create {}: {error}", root.display()),
            )
        })?;
        for provider in &posture.agent_providers {
            let id = provider
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AikitError::new(
                        "gateway.hoist_bundle_invalid",
                        "a packed agent provider names no id",
                    )
                })?
                .to_owned();
            let path = root.join(format!("{id}.json"));
            write_atomic(
                &path,
                &serde_json::to_vec_pretty(provider).map_err(|error| {
                    AikitError::new("gateway.hoist_receive_write", error.to_string())
                })?,
            )?;
            landed.push(format!("agent provider {id} at {}", path.display()));
        }
    }

    std::fs::remove_file(&staged).map_err(|error| {
        AikitError::new(
            "gateway.hoist_receive_write",
            format!("remove staged bundle {}: {error}", staged.display()),
        )
    })?;

    let install_hint = match &posture.target_declared_bind {
        Some(bind) => format!(
            "aikit gateway install-service --ws {bind} --ws-token-location \
             file:$HOME/.aikit/gateway.token --workcell-ref {} --gateway-ref {}",
            posture.target_workcell_ref, posture.target_gateway_ref
        ),
        None => format!(
            "aikit gateway install-service --ws HOST:PORT --ws-token-location \
             file:$HOME/.aikit/gateway.token --workcell-ref {} --gateway-ref {}",
            posture.target_workcell_ref, posture.target_gateway_ref
        ),
    };
    Ok(json!({
        "schema": "aikit.gateway-hoist-receive/v1",
        "verb": "receive",
        "target": {
            "workcell_ref": posture.target_workcell_ref,
            "gateway_ref": posture.target_gateway_ref,
        },
        "landed": landed,
        "kept": "binding refs, stream refs and journal sequence, and every Communique ref \
                 arrived exactly as they were sent"
            .to_owned(),
        "next": install_hint,
    }))
}

/// What gateway posture this home already holds, named item by item.
fn existing_posture(home: &AikitHome) -> Result<Vec<String>> {
    let mut existing = Vec::new();
    let connectors = load_gateway_connectors(&crate::gateway_connectors::connectors_path(home))?;
    if !connectors.connectors.is_empty() {
        existing.push(format!(
            "{} connector declarations",
            connectors.connectors.len()
        ));
    }
    let state_file = home.gateway_state();
    if state_file.exists() {
        let snapshot: GatewaySnapshot =
            serde_json::from_slice(&std::fs::read(&state_file).map_err(|error| {
                AikitError::new(
                    "gateway.hoist_receive_write",
                    format!("read {}: {error}", state_file.display()),
                )
            })?)
            .map_err(|error| {
                AikitError::new(
                    "gateway.hoist_receive_write",
                    format!("decode {}: {error}", state_file.display()),
                )
            })?;
        let held = snapshot.bindings.len()
            + snapshot.streams.len()
            + snapshot.communiques.len()
            + snapshot.pending_deliveries.len()
            + snapshot.connectors.len();
        if held > 0 {
            existing.push(format!(
                "gateway state holding {held} semantic records ({})",
                state_file.display()
            ));
        }
    }
    if crate::gateway_ops::coexistence_path(home).exists() {
        existing.push("a coexistence document".into());
    }
    let providers = home.state().join("encounter-providers");
    if providers.exists()
        && std::fs::read_dir(&providers)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false)
    {
        existing.push(format!("agent providers in {}", providers.display()));
    }
    Ok(existing)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "gateway.hoist_receive_write",
                format!("create {}: {error}", parent.display()),
            )
        })?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|error| {
        AikitError::new(
            "gateway.hoist_receive_write",
            format!("write {}: {error}", tmp.display()),
        )
    })?;
    std::fs::rename(&tmp, path).map_err(|error| {
        AikitError::new(
            "gateway.hoist_receive_write",
            format!("write {}: {error}", path.display()),
        )
    })
}

// ---------------------------------------------------------------------------
// The verb.
// ---------------------------------------------------------------------------

/// `aikit gateway hoist` — plan, apply or receive, refused honestly when the
/// flags contradict each other.
pub fn hoist_command(home: &AikitHome, args: &HoistArgs) -> Result<Value> {
    if args.receive {
        if args.to.is_some() || args.apply || args.yes || args.ssh.is_some() || args.include_tokens
        {
            return Err(AikitError::new(
                "cli.usage",
                "--receive unpacks a staged bundle here; it takes no --to, --apply, --ssh, \
                 --yes or --include-tokens",
            ));
        }
        return receive(home, args.force);
    }
    let posture = pack(home, args)?;
    if !args.apply {
        if args.yes || args.ssh.is_some() || args.include_tokens {
            return Err(AikitError::new(
                "cli.usage",
                "--ssh, --yes and --include-tokens act when --apply stages; a plan only prints",
            ));
        }
        return plan_value(&posture);
    }
    apply(home, args)
}
