//! Star prompt-commands (W2): the explicit continuity protocols a session can
//! invoke by name.
//!
//! A star command is recognised at UserPromptSubmit, **before** domain
//! matching, and a match short-circuits the domain branch: the user asked for
//! a specific protocol, and ambient domain guidance arriving on top of it would
//! bury the thing they asked for.
//!
//! What a star command injects is a *protocol* — the routing, the carriers, and
//! the exact commands that write them. The active model performs the synthesis;
//! nothing here embeds a model, and nothing here writes a carrier on the user's
//! behalf. That division is the whole point: AIKit owns the reaction, the
//! native owners (Central NOW, Factory's development ledger) own the objects,
//! and the protocol names which is which so the close-out is checkable
//! afterwards rather than asserted.
//!
//! Packs are profile tunables and **the default arms none**: a session gets
//! star commands because its composition selected them, never because the
//! engine shipped them.

use std::fmt;
use std::path::PathBuf;

/// The close-out pack: the three commands that leave continuation state in
/// existing carriers (PROGRAMME §8).
pub const CLOSEOUT_PACK: &str = "continuity-closeout";

/// The packs the engine knows how to arm, for honest "not armed" answers.
pub const KNOWN_PACKS: &[&str] = &[CLOSEOUT_PACK];

/// A star command the engine implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StarCommand {
    /// Close the session out: route learned material, task state, deferred
    /// work and the continuation to their owners, then verify.
    End,
    /// Register worthwhile side-work without disturbing the current line.
    Fork,
    /// Register a continuation now, superseding the previous open one.
    Handoff,
}

impl StarCommand {
    /// The token as it is typed, without the star.
    pub const fn token(self) -> &'static str {
        match self {
            Self::End => "end",
            Self::Fork => "fork",
            Self::Handoff => "handoff",
        }
    }

    /// The pack that carries this command.
    pub const fn pack(self) -> &'static str {
        CLOSEOUT_PACK
    }

    /// One line naming what the command is for — what `aikit continuity
    /// commands` prints, and what a protocol block opens with.
    pub const fn synopsis(self) -> &'static str {
        match self {
            Self::End => {
                "close out: learned material, task state, deferred work and the continuation \
                 go to their owners, then verification confirms they exist"
            }
            Self::Fork => {
                "register side-work as a deferred observation the owner returns to, \
                 additively, without disturbing the current line of work"
            }
            Self::Handoff => "register a continuation now; it supersedes the previous open one",
        }
    }

    /// Parse a bare token (`end`, not `*end`).
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "end" => Some(Self::End),
            "fork" => Some(Self::Fork),
            "handoff" => Some(Self::Handoff),
            _ => None,
        }
    }

    /// Every command the engine implements, in canonical order.
    pub const fn all() -> &'static [StarCommand] {
        &[Self::End, Self::Fork, Self::Handoff]
    }
}

impl fmt::Display for StarCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "*{}", self.token())
    }
}

/// The commands a pack arms.
pub fn pack_commands(pack: &str) -> Vec<StarCommand> {
    match pack {
        CLOSEOUT_PACK => StarCommand::all().to_vec(),
        _ => Vec::new(),
    }
}

/// The commands armed by the composition's declared packs.
///
/// An empty or absent pack list arms nothing — the default. An unknown pack
/// name arms nothing and is reported by [`unknown_packs`] rather than silently
/// ignored, because a typo that quietly disarms a protocol is worse than a
/// visible refusal.
pub fn armed(packs: &[String]) -> Vec<StarCommand> {
    let mut commands: Vec<StarCommand> = Vec::new();
    for pack in packs {
        for command in pack_commands(pack) {
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
    }
    commands.sort();
    commands
}

/// Declared pack names the engine does not know.
pub fn unknown_packs(packs: &[String]) -> Vec<String> {
    packs
        .iter()
        .filter(|pack| !KNOWN_PACKS.contains(&pack.as_str()))
        .cloned()
        .collect()
}

/// One recognised invocation: the command, and whatever the user wrote after
/// it on the same line (a fork's subject, an end's note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: StarCommand,
    pub argument: Option<String>,
}

/// Recognise the star commands in a prompt, in the order they appear.
///
/// Recognition is deliberately narrow:
/// * only **armed** commands are recognised — an unarmed `*end` is ordinary
///   prose, and under the default composition every star token is ordinary
///   prose;
/// * a token counts only at a word boundary (`*end` after whitespace or at the
///   start), so `2*end` and `foo*fork` are arithmetic and prose, not commands;
/// * a repeated command is one invocation — stacking means *different*
///   commands in one prompt, not the same command twice.
pub fn recognise(prompt: &str, armed: &[StarCommand]) -> Vec<Invocation> {
    let mut found: Vec<Invocation> = Vec::new();
    let bytes = prompt.as_bytes();
    for (index, _) in prompt.match_indices('*') {
        let preceded_ok = index == 0
            || bytes
                .get(index - 1)
                .map(|byte| !byte.is_ascii_alphanumeric() && *byte != b'*')
                .unwrap_or(true);
        if !preceded_ok {
            continue;
        }
        let rest = &prompt[index + 1..];
        let token_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(rest.len());
        let token = rest[..token_end].to_ascii_lowercase();
        let Some(command) = StarCommand::parse(&token) else {
            continue;
        };
        if !armed.contains(&command) {
            continue;
        }
        if found.iter().any(|invocation| invocation.command == command) {
            continue;
        }
        // The argument is the remainder of that line, stopping at the next
        // star token so a stacked prompt does not swallow its neighbour.
        let tail = &rest[token_end..];
        let line_end = tail.find('\n').unwrap_or(tail.len());
        let line = &tail[..line_end];
        let argument_end = line.find('*').unwrap_or(line.len());
        let argument = line[..argument_end].trim().trim_start_matches(':').trim();
        found.push(Invocation {
            command,
            argument: (!argument.is_empty()).then(|| argument.to_owned()),
        });
    }
    found
}

/// The concrete coordinates a protocol needs to name real commands: which
/// project's NOW field, which `ctrl`, which Factory Run ledger.
///
/// Every field is optional because absence is a real state. A protocol that
/// cannot name a carrier says so and says what would bind it, rather than
/// printing a command that would fail or, worse, one that would write
/// somewhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingContext {
    pub project: Option<String>,
    pub central_root: Option<PathBuf>,
    pub ctrl_bin: String,
    pub actor: Option<String>,
    pub factory_bin: String,
    pub factory_ledger_root: Option<String>,
    pub factory_run_ref: Option<String>,
}

impl RoutingContext {
    /// The `ctrl` invocation prefix for this world, as it would be typed.
    fn ctrl_prefix(&self) -> String {
        let binary = if self.ctrl_bin.is_empty() {
            "ctrl"
        } else {
            &self.ctrl_bin
        };
        match &self.central_root {
            Some(root) => format!("{binary} --json --root {} action run", root.display()),
            None => format!("{binary} --json action run"),
        }
    }

    fn factory_prefix(&self) -> &str {
        if self.factory_bin.is_empty() {
            "factory"
        } else {
            &self.factory_bin
        }
    }

    /// Is the Factory development ledger bound for this session?
    pub fn factory_bound(&self) -> bool {
        self.factory_ledger_root.is_some() && self.factory_run_ref.is_some()
    }
}

fn actor_of(context: &RoutingContext) -> String {
    context
        .actor
        .clone()
        .unwrap_or_else(|| "<your actor id>".to_owned())
}

/// Render the deferred-work route, or the honest absence of it.
fn factory_route(context: &RoutingContext, subject: &str) -> Vec<String> {
    match (&context.factory_ledger_root, &context.factory_run_ref) {
        (Some(root), Some(run)) => vec![
            format!(
                "     {} development observe {root} {run} - --json",
                context.factory_prefix()
            ),
            "     with a request body:".to_owned(),
            "       {\"observation_ref\": \"observation:<slug>\", \"kind\": \"insufficient-evidence\","
                .to_owned(),
            format!(
                "        \"statement\": \"{subject}\", \"subject_refs\": [], \"evidence_refs\": [],"
            ),
            "        \"owner_return\": {\"owner_ref\": \"<owning product>\", \"source_ref\": null,"
                .to_owned(),
            "         \"proposal_ref\": \"proposal:<slug>\", \"recognition_required\": true}}"
                .to_owned(),
            "     (additive: several observations stand open at once; a repeated".to_owned(),
            "      observation_ref is refused rather than overwriting one)".to_owned(),
        ],
        _ => vec![
            "     no Factory Run ledger is bound to this session, so deferred work".to_owned(),
            "     cannot be registered from here. Bind one by setting".to_owned(),
            "     factory_ledger_root and factory_run_ref in the composition's".to_owned(),
            "     [config.\"hook/continuity/star-commands\"] table, or record the".to_owned(),
            "     deferred item as a Central NOW note instead and say so.".to_owned(),
        ],
    }
}

fn central_returns(context: &RoutingContext) -> (String, String) {
    let prefix = context.ctrl_prefix();
    let project = context
        .project
        .clone()
        .unwrap_or_else(|| "<project>".to_owned());
    (prefix, project)
}

/// Render the protocol for one invocation.
///
/// The text is the injected material itself: it names the routing law, the
/// carrier each piece of material belongs to, and the exact command that
/// writes it. It never claims a write happened.
pub fn protocol(invocation: &Invocation, context: &RoutingContext) -> String {
    let (prefix, project) = central_returns(context);
    let actor = actor_of(context);
    let mut lines = Vec::new();
    lines.push(format!(
        "[continuity/star-commands] {} recognised (pack {}, composed) — {}",
        invocation.command,
        invocation.command.pack(),
        invocation.command.synopsis()
    ));
    if let Some(argument) = &invocation.argument {
        lines.push(format!("  you wrote: {argument}"));
    }
    if context.project.is_none() {
        lines.push(
            "  this session is not standing in a project of the Central world, so the".to_owned(),
        );
        lines.push(
            "  NOW routes below have no field to write to; name the project explicitly or run"
                .to_owned(),
        );
        lines.push("  the close-out from the project's own directory.".to_owned());
    }
    let subject = invocation
        .argument
        .clone()
        .unwrap_or_else(|| "<subject>".to_owned());

    match invocation.command {
        StarCommand::End => {
            lines.push(
                "  route what this session produced to its owners, in this order:".to_owned(),
            );
            lines.push(
                "  1. durable learned material → Central NOW (learned, never authored;".to_owned(),
            );
            lines.push(
                "     promotion to authored source happens only through human Recognition)"
                    .to_owned(),
            );
            lines.push(format!(
                "     {prefix} projectcentral.now.return '{{\"project\":\"{project}\",\"actor\":\"{actor}\",\"kind\":\"learning\",\"subject\":\"...\",\"result\":\"...\",\"status\":\"active\"}}'"
            ));
            lines.push(
                "  2. task state → the same field, as the status of the records it moved"
                    .to_owned(),
            );
            lines.push(format!(
                "     {prefix} projectcentral.now.update '{{\"project\":\"{project}\",\"id\":\"<record id>\",\"status\":\"resolved\"}}'"
            ));
            lines
                .push("  3. genuine deferred work → Factory, as an observation with an".to_owned());
            lines.push(
                "     owner-return proposal (fork behaviour: additive, several open)".to_owned(),
            );
            lines.extend(factory_route(context, "<what was deferred and why>"));
            lines.push(
                "  4. the continuation → Central NOW as a handoff return, and the".to_owned(),
            );
            lines.push("     previous open handoff is superseded, not left standing".to_owned());
            lines.push(format!(
                "     {prefix} projectcentral.now.return '{{\"project\":\"{project}\",\"actor\":\"{actor}\",\"kind\":\"handoff\",\"subject\":\"...\",\"result\":\"...\",\"status\":\"active\"}}'"
            ));
            lines.push(format!(
                "     {prefix} projectcentral.now.update '{{\"project\":\"{project}\",\"id\":\"<previous handoff id>\",\"status\":\"resolved\",\"preserve_refs\":[\"<new handoff id>\"]}}'"
            ));
            lines.push("  5. verify — the objects, not the intention:".to_owned());
            lines.push(format!(
                "     aikit continuity closeout verify --project {project} --json"
            ));
            lines.push(
                "  then stop. A close-out that starts new work has not closed anything.".to_owned(),
            );
        }
        StarCommand::Fork => {
            lines.push(
                "  register the side-work and carry on with the line you were on:".to_owned(),
            );
            lines.extend(factory_route(context, &subject));
            lines.push("  read the open forks back before you claim one exists:".to_owned());
            match (&context.factory_ledger_root, &context.factory_run_ref) {
                (Some(root), Some(run)) => lines.push(format!(
                    "     {} development observations {root} {run} --json",
                    context.factory_prefix()
                )),
                _ => lines.push("     (no ledger bound — see above)".to_owned()),
            }
            lines.push(
                "  the current work continues undisturbed: a fork is a registration, not a switch."
                    .to_owned(),
            );
        }
        StarCommand::Handoff => {
            lines.push("  register the continuation now:".to_owned());
            lines.push(format!(
                "     {prefix} projectcentral.now.return '{{\"project\":\"{project}\",\"actor\":\"{actor}\",\"kind\":\"handoff\",\"subject\":\"{subject}\",\"result\":\"...\",\"status\":\"active\"}}'"
            ));
            lines.push(
                "  then supersede the handoff it replaces, so exactly one stands open:".to_owned(),
            );
            lines.push(format!(
                "     {prefix} projectcentral.now.update '{{\"project\":\"{project}\",\"id\":\"<previous handoff id>\",\"status\":\"resolved\",\"preserve_refs\":[\"<new handoff id>\"]}}'"
            ));
            lines.push(format!(
                "     aikit continuity closeout verify --project {project} --json"
            ));
        }
    }
    lines.join("\n")
}
