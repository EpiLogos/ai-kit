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
//! Agent identity resolution order: explicit `--agent-ref` (pinned to its
//! scope's Central AgentProfile when that scope — the root or a Project —
//! declares one for it), else the Central AgentProfile for the project scope
//! when exactly one exists, else a derived, clearly-attributed
//! `agent/<project-slug>-chat`. A human `requester_ref` is never fabricated:
//! it is always the template's.
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
    collections::{BTreeMap, BTreeSet},
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
/// The named Agent, when the caller names one, is the Agent the context is
/// composed for: a ground with several profiles resolves without a guess.
fn resolve_project(
    home: &AikitHome,
    project_cwd: &Path,
    agent: Option<&ResourceRef>,
) -> Result<ProjectRef> {
    let project_service = Service::open(home.clone(), project_cwd, |key| std::env::var(key).ok())?;
    let project_service = match agent {
        Some(agent) => project_service.with_selected_agent(agent.clone()),
        None => project_service,
    };
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

/// The agent identity the mint provisions with: the resolved ref, the
/// resolution source's own attribution, and — when the identity came from an
/// accepted Central AgentProfile — the exact profile basis (its ref and the
/// Central world it was read against) the minted binding carries so later
/// turns can compose the accepted self orientation.
struct ResolvedAgentIdentity {
    agent_ref: ResourceRef,
    source: &'static str,
    central: Option<CentralProfileBasis>,
}
struct CentralProfileBasis {
    profile_ref: String,
    central_root: PathBuf,
    /// `root` for the Central root itself, `project` for a Work member.
    scope: &'static str,
    project: Option<String>,
}

/// The agent identity: explicit ref wins; else exactly one Central
/// AgentProfile in the project's scope; else a derived, clearly-attributed
/// project agent. The human requester is never fabricated here.
fn resolve_agent_identity(
    runner: &dyn CommandRunner,
    project_cwd: &Path,
    world: &ProjectRef,
    explicit: Option<ResourceRef>,
) -> Result<ResolvedAgentIdentity> {
    if let Some(agent_ref) = explicit {
        // A named Agent is still a Central Agent: its scope's profile for it
        // is what lets every turn say who it is. None declared, or one the
        // person has not accepted, is an honest absence (every turn would
        // otherwise refuse on the unaccepted definition); two for one Agent
        // cannot be chosen between.
        let central = match scope_profiles(runner, project_cwd, true)? {
            Some(declared) => match declared.agents.get(agent_ref.as_str()) {
                Some(profile_refs) if profile_refs.len() > 1 => {
                    let named: Vec<&str> = profile_refs.iter().map(String::as_str).collect();
                    return Err(mint_error(
                        "agency_mint.agent_ambiguous",
                        format!(
                            "the Central {} scope declares {} profiles for {agent_ref} ({}); \
                             remove all but one before opening a conversation with it",
                            declared.scope,
                            named.len(),
                            named.join(", ")
                        ),
                    ));
                }
                Some(profile_refs) => profile_refs
                    .iter()
                    .next()
                    .filter(|profile_ref| !profile_ref.is_empty())
                    .map(|profile_ref| CentralProfileBasis {
                        profile_ref: profile_ref.clone(),
                        central_root: declared.central_root.clone(),
                        scope: declared.scope,
                        project: declared.project.clone(),
                    })
                    .filter(|basis| profile_accepted(runner, basis)),
                None => None,
            },
            None => None,
        };
        return Ok(ResolvedAgentIdentity {
            agent_ref,
            source: if central.is_some() {
                "explicit-central-agent-profile"
            } else {
                "explicit"
            },
            central,
        });
    }
    let derived = |world: &ProjectRef| -> Result<ResolvedAgentIdentity> {
        Ok(ResolvedAgentIdentity {
            agent_ref: ResourceRef::parse(format!("agent/{}-chat", project_slug(world.as_str())))?,
            source: "derived-project-agent",
            central: None,
        })
    };
    // With no Agent named, only a Project scope's sole profile is taken as
    // the conversation's Agent; the root keeps its derived default.
    let Some(declared) = scope_profiles(runner, project_cwd, false)? else {
        return derived(world);
    };
    match declared.agents.len() {
        // An empty but successful lookup declares no agent: the derived
        // project agent is the honest fallback, not an ambiguity.
        0 => derived(world),
        1 => {
            let (agent_ref, profile_refs) =
                declared.agents.into_iter().next().expect("exactly one");
            if profile_refs.len() > 1 {
                let named: Vec<String> = profile_refs.into_iter().collect();
                return Err(mint_error(
                    "agency_mint.agent_ambiguous",
                    format!(
                        "the Central project scope declares {} profiles for {agent_ref} ({}); \
                         pass --agent-ref to choose one",
                        named.len(),
                        named.join(", ")
                    ),
                ));
            }
            let profile_ref = profile_refs.into_iter().next().expect("exactly one");
            Ok(ResolvedAgentIdentity {
                agent_ref: ResourceRef::parse(&agent_ref)?,
                source: "central-agent-profile",
                // A listing record without a ref cannot be re-read later: the
                // identity is still the profile's, but nothing is invented.
                central: (!profile_ref.is_empty()).then_some(CentralProfileBasis {
                    profile_ref,
                    central_root: declared.central_root,
                    scope: declared.scope,
                    project: declared.project,
                }),
            })
        }
        _ => {
            let named: Vec<String> = declared.agents.keys().cloned().collect();
            Err(mint_error(
                "agency_mint.agent_ambiguous",
                format!(
                    "the Central project scope declares {} AgentProfiles ({}); pass \
                     --agent-ref to choose one",
                    named.len(),
                    named.join(", ")
                ),
            ))
        }
    }
}

/// Whether Central's `agent-profile.review` reads this exact profile as
/// accepted by the person. Any refusal or unreadable answer is `false`.
fn profile_accepted(runner: &dyn CommandRunner, basis: &CentralProfileBasis) -> bool {
    let mut input = json!({"scope": basis.scope, "profile_ref": basis.profile_ref});
    if let Some(project) = &basis.project {
        input["project"] = json!(project);
    }
    let argv = vec![
        ctrl_executable().to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        basis.central_root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "agent-profile.review".into(),
        input.to_string(),
    ];
    match runner.run(&argv) {
        Ok(output) if output.status == 0 => serde_json::from_str::<Value>(&output.stdout)
            .is_ok_and(|envelope| envelope["ok"] == true && envelope["data"]["accepted"] == true),
        Ok(_) | Err(_) => false,
    }
}

fn ctrl_executable() -> PathBuf {
    std::env::var_os("CENTRAL_CTRL_BIN")
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"))
}

/// One scope's Central AgentProfile listing: the Central root and scope it
/// was read against (the root itself, or a Work member), and each distinct
/// agent ref with the set of profile refs that name it. A listing record
/// without a profile ref is carried as the empty string and stays absent —
/// never invented.
struct ScopeProfiles {
    central_root: PathBuf,
    scope: &'static str,
    project: Option<String>,
    agents: BTreeMap<String, BTreeSet<String>>,
}

/// Ask Central's `agent-profile.list` for the scope `project_cwd` stands in:
/// a Work member's Project scope, or — when `include_root` — the Central root
/// itself. `None` means the lookup is unavailable (no Central root, a
/// directory in neither scope, no ctrl, refusing scope) — an absence, never an
/// invented agent.
fn scope_profiles(
    runner: &dyn CommandRunner,
    project_cwd: &Path,
    include_root: bool,
) -> Result<Option<ScopeProfiles>> {
    let Some(central_root) = project_cwd
        .ancestors()
        .find(|candidate| candidate.join("Control").is_dir() && candidate.join("Work").is_dir())
    else {
        return Ok(None);
    };
    let at_root = project_cwd
        .canonicalize()
        .ok()
        .zip(central_root.canonicalize().ok())
        .is_some_and(|(cwd, root)| cwd == root);
    let project = project_cwd
        .canonicalize()
        .ok()
        .zip(central_root.join("Work").canonicalize().ok())
        .and_then(|(cwd, work)| cwd.strip_prefix(work).ok().map(|p| p.to_path_buf()))
        .map(|relative| relative.to_string_lossy().into_owned());
    let (scope, input) = match project {
        Some(project) => ("project", json!({"scope":"project","project":project})),
        None if at_root && include_root => ("root", json!({"scope":"root"})),
        None => return Ok(None),
    };
    let argv = vec![
        ctrl_executable().to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        central_root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "agent-profile.list".into(),
        input.to_string(),
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
    let mut agents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in profiles {
        let Some(agent_ref) = entry["profile"]["agent_ref"].as_str() else {
            continue;
        };
        let profile_ref = entry["profile"]["ref"].as_str().unwrap_or("").to_owned();
        agents
            .entry(agent_ref.to_owned())
            .or_default()
            .insert(profile_ref);
    }
    Ok(Some(ScopeProfiles {
        central_root: central_root.to_path_buf(),
        scope,
        project: input["project"].as_str().map(str::to_owned),
        agents,
    }))
}

/// Central's native scope_ref (`central.now.allocate`/`central.work.policy`)
/// always names a project as `project:<id>`; AIKit's own `ProjectRef` (the
/// `project-context` resolver's `project_binding.project`) carries the bare
/// id instead (confirmed against a real project: `ProjectCentral/project.json`'s
/// `project_id` is `"O-I"`, not `"project:O-I"`). The mint's `world_ref` must
/// agree with Central's form exactly — `encounter-task-configure` refuses a
/// task whose allocation `scope_ref` differs from the Agency's `world_ref` —
/// so it is canonicalised here rather than passed through bare.
fn canonical_world_ref(project_ref: &str) -> String {
    if project_ref.starts_with("project:") || project_ref.starts_with("control:") {
        project_ref.to_owned()
    } else {
        format!("project:{project_ref}")
    }
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
    let world = resolve_project(home, project_cwd, explicit_agent_ref.as_ref())?;
    let world_ref = ResourceRef::parse(canonical_world_ref(world.as_str()))?;
    let ResolvedAgentIdentity {
        agent_ref,
        source: identity_source,
        central: central_profile_basis,
    } = resolve_agent_identity(runner, project_cwd, &world, explicit_agent_ref)?;
    // The profile basis pins the Central world the accepted definition was
    // read against, so turn-time identity composition resolves the exact
    // accepted sources instead of re-discovering a world.
    let (central_profile_ref, central_identity_world) = match central_profile_basis {
        Some(basis) => (
            Some(basis.profile_ref),
            Some(super::EncounterCentralIdentityWorld {
                central_root: basis.central_root,
                project_cwd: std::fs::canonicalize(project_cwd).map_err(super::error)?,
                scope: basis.scope.to_owned(),
                project: basis.project,
            }),
        ),
        None => (None, None),
    };

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
        central_profile_ref,
        central_identity_world,
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
            "central_profile_ref": binding.central_profile_ref.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::runner::Output;
    use std::sync::Mutex;

    /// Answers `agent-profile.list` from a fixed table, recording the scope
    /// input it was asked for, and `agent-profile.review` as accepted unless
    /// the profile is named unaccepted.
    struct ListRunner {
        profiles: Value,
        listed: Mutex<Vec<Value>>,
        unaccepted: Vec<&'static str>,
    }

    impl CommandRunner for ListRunner {
        fn run(&self, argv: &[String]) -> Result<Output> {
            if argv.iter().any(|arg| arg == "agent-profile.review") {
                let input: Value = serde_json::from_str(argv.last().unwrap()).unwrap();
                let accepted = !self
                    .unaccepted
                    .iter()
                    .any(|profile_ref| input["profile_ref"] == *profile_ref);
                return Ok(Output::success(
                    json!({"ok":true,"status":"success","data":{"accepted":accepted}}).to_string(),
                ));
            }
            assert!(
                argv.iter().any(|arg| arg == "agent-profile.list"),
                "{argv:?}"
            );
            self.listed
                .lock()
                .unwrap()
                .push(serde_json::from_str(argv.last().unwrap()).unwrap());
            Ok(Output::success(
                json!({"ok":true,"status":"success","data":{"profiles":self.profiles}}).to_string(),
            ))
        }
    }

    fn central_root() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("Control")).unwrap();
        std::fs::create_dir_all(root.join("Work")).unwrap();
        (temp, root)
    }

    /// Messages' Make writes a personal Agent at the Central root, and the
    /// kernel opens its conversation there with `--agent-ref`. The root scope
    /// is listed and that Agent's own profile pinned as `root`, no Project.
    #[test]
    fn a_named_agent_at_the_central_root_is_pinned_to_its_root_profile() {
        let (_temp, root) = central_root();
        let runner = ListRunner {
            profiles: json!([
                {"profile":{"ref":"agent-profile:expressed-a","agent_ref":"agent:expressed-a"}},
                {"profile":{"ref":"agent-profile:expressed-b","agent_ref":"agent:expressed-b"}}
            ]),
            listed: Mutex::default(),
            unaccepted: Vec::new(),
        };
        let resolved = resolve_agent_identity(
            &runner,
            &root,
            &ProjectRef::parse("control:root").unwrap(),
            Some(ResourceRef::parse("agent:expressed-b").unwrap()),
        )
        .unwrap();
        assert_eq!(resolved.agent_ref.as_str(), "agent:expressed-b");
        assert_eq!(resolved.source, "explicit-central-agent-profile");
        let central = resolved.central.expect("the root profile is pinned");
        assert_eq!(central.profile_ref, "agent-profile:expressed-b");
        assert_eq!(central.scope, "root");
        assert_eq!(central.project, None);
        assert_eq!(central.central_root, root);
        assert_eq!(
            runner.listed.lock().unwrap().as_slice(),
            [json!({"scope":"root"})]
        );
    }

    /// With no Agent named, the root keeps its derived default: the root's
    /// profiles are not consulted, so several of them refuse nothing.
    #[test]
    fn an_unnamed_agent_at_the_root_stays_derived() {
        let (_temp, root) = central_root();
        let runner = ListRunner {
            profiles: json!([]),
            listed: Mutex::default(),
            unaccepted: Vec::new(),
        };
        let resolved = resolve_agent_identity(
            &runner,
            &root,
            &ProjectRef::parse("control:root").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(resolved.source, "derived-project-agent");
        assert!(resolved.central.is_none());
        assert!(runner.listed.lock().unwrap().is_empty());
    }

    /// A named Agent whose profile the person has not accepted stays
    /// unpinned: pinning it would refuse every turn on the unaccepted
    /// definition, where today the conversation works.
    #[test]
    fn an_unaccepted_profile_is_not_pinned() {
        let (_temp, root) = central_root();
        let runner = ListRunner {
            profiles: json!([
                {"profile":{"ref":"agent-profile:expressed-b","agent_ref":"agent:expressed-b"}}
            ]),
            listed: Mutex::default(),
            unaccepted: vec!["agent-profile:expressed-b"],
        };
        let resolved = resolve_agent_identity(
            &runner,
            &root,
            &ProjectRef::parse("control:root").unwrap(),
            Some(ResourceRef::parse("agent:expressed-b").unwrap()),
        )
        .unwrap();
        assert_eq!(resolved.source, "explicit");
        assert!(resolved.central.is_none());
    }

    /// Two profiles for the one named Agent cannot be chosen between.
    #[test]
    fn two_profiles_for_the_named_agent_refuse() {
        let (_temp, root) = central_root();
        let runner = ListRunner {
            profiles: json!([
                {"profile":{"ref":"agent-profile:one","agent_ref":"agent:expressed-b"}},
                {"profile":{"ref":"agent-profile:two","agent_ref":"agent:expressed-b"}}
            ]),
            listed: Mutex::default(),
            unaccepted: Vec::new(),
        };
        let error = resolve_agent_identity(
            &runner,
            &root,
            &ProjectRef::parse("control:root").unwrap(),
            Some(ResourceRef::parse("agent:expressed-b").unwrap()),
        )
        .err()
        .expect("ambiguous");
        assert_eq!(error.code(), "agency_mint.agent_ambiguous");
    }
}
