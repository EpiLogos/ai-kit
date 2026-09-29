//! The per-project agency mint.
//!
//! Until now every ACP chat session reused one hand-authored agency source
//! (`Work/O-I/.aikit/sf6-agency/agency-request.json`), which the disclosure
//! named as a limitation. This module derives a *fresh* native chain per
//! project + agent from that standing template and runs the real native owner
//! (`actuation agency actualise`) against it, through the same
//! [`CommandRunner`](aikit_adapters::runner::CommandRunner) seam
//! [`admit_agency`](aikit_adapters::agency_admission::admit_agency) uses.
//!
//! Derivation law (what is minted vs what is carried):
//!
//! * Carried verbatim from the owner's standing template — never invented
//!   here: `requester_ref`, `governing_binding`, `metagency_grant`, the
//!   determination's `bounds_refs` and `authority_refs`, the delegated
//!   autonomy's denials, and the `scope_ref` of the child binding. These are
//!   the owner's standing authority, not the mint's.
//! * Minted freshly per (agent, project), deterministically (the same
//!   agent + project always derives byte-identical request text, so the
//!   persisted source is content-addressed and re-mints are honest replays):
//!   `request_ref`, `determination_ref`, `differentiated_agency_ref`, the
//!   child `binding_ref`, `return_relation_ref`, `continuity_ref`,
//!   `agent_identity` evidence and provenance source refs.
//! * The child `world_ref` is the canonical Project identity resolved from
//!   `--project-cwd` through the same `session-space project-context`
//!   resolution path; it is never inferred from the directory name here.
//! * The delegated autonomy's allowed Actions are the template's allowed
//!   refs *plus* the two Actions an ACP chat session cannot work without
//!   (`action/aikit/encounter-send`, `action/aikit/model-realise`) — the
//!   native admission refuses a binding that cannot send.
//!
//! Agent identity resolution order: explicit `--agent-ref`, else the Central
//! AgentProfile for the project scope when exactly one exists, else a derived,
//! clearly-attributed `agent/<project-slug>-chat`. A human `requester_ref` is
//! never fabricated: it is always the template's.
use super::{read_binding, EncounterAgencyBinding};
use crate::app::Service;
use aikit_adapters::{
    agency_admission::{admit_agency, AgencySourceBasis, AGENCY_ACTUALISATION_SCHEMA},
    runner::{CommandRunner, SystemRunner},
};
use aikit_core::{AikitError, ProjectRef, ResourceRef, Result, SourceRevision};
use aikit_store::AikitHome;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The two Actions a chat session's determination must delegate; the native
/// encounter admission refuses send without the first and model dispatch
/// without the second.
pub const REQUIRED_MINTED_ACTIONS: [&str; 2] =
    ["action/aikit/encounter-send", "action/aikit/model-realise"];

/// Owner override for the standing template; without it the template is read
/// from the Central root above the project (`Work/O-I/.aikit/sf6-agency/agency-request.json`).
pub const TEMPLATE_ENV: &str = "AIKIT_AGENCY_MINT_TEMPLATE";

/// The minted request document, derived from the standing template.
///
/// `tag` is the deterministic per-(agent, project) suffix shared by every
/// freshly minted ref.
pub fn mint_request_document(
    template: &Value,
    agent_ref: &str,
    world_ref: &str,
) -> Result<(Value, String)> {
    if template["schema"] != AGENCY_ACTUALISATION_SCHEMA {
        return Err(mint_error(
            "agency_mint.template_invalid",
            format!(
                "the standing template must declare {AGENCY_ACTUALISATION_SCHEMA}, found {}",
                template["schema"]
            ),
        ));
    }
    let requester_ref = template["requester_ref"].as_str().ok_or_else(|| {
        mint_error(
            "agency_mint.template_invalid",
            "the standing template has no requester_ref; the mint never invents one",
        )
    })?;
    let governing = &template["governing_binding"];
    if !governing.is_object() {
        return Err(mint_error(
            "agency_mint.template_invalid",
            "the standing template has no governing_binding to carry",
        ));
    }
    let grant = &template["metagency_grant"];
    if !grant.is_object() {
        return Err(mint_error(
            "agency_mint.template_invalid",
            "the standing template has no metagency_grant to carry",
        ));
    }
    let template_determination = &template["determination"];
    if template_determination["kind"] != "delegation" {
        return Err(mint_error(
            "agency_mint.template_invalid",
            format!(
                "the mint derives delegation chains; the template's determination kind is {}",
                template_determination["kind"]
            ),
        ));
    }
    let child_scope = template["differentiated_binding"]["scope_ref"]
        .as_str()
        .unwrap_or("scope:root");
    let determining_agency = governing["agency_ref"]
        .as_str()
        .ok_or_else(|| {
            mint_error(
                "agency_mint.template_invalid",
                "the template's governing_binding names no agency_ref",
            )
        })?
        .to_owned();

    let tag = mint_tag(agent_ref, world_ref);
    // Bounds and authority are the owner's standing structure, carried
    // verbatim from the template's own determination.
    let bounds_refs = template_determination["bounds_refs"].clone();
    let authority_refs = template_determination["authority_refs"].clone();
    let denied = template_determination["delegated_autonomy"]["denied_action_refs"].clone();
    let may_determine = template_determination["delegated_autonomy"]["may_determine_within_bounds"]
        .as_bool()
        .unwrap_or(false);
    let mut allowed: Vec<Value> = REQUIRED_MINTED_ACTIONS
        .iter()
        .map(|action| json!(action))
        .collect();
    if let Some(template_allowed) =
        template_determination["delegated_autonomy"]["allowed_action_refs"].as_array()
    {
        for action in template_allowed {
            if !allowed.contains(action) {
                allowed.push(action.clone());
            }
        }
    }
    let mode = template_determination["return_policy"]["mode"]
        .as_str()
        .unwrap_or("required");
    let continuity_ref = format!("continuity:{agent_ref}");

    let request = json!({
        "schema": AGENCY_ACTUALISATION_SCHEMA,
        "request_ref": format!("actualisation-request:aikit-mint-{tag}"),
        "requester_ref": requester_ref,
        "governing_binding": governing,
        "metagency_grant": grant,
        "determination": {
            "schema": "actuation.agency/v1",
            "determination_ref": format!("determination:aikit-mint-{tag}"),
            "kind": "delegation",
            "determining_agency_ref": determining_agency,
            "differentiated_agency_ref": format!("agency:aikit-mint-{tag}"),
            "world_binding_ref": format!("binding:aikit-mint-{tag}"),
            "bounds_refs": bounds_refs,
            "authority_refs": authority_refs,
            "delegated_autonomy": {
                "allowed_action_refs": allowed,
                "denied_action_refs": denied,
                "may_determine_within_bounds": may_determine,
            },
            "return_policy": {
                "mode": mode,
                "return_relation_ref": format!("return-relation:aikit-mint-{tag}"),
            },
        },
        "differentiated_binding": {
            "schema": "actuation.agency/v1",
            "binding_ref": format!("binding:aikit-mint-{tag}"),
            "agent_ref": agent_ref,
            "agency_ref": format!("agency:aikit-mint-{tag}"),
            "world_ref": world_ref,
            "scope_ref": child_scope,
            "determining_agency_ref": determining_agency,
            "bounds_refs": bounds_refs,
            "authority_refs": authority_refs,
            "return_relation_ref": format!("return-relation:aikit-mint-{tag}"),
            "continuity_ref": continuity_ref,
        },
        "agent_identity": {
            "standing": "existing",
            "evidence_refs": [format!("evidence/aikit-agency-mint/{tag}")],
        },
        "provenance": {
            "source_refs": [format!("source/aikit-agency-mint/{tag}")],
            "context_refs": [],
        },
    });
    Ok((request, tag))
}

/// Resolve the project identity from `project_cwd` through the same
/// application resolution `aikit session-space project-context` uses.
fn resolve_project(home: &AikitHome, project_cwd: &Path) -> Result<ProjectRef> {
    let project_service = Service::open(home.clone(), project_cwd, |key| std::env::var(key).ok())?;
    let resolution = aikit_tui::project_world_service::context_resolution(&project_service)
        .map_err(|error| {
            mint_error(
                "agency_mint.project_unresolved",
                format!(
                    "could not resolve the canonical project for {} (the same resolution \
                     `session-space project-context` applies): {}",
                    project_cwd.display(),
                    error.message()
                ),
            )
        })?;
    Ok(resolution.project_binding.project)
}

/// Where the owner's standing template lives: the explicit override, else the
/// sf6 chain inside the Central root above the project.
fn locate_template(project_cwd: &Path) -> Result<PathBuf> {
    if let Some(override_path) = std::env::var_os(TEMPLATE_ENV) {
        return Ok(PathBuf::from(override_path));
    }
    let central_root = project_cwd
        .ancestors()
        .find(|candidate| candidate.join("Control").is_dir() && candidate.join("Work").is_dir());
    let Some(central_root) = central_root else {
        return Err(mint_error(
            "agency_mint.template_missing",
            format!(
                "no Central root (Control/ + Work/) above {}; cannot locate the standing \
                 agency template. Set {TEMPLATE_ENV} to the owner's \
                 actuation.agency-actualisation/v1 request",
                project_cwd.display()
            ),
        ));
    };
    Ok(central_root
        .join("Work")
        .join("O-I")
        .join(".aikit")
        .join("sf6-agency")
        .join("agency-request.json"))
}

fn load_template(project_cwd: &Path) -> Result<Value> {
    let path = locate_template(project_cwd)?;
    let bytes = std::fs::read(&path).map_err(|error| {
        mint_error(
            "agency_mint.template_missing",
            format!(
                "the standing agency template is not readable at {}: {error}; set \
                 {TEMPLATE_ENV} to the owner's actualisation request",
                path.display()
            ),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        mint_error(
            "agency_mint.template_invalid",
            format!(
                "the standing agency template at {} is invalid: {error}",
                path.display()
            ),
        )
    })
}

/// The agent identity: explicit ref wins; else exactly one Central
/// AgentProfile in the project's scope; else a derived, clearly-attributed
/// project agent. The human requester is never fabricated here.
fn resolve_agent_identity(
    runner: &dyn CommandRunner,
    project_cwd: &Path,
    world: &ProjectRef,
    explicit: Option<ResourceRef>,
) -> Result<(ResourceRef, &'static str)> {
    if let Some(agent_ref) = explicit {
        return Ok((agent_ref, "explicit"));
    }
    let derived = |world: &ProjectRef| -> Result<(ResourceRef, &'static str)> {
        Ok((
            ResourceRef::parse(format!("agent/{}-chat", project_slug(world.as_str())))?,
            "derived-project-agent",
        ))
    };
    if let Some(declared) = project_scope_agent_profile(runner, project_cwd)? {
        return match declared.len() {
            // An empty but successful lookup declares no agent: the derived
            // project agent is the honest fallback, not an ambiguity.
            0 => derived(world),
            1 => Ok((
                ResourceRef::parse(declared.first().expect("exactly one"))?,
                "central-agent-profile",
            )),
            _ => Err(mint_error(
                "agency_mint.agent_ambiguous",
                format!(
                    "the Central project scope declares {} AgentProfiles ({}); pass \
                     --agent-ref to choose one",
                    declared.len(),
                    declared.join(", ")
                ),
            )),
        };
    }
    derived(world)
}

/// Ask Central's `agent-profile.list` for the project scope. `None` means the
/// lookup is unavailable (no Central root, no ctrl, refusing scope) — an
/// absence, never an invented agent.
fn project_scope_agent_profile(
    runner: &dyn CommandRunner,
    project_cwd: &Path,
) -> Result<Option<Vec<String>>> {
    let Some(central_root) = project_cwd
        .ancestors()
        .find(|candidate| candidate.join("Control").is_dir() && candidate.join("Work").is_dir())
    else {
        return Ok(None);
    };
    let project = project_cwd
        .canonicalize()
        .ok()
        .zip(central_root.join("Work").canonicalize().ok())
        .and_then(|(cwd, work)| cwd.strip_prefix(work).ok().map(|p| p.to_path_buf()))
        .map(|relative| relative.to_string_lossy().into_owned());
    let Some(project) = project else {
        return Ok(None);
    };
    let executable = std::env::var_os("CENTRAL_CTRL_BIN")
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"));
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        central_root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "agent-profile.list".into(),
        json!({"scope":"project","project":project}).to_string(),
    ];
    let output = match runner.run(&argv) {
        Ok(output) if output.status == 0 => output,
        // A refusing or missing ctrl is a disclosed absence: the mint falls
        // back to the derived project agent rather than guessing a profile.
        Ok(_) | Err(_) => return Ok(None),
    };
    let envelope: Value = match serde_json::from_str(&output.stdout) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(None),
    };
    if envelope["ok"] != true {
        return Ok(None);
    }
    let Some(profiles) = envelope["data"]["profiles"].as_array() else {
        return Ok(None);
    };
    let mut agent_refs = BTreeSet::new();
    for entry in profiles {
        if let Some(agent_ref) = entry["profile"]["agent_ref"].as_str() {
            agent_refs.insert(agent_ref.to_owned());
        }
    }
    Ok(Some(agent_refs.into_iter().collect()))
}

/// `project:Factory` and `central/native-context` become `factory` and
/// `central-native-context`: a lowercase, slug-safe attribution token.
fn project_slug(world_ref: &str) -> String {
    let base = world_ref.strip_prefix("project:").unwrap_or(world_ref);
    let mut slug = String::new();
    for character in base.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("project");
    }
    slug
}

/// Deterministic per-(agent, project) tag shared by every freshly minted ref:
/// readable slug plus a short content digest, so distinct agents or worlds
/// never collide and re-mints reproduce the exact same request bytes.
fn mint_tag(agent_ref: &str, world_ref: &str) -> String {
    let basis = format!("{agent_ref}\u{0}{world_ref}");
    let digest = blake3::hash(basis.as_bytes()).to_hex();
    format!("{}-{}", project_slug(world_ref), &digest[..8])
}

/// Run the real native actualise. On refusal the owner's own error text is
/// surfaced verbatim: this verb is an owner-side operation whose diagnostics
/// are the owner's terminal, not a shared surface.
fn run_actualise(
    runner: &dyn CommandRunner,
    actuation_bin: &Path,
    source_path: &Path,
) -> Result<Value> {
    let argv = vec![
        actuation_bin.to_string_lossy().into_owned(),
        "agency".into(),
        "actualise".into(),
        source_path.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    let output = runner.run(&argv)?;
    if output.status != 0 {
        let owner_text = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        return Err(AikitError::new(
            "agency_mint.refused",
            format!(
                "Actuation refused the minted agency request (exit {}): {owner_text}",
                output.status
            ),
        ));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|error| mint_error("agency_mint.receipt_invalid", error.to_string()))
}

/// Resolve the Actuation executable the way every other AIKit intake does:
/// `actuation` on PATH, pinned to its absolute location for the persisted
/// binding.
fn resolve_actuation_bin() -> PathBuf {
    let name = "actuation";
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

fn fresh_revision() -> Result<SourceRevision> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| mint_error("agency_mint.clock", error.to_string()))?;
    let nonce = blake3::hash(&now.as_nanos().to_le_bytes()).to_hex();
    SourceRevision::parse(format!("rev/aikit-mint-{}-{}", now.as_secs(), &nonce[..4]))
}

fn mint_error(code: &'static str, message: impl std::fmt::Display) -> AikitError {
    AikitError::new(code, message.to_string())
}

/// Mint the per-project agency chain and provision the session's binding in
/// one owner operation. Returns the verb's success JSON.
pub fn mint_per_project_agency(
    home: &AikitHome,
    project_cwd: &Path,
    session: &ResourceRef,
    explicit_agent_ref: Option<ResourceRef>,
    actuation_bin: &Path,
    runner: &dyn CommandRunner,
) -> Result<Value> {
    mint_per_project_agency_with_intent(
        home,
        project_cwd,
        session,
        explicit_agent_ref,
        actuation_bin,
        runner,
        false,
    )
}
fn mint_per_project_agency_with_intent(
    home: &AikitHome,
    project_cwd: &Path,
    session: &ResourceRef,
    explicit_agent_ref: Option<ResourceRef>,
    actuation_bin: &Path,
    runner: &dyn CommandRunner,
    for_task: bool,
) -> Result<Value> {
    if !session.as_str().starts_with("agent-session/") {
        return Err(mint_error(
            "agency_mint.session_invalid",
            format!(
                "{} is not a canonical agent-session/ ref; the mint provisions a session \
                 binding, not a display name",
                session.as_str()
            ),
        ));
    }
    let world = resolve_project(home, project_cwd)?;
    let world_ref = ResourceRef::parse(world.as_str())?;
    let (agent_ref, identity_source) =
        resolve_agent_identity(runner, project_cwd, &world, explicit_agent_ref)?;

    let template = load_template(project_cwd)?;
    let (mut request, tag) =
        mint_request_document(&template, agent_ref.as_str(), world_ref.as_str())?;
    if for_task {
        let allowed = request["determination"]["delegated_autonomy"]["allowed_action_refs"]
            .as_array_mut()
            .ok_or_else(|| {
                mint_error(
                    "agency_mint.actions",
                    "Native determination actions are absent",
                )
            })?;
        let action = json!("action/aikit/encounter-task");
        if !allowed.contains(&action) {
            allowed.push(action);
        }
        // This is a requested determination. Actualise below still decides
        // against the owner's unchanged grant and bounds.
    }

    // Content-addressed persistence: the same agent + project derives
    // byte-identical request text, so re-mints land on the same source.
    let bytes = serde_json::to_vec_pretty(&request)
        .map_err(|error| mint_error("agency_mint.encode", error.to_string()))?;
    let digest = blake3::hash(&bytes).to_hex();
    let directory = home.state().join("agency-mints");
    std::fs::create_dir_all(&directory).map_err(super::error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(super::error)?;
    }
    let source_path = directory.join(format!("{digest}.json"));
    if std::fs::read(&source_path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&source_path, &bytes).map_err(super::error)?;
    }
    let source_path = source_path.canonicalize().map_err(super::error)?;

    // The real native owner evaluates the minted chain first, so a refusal
    // reaches the owner in Actuation's own words.
    let receipt = run_actualise(runner, actuation_bin, &source_path)?;
    let basis = AgencySourceBasis {
        source_ref: ResourceRef::parse(format!("source/aikit-agency-mint/{tag}"))?,
        revision: SourceRevision::parse(format!("rev/{}", &digest[..16]))?,
        path: source_path.clone(),
        content_digest: format!("blake3:{digest}"),
    };
    // The canonical admission seam re-runs and validates the receipt against
    // the request exactly as every later encounter admission will.
    let admitted = admit_agency(
        runner,
        actuation_bin.to_string_lossy().as_ref(),
        &basis,
        &agent_ref,
        &world_ref,
    )?;

    // CAS law: a fresh session has no expectation; an existing binding bumps
    // from its current revision. The agent identity of an existing session is
    // immutable below this point (configure_agency refuses the change).
    let current = read_binding(home, session)?;
    let expected_revision = current.as_ref().map(|existing| existing.revision.clone());
    let binding = EncounterAgencyBinding {
        revision: fresh_revision()?,
        active: true,
        agent_ref: agent_ref.clone(),
        agency_ref: admitted.agency_ref.clone(),
        world_ref: world_ref.clone(),
        world_binding_ref: admitted.world_binding_ref.clone(),
        agency_source: basis,
        actuation_bin: actuation_bin.to_path_buf(),
        allowed_senders: [ResourceRef::parse(
            request["requester_ref"]
                .as_str()
                .expect("template requester"),
        )?]
        .into(),
        allowed_packet_sources: BTreeSet::new(),
        context: None,
    };
    super::EncounterService::configure_agency(home, session, &binding, expected_revision.as_ref())?;

    Ok(json!({
        "ok": true,
        "data": {
            "configured": true,
            "standing": "minted-per-project-agency",
            "agent_ref": agent_ref.as_str(),
            "world_ref": world_ref.as_str(),
            "source_ref": binding.agency_source.source_ref.as_str(),
            "revision": binding.revision.as_str(),
            "agency_ref": admitted.agency_ref.as_str(),
            "world_binding_ref": admitted.world_binding_ref.as_str(),
            "agency_source": &binding.agency_source,
            "agency_admission": &admitted,
            "receipt_ref": receipt["receipt_ref"],
            "source_path": source_path.display().to_string(),
            "agent_identity_source": identity_source,
            "idempotency": "re-minting the same session+project re-runs actualise and CAS-bumps the revision; the request bytes and source path are deterministic per agent+project",
        }
    }))
}

/// The CLI entry: resolve `actuation` on PATH and run the mint with the real
/// system runner.
pub fn mint_from_cli(
    home: &AikitHome,
    project_cwd: &Path,
    session: &ResourceRef,
    explicit_agent_ref: Option<ResourceRef>,
) -> Result<Value> {
    mint_per_project_agency(
        home,
        project_cwd,
        session,
        explicit_agent_ref,
        &resolve_actuation_bin(),
        &SystemRunner::new(),
    )
}

/// Explicit task intent; ordinary chat minting retains its existing Actions.
pub fn mint_task_from_cli(
    home: &AikitHome,
    project_cwd: &Path,
    session: &ResourceRef,
    agent: Option<ResourceRef>,
) -> Result<Value> {
    mint_per_project_agency_with_intent(
        home,
        project_cwd,
        session,
        agent,
        &resolve_actuation_bin(),
        &SystemRunner::new(),
        true,
    )
}
