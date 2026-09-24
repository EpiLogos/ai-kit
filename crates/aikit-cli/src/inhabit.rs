//! `aikit inhabit`: the launcher seam that stamps a Position occupancy into a
//! body (O:I `WORLD-INHABITATION-V1` §2: "Launch stamps `OI_POSITION_REF` and
//! `OI_OCCUPANT_GENERATION` into the body's environment").
//!
//! AIKit resolves; the owners decide. Central answers the Position and its
//! eligible Agents, Actuation opens (or refuses) the tenure, and the harness
//! is exec'd with the two identity variables added to the environment it
//! already carries. Leaving is explicit: nothing is released when the harness
//! exits — `aikit inhabit --release` ends the tenure this body holds.
//!
//! When the inhabiting Agent orchestrates a Central agent set, its team rides
//! along into a Claude Code harness as session subagents
//! ([`crate::inhabit_team`]); the team is resolved whole before anything is
//! claimed and removed when the tenure is released.

use std::path::{Path, PathBuf};

use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};

use crate::inhabit_team::{HarnessTarget, TeamOutcome};
use crate::inhabitation::{pick, Answer, Owners, GENERATION_VAR, POSITION_VAR};

/// A three-part refusal: the current state, what did not happen, and the exact
/// next lawful command.
pub fn refusal(code: &'static str, fact: String, consequence: &str, action: String) -> AikitError {
    AikitError::new(code, format!("{fact}\n  {consequence}\n  {action}"))
        .with("fact", fact)
        .with("consequence", consequence.to_owned())
        .with("action", action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimMode {
    /// Expect a vacant Position (the default).
    Vacant,
    /// Take over from the current occupant with continuity.
    Handover,
    /// Replace any current occupant with a fresh one (no continuity).
    Fresh,
}

#[derive(Debug, Clone)]
pub struct InhabitRequest {
    pub position: String,
    pub agent: Option<String>,
    pub agency: Option<String>,
    pub mode: ClaimMode,
    pub reason: String,
    pub agent_session: Option<String>,
    pub session_space: Option<String>,
    pub harness_composition: Option<String>,
    pub model: Option<String>,
    pub cwd: PathBuf,
    /// The harness argv after `--`; empty when only the exports are asked.
    pub harness_argv: Vec<String>,
    /// Launch without the Agent's agent-set team (`--no-team`).
    pub no_team: bool,
}

#[derive(Debug, Clone)]
pub struct Claimed {
    pub position_ref: String,
    pub generation_ref: String,
    pub env: Vec<(String, String)>,
    pub claim: Value,
    /// The Agent's agent-set team, resolved before the claim.
    pub team: TeamOutcome,
}

fn owner_refusal(code: &'static str, answer: &Answer, consequence: &str) -> AikitError {
    let action = match answer {
        Answer::Refused { message, .. } => message
            .split(" · ")
            .last()
            .unwrap_or("read the owner's refusal above")
            .to_owned(),
        _ => "install or update the owner binary named above, then re-run".to_owned(),
    };
    refusal(code, answer.describe(), consequence, action)
}

/// `@handle` → the one Position carrying it in this Project World (own and
/// inherited); a ref passes through.
fn resolve_position(
    owners: &Owners<'_>,
    raw: &str,
    project: Option<&str>,
) -> Result<(String, Value)> {
    let position_ref = if let Some(handle) = raw.strip_prefix('@') {
        let listing = owners.ctrl(
            "central.position.list",
            project
                .map(|p| json!({ "project": p }))
                .unwrap_or_else(|| json!({})),
        );
        let data = listing.ok().ok_or_else(|| {
            owner_refusal(
                "inhabit.position_unresolved",
                &listing,
                "Nothing was claimed.",
            )
        })?;
        let matches: Vec<String> = ["positions", "inherited"]
            .iter()
            .filter_map(|key| data.get(*key).and_then(Value::as_array))
            .flatten()
            // Central wraps each listed Position as `{record, source}`.
            .map(|entry| {
                entry
                    .get("record")
                    .filter(|r| r.is_object())
                    .unwrap_or(entry)
            })
            .filter(|record| {
                pick(record, &["handle"]).as_deref() == Some(format!("@{handle}").as_str())
            })
            .filter_map(|record| pick(record, &["ref"]))
            .collect();
        match matches.as_slice() {
            [one] => one.clone(),
            [] => {
                return Err(refusal(
                    "inhabit.position_unknown",
                    format!("No Position in this World carries the handle @{handle}."),
                    "Nothing was claimed.",
                    format!(
                        "List them: ctrl --json action run central.position.list '{}'",
                        project
                            .map(|p| format!("{{\"project\":\"{p}\"}}"))
                            .unwrap_or_else(|| "{}".into())
                    ),
                ))
            }
            many => {
                return Err(refusal(
                    "inhabit.position_ambiguous",
                    format!(
                        "@{handle} names {} Positions: {}.",
                        many.len(),
                        many.join(", ")
                    ),
                    "Nothing was claimed.",
                    "Re-run with --position <one full central:position:… ref>.".into(),
                ))
            }
        }
    } else {
        raw.to_owned()
    };
    let read = owners.ctrl(
        "central.position.read",
        json!({ "position_ref": position_ref }),
    );
    let data = read.ok().ok_or_else(|| {
        owner_refusal("inhabit.position_unresolved", &read, "Nothing was claimed.")
    })?;
    let record = data.get("record").cloned().unwrap_or_else(|| data.clone());
    Ok((position_ref, record))
}

/// A read-only default for the Agency: the one active, already-admitted AIKit
/// encounter agency for this Agent (narrowed to this Project World when the
/// Agent has several). Minting is a session-bound mutation, so it is never
/// done implicitly here.
pub fn existing_agency(home: &AikitHome, agent: &str, project: Option<&str>) -> Vec<String> {
    let dir = home.state().join("encounter-agencies");
    let mut bindings: Vec<(String, String)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| {
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            if !metadata.is_file() || metadata.len() > 1024 * 1024 {
                return None;
            }
            let value: Value = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
            if value["active"] != true || pick(&value, &["agent_ref"]).as_deref() != Some(agent) {
                return None;
            }
            Some((
                pick(&value, &["agency_ref"])?,
                pick(&value, &["world_ref"]).unwrap_or_default(),
            ))
        })
        .collect();
    let distinct = |bindings: &[(String, String)]| {
        let mut agencies: Vec<String> = bindings.iter().map(|(agency, _)| agency.clone()).collect();
        agencies.sort();
        agencies.dedup();
        agencies
    };
    if distinct(&bindings).len() > 1 {
        if let Some(project) = project {
            bindings
                .retain(|(_, world)| world == project || world == &format!("project:{project}"));
        }
    }
    distinct(&bindings)
}

/// Resolve the Position, Agent and Agency, and open the tenure through
/// Actuation. Every refusal names what is, what did not happen, and the next
/// command.
pub fn claim(
    owners: &Owners<'_>,
    home: Option<&AikitHome>,
    request: &InhabitRequest,
) -> Result<Claimed> {
    let here = owners.ctrl(
        "central.world.here",
        json!({ "cwd": request.cwd.display().to_string() }),
    );
    let here_root = here
        .ok()
        .and_then(|data| pick(&data["local_world"], &["root"]))
        .map(PathBuf::from);
    let (project, workcell) = match here.ok() {
        Some(data) => (
            pick(&data["project_world"], &["name"]),
            data["workcells"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|cell| pick(cell, &["role"]).as_deref() == Some("current"))
                .and_then(|cell| pick(cell, &["ref"])),
        ),
        None => (None, None),
    };
    let (position_ref, record) = resolve_position(owners, &request.position, project.as_deref())?;
    let base = format!("aikit inhabit --position {position_ref} --reason <why>");

    let agent = match &request.agent {
        Some(agent) => agent.clone(),
        None => {
            let eligible: Vec<String> = record
                .get("eligible_agent_refs")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            match eligible.as_slice() {
                [one] => one.clone(),
                [] => {
                    return Err(refusal(
                        "inhabit.agent_required",
                        format!("{position_ref} names no eligible Agent."),
                        "Nothing was claimed.",
                        format!("{base} --agent <agent ref> -- <harness argv>"),
                    ))
                }
                many => {
                    return Err(refusal(
                        "inhabit.agent_ambiguous",
                        format!(
                            "{position_ref} names {} eligible Agents: {}.",
                            many.len(),
                            many.join(", ")
                        ),
                        "Nothing was claimed.",
                        format!("{base} --agent <one of them> -- <harness argv>"),
                    ))
                }
            }
        }
    };
    let agency = match &request.agency {
        Some(agency) => agency.clone(),
        None => {
            let found = home
                .map(|home| existing_agency(home, &agent, project.as_deref()))
                .unwrap_or_default();
            match found.as_slice() {
                [one] => one.clone(),
                [] => {
                    return Err(refusal(
                        "inhabit.agency_required",
                        format!("AIKit holds no admitted Agency for {agent}."),
                        "Nothing was claimed.",
                        format!(
                            "{base} --agent {agent} --agency <agency ref> -- <harness argv>   (or mint one: aikit session-space encounter-agency-mint --agent-session <agent-session/…> --project-cwd {} --agent-ref {agent})",
                            request.cwd.display()
                        ),
                    ))
                }
                many => {
                    return Err(refusal(
                        "inhabit.agency_ambiguous",
                        format!("{agent} has {} admitted Agencies: {}.", many.len(), many.join(", ")),
                        "Nothing was claimed.",
                        format!("{base} --agent {agent} --agency <one of them> -- <harness argv>"),
                    ))
                }
            }
        }
    };

    // The team is resolved whole before anything is claimed: a member that
    // cannot be projected refuses here, with no tenure opened.
    let central_root = owners.central_root.clone().or(here_root);
    // The team's skills come from AIKit's own catalogue; an unreadable
    // catalogue leaves every member skill disclosed as missing, never refused.
    let catalog = home
        .as_ref()
        .and_then(|home| crate::inhabit_team::CatalogSkills::load(home).ok());
    let team = crate::inhabit_team::resolve(
        owners,
        central_root.as_deref(),
        &agent,
        &HarnessTarget::from_argv(&request.harness_argv),
        request.no_team,
        &base,
        catalog
            .as_ref()
            .map(|catalog| catalog as &dyn crate::inhabit_team::SkillSource),
    )?;
    if matches!(team, TeamOutcome::Planned { .. }) && home.is_none() {
        return Err(refusal(
            "inhabit.team_home_unresolved",
            format!("{agent} orchestrates an agent set, and no AIKit home could be resolved to write its team into."),
            "Nothing was claimed and no harness was started.",
            format!("Set AIKIT_HOME (or HOME), or launch without the team: {base} --no-team -- <harness argv>"),
        ));
    }

    let mut expectation: Vec<String> = Vec::new();
    match request.mode {
        ClaimMode::Vacant => expectation.push("--expect-vacant".into()),
        ClaimMode::Handover | ClaimMode::Fresh => {
            let read = owners.occupancy(&["read", "--position", &position_ref]);
            let data = read.ok().ok_or_else(|| {
                owner_refusal(
                    "inhabit.occupancy_unreadable",
                    &read,
                    "Nothing was claimed.",
                )
            })?;
            let current = data
                .get("current")
                .and_then(|current| pick(current, &["generation_ref", "generationRef"]));
            match (current, request.mode) {
                (Some(generation), mode) => {
                    expectation.extend(["--expect-generation".into(), generation]);
                    if mode == ClaimMode::Fresh {
                        expectation.extend(["--kind".into(), "fresh".into()]);
                    }
                }
                (None, ClaimMode::Fresh) => {
                    expectation.extend(["--expect-vacant".into(), "--kind".into(), "fresh".into()])
                }
                (None, _) => {
                    return Err(refusal(
                        "inhabit.nothing_to_hand_over",
                        format!(
                            "{position_ref} is vacant; there is no occupant to hand over from."
                        ),
                        "Nothing was claimed.",
                        format!("{base} -- <harness argv>   (drop --handover)"),
                    ))
                }
            }
        }
    }

    let mut args: Vec<String> = vec![
        "claim".into(),
        "--position".into(),
        position_ref.clone(),
        "--agent".into(),
        agent,
        "--agency".into(),
        agency,
    ];
    for (flag, value) in [
        ("--agent-session", &request.agent_session),
        ("--session-space", &request.session_space),
        ("--harness-composition", &request.harness_composition),
        ("--model", &request.model),
        ("--workcell", &workcell),
    ] {
        if let Some(value) = value {
            args.push(flag.into());
            args.push(value.clone());
        }
    }
    args.push("--reason".into());
    args.push(request.reason.clone());
    args.extend(expectation);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let answer = owners.occupancy(&refs);
    let data = answer.ok().cloned().ok_or_else(|| {
        owner_refusal(
            "inhabit.claim_refused",
            &answer,
            "No occupancy was opened and no harness was started.",
        )
    })?;
    let generation_ref = pick(&data["env"], &[GENERATION_VAR])
        .or_else(|| {
            data.get("tenure")
                .and_then(|t| pick(t, &["generation_ref"]))
        })
        .ok_or_else(|| {
            AikitError::new(
                "inhabit.claim_unreadable",
                "Actuation answered the claim without an occupant generation",
            )
        })?;
    let position = pick(&data["env"], &[POSITION_VAR]).unwrap_or_else(|| position_ref.clone());
    Ok(Claimed {
        env: vec![
            (POSITION_VAR.to_owned(), position.clone()),
            (GENERATION_VAR.to_owned(), generation_ref.clone()),
        ],
        position_ref: position,
        generation_ref,
        claim: data,
        team,
    })
}

/// Continue the tenure this occupant already holds: the generation (flag or
/// stamped env) must still be the Position's current one, verified by
/// Actuation. Returns the stamps to exec the harness with; claims nothing.
pub fn attach(
    owners: &Owners<'_>,
    position: &str,
    generation: Option<&str>,
    cwd: &Path,
) -> Result<Claimed> {
    let position_ref = position_ref_for(owners, position, cwd)?;
    let generation = held_generation(
        &position_ref,
        generation,
        "Nothing was launched.",
        "--attach",
    )?;
    let answer = owners.occupancy(&[
        "verify",
        "--position",
        &position_ref,
        "--generation",
        &generation,
    ]);
    let verified = answer.ok().cloned().ok_or_else(|| {
        owner_refusal(
            "inhabit.attach_refused",
            &answer,
            "Nothing was launched; a superseded or unknown generation holds no standing.",
        )
    })?;
    Ok(Claimed {
        env: vec![
            (POSITION_VAR.to_owned(), position_ref.clone()),
            (GENERATION_VAR.to_owned(), generation.clone()),
        ],
        position_ref,
        generation_ref: generation,
        claim: verified,
        team: TeamOutcome::None,
    })
}

fn position_ref_for(owners: &Owners<'_>, position: &str, cwd: &Path) -> Result<String> {
    if !position.starts_with('@') {
        return Ok(position.to_owned());
    }
    let here = owners.ctrl(
        "central.world.here",
        json!({ "cwd": cwd.display().to_string() }),
    );
    let project = here
        .ok()
        .and_then(|data| pick(&data["project_world"], &["name"]));
    Ok(resolve_position(owners, position, project.as_deref())?.0)
}

/// The generation this body holds for `position_ref`: the flag, else the
/// stamped `OI_OCCUPANT_GENERATION` when it was stamped for this Position.
fn held_generation(
    position_ref: &str,
    generation: Option<&str>,
    consequence: &str,
    verb: &str,
) -> Result<String> {
    if let Some(generation) = generation {
        return Ok(generation.to_owned());
    }
    let stamped_position = std::env::var(POSITION_VAR).ok();
    match std::env::var(GENERATION_VAR)
        .ok()
        .filter(|g| !g.trim().is_empty())
    {
        Some(generation) if stamped_position.as_deref() == Some(position_ref) => Ok(generation),
        _ => Err(refusal(
            "inhabit.no_held_generation",
            format!("This body carries no occupant generation stamped for {position_ref}."),
            consequence,
            format!("aikit inhabit {verb} --position {position_ref} --generation <the generation you hold>"),
        )),
    }
}

/// End the tenure this body holds. The generation comes from `--generation`
/// or the stamped `OI_OCCUPANT_GENERATION`; a body that holds nothing refuses.
/// End the tenure this body holds, and remove the team projected for it.
pub fn release(
    owners: &Owners<'_>,
    home: Option<&AikitHome>,
    position: &str,
    generation: Option<&str>,
    reason: &str,
    cwd: &Path,
) -> Result<Value> {
    let position_ref = position_ref_for(owners, position, cwd)?;
    let generation = held_generation(
        &position_ref,
        generation,
        "Nothing was released.",
        "--release",
    )?;
    let answer = owners.occupancy(&[
        "release",
        "--position",
        &position_ref,
        "--generation",
        &generation,
        "--reason",
        reason,
    ]);
    let mut released = answer.ok().cloned().ok_or_else(|| {
        owner_refusal("inhabit.release_refused", &answer, "Nothing was released.")
    })?;
    // The tenure is over; its team leaves with it. A removal that fails is
    // reported beside the release, never as a failed release.
    if let Some(home) = home {
        match crate::inhabit_team::remove(home, &generation) {
            Ok(Some(removed)) => released["team_projection"] = removed,
            Ok(None) => {}
            Err(error) => {
                released["team_projection"] = json!({ "error": error.to_string() });
            }
        }
    }
    Ok(released)
}

/// Replace this process with the harness, its environment extended by the
/// occupancy stamps. Returns only if the exec itself failed.
pub fn exec_harness(argv: &[String], claimed: &Claimed) -> AikitError {
    let Some((program, args)) = argv.split_first() else {
        return AikitError::new("inhabit.no_harness", "no harness argv after `--`");
    };
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .envs(claimed.env.iter().map(|(k, v)| (k, v)));
    #[cfg(unix)]
    let error = {
        use std::os::unix::process::CommandExt;
        command.exec()
    };
    #[cfg(not(unix))]
    let error = match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    };
    refusal(
        "inhabit.exec_failed",
        format!("`{program}` could not be started: {error}."),
        "The occupancy was claimed and stays open (leaving is explicit).",
        format!(
            "Release it: aikit inhabit --release --position {} --generation {}",
            claimed.position_ref, claimed.generation_ref
        ),
    )
}
