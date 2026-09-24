//! A Central agent set's members as Claude Code session subagents.
//!
//! When a body inhabits a Position as the orchestrator of an agent set
//! (`central.agent-set/v1`, `orchestrator_agent_ref`), each other member of
//! the set becomes a Claude Code subagent for that body's session. This
//! module is the pure half of that projection: it reads one member's
//! expression file (YAML frontmatter + operating instructions), joins it with
//! the member's Central profile, and renders the subagent Markdown file and
//! the session plugin manifest that carries them. Resolving the set, the
//! profiles and the files through Central, writing them and removing them are
//! the caller's (`aikit inhabit`).
//!
//! The files are laid out as a Claude Code plugin directory — manifest at
//! `.claude-plugin/plugin.json`, one `agents/<member>.md` per member —
//! because `claude --plugin-dir <dir>` loads a plugin for one session only.
//! That keeps the team out of the user's repository and out of
//! `~/.claude/agents`, and lets it disappear with the tenure.

use std::collections::BTreeSet;

use aikit_core::{AikitError, Result};
use serde::Deserialize;
use serde_json::{json, Value};

/// Largest member expression file read (they are operating instructions,
/// not corpora).
pub const MAX_MEMBER_EXPRESSION_BYTES: u64 = 256 * 1024;

/// The frontmatter of a member expression file
/// (`Control/agents/expressions/<team>/members/<member>.md`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MemberFrontmatter {
    /// The member's agent slug (`anima-nous` for `agent/anima-nous`).
    pub name: String,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub cf: Option<String>,
    pub description: String,
    /// Claude Code tool names the member may use.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Native AIKit skill ids (`skill/ql/vak-evaluate`).
    #[serde(default)]
    pub skills: Vec<String>,
    /// Where the instructions were authored (`{repo, path, blob}`).
    #[serde(default)]
    pub source: Option<serde_yaml::Value>,
}

/// One parsed member expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberExpression {
    pub frontmatter: MemberFrontmatter,
    /// The operating instructions: everything after the frontmatter.
    pub body: String,
}

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("claude_team.expression_invalid", message.into())
}

/// Split `---\n<yaml>\n---\n<body>` and parse the YAML.
pub fn parse_member_expression(text: &str) -> Result<MemberExpression> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| invalid("the file does not open with a `---` frontmatter block"))?;
    let (yaml, body) = rest
        .split_once("\n---\n")
        .or_else(|| rest.split_once("\r\n---\r\n"))
        .or_else(|| rest.strip_suffix("\n---").map(|yaml| (yaml, "")))
        .ok_or_else(|| invalid("the frontmatter block is never closed with `---`"))?;
    let frontmatter: MemberFrontmatter = serde_yaml::from_str(yaml).map_err(|error| {
        invalid(format!(
            "the frontmatter is not a member expression: {error}"
        ))
    })?;
    if frontmatter.description.trim().is_empty() {
        return Err(invalid("the frontmatter `description` is empty"));
    }
    let body = body.trim_matches('\n').to_owned();
    if body.trim().is_empty() {
        return Err(invalid(
            "the expression carries no operating instructions after its frontmatter",
        ));
    }
    Ok(MemberExpression { frontmatter, body })
}

/// `agent/anima-nous` → `anima-nous`, the subagent name Claude Code shows.
/// Refuses anything that is not a slash-form agent ref with a lowercase,
/// digit and hyphen slug.
pub fn subagent_name(agent_ref: &str) -> Result<String> {
    let slug = agent_ref.strip_prefix("agent/").unwrap_or_default();
    let valid = !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(slug.to_owned())
    } else {
        Err(AikitError::new(
            "claude_team.member_name_invalid",
            format!(
                "{agent_ref:?} cannot name a Claude Code subagent: a member must be `agent/<slug>` with a lowercase, digit and hyphen slug"
            ),
        ))
    }
}

/// A native AIKit skill id as Claude Code names the projected skill: the
/// id's last segment, the same default export name the Claude projection
/// gives every skill (`skill/ql/vak-evaluate` → `vak-evaluate`).
pub fn claude_skill_name(skill_ref: &str) -> String {
    skill_ref.rsplit('/').next().unwrap_or(skill_ref).to_owned()
}

/// Everything one member contributes: its Central profile and its expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMember {
    pub agent_ref: String,
    pub profile_ref: String,
    pub purpose: Option<String>,
    pub profile_skill_refs: Vec<String>,
    pub governance_refs: Vec<String>,
    /// The governance ref of the expression file the instructions came from.
    pub expression_ref: String,
    pub expression: MemberExpression,
}

impl TeamMember {
    /// The member's skills as Claude Code names them: the expression's first,
    /// then any further ones the profile grants, each once.
    pub fn claude_skills(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.expression
            .frontmatter
            .skills
            .iter()
            .chain(self.profile_skill_refs.iter())
            .map(|skill| claude_skill_name(skill))
            .filter(|name| !name.is_empty() && seen.insert(name.clone()))
            .collect()
    }
}

/// A frontmatter scalar Claude Code reads back exactly: a JSON string is a
/// valid YAML double-quoted scalar.
fn yaml_string(value: &str) -> String {
    Value::String(value.to_owned()).to_string()
}

/// Render one member as a Claude Code subagent file: frontmatter `name`,
/// `description`, `tools`, `skills`; body = the member's operating
/// instructions, then where they and the profile came from.
pub fn render_subagent(member: &TeamMember) -> Result<String> {
    let name = subagent_name(&member.agent_ref)?;
    if member.expression.frontmatter.name != name {
        return Err(AikitError::new(
            "claude_team.expression_mismatch",
            format!(
                "{} is the expression named by {}'s profile, but it names {:?}",
                member.expression_ref, member.agent_ref, member.expression.frontmatter.name
            ),
        ));
    }
    let description = member
        .expression
        .frontmatter
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut text = format!(
        "---\nname: {name}\ndescription: {}\n",
        yaml_string(&description)
    );
    let tools: Vec<&str> = member
        .expression
        .frontmatter
        .tools
        .iter()
        .map(|tool| tool.trim())
        .filter(|tool| !tool.is_empty())
        .collect();
    if !tools.is_empty() {
        text.push_str(&format!("tools: {}\n", tools.join(", ")));
    }
    let skills = member.claude_skills();
    if !skills.is_empty() {
        text.push_str(&format!("skills: {}\n", skills.join(", ")));
    }
    text.push_str("---\n\n");
    text.push_str(&member.expression.body);
    text.push_str("\n\n---\n\n");
    text.push_str(&format!(
        "Projected by AIKit for one inhabitation from {} ({}) and {}. \
         It is regenerated at each launch and removed when the tenure is released; \
         change the profile or the expression, not this file.\n",
        member.profile_ref, member.agent_ref, member.expression_ref
    ));
    if let Some(purpose) = member.purpose.as_deref().filter(|p| !p.trim().is_empty()) {
        text.push_str(&format!("\nProfile purpose: {}\n", purpose.trim()));
    }
    let governance: Vec<&String> = member
        .governance_refs
        .iter()
        .filter(|reference| *reference != &member.expression_ref)
        .collect();
    if !governance.is_empty() {
        text.push_str("\nFurther governance (read on demand):\n");
        for reference in governance {
            text.push_str(&format!("- {reference}\n"));
        }
    }
    Ok(text)
}

/// A plugin name for a set: kebab-case, `<set>-team`.
pub fn team_plugin_name(agent_set_ref: &str) -> String {
    let mut name = String::new();
    for c in agent_set_ref.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            name.push(c);
        } else if !name.ends_with('-') && !name.is_empty() {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    format!("{}-team", if name.is_empty() { "agent-set" } else { name })
}

/// The session plugin manifest (`.claude-plugin/plugin.json`).
pub fn render_plugin_manifest(
    agent_set_ref: &str,
    agent_set_revision: &str,
    orchestrator_agent_ref: &str,
    members: &[TeamMember],
) -> String {
    let manifest = json!({
        "name": team_plugin_name(agent_set_ref),
        "version": "0.0.0",
        "description": format!(
            "The {} members of Central agent set {agent_set_ref} (revision {agent_set_revision}), orchestrated by {orchestrator_agent_ref}; projected by AIKit for one inhabitation.",
            members.len()
        ),
        "author": { "name": "AIKit" },
    });
    let mut text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOUS: &str = r#"---
name: anima-nous
team: anima
cf: CF1
description: "Nous, the epistemic clearing — invoked fresh before a task; never the task executor."
tools: [Read, Grep, Glob, Bash]
skills: [skill/ql/vak-coordinate-frame, skill/ql/gnosis-retrieve, skill/personal/brainstorming]
source: {repo: EpiLogos/Epi-Logos-C-Experiments, path: "Body/S/agents/nous.md", blob: 6e1bd6f2}
---

# Nous

You are Nous. You open; you do not conclude.
"#;

    fn member(expression: &str) -> TeamMember {
        TeamMember {
            agent_ref: "agent/anima-nous".into(),
            profile_ref: "profile/anima-nous".into(),
            purpose: Some("The clearing before the form.".into()),
            profile_skill_refs: vec![
                "skill/ql/gnosis-retrieve".into(),
                "skill/ql/darshana".into(),
            ],
            governance_refs: vec![
                "central:source:control:root:Control/agents/expressions/anima/TEAM.md".into(),
                "central:source:control:root:Control/agents/expressions/anima/members/nous.md"
                    .into(),
            ],
            expression_ref:
                "central:source:control:root:Control/agents/expressions/anima/members/nous.md"
                    .into(),
            expression: parse_member_expression(expression).unwrap(),
        }
    }

    #[test]
    fn a_member_expression_parses_its_flow_style_frontmatter_and_keeps_its_body() {
        let parsed = parse_member_expression(NOUS).unwrap();
        assert_eq!(parsed.frontmatter.name, "anima-nous");
        assert_eq!(parsed.frontmatter.cf.as_deref(), Some("CF1"));
        assert_eq!(parsed.frontmatter.tools, ["Read", "Grep", "Glob", "Bash"]);
        assert_eq!(parsed.frontmatter.skills.len(), 3);
        assert!(parsed.frontmatter.source.is_some());
        assert!(parsed.body.starts_with("# Nous"));
        assert!(parsed.body.ends_with("you do not conclude."));

        for broken in [
            "no frontmatter at all",
            "---\nname: x\ndescription: y\n",
            "---\nname: x\n---\nbody",
            "---\nname: x\ndescription: y\n---\n\n",
        ] {
            assert_eq!(
                parse_member_expression(broken).unwrap_err().code(),
                "claude_team.expression_invalid",
                "{broken:?}"
            );
        }
    }

    #[test]
    fn a_member_renders_as_a_claude_subagent_with_name_description_tools_and_skills() {
        let text = render_subagent(&member(NOUS)).unwrap();
        let (frontmatter, body) = text
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap();
        let fields: serde_yaml::Value = serde_yaml::from_str(frontmatter).unwrap();
        assert_eq!(fields["name"], "anima-nous");
        assert_eq!(
            fields["description"],
            "Nous, the epistemic clearing — invoked fresh before a task; never the task executor."
        );
        assert_eq!(fields["tools"], "Read, Grep, Glob, Bash");
        // Expression skills first, then the profile's extra one, each once,
        // by the name the Claude projection gives them.
        assert_eq!(
            fields["skills"],
            "vak-coordinate-frame, gnosis-retrieve, brainstorming, darshana"
        );
        assert_eq!(
            frontmatter
                .lines()
                .map(|l| l.split(':').next().unwrap())
                .collect::<Vec<_>>(),
            ["name", "description", "tools", "skills"]
        );
        assert!(body.contains("You are Nous."));
        assert!(body.contains("profile/anima-nous"));
        assert!(body.contains("Profile purpose: The clearing before the form."));
        assert!(
            body.contains("- central:source:control:root:Control/agents/expressions/anima/TEAM.md")
        );
    }

    #[test]
    fn an_expression_that_names_another_member_or_a_bad_ref_is_refused() {
        let mut wrong = member(NOUS);
        wrong.agent_ref = "agent/anima-logos".into();
        assert_eq!(
            render_subagent(&wrong).unwrap_err().code(),
            "claude_team.expression_mismatch"
        );
        for bad in [
            "anima-nous",
            "agent/",
            "agent/Anima",
            "agent/a b",
            "agent:anima",
        ] {
            assert_eq!(
                subagent_name(bad).unwrap_err().code(),
                "claude_team.member_name_invalid",
                "{bad}"
            );
        }
    }

    #[test]
    fn the_plugin_manifest_is_named_for_the_set_and_counts_its_members() {
        let manifest: Value = serde_json::from_str(&render_plugin_manifest(
            "anima",
            "r1",
            "agent/anima",
            &[member(NOUS)],
        ))
        .unwrap();
        assert_eq!(manifest["name"], "anima-team");
        assert!(manifest["description"]
            .as_str()
            .unwrap()
            .contains("The 1 members of Central agent set anima"));
        assert_eq!(team_plugin_name("World Operators!"), "world-operators-team");
    }
}
