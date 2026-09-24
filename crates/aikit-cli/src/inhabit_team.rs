//! `aikit inhabit` launches an orchestrator together with its team.
//!
//! When the Agent that inhabits a Position is the orchestrator of a Central
//! agent set (`central.agent-set/v1`, `orchestrator_agent_ref`, root
//! register), every other member of that set is projected as a Claude Code
//! subagent of the launched session. The member's Central profile
//! (`agent-profile.list`: purpose, skill_refs, governance_refs) names its
//! expression file among its `governance_refs`
//! (`…/Control/agents/expressions/<team>/members/<member>.md`); that file's
//! frontmatter gives the description, tools and skills and its body the
//! operating instructions.
//!
//! Where it lands, and why there. Neither `aikit compose` nor `aikit inhabit`
//! publishes an AIKit generation (only `aikit apply` does), so the team is not
//! written into a generation. It is written for exactly one inhabitation:
//!
//! ```text
//! $AIKIT_HOME/state/inhabitations/<generation>/
//!   receipt.json                          aikit.inhabitation-team-projection/v1
//!   claude/<set>-team/.claude-plugin/plugin.json
//!   claude/<set>-team/agents/<member>.md
//!   claude/<set>-team/skills/<skill>/…      each member skill AIKit catalogues
//! ```
//!
//! The team carries its own skills. Every skill a member names (expression
//! frontmatter, then profile `skill_refs`) that AIKit's catalogue can supply
//! is copied into the plugin, and the member's `skills:` names it by the only
//! name Claude Code gives a plugin skill, `<plugin>:<skill>`. So a team works
//! on a machine where its skills are catalogued but not active in any scope,
//! and nothing is activated for unrelated sessions. A skill the catalogue
//! cannot supply keeps its bare name and is disclosed, never silently dropped;
//! it does not refuse the team.
//!
//! and the Claude Code harness is launched with `--plugin-dir` naming that
//! directory, which Claude Code loads for that session only. Nothing is
//! written into the user's repository or `~/.claude`. `aikit inhabit
//! --release` removes the directory with the tenure; `--attach` hands the
//! same directory to the continued harness.
//!
//! The law: a team is projected whole or not at all. A member without a
//! profile, without an expression, or whose expression cannot be read is a
//! three-part refusal before anything is claimed. A harness other than
//! Claude Code is launched without the team and told so. When Central cannot
//! say whether the Agent orchestrates a set (an older `ctrl`), nothing is
//! projected and that is said too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use aikit_adapters::clients::claude_team::{
    claude_skill_name, parse_member_expression, render_plugin_manifest, render_subagent_with,
    subagent_name, team_plugin_name, TeamMember, MAX_MEMBER_EXPRESSION_BYTES,
};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};

use crate::inhabit::refusal;
use crate::inhabitation::{pick, Owners};

pub const TEAM_PROJECTION_SCHEMA: &str = "aikit.inhabitation-team-projection/v1";
const CENTRAL_ROOT_SOURCE_PREFIX: &str = "central:source:control:root:";

/// Which harness the body is launched into, read from its argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessTarget {
    /// `claude …`: session subagents are projected.
    ClaudeCode,
    /// Another harness: the team is disclosed as not supported.
    Other(String),
    /// No argv (the claim prints exports): the team is written and reported
    /// so the caller can pass `--plugin-dir` itself.
    Unspecified,
}

impl HarnessTarget {
    pub fn from_argv(argv: &[String]) -> Self {
        match argv.first() {
            None => Self::Unspecified,
            Some(program) => {
                let name = Path::new(program)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(program);
                if name == "claude" {
                    Self::ClaudeCode
                } else {
                    Self::Other(name.to_owned())
                }
            }
        }
    }
}

/// One set's members, rendered in memory before anything is claimed.
#[derive(Debug, Clone)]
pub struct SetProjection {
    pub agent_set_ref: String,
    pub agent_set_revision: String,
    pub plugin_name: String,
    /// `(relative path inside the plugin directory, contents, provenance)`.
    pub files: Vec<(PathBuf, String, Value)>,
    /// How many members became subagents.
    pub members: usize,
    /// Skill refs whose files travel inside the plugin.
    pub skills_bundled: Vec<String>,
    /// Skill refs a member names that the catalogue could not supply, each
    /// with the reason.
    pub skills_missing: Vec<String>,
}

/// One catalogued skill's files, ready to travel inside a team plugin.
#[derive(Debug, Clone)]
pub struct SkillPayload {
    /// The directory name the skill takes in the plugin (the capsule leaf).
    pub name: String,
    pub revision: Option<String>,
    /// `(path relative to the skill directory, contents)`.
    pub files: Vec<(PathBuf, String)>,
}

/// Where a team's skills come from. The CLI answers from AIKit's catalogue;
/// tests may answer from anything.
pub trait SkillSource {
    fn payload(&self, skill_ref: &str) -> std::result::Result<SkillPayload, String>;
}

/// Skills as AIKit's own catalogue holds them: every registry under the home
/// and every promoted skill source, the same load `aikit status` reads.
pub struct CatalogSkills {
    skills: BTreeMap<String, (PathBuf, Option<String>)>,
}

impl CatalogSkills {
    pub fn load(home: &AikitHome) -> Result<Self> {
        use aikit_core::catalog::Catalog as _;
        let load = crate::app::load_catalog(home, None)?;
        let skills = load
            .catalog
            .capsules()
            .into_iter()
            .filter_map(|capsule| {
                let section = capsule.skill()?;
                let root = capsule.root.clone()?;
                let payload_root = if section.root.trim().is_empty() {
                    "payload"
                } else {
                    section.root.as_str()
                };
                Some((
                    capsule.id.to_string(),
                    (
                        root.join(payload_root),
                        capsule.revision.as_ref().map(ToString::to_string),
                    ),
                ))
            })
            .collect();
        Ok(Self { skills })
    }
}

/// Largest single skill file carried into a team plugin.
const MAX_BUNDLED_SKILL_FILE_BYTES: u64 = 1024 * 1024;

impl SkillSource for CatalogSkills {
    fn payload(&self, skill_ref: &str) -> std::result::Result<SkillPayload, String> {
        let (payload, revision) = self
            .skills
            .get(skill_ref)
            .ok_or_else(|| "not in AIKit's catalogue on this machine".to_owned())?;
        let skill = aikit_adapters::clients::agent_skills::validate(payload).map_err(|error| {
            format!(
                "its payload is not a valid Agent Skill: {}",
                error.message()
            )
        })?;
        let mut files = Vec::new();
        for relative in &skill.files {
            let path = skill.root.join(relative);
            let size = std::fs::metadata(&path)
                .map_err(|error| format!("{} cannot be read: {error}", path.display()))?
                .len();
            if size > MAX_BUNDLED_SKILL_FILE_BYTES {
                return Err(format!(
                    "{} is {size} bytes; a bundled skill file is at most {MAX_BUNDLED_SKILL_FILE_BYTES}",
                    path.display()
                ));
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{} is not UTF-8 text: {error}", path.display()))?;
            files.push((PathBuf::from(relative), text));
        }
        Ok(SkillPayload {
            name: claude_skill_name(skill_ref),
            revision: revision.clone(),
            files,
        })
    }
}

/// What `aikit inhabit` does about the Agent's team.
#[derive(Debug, Clone, Default)]
pub enum TeamOutcome {
    /// The Agent orchestrates no agent set.
    #[default]
    None,
    /// The team resolved whole; written once the tenure opens.
    Planned {
        orchestrator_agent_ref: String,
        sets: Vec<SetProjection>,
    },
    /// Nothing is projected, and this says why.
    Disclosed(String),
}

impl TeamOutcome {
    pub fn disclosure(&self) -> Option<&str> {
        match self {
            Self::Disclosed(reason) => Some(reason),
            _ => None,
        }
    }
}

fn claimed_nothing() -> &'static str {
    "Nothing was claimed and no harness was started; AIKit does not launch a partial team."
}

/// Resolve and render the team `agent` orchestrates, before any claim.
pub fn resolve(
    owners: &Owners<'_>,
    central_root: Option<&Path>,
    agent: &str,
    target: &HarnessTarget,
    skip: bool,
    base_command: &str,
    skills: Option<&dyn SkillSource>,
) -> Result<TeamOutcome> {
    if skip {
        return Ok(TeamOutcome::Disclosed(
            "--no-team: no agent-set members were projected as subagents".into(),
        ));
    }
    let listing = owners.ctrl("central.agent-set.list", json!({ "scope": "root" }));
    let Some(listing) = listing.ok().cloned() else {
        return Ok(TeamOutcome::Disclosed(format!(
            "Central could not list agent sets ({}), so whether {agent} orchestrates a team is unknown; no subagents were projected",
            listing.describe()
        )));
    };
    let orchestrated: Vec<(String, String)> = listing
        .get("records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .pointer("/record/orchestrator_agent_ref")
                .and_then(Value::as_str)
                == Some(agent)
        })
        .filter_map(|entry| {
            Some((
                pick(entry, &["ref"])?,
                pick(entry, &["revision"]).unwrap_or_else(|| "unversioned".into()),
            ))
        })
        .collect();
    if orchestrated.is_empty() {
        return Ok(TeamOutcome::None);
    }
    let named = orchestrated
        .iter()
        .map(|(set, _)| set.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if let HarnessTarget::Other(program) = target {
        return Ok(TeamOutcome::Disclosed(format!(
            "{agent} orchestrates agent set {named}, but projecting its members as subagents is supported for Claude Code only; `{program}` is launched without them"
        )));
    }

    // Members of each set, orchestrator excluded (it is the session itself).
    let mut members_by_set = Vec::new();
    for (set, revision) in &orchestrated {
        let resolved = owners.ctrl(
            "central.agent-set.resolve",
            json!({ "scope": "root", "ref": set }),
        );
        let data = resolved.ok().cloned().ok_or_else(|| {
            refusal(
                "inhabit.team_unresolved",
                format!(
                    "{agent} orchestrates agent set {set}, and Central could not resolve its members: {}.",
                    resolved.describe()
                ),
                claimed_nothing(),
                format!(
                    "Check the set: ctrl --json action run central.agent-set.resolve '{{\"scope\":\"root\",\"ref\":\"{set}\"}}'; or launch without the team: {base_command} --no-team -- <harness argv>"
                ),
            )
        })?;
        let members: Vec<String> = data
            .get("authored_agents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|member| *member != agent)
            .map(str::to_owned)
            .collect();
        members_by_set.push((set.clone(), revision.clone(), members));
    }

    let profiles = owners.ctrl("agent-profile.list", json!({ "scope": "root" }));
    let profiles = profiles.ok().cloned().ok_or_else(|| {
        refusal(
            "inhabit.team_profiles_unavailable",
            format!(
                "{agent} orchestrates agent set {named}, and Central could not list the member profiles: {}.",
                profiles.describe()
            ),
            claimed_nothing(),
            format!("Make `ctrl --json action run agent-profile.list '{{\"scope\":\"root\"}}'` answer, or launch without the team: {base_command} --no-team -- <harness argv>"),
        )
    })?;
    let mut by_agent: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for entry in profiles
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let profile = entry.get("profile").unwrap_or(entry);
        if let Some(agent_ref) = pick(profile, &["agent_ref"]) {
            by_agent.entry(agent_ref).or_default().push(profile.clone());
        }
    }

    let mut problems = Vec::new();
    let mut sets = Vec::new();
    for (set, revision, members) in members_by_set {
        let mut team = Vec::new();
        for member in &members {
            match team_member(member, by_agent.get(member), central_root) {
                Ok(member) => team.push(member),
                Err(problem) => problems.push(problem),
            }
        }
        let plugin_name = team_plugin_name(&set);
        let mut files = vec![(
            PathBuf::from(".claude-plugin/plugin.json"),
            render_plugin_manifest(&set, &revision, agent, &team),
            json!({ "agent_set_ref": set, "agent_set_revision": revision }),
        )];
        // The team's skills travel with it, named as Claude Code names a
        // plugin skill. A skill the catalogue cannot supply keeps its bare
        // name and is disclosed.
        let mut bundled: BTreeMap<String, String> = BTreeMap::new();
        let mut skills_missing = Vec::new();
        let wanted: BTreeSet<String> = team.iter().flat_map(TeamMember::skill_refs).collect();
        for skill_ref in &wanted {
            let Some(source) = skills else {
                skills_missing.push(format!("{skill_ref} (no skill catalogue was available)"));
                continue;
            };
            match source.payload(skill_ref) {
                Ok(payload) => {
                    if bundled
                        .values()
                        .any(|name| name == &format!("{plugin_name}:{}", payload.name))
                    {
                        skills_missing.push(format!(
                            "{skill_ref} (another bundled skill already takes the name {})",
                            payload.name
                        ));
                        continue;
                    }
                    for (relative, text) in payload.files {
                        files.push((
                            PathBuf::from("skills").join(&payload.name).join(relative),
                            text,
                            json!({ "skill_ref": skill_ref, "skill_revision": payload.revision }),
                        ));
                    }
                    bundled.insert(skill_ref.clone(), format!("{plugin_name}:{}", payload.name));
                }
                Err(reason) => skills_missing.push(format!("{skill_ref} ({reason})")),
            }
        }
        for member in &team {
            match render_subagent_with(member, &bundled) {
                Ok(text) => files.push((
                    PathBuf::from("agents")
                        .join(format!("{}.md", subagent_name(&member.agent_ref)?)),
                    text,
                    json!({
                        "agent_ref": member.agent_ref,
                        "profile_ref": member.profile_ref,
                        "expression_ref": member.expression_ref,
                    }),
                )),
                Err(error) => problems.push(format!("{}: {}", member.agent_ref, error.message())),
            }
        }
        sets.push(SetProjection {
            agent_set_ref: set,
            agent_set_revision: revision,
            plugin_name,
            files,
            members: team.len(),
            skills_bundled: bundled.keys().cloned().collect(),
            skills_missing,
        });
    }
    if !problems.is_empty() {
        return Err(refusal(
            "inhabit.team_incomplete",
            format!(
                "{agent} orchestrates agent set {named}, and {} of its members cannot be projected: {}.",
                problems.len(),
                problems.join("; ")
            ),
            claimed_nothing(),
            format!(
                "Give each member a Central profile whose governance_refs name its expression file (Control/agents/expressions/<team>/members/<member>.md), then re-run; or launch without the team: {base_command} --no-team -- <harness argv>"
            ),
        ));
    }
    Ok(TeamOutcome::Planned {
        orchestrator_agent_ref: agent.to_owned(),
        sets,
    })
}

/// One member's profile and expression, or the plain reason it has none.
fn team_member(
    agent_ref: &str,
    profiles: Option<&Vec<Value>>,
    central_root: Option<&Path>,
) -> std::result::Result<TeamMember, String> {
    let profile = match profiles.map(Vec::as_slice).unwrap_or_default() {
        [profile] => profile,
        [] => return Err(format!("{agent_ref} has no Central profile")),
        many => {
            return Err(format!(
                "{agent_ref} has {} Central profiles ({})",
                many.len(),
                many.iter()
                    .filter_map(|profile| pick(profile, &["ref"]))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let profile_ref = pick(profile, &["ref"]).unwrap_or_else(|| "an unnamed profile".into());
    let strings = |key: &str| -> Vec<String> {
        profile
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let governance_refs = strings("governance_refs");
    let expressions: Vec<&String> = governance_refs
        .iter()
        .filter(|reference| {
            reference.starts_with(CENTRAL_ROOT_SOURCE_PREFIX)
                && reference.contains("/members/")
                && reference.ends_with(".md")
        })
        .collect();
    let expression_ref = match expressions.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(format!(
                "{agent_ref}'s profile {profile_ref} names no member expression file (…/members/<member>.md) among its governance_refs"
            ))
        }
        many => {
            return Err(format!(
                "{agent_ref}'s profile {profile_ref} names {} member expression files ({})",
                many.len(),
                many.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ")
            ))
        }
    };
    let Some(root) = central_root else {
        return Err(format!(
            "{expression_ref} cannot be read: this cwd stands in no Central root"
        ));
    };
    let relative = Path::new(&expression_ref[CENTRAL_ROOT_SOURCE_PREFIX.len()..]);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{expression_ref} is not a path inside the Central root"
        ));
    }
    let path = root.join(relative);
    let text = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_MEMBER_EXPRESSION_BYTES => {
            std::fs::read_to_string(&path)
                .map_err(|error| format!("{} cannot be read: {error}", path.display()))?
        }
        Ok(metadata) if metadata.is_file() => {
            return Err(format!(
                "{} is {} bytes; a member expression is at most {MAX_MEMBER_EXPRESSION_BYTES}",
                path.display(),
                metadata.len()
            ))
        }
        Ok(_) => return Err(format!("{} is not a file", path.display())),
        Err(error) => {
            return Err(format!(
                "{agent_ref}'s expression {} does not exist ({error})",
                path.display()
            ))
        }
    };
    let expression = parse_member_expression(&text)
        .map_err(|error| format!("{}: {}", path.display(), error.message()))?;
    Ok(TeamMember {
        agent_ref: agent_ref.to_owned(),
        profile_ref,
        purpose: pick(profile, &["purpose"]),
        profile_skill_refs: strings("skill_refs"),
        governance_refs,
        expression_ref,
        expression,
    })
}

/// This inhabitation's own directory, named from its occupant generation.
pub fn inhabitation_dir(home: &AikitHome, generation_ref: &str) -> PathBuf {
    let key: String = generation_ref
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    home.state()
        .join("inhabitations")
        .join(key.trim_matches('.'))
}

fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

/// Write a planned team for the tenure that just opened, replacing whatever
/// an earlier launch of the same generation left. Returns the receipt.
pub fn write(
    home: &AikitHome,
    position_ref: &str,
    generation_ref: &str,
    orchestrator_agent_ref: &str,
    sets: &[SetProjection],
) -> std::io::Result<Value> {
    let dir = inhabitation_dir(home, generation_ref);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    let mut plugin_dirs = Vec::new();
    let mut files = Vec::new();
    for set in sets {
        let plugin_dir = dir.join("claude").join(&set.plugin_name);
        for (relative, text, provenance) in &set.files {
            let path = plugin_dir.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, text)?;
            let mut entry = json!({
                "path": path.display().to_string(),
                "digest": digest(text),
            });
            if let (Some(entry), Some(provenance)) = (entry.as_object_mut(), provenance.as_object())
            {
                entry.extend(provenance.clone());
            }
            files.push(entry);
        }
        plugin_dirs.push(json!({
            "agent_set_ref": set.agent_set_ref,
            "agent_set_revision": set.agent_set_revision,
            "plugin_dir": plugin_dir.display().to_string(),
            "members": set.members,
            "skills_bundled": set.skills_bundled,
            "skills_missing": set.skills_missing,
        }));
    }
    let receipt = json!({
        "schema": TEAM_PROJECTION_SCHEMA,
        "position_ref": position_ref,
        "generation_ref": generation_ref,
        "orchestrator_agent_ref": orchestrator_agent_ref,
        "harness": "claude-code",
        "directory": dir.display().to_string(),
        "plugins": plugin_dirs,
        "files": files,
        "written_at_unix_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as u64)
            .unwrap_or(0),
        "removed_by": format!("aikit inhabit --release --position {position_ref} --generation {generation_ref}"),
    });
    std::fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap_or_default(),
    )?;
    Ok(receipt)
}

/// The receipt an earlier launch of this generation wrote, if any.
pub fn existing(home: &AikitHome, generation_ref: &str) -> Option<Value> {
    let path = inhabitation_dir(home, generation_ref).join("receipt.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Remove this generation's projection with its tenure. `None` when nothing
/// was projected for it.
pub fn remove(home: &AikitHome, generation_ref: &str) -> Result<Option<Value>> {
    let dir = inhabitation_dir(home, generation_ref);
    if !dir.exists() {
        return Ok(None);
    }
    let receipt = existing(home, generation_ref);
    std::fs::remove_dir_all(&dir).map_err(|error| {
        AikitError::new(
            "inhabit.team_projection_remove_failed",
            format!(
                "the tenure was released, but its team projection {} could not be removed: {error}",
                dir.display()
            ),
        )
    })?;
    Ok(Some(json!({
        "directory": dir.display().to_string(),
        "removed": receipt
            .as_ref()
            .and_then(|receipt| receipt.get("files"))
            .and_then(Value::as_array)
            .map(|files| files.iter().filter_map(|file| file.get("path").cloned()).collect::<Vec<_>>())
            .unwrap_or_default(),
    })))
}

/// The plugin directories a receipt names.
pub fn plugin_dirs(receipt: &Value) -> Vec<String> {
    receipt
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| pick(plugin, &["plugin_dir"]))
        .collect()
}

/// The harness argv with one `--plugin-dir` per team plugin, placed right
/// after the program so a trailing prompt or `--` in the argv is untouched.
pub fn with_plugin_dirs(argv: &[String], dirs: &[String]) -> Vec<String> {
    let Some((program, rest)) = argv.split_first() else {
        return argv.to_vec();
    };
    let mut out = vec![program.clone()];
    for dir in dirs {
        out.push("--plugin-dir".into());
        out.push(dir.clone());
    }
    out.extend(rest.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_claude_argv_is_a_claude_code_target() {
        let argv = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            HarnessTarget::from_argv(&argv(&["claude", "-p", "x"])),
            HarnessTarget::ClaudeCode
        );
        assert_eq!(
            HarnessTarget::from_argv(&argv(&["/opt/bin/claude"])),
            HarnessTarget::ClaudeCode
        );
        assert_eq!(
            HarnessTarget::from_argv(&argv(&["codex", "exec"])),
            HarnessTarget::Other("codex".into())
        );
        assert_eq!(HarnessTarget::from_argv(&[]), HarnessTarget::Unspecified);
        assert_eq!(
            with_plugin_dirs(&argv(&["claude", "-p", "go"]), &["/a".into(), "/b".into()]),
            argv(&[
                "claude",
                "--plugin-dir",
                "/a",
                "--plugin-dir",
                "/b",
                "-p",
                "go"
            ])
        );
    }

    #[test]
    fn a_generation_ref_names_one_directory_under_the_home_state() {
        let home = AikitHome::at("/tmp/aikit-home");
        assert_eq!(
            inhabitation_dir(&home, "actuation:generation:0a/../b"),
            PathBuf::from("/tmp/aikit-home/state/inhabitations/actuation-generation-0a-..-b")
        );
    }
}
