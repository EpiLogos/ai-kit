//! The per-project agency mint.
//!
//! Each chat session's Agency is a *fresh* native chain per (agent, project),
//! resolved through the World's standing authority in Actuation and actualised
//! by the real native owner (`actuation agency actualise`), through the same
//! [`CommandRunner`](aikit_adapters::runner::CommandRunner) seam
//! [`admit_agency`](aikit_adapters::agency_admission::admit_agency) uses.
//!
//! The World's authority is the person's: they grant it at the World's Setup
//! (the Ground step, `actuation.authority.grant-world`), and Actuation keeps it
//! in its authority store. The mint asks Actuation for the live grant that
//! names the child's world (`actuation authority list`), composes the
//! resolution request from its own per-(agent, project) refs and has Actuation
//! resolve it (`actuation authority resolve`). Only an admitted resolution's
//! actualisation request is persisted and actualised; any other answer is
//! refused in Actuation's words.
//!
//! Derivation law (what is minted vs what is carried):
//!
//! * Carried from the World's grant — never invented here: the requester (the
//!   grant's holder), the governing binding and metagency grant (which only
//!   Actuation's stored record supplies), the requested bounds (the grant's)
//!   and the authority refs (the governing binding's).
//! * Minted freshly per (agent, project), deterministically (the same
//!   agent + project always derives the same refs, so the persisted source is
//!   content-addressed and re-mints are honest replays): `request_ref`,
//!   `determination_ref`, the child `agency_ref` and `binding_ref`,
//!   `return_relation_ref`, `continuity_ref`, `agent_identity` evidence and
//!   provenance source refs.
//! * The child `world_ref` is the canonical Project identity resolved from
//!   `--project-cwd` through the same `session-space project-context`
//!   resolution path; it is never inferred from the directory name here. The
//!   child's scope is its own world, so the Central root's child is the root
//!   context and a Project's child needs its Project binding.
//! * The delegated Actions are the two an ACP chat session cannot work without
//!   (`action/aikit/encounter-send`, `action/aikit/model-realise`), plus the
//!   task Action when the mint is for a task — the native admission refuses a
//!   binding that cannot send.
//!
//! Agent identity resolution order: explicit `--agent-ref` (pinned to its
//! scope's Central AgentProfile when that scope — the root or a Project —
//! declares one for it), else the Central AgentProfile for the project scope
//! when exactly one exists, else a derived, clearly-attributed
//! `agent/<project-slug>-chat`. A human `requester_ref` is never fabricated:
//! it is always the grant's holder.
use super::{read_binding, EncounterAgencyBinding};
use crate::app::Service;
use aikit_adapters::{
    agency_admission::{admit_agency, AgencySourceBasis},
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

/// What a refusal for want of the World's authority names as its fix.
const AUTHORITY_FIX: &str = "the World's Setup grants it at the Ground step \
     (`actuation.authority.grant-world`)";

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

/// The live grant in Actuation's authority store that names a world: its
/// source ref and the governing record Actuation keeps for it.
struct WorldGrant {
    authority_source_ref: String,
    record: Value,
}

/// Ask Actuation for the grants it holds and take the live one naming
/// `world_ref` — not revoked, not expired, the world among its allowed
/// worlds; the newest when several do. None is a refusal naming the fix.
fn find_world_grant(
    runner: &dyn CommandRunner,
    actuation_bin: &Path,
    world_ref: &str,
) -> Result<WorldGrant> {
    let argv = vec![
        actuation_bin.to_string_lossy().into_owned(),
        "authority".into(),
        "list".into(),
        "--json".into(),
    ];
    let output = runner.run(&argv)?;
    if output.status != 0 {
        return Err(mint_error(
            "agency_mint.authority_missing",
            format!(
                "Actuation could not list the World's authority (exit {}): {}; {AUTHORITY_FIX}",
                output.status,
                owner_text(&output)
            ),
        ));
    }
    let listing: Value = serde_json::from_str(&output.stdout).map_err(|error| {
        mint_error(
            "agency_mint.authority_missing",
            format!("Actuation's authority listing is not readable: {error}"),
        )
    })?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let newest = listing["authorities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|listed| listed["revoked"].is_null())
        .filter(|listed| {
            listed["record"]["expires_at_unix_seconds"]
                .as_u64()
                .is_none_or(|expires| expires > now)
        })
        .filter(|listed| {
            listed["record"]["allowed_world_refs"]
                .as_array()
                .is_some_and(|worlds| worlds.iter().any(|world| world == world_ref))
        })
        .filter_map(|listed| {
            Some((
                listed["issued_at_unix_seconds"]
                    .as_u64()
                    .unwrap_or_default(),
                listed["authority_source_ref"].as_str()?,
                listed,
            ))
        })
        .max_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
    let Some((_, authority_source_ref, listed)) = newest else {
        return Err(mint_error(
            "agency_mint.authority_missing",
            format!("no live authority in Actuation names the world {world_ref}; {AUTHORITY_FIX}"),
        ));
    };
    Ok(WorldGrant {
        authority_source_ref: authority_source_ref.to_owned(),
        record: listed["record"].clone(),
    })
}

/// The task Action a mint for a task delegates as well.
const TASK_ACTION: &str = "action/aikit/encounter-task";

/// The resolution request the mint puts to Actuation
/// (`actuation.local-authority/v1`): its own per-(agent, project) refs, the
/// grant's holder as requester, the grant's bounds and the governing
/// binding's authority. The governing side is the stored record's alone.
fn resolution_request(
    grant: &WorldGrant,
    agent_ref: &str,
    world_ref: &str,
    tag: &str,
    for_task: bool,
) -> Result<Value> {
    let record = &grant.record;
    let named = |value: &Value, what: &str| -> Result<String> {
        value.as_str().map(str::to_owned).ok_or_else(|| {
            mint_error(
                "agency_mint.authority_refused",
                format!(
                    "the World's authority {} names no {what}; {AUTHORITY_FIX}",
                    grant.authority_source_ref
                ),
            )
        })
    };
    let holder = named(&record["holder"], "holder")?;
    let determining_agency = named(
        &record["governing_binding"]["agency_ref"],
        "governing agency",
    )?;
    let bounds_refs = record["metagency_grant"]["bounds_refs"].clone();
    let authority_refs = record["governing_binding"]["authority_refs"].clone();
    let mut allowed: Vec<Value> = REQUIRED_MINTED_ACTIONS
        .iter()
        .map(|action| json!(action))
        .collect();
    if for_task {
        allowed.push(json!(TASK_ACTION));
    }
    Ok(json!({
        "schema": "actuation.local-authority/v1",
        "resolution_ref": format!("resolution:aikit-mint-{tag}"),
        "request_ref": format!("actualisation-request:aikit-mint-{tag}"),
        "requester_ref": holder,
        "authority_source_ref": grant.authority_source_ref,
        "determination_ref": format!("determination:aikit-mint-{tag}"),
        "differentiated_binding": {
            "schema": "actuation.agency/v1",
            "binding_ref": format!("binding:aikit-mint-{tag}"),
            "agent_ref": agent_ref,
            "agency_ref": format!("agency:aikit-mint-{tag}"),
            "world_ref": world_ref,
            "scope_ref": world_ref,
            "determining_agency_ref": determining_agency,
            "bounds_refs": bounds_refs,
            "authority_refs": authority_refs,
            "return_relation_ref": format!("return-relation:aikit-mint-{tag}"),
            "continuity_ref": format!("continuity:{agent_ref}"),
        },
        "agent_identity": {
            "standing": "existing",
            "evidence_refs": [format!("evidence/aikit-agency-mint/{tag}")],
        },
        "requested_bounds_refs": bounds_refs,
        "delegated_autonomy": {
            "allowed_action_refs": allowed,
            "denied_action_refs": [],
            "may_determine_within_bounds": false,
        },
        "return_policy": {
            "mode": "required",
            "return_relation_ref": format!("return-relation:aikit-mint-{tag}"),
        },
        "provenance": {
            "source_refs": [format!("source/aikit-agency-mint/{tag}")],
            "context_refs": [],
        },
    }))
}

/// Have Actuation resolve the request against the World's grant. The request
/// is written beside the minted sources (content-addressed) and passed by
/// path. Admitted, the answer's actualisation request is returned; any other
/// answer is refused in Actuation's words.
fn resolve_through_authority(
    runner: &dyn CommandRunner,
    actuation_bin: &Path,
    directory: &Path,
    resolution: &Value,
) -> Result<Value> {
    let bytes = serde_json::to_vec_pretty(resolution)
        .map_err(|error| mint_error("agency_mint.encode", error.to_string()))?;
    let directory = directory.join("resolutions");
    std::fs::create_dir_all(&directory).map_err(super::error)?;
    let path = directory.join(format!("{}.json", blake3::hash(&bytes).to_hex()));
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&path, &bytes).map_err(super::error)?;
    }
    let argv = vec![
        actuation_bin.to_string_lossy().into_owned(),
        "authority".into(),
        "resolve".into(),
        path.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    let output = runner.run(&argv)?;
    let authority = resolution["authority_source_ref"].as_str().unwrap_or("?");
    let world = resolution["differentiated_binding"]["world_ref"]
        .as_str()
        .unwrap_or("?");
    if output.status != 0 {
        return Err(mint_error(
            "agency_mint.authority_refused",
            format!(
                "Actuation refused to resolve {world} against the World's authority {authority} \
                 (exit {}): {}; {AUTHORITY_FIX}",
                output.status,
                owner_text(&output)
            ),
        ));
    }
    let outcome: Value = serde_json::from_str(&output.stdout).map_err(|error| {
        mint_error(
            "agency_mint.authority_refused",
            format!("Actuation's resolution is not readable: {error}"),
        )
    })?;
    match (&outcome["standing"], &outcome["actualisation_request"]) {
        (standing, request) if standing == "admitted" && request.is_object() => Ok(request.clone()),
        (standing, _) => Err(mint_error(
            "agency_mint.authority_refused",
            format!(
                "Actuation answered {} for {world} against the World's authority {authority}: \
                 {}: {}; {AUTHORITY_FIX}",
                standing.as_str().unwrap_or("no standing"),
                outcome["refusal"]["code"].as_str().unwrap_or("no code"),
                outcome["refusal"]["reason"]
                    .as_str()
                    .unwrap_or("no reason given"),
            ),
        )),
    }
}

/// A native owner's own words for a failed command: stderr, else stdout.
fn owner_text(output: &aikit_adapters::runner::Output) -> &str {
    if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    }
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
        return Err(AikitError::new(
            "agency_mint.refused",
            format!(
                "Actuation refused the minted agency request (exit {}): {}",
                output.status,
                owner_text(&output)
            ),
        ));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|error| mint_error("agency_mint.receipt_invalid", error.to_string()))
}

/// Actuation as every AIKit owner call names it (gateway_owners.rs
/// `ProcessOwners`): `ACTUATION_BIN` or `OI_ACTUATION_BIN` where the suite
/// names its build, else `actuation` on PATH, pinned to its absolute location
/// for the persisted binding. The mint asks the same Actuation that granted
/// the World, never a different one found first on PATH.
fn resolve_actuation_bin() -> PathBuf {
    let named = crate::gateway_owners::ProcessOwners::from_env().actuation;
    crate::probe::which(&named).unwrap_or_else(|| PathBuf::from(named))
}

/// The owner-only directory the minted sources (and the resolutions put to
/// Actuation) live in.
fn prepare_mint_directory(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory).map_err(super::error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(super::error)?;
    }
    Ok(())
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

    let directory = home.state().join("agency-mints");
    let tag = mint_tag(agent_ref.as_str(), world_ref.as_str());
    // The actualisation request, resolved through the World's authority in
    // Actuation: a requested determination; actualise below still decides.
    let grant = find_world_grant(runner, actuation_bin, world_ref.as_str())?;
    let resolution = resolution_request(
        &grant,
        agent_ref.as_str(),
        world_ref.as_str(),
        &tag,
        for_task,
    )?;
    prepare_mint_directory(&directory)?;
    let request = resolve_through_authority(runner, actuation_bin, &directory, &resolution)?;
    let authority_source_ref = grant.authority_source_ref;

    // Content-addressed persistence: the same agent + project derives
    // byte-identical request text, so re-mints land on the same source.
    let bytes = serde_json::to_vec_pretty(&request)
        .map_err(|error| mint_error("agency_mint.encode", error.to_string()))?;
    let digest = blake3::hash(&bytes).to_hex();
    prepare_mint_directory(&directory)?;
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
            request["requester_ref"].as_str().ok_or_else(|| {
                mint_error(
                    "agency_mint.receipt_invalid",
                    "the actualised request names no requester",
                )
            })?,
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
            "authority_source_ref": authority_source_ref,
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

    /// Actuation's authority verbs through the seam: `authority list`
    /// answers `authorities`; `authority resolve <file>` records the request
    /// it was given and answers `outcome`.
    struct AuthorityRunner {
        authorities: Value,
        outcome: Value,
        resolved: Mutex<Vec<Value>>,
    }

    impl CommandRunner for AuthorityRunner {
        fn run(&self, argv: &[String]) -> Result<Output> {
            match (
                argv.get(1).map(String::as_str),
                argv.get(2).map(String::as_str),
            ) {
                (Some("authority"), Some("list")) => Ok(Output::success(
                    json!({"schema": "actuation.local-authority-listing/v1",
                           "authorities": self.authorities, "invalid": []})
                    .to_string(),
                )),
                (Some("authority"), Some("resolve")) => {
                    let asked: Value =
                        serde_json::from_slice(&std::fs::read(&argv[3]).unwrap()).unwrap();
                    self.resolved.lock().unwrap().push(asked);
                    Ok(Output::success(self.outcome.to_string()))
                }
                _ => panic!("only the authority verbs are asked: {argv:?}"),
            }
        }
    }

    /// A grant as `actuation authority grant-world` composes it.
    fn listed_grant(source: &str, issued_at: u64, worlds: &[&str]) -> Value {
        let root = worlds[0];
        let bounds: Vec<String> = worlds
            .iter()
            .map(|world| format!("bound:{world}"))
            .collect();
        json!({
            "authority_source_ref": source,
            "issued_at_unix_seconds": issued_at,
            "record": {
                "schema": "actuation.local-authority/v1",
                "authority_source_ref": source,
                "holder": "human:owner",
                "issued_by": "human:owner",
                "governing_binding": {
                    "schema": "actuation.agency/v1",
                    "binding_ref": format!("binding:world-root@{root}"),
                    "agent_ref": format!("agent:world-root@{root}"),
                    "agency_ref": format!("agency:world-root@{root}"),
                    "world_ref": root,
                    "scope_ref": root,
                    "bounds_refs": bounds,
                    "authority_refs": [format!("authority:world@{root}")],
                    "return_relation_ref": format!("return-relation:world-root@{root}")
                },
                "metagency_grant": {
                    "schema": "actuation.agency/v1",
                    "grant_ref": format!("grant:world-root@{root}"),
                    "agency_ref": format!("agency:world-root@{root}"),
                    "world_binding_ref": format!("binding:world-root@{root}"),
                    "authority_ref": format!("authority:world@{root}"),
                    "bounds_refs": bounds,
                    "operations": ["determine-agency", "actualise-agency"]
                },
                "allowed_world_refs": worlds,
                "issued_at_unix_seconds": issued_at
            }
        })
    }

    fn authority_runner(authorities: Value, outcome: Value) -> AuthorityRunner {
        AuthorityRunner {
            authorities,
            outcome,
            resolved: Mutex::default(),
        }
    }

    /// The live grant naming the world is taken, the newest when several do;
    /// a revoked, expired or other-world grant never is.
    #[test]
    fn the_newest_live_grant_naming_the_world_is_taken() {
        let mut revoked = listed_grant("authority:revoked", 30, &["control:root", "project:a"]);
        revoked["revoked"] = json!({"at_unix_seconds": 31, "reason": "superseded"});
        let mut expired = listed_grant("authority:expired", 40, &["control:root", "project:a"]);
        expired["record"]["expires_at_unix_seconds"] = json!(41);
        let runner = authority_runner(
            json!([
                listed_grant("authority:older", 10, &["control:root", "project:a"]),
                listed_grant("authority:newer", 20, &["control:root", "project:a"]),
                listed_grant("authority:elsewhere", 50, &["control:other", "project:b"]),
                revoked,
                expired,
            ]),
            Value::Null,
        );
        let grant = find_world_grant(&runner, Path::new("actuation"), "project:a").unwrap();
        assert_eq!(grant.authority_source_ref, "authority:newer");
        let grant = find_world_grant(&runner, Path::new("actuation"), "control:root").unwrap();
        assert_eq!(grant.authority_source_ref, "authority:newer");
        let missing = find_world_grant(&runner, Path::new("actuation"), "project:c")
            .err()
            .expect("no grant names it");
        assert_eq!(missing.code(), "agency_mint.authority_missing");
        assert!(
            missing.message().contains("project:c"),
            "{}",
            missing.message()
        );
        assert!(
            missing
                .message()
                .contains("actuation.authority.grant-world"),
            "{}",
            missing.message()
        );
    }

    /// The resolution carries the grant's holder, bounds and authority and
    /// the mint's own refs; the child's scope is its own world; the task
    /// Action is asked for only with task intent.
    #[test]
    fn the_resolution_carries_the_grant_and_mints_its_own_refs() {
        let grant = WorldGrant {
            authority_source_ref: "authority:world".into(),
            record: listed_grant("authority:world", 10, &["control:root", "project:a"])["record"]
                .clone(),
        };
        let chat =
            resolution_request(&grant, "agent/a-chat", "project:a", "a-1234", false).unwrap();
        assert_eq!(chat["schema"], "actuation.local-authority/v1");
        assert_eq!(chat["requester_ref"], "human:owner");
        assert_eq!(chat["authority_source_ref"], "authority:world");
        assert_eq!(
            chat["requested_bounds_refs"],
            json!(["bound:control:root", "bound:project:a"])
        );
        let child = &chat["differentiated_binding"];
        assert_eq!(child["world_ref"], "project:a");
        assert_eq!(child["scope_ref"], "project:a");
        assert_eq!(child["agent_ref"], "agent/a-chat");
        assert_eq!(
            child["determining_agency_ref"],
            "agency:world-root@control:root"
        );
        assert_eq!(child["bounds_refs"], chat["requested_bounds_refs"]);
        assert_eq!(
            child["authority_refs"],
            json!(["authority:world@control:root"])
        );
        assert_eq!(child["agency_ref"], "agency:aikit-mint-a-1234");
        assert_eq!(
            chat["delegated_autonomy"]["allowed_action_refs"],
            json!(REQUIRED_MINTED_ACTIONS)
        );
        let task = resolution_request(&grant, "agent/a-chat", "project:a", "a-1234", true).unwrap();
        assert_eq!(
            task["delegated_autonomy"]["allowed_action_refs"],
            json!([
                REQUIRED_MINTED_ACTIONS[0],
                REQUIRED_MINTED_ACTIONS[1],
                TASK_ACTION
            ])
        );
        let mut holderless = grant.record.clone();
        holderless["holder"] = Value::Null;
        let refused = resolution_request(
            &WorldGrant {
                authority_source_ref: "authority:world".into(),
                record: holderless,
            },
            "agent/a-chat",
            "project:a",
            "a-1234",
            false,
        )
        .unwrap_err();
        assert_eq!(refused.code(), "agency_mint.authority_refused");
    }

    /// Admitted, the answer's actualisation request comes back; refused, the
    /// mint refuses in Actuation's words and names the fix.
    #[test]
    fn a_refused_resolution_is_refused_in_actuations_words() {
        let temp = tempfile::tempdir().unwrap();
        let grant = WorldGrant {
            authority_source_ref: "authority:world".into(),
            record: listed_grant("authority:world", 10, &["control:root", "project:a"])["record"]
                .clone(),
        };
        let resolution =
            resolution_request(&grant, "agent/a-chat", "project:a", "a-1234", false).unwrap();
        let admitted = authority_runner(
            json!([]),
            json!({"standing": "admitted", "actualisation_request": {"request_ref": "r"}}),
        );
        let request =
            resolve_through_authority(&admitted, Path::new("actuation"), temp.path(), &resolution)
                .unwrap();
        assert_eq!(request, json!({"request_ref": "r"}));
        assert_eq!(
            admitted.resolved.lock().unwrap().as_slice(),
            std::slice::from_ref(&resolution)
        );

        let refused = authority_runner(
            json!([]),
            json!({"standing": "refused", "refusal": {"code": "authority.wrong_holder",
                   "reason": "human:owner is not the holder of authority:world"}}),
        );
        let error =
            resolve_through_authority(&refused, Path::new("actuation"), temp.path(), &resolution)
                .unwrap_err();
        assert_eq!(error.code(), "agency_mint.authority_refused");
        for words in [
            "authority.wrong_holder",
            "human:owner is not the holder of authority:world",
            "authority:world",
            "project:a",
            "actuation.authority.grant-world",
        ] {
            assert!(
                error.message().contains(words),
                "{words}: {}",
                error.message()
            );
        }
    }
}
