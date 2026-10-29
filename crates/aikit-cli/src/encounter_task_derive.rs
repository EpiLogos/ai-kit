//! A task made from what its session already is (O-I #768: a constellation
//! dropped on a session that holds no task makes "a task in Factory"). The
//! caller names only what is the task's own — its purpose, the constellation
//! it is made of (`source`) and, if it chooses, its ref — and the owner
//! completes the request `encounter-task-configure` takes from its own
//! standing, never from another session's task or from the caller:
//!
//! * participant — the agent the session's Agency binding names;
//! * authority — the one authority the Agency's admitted determination holds;
//! * Central — the selected Central executable (`CENTRAL_CTRL_BIN`,
//!   `OI_CENTRAL_CTRL_BIN`, else `ctrl` on PATH, pinned to its target) and the
//!   World and Project the session works in (`CENTRAL_ROOT`, else
//!   `~/Central`, whose `Work` tree holds the session's working directory);
//! * provider and working directory — the agent the session opened on and
//!   where it opened (its journal's last native binding);
//! * writable directories — none beyond the task's own NOW, which Central
//!   allocates and the boundary always grants: nothing in the session's
//!   standing names a directory the task may change (the place it opened is
//!   most often a repository root, whose protected descendants Central
//!   rightly refuses as a write grant), so none is inferred;
//! * Workcell boundary — the boundary executable of the selected Workcell
//!   installation, else `workcell-write-boundary` on PATH.
//!
//! What cannot be derived is refused, each fact named with why. Nothing here
//! authorises: the completed request passes the same admission a whole one
//! does, so the Agency must still authorise the task, its authority and its
//! participant, and a short request cannot name any of them.
use super::{native_admission, read_binding};
use crate::encounter_service::{EncounterProvider, EncounterService};
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const UNDERIVABLE: &str = "encounter.task_underivable";
const BOUNDARY: &str = "workcell-write-boundary";

/// What a short request names: what is the task's own, nothing of its setup.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShortRequest {
    purpose: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    task_ref: Option<ResourceRef>,
}

/// A fact read from its owner, or why it could not be.
type Fact<T> = std::result::Result<T, String>;

/// The facts a task takes from its session's standing.
struct Standing {
    participant: Fact<ResourceRef>,
    authority: Fact<ResourceRef>,
    ctrl_bin: Fact<PathBuf>,
    central: Fact<(PathBuf, Option<String>)>,
    provider: Fact<EncounterProvider>,
    cwd: Fact<PathBuf>,
    boundary: Fact<PathBuf>,
}

/// A request is short when it names no Central placement; the whole form
/// always does.
pub(super) fn is_short(input: &Value) -> bool {
    input
        .as_object()
        .is_some_and(|fields| !fields.contains_key("central"))
}

/// The whole request the short `input` stands for on `session`.
pub(super) fn complete(home: &AikitHome, session: &ResourceRef, input: Value) -> Result<Value> {
    let short: ShortRequest = serde_json::from_value(input).map_err(|refused| {
        AikitError::new(
            UNDERIVABLE,
            format!(
                "A short task request names its purpose, and may name its source and task_ref; \
                 the rest is the session's own and is derived ({refused})"
            ),
        )
    })?;
    let standing = standing(home, session)?;
    compose(short, standing, ulid::Ulid::generate())
}

/// The whole request: the short one's own fields and the standing's, or a
/// refusal naming each fact the standing could not supply.
fn compose(short: ShortRequest, standing: Standing, at: ulid::Ulid) -> Result<Value> {
    fn take<T>(missing: &mut Vec<String>, fact: &str, read: Fact<T>) -> Option<T> {
        read.map_err(|why| missing.push(format!("{fact}: {why}")))
            .ok()
    }
    let mut missing = Vec::new();
    let participant = take(
        &mut missing,
        "its participant (central.participant_refs)",
        standing.participant,
    );
    let authority = take(
        &mut missing,
        "its authority (authority_ref)",
        standing.authority,
    );
    let ctrl_bin = take(
        &mut missing,
        "Central (central.ctrl_bin)",
        standing.ctrl_bin,
    );
    let central = take(
        &mut missing,
        "its Central World and Project (central.central_root, central.project)",
        standing.central,
    );
    let provider = take(&mut missing, "its provider (provider)", standing.provider);
    let cwd = take(&mut missing, "its working directory (cwd)", standing.cwd);
    let boundary = take(
        &mut missing,
        "its Workcell boundary (workcell_boundary_bin)",
        standing.boundary,
    );
    let (
        Some(participant),
        Some(authority),
        Some(ctrl_bin),
        Some((central_root, project)),
        Some(provider),
        Some(cwd),
        Some(boundary),
    ) = (
        participant,
        authority,
        ctrl_bin,
        central,
        provider,
        cwd,
        boundary,
    )
    else {
        return Err(AikitError::new(
            UNDERIVABLE,
            format!(
                "No task is made from this purpose alone: the session's standing does not give {}. \
                 Repair the session, or send the whole request.",
                missing.join("; ")
            ),
        ));
    };
    let task_ref = match short.task_ref {
        Some(task_ref) => task_ref,
        None => ResourceRef::parse(format!(
            "task:{}-{}",
            slug(&short.purpose),
            at.to_string().to_lowercase()
        ))?,
    };
    let mut request = json!({
        "central": {"ctrl_bin": ctrl_bin, "central_root": central_root, "project": project,
            "task_ref": task_ref, "purpose": short.purpose,
            "participant_refs": [participant], "source_refs": []},
        "provider": serde_json::to_value(provider).map_err(super::error)?,
        "cwd": cwd, "selected_directories": [],
        "workcell_boundary_bin": boundary,
        "authority_ref": authority,
    });
    if let Some(source) = short.source {
        request["source"] = json!(source);
    }
    Ok(request)
}

/// A purpose in ref-safe words: lowercase ASCII letters and digits, the rest
/// one hyphen, at most 48 characters; `task` when nothing is left.
pub(super) fn slug(purpose: &str) -> String {
    let mut slug = String::new();
    for character in purpose.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "task".into()
    } else {
        slug.into()
    }
}

/// What the session holds: the agent its Agency binding names and the one
/// authority its admitted determination holds; the agent it opened on (as
/// configured, the raw row) and where it opened, from its own journal; and
/// the Central, World and Workcell boundary that place stands in.
fn standing(home: &AikitHome, session: &ResourceRef) -> Result<Standing> {
    let (participant, authority) = match read_binding(home, session)? {
        None => {
            let why = "the session has no Agency binding".to_owned();
            (Err(why.clone()), Err(why))
        }
        Some(binding) => {
            let authority = match native_admission(&binding) {
                Ok(admitted) => one_authority(&admitted.receipt["determination"]["authority_refs"]),
                Err(refused) => Err(format!("the session's Agency was not admitted ({refused})")),
            };
            (Ok(binding.agent_ref), authority)
        }
    };
    let never = || {
        "the session has never opened, so the agent it works with and where are not known"
            .to_owned()
    };
    let (provider, cwd) = match EncounterStore::open(home)?.last_native_binding(session)? {
        None => (Err(never()), Err(never())),
        Some(opened) => {
            let provider = match EncounterService::session_agent(home, session)? {
                Some(agent) => configured(home, &agent.provider),
                None => Err(never()),
            };
            let cwd = opened["cwd"]
                .as_str()
                .map(PathBuf::from)
                .filter(|cwd| cwd.is_absolute() && cwd.is_dir())
                .ok_or_else(|| {
                    "the session's last opening names no working directory that still exists"
                        .to_owned()
                });
            (provider, cwd)
        }
    };
    let central = match &cwd {
        Ok(cwd) => central_of(cwd),
        Err(_) => {
            Err("it is the World of the session's working directory, which is not known".into())
        }
    };
    Ok(Standing {
        participant,
        authority,
        ctrl_bin: ctrl_bin(),
        central,
        provider,
        cwd,
        boundary: boundary(),
    })
}

/// The one authority an admitted determination holds.
fn one_authority(refs: &Value) -> Fact<ResourceRef> {
    let refs: Vec<&str> = refs
        .as_array()
        .map(|refs| refs.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    match refs.as_slice() {
        [one] => ResourceRef::parse(*one).map_err(|refused| refused.to_string()),
        [] => Err("the session's Agency determination names no authority".into()),
        many => Err(format!(
            "the session's Agency determination names {} authorities ({}), and nothing here chooses one",
            many.len(),
            many.join(", ")
        )),
    }
}

/// An encounter provider as it was configured: its raw row, so a profile
/// row keeps `from_profile` and resolves again where the task runs.
fn configured(home: &AikitHome, id: &str) -> Fact<EncounterProvider> {
    let withdrawn = || {
        format!(
            "the agent the session opened on ({id}) is no longer a configured encounter provider"
        )
    };
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(withdrawn());
    }
    let path = home
        .state()
        .join("encounter-providers")
        .join(format!("{id}.json"));
    let bytes = std::fs::read(&path).map_err(|_| withdrawn())?;
    serde_json::from_slice(&bytes)
        .map_err(|refused| format!("the configured provider {id} is unreadable ({refused})"))
}

/// The World a working directory stands in: the Central root whose `Work`
/// tree holds it (a Project, or a seat checked out of one) and that
/// Project's name; the root itself is the root World.
fn central_of(cwd: &Path) -> Fact<(PathBuf, Option<String>)> {
    if let Some(root) = crate::temporal::process_central_root(Some(cwd)) {
        let project = aikit_adapters::central_temporal::project_ground_root(&root, cwd)
            .and_then(|ground| ground.file_name()?.to_str().map(str::to_owned));
        if let Some(project) = project {
            return Ok((root, Some(project)));
        }
    }
    if crate::temporal::central_root_enclosing(Some(cwd)).as_deref() == Some(cwd) {
        return Ok((cwd.to_path_buf(), None));
    }
    Err(format!(
        "{} is in no Central World's Work tree (CENTRAL_ROOT, else ~/Central)",
        cwd.display()
    ))
}

/// The selected Central executable, pinned to its target as every owner
/// crossing pins it.
fn ctrl_bin() -> Fact<PathBuf> {
    use crate::gateway_owners::{CtrlActionError, ProcessOwners};
    match ProcessOwners::from_env().resolved_ctrl() {
        Ok(owners) => Ok(PathBuf::from(owners.ctrl)),
        Err(CtrlActionError::Unavailable(why)) => Err(why),
        Err(CtrlActionError::Refused { message, .. }) => Err(message),
    }
}

/// The Workcell boundary: the one the selected Workcell installation carries
/// beside `workcell` (as a prepared run selects it), else the boundary
/// executable on PATH.
fn boundary() -> Fact<PathBuf> {
    let beside = crate::probe::which("workcell")
        .and_then(|workcell| workcell.canonicalize().ok())
        .and_then(|workcell| {
            crate::probe::which(&workcell.parent()?.join(BOUNDARY).display().to_string())
        });
    beside
        .or_else(|| crate::probe::which(BOUNDARY))
        .ok_or_else(|| {
            format!("neither the selected Workcell installation nor PATH carries {BOUNDARY}")
        })?
        .canonicalize()
        .map_err(|why| format!("{BOUNDARY} cannot be resolved ({why})"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    fn r(s: &str) -> ResourceRef {
        ResourceRef::parse(s).unwrap()
    }

    fn provider() -> EncounterProvider {
        serde_json::from_value(json!({"id": "task-agent", "label": "The session's agent",
            "protocol": "acp", "argv": ["/opt/agent", "--acp"]}))
        .unwrap()
    }

    fn whole(root: &Path) -> Standing {
        Standing {
            participant: Ok(r("agent:buzz")),
            authority: Ok(r("authority:owner")),
            ctrl_bin: Ok(root.join("bin/ctrl")),
            central: Ok((root.to_path_buf(), Some("O-I".into()))),
            provider: Ok(provider()),
            cwd: Ok(root.join("Work/O-I")),
            boundary: Ok(root.join("bin/workcell-write-boundary")),
        }
    }

    fn short(value: Value) -> ShortRequest {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_short_request_is_completed_from_the_sessions_standing_and_reads_as_a_whole_one() {
        let root = Path::new("/world");
        let made = compose(
            short(
                json!({"purpose": "The lived-in world", "source": "wiki:anchor:lived-in",
                "task_ref": "task:lived-in"}),
            ),
            whole(root),
            ulid::Ulid::nil(),
        )
        .unwrap();
        assert_eq!(
            made,
            json!({
                "central": {"ctrl_bin": "/world/bin/ctrl", "central_root": "/world", "project": "O-I",
                    "task_ref": "task:lived-in", "purpose": "The lived-in world",
                    "participant_refs": ["agent:buzz"], "source_refs": []},
                "provider": serde_json::to_value(provider()).unwrap(),
                "cwd": "/world/Work/O-I", "selected_directories": [],
                "workcell_boundary_bin": "/world/bin/workcell-write-boundary",
                "authority_ref": "authority:owner", "source": "wiki:anchor:lived-in"
            })
        );
        // The completed request is the whole form the owner already takes.
        let parsed: super::super::TaskRequest = serde_json::from_value(made).unwrap();
        assert_eq!(parsed.source.as_deref(), Some("wiki:anchor:lived-in"));
        // A task made of no constellation keeps no source, as a whole request does.
        let plain = compose(
            short(json!({"purpose": "Plain"})),
            whole(root),
            ulid::Ulid::nil(),
        )
        .unwrap();
        assert!(plain.get("source").is_none(), "{plain}");
    }

    #[test]
    fn a_short_request_without_a_ref_is_given_one_named_for_its_purpose() {
        let at = ulid::Ulid::from_string("01JBX7Q2Z9R3M4N5P6Q7R8S9T0").unwrap();
        let made = compose(
            short(json!({"purpose": "The Lived-in World!", "source": "wiki:anchor:lived-in"})),
            whole(Path::new("/world")),
            at,
        )
        .unwrap();
        assert_eq!(
            made["central"]["task_ref"],
            "task:the-lived-in-world-01jbx7q2z9r3m4n5p6q7r8s9t0"
        );
        let blank = compose(
            short(json!({"purpose": "…"})),
            whole(Path::new("/world")),
            at,
        )
        .unwrap();
        assert_eq!(
            blank["central"]["task_ref"],
            "task:task-01jbx7q2z9r3m4n5p6q7r8s9t0"
        );
    }

    #[test]
    fn each_fact_the_standing_cannot_supply_is_named_with_why() {
        let mut thin = whole(Path::new("/world"));
        thin.participant = Err("the session has no Agency binding".into());
        thin.authority = Err("the session has no Agency binding".into());
        thin.provider = Err("the session has never opened".into());
        thin.boundary = Err("no Workcell boundary on PATH".into());
        let refused = compose(
            short(json!({"purpose": "p", "source": "wiki:anchor:lived-in"})),
            thin,
            ulid::Ulid::nil(),
        )
        .unwrap_err();
        assert_eq!(refused.code(), UNDERIVABLE);
        let words = refused.to_string();
        for named in [
            "participant (central.participant_refs): the session has no Agency binding",
            "authority (authority_ref): the session has no Agency binding",
            "provider (provider): the session has never opened",
            "Workcell boundary (workcell_boundary_bin): no Workcell boundary on PATH",
        ] {
            assert!(words.contains(named), "{named} in {words}");
        }
        for derived in ["central_root", "ctrl_bin", "(cwd)"] {
            assert!(!words.contains(derived), "{derived} was derived: {words}");
        }
    }

    #[test]
    fn a_short_request_names_only_what_is_the_tasks_own() {
        assert!(is_short(
            &json!({"purpose": "p", "source": "wiki:anchor:x"})
        ));
        assert!(!is_short(&json!({"central": {}, "purpose": "p"})));
        assert!(!is_short(&json!(["purpose"])));
        // The caller cannot name an authority, a participant, a directory or an
        // agent beside the short form: those are the session's own.
        for extra in ["authority_ref", "cwd", "provider", "participant_refs"] {
            let mut input = json!({"purpose": "p", "source": "wiki:anchor:x"});
            input[extra] = json!("supplied");
            let refused = complete(
                &AikitHome::at(std::env::temp_dir().join("aikit-task-derive-unused")),
                &r("agent-session/unused"),
                input,
            )
            .unwrap_err();
            assert_eq!(refused.code(), UNDERIVABLE);
            assert!(refused.to_string().contains(extra), "{refused}");
        }
    }

    #[test]
    fn the_authority_is_the_one_the_determination_holds() {
        assert_eq!(
            one_authority(&json!(["authority:owner"])).unwrap(),
            r("authority:owner")
        );
        let none = one_authority(&json!([])).unwrap_err();
        assert!(none.contains("names no authority"), "{none}");
        let many = one_authority(&json!(["authority:a", "authority:b"])).unwrap_err();
        assert!(
            many.contains("2 authorities (authority:a, authority:b)"),
            "{many}"
        );
    }

    #[test]
    fn the_sessions_agency_and_journal_supply_its_agent_authority_provider_and_place() {
        use super::super::super::queue_tests::QueueWorld;
        let world = QueueWorld::new();
        let (_space, session) = world.attach_with(
            "derive",
            &["human:owner"],
            &["source/shared"],
            "caw_provider.py",
        );
        // Attached and admitted, never opened: its agent and authority are known,
        // the agent it works with and where are not.
        let held = standing(&world.home, &session).unwrap();
        assert_eq!(held.participant.unwrap(), r("agent:derive"));
        assert_eq!(held.authority.unwrap(), r("authority:project:delegation"));
        assert!(held.provider.unwrap_err().contains("never opened"));
        assert!(held.cwd.unwrap_err().contains("never opened"));
        assert!(held.central.unwrap_err().contains("not known"));
        // Opened on its configured agent in its working directory.
        let place = world.cwd.join("lane");
        fs::create_dir_all(&place).unwrap();
        let place = place.canonicalize().unwrap();
        EncounterStore::open(&world.home)
            .unwrap()
            .append(
                &session,
                &json!({"kind": "binding", "provider": "derive", "cwd": place}),
            )
            .unwrap();
        let held = standing(&world.home, &session).unwrap();
        let provider = held.provider.unwrap();
        assert_eq!(provider.id, "derive");
        assert_eq!(provider.argv[0], "python3", "the row as configured");
        assert_eq!(held.cwd.unwrap(), place);
        // The owner's task creation takes the short request and refuses, in its
        // words, what a session's standing cannot give, keeping nothing: here a
        // session with no Agency that never opened.
        let refused = EncounterService::configure_task(
            &world.home,
            &r("agent-session/never-opened"),
            json!({"purpose": "The lived-in world", "source": "wiki:anchor:lived-in"}),
            None,
        )
        .unwrap_err();
        assert_eq!(refused.code(), UNDERIVABLE, "{refused}");
        assert!(
            refused.to_string().contains(
                "its participant (central.participant_refs): the session has no Agency binding"
            ),
            "{refused}"
        );
        assert!(
            refused
                .to_string()
                .contains("its provider (provider): the session has never opened"),
            "{refused}"
        );
        assert_eq!(
            EncounterService::read_task(&world.home, &r("agent-session/never-opened")).unwrap(),
            Value::Null
        );
        // A session with no Agency names neither its agent nor an authority.
        let unbound = standing(&world.home, &r("agent-session/unbound")).unwrap();
        assert!(unbound
            .participant
            .unwrap_err()
            .contains("no Agency binding"));
        assert!(unbound.authority.unwrap_err().contains("no Agency binding"));
        // The agent it opened on, withdrawn since, is refused by name.
        EncounterService::deconfigure(&world.home, "derive").unwrap();
        let withdrawn = standing(&world.home, &session)
            .unwrap()
            .provider
            .unwrap_err();
        assert!(withdrawn.contains("derive"), "{withdrawn}");
    }
}
