//! Star prompt-commands (W2, CASE 07/08): the composed reaction that meets an
//! explicit `*end` / `*fork` / `*handoff` with its protocol.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/star-commands`, and even then it recognises only the
//! commands the composition's declared packs arm — the default arms none.
//!
//! Precedence: recognition happens at UserPromptSubmit *before* domain
//! matching, and a match short-circuits the domain branch (the caller enforces
//! that by not running domains when this reaction returns a match). A user who
//! asked for a specific protocol should get that protocol, not it plus
//! whatever ambient guidance also matched their words.
//!
//! No dedup: a star command is an explicit act, and the second `*fork` in a
//! session means a second fork. Dedup exists to stop *unasked-for* material
//! repeating; suppressing an asked-for protocol would be a bug wearing a law's
//! clothes.

use std::path::{Path, PathBuf};

use aikit_core::star::{
    armed, protocol, recognise, unknown_packs, Invocation, RoutingContext, StarCommand,
};

/// The tunings the composition may set on the star capsule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StarConfig {
    /// Packs to arm. Empty is the default and arms nothing.
    pub packs: Vec<String>,
    /// The actor id the protocol should attribute returns to.
    pub actor: Option<String>,
    /// The Factory development-ledger root and Run this session's deferred
    /// work belongs to. Both are required before any Factory route is printed:
    /// AIKit never mints Factory Run identity, so an unbound session is told
    /// it is unbound.
    pub factory_ledger_root: Option<String>,
    pub factory_run_ref: Option<String>,
}

impl StarConfig {
    pub fn from_config(config: Option<&toml::value::Table>) -> Self {
        let mut tuned = Self::default();
        let Some(config) = config else {
            return tuned;
        };
        if let Some(packs) = config.get("packs").and_then(|value| value.as_array()) {
            tuned.packs = packs
                .iter()
                .filter_map(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
        }
        for (key, slot) in [
            ("actor", &mut tuned.actor),
            ("factory_ledger_root", &mut tuned.factory_ledger_root),
            ("factory_run_ref", &mut tuned.factory_run_ref),
        ] {
            if let Some(value) = config
                .get(key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
            {
                *slot = Some(value.to_owned());
            }
        }
        tuned
    }

    /// The commands this composition arms.
    pub fn armed(&self) -> Vec<StarCommand> {
        armed(&self.packs)
    }
}

/// Assemble the routing coordinates for this event: which project's NOW field,
/// which `ctrl`, which Factory Run ledger.
pub fn routing_context(
    config: &StarConfig,
    central_root: Option<&Path>,
    cwd: Option<&Path>,
) -> RoutingContext {
    let project = match (central_root, cwd) {
        (Some(root), Some(cwd)) => crate::orientation_packet::project_of(root, cwd),
        _ => None,
    };
    RoutingContext {
        project,
        central_root: central_root.map(Path::to_path_buf),
        ctrl_bin: executable("CENTRAL_CTRL_BIN", "OI_CENTRAL_CTRL_BIN", "ctrl"),
        actor: config.actor.clone(),
        factory_bin: executable("FACTORY_BIN", "OI_FACTORY_BIN", "factory"),
        factory_ledger_root: config.factory_ledger_root.clone(),
        factory_run_ref: config.factory_run_ref.clone(),
    }
}

fn executable(primary: &str, secondary: &str, fallback: &str) -> String {
    std::env::var_os(primary)
        .or_else(|| std::env::var_os(secondary))
        .map(PathBuf::from)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_owned())
}

/// What the reaction decided for one event.
pub struct StarReaction {
    /// The protocol blocks to inject, one per recognised command.
    pub blocks: Vec<String>,
    /// The commands recognised — empty means the domain branch still runs.
    pub matched: Vec<Invocation>,
    pub warnings: Vec<String>,
}

impl StarReaction {
    pub fn matched_any(&self) -> bool {
        !self.matched.is_empty()
    }
}

/// Run the reaction against a prompt.
pub fn run(prompt: Option<&str>, config: &StarConfig, context: &RoutingContext) -> StarReaction {
    let mut warnings = unknown_packs(&config.packs)
        .into_iter()
        .map(|pack| {
            format!("continuity/star-commands: composition declares unknown pack `{pack}`; it arms nothing")
        })
        .collect::<Vec<_>>();
    let armed = config.armed();
    let Some(prompt) = prompt else {
        return StarReaction {
            blocks: Vec::new(),
            matched: Vec::new(),
            warnings,
        };
    };
    let matched = recognise(prompt, &armed);
    let blocks = matched
        .iter()
        .map(|invocation| protocol(invocation, context))
        .collect();
    if matched
        .iter()
        .any(|invocation| invocation.command != StarCommand::Handoff)
        && context.project.is_none()
    {
        warnings.push(
            "continuity/star-commands: no Central project for this working directory; \
             the protocol's NOW routes name no field"
                .to_owned(),
        );
    }
    StarReaction {
        blocks,
        matched,
        warnings,
    }
}
