//! `aikit mux install` as a Procedure.
//!
//! Editing a user's `~/.tmux.conf` is exactly the case Spec II §1.3 is about: a
//! file AIKit does not own, edited through a **marked block** so applying twice
//! replaces rather than appends, and human prose outside the markers is never
//! touched. Staged and reversible like every other world mutation.

use std::path::PathBuf;

use aikit_core::procedure::{
    splice_marked_block, Inverse, Plan, Procedure, ProcedureKind, WorldEdit,
};
use aikit_core::{AikitError, MuxKind, Result};

use aikit_adapters::mux::{cmux::Cmux, tmux::Tmux, MuxAdapter};

use crate::app::Service;

/// Choose the multiplexer to install for: the named one, or whichever is actually
/// present. Detection beats assumption — installing tmux integration on a machine
/// without tmux writes a file nothing will ever read.
fn choose(named: Option<&str>) -> Result<MuxKind> {
    if let Some(raw) = named {
        return raw.parse::<MuxKind>();
    }
    if Tmux::system()
        .detect()
        .map(|p| p.installed)
        .unwrap_or(false)
    {
        return Ok(MuxKind::Tmux);
    }
    if Cmux::system()
        .detect()
        .map(|p| p.installed)
        .unwrap_or(false)
    {
        return Ok(MuxKind::Cmux);
    }
    Err(AikitError::new(
        "mux.none_detected",
        "no multiplexer was detected; name one explicitly if you want its integration installed",
    ))
}

pub struct MuxInstallPlan {
    pub procedure: Procedure,
    pub mux: MuxKind,
    pub key: String,
    pub previous_key: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallVerification {
    pub live: bool,
    pub verified: bool,
    pub binding: Option<String>,
    pub warnings: Vec<String>,
}

fn validate_key(key: &str) -> Result<&str> {
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AikitError::new(
            "mux.invalid_key",
            "a tmux key must be one token containing only letters, numbers, `-`, or `_`",
        )
        .with("key", key.to_string()));
    }
    Ok(key)
}

fn popup_binding(key: &str) -> String {
    format!(
        "bind-key -n {key} display-popup -E -w 82% -h 70% -d '#{{pane_current_path}}' -T AIKit 'aikit ui'"
    )
}

fn configured_binding_outside_aikit(contents: &str, key: &str) -> Option<String> {
    let mut managed = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.contains(aikit_core::procedure::MARKER_BEGIN) {
            managed = true;
            continue;
        }
        if trimmed.contains(aikit_core::procedure::MARKER_END) {
            managed = false;
            continue;
        }
        if managed || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Ok(tokens) = shell_words::split(trimmed) else {
            continue;
        };
        if parsed_binding(&tokens).is_some_and(|binding| binding.root && binding.key == key) {
            return Some(trimmed.to_string());
        }
    }
    None
}

struct ParsedBinding<'a> {
    key: &'a str,
    root: bool,
    command: &'a [String],
}

fn parsed_binding(tokens: &[String]) -> Option<ParsedBinding<'_>> {
    if !matches!(
        tokens.first().map(String::as_str),
        Some("bind" | "bind-key")
    ) {
        return None;
    }
    let mut table: Option<&str> = None;
    let mut no_prefix = false;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "-T" => {
                table = tokens.get(index + 1).map(String::as_str);
                index += 2;
            }
            token if token.starts_with("-T") && token.len() > 2 => {
                table = Some(&token[2..]);
                index += 1;
            }
            "-N" => index += 2,
            "-n" => {
                no_prefix = true;
                index += 1;
            }
            "-r" => index += 1,
            token if token.starts_with('-') => index += 1,
            token => {
                return Some(ParsedBinding {
                    key: token,
                    root: table == Some("root") || (table.is_none() && no_prefix),
                    command: &tokens[index + 1..],
                })
            }
        }
    }
    None
}

fn managed_config_key(contents: &str) -> Option<String> {
    let mut managed = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.contains(aikit_core::procedure::MARKER_BEGIN) {
            managed = true;
            continue;
        }
        if trimmed.contains(aikit_core::procedure::MARKER_END) {
            managed = false;
            continue;
        }
        if !managed {
            continue;
        }
        let Ok(tokens) = shell_words::split(trimmed) else {
            continue;
        };
        let Some(binding) = parsed_binding(&tokens) else {
            continue;
        };
        if binding.root
            && binding.command.first().map(String::as_str) == Some("display-popup")
            && binding.command.last().map(String::as_str) == Some("aikit ui")
        {
            return Some(binding.key.to_string());
        }
    }
    None
}

fn is_managed_live_binding(binding: &str, key: &str) -> bool {
    let Ok(tokens) = shell_words::split(binding) else {
        return false;
    };
    let Some(binding) = parsed_binding(&tokens) else {
        return false;
    };
    if !binding.root
        || binding.key != key
        || binding.command.first().map(String::as_str) != Some("display-popup")
    {
        return false;
    }

    let mut auto_close = false;
    let mut width = None;
    let mut height = None;
    let mut directory = None;
    let mut title = None;
    let mut index = 1;
    while index < binding.command.len() {
        match binding.command[index].as_str() {
            "-E" if !auto_close => {
                auto_close = true;
                index += 1;
            }
            "-w" if width.is_none() => {
                width = binding.command.get(index + 1).map(String::as_str);
                index += 2;
            }
            "-h" if height.is_none() => {
                height = binding.command.get(index + 1).map(String::as_str);
                index += 2;
            }
            "-d" if directory.is_none() => {
                directory = binding.command.get(index + 1).map(String::as_str);
                index += 2;
            }
            "-T" if title.is_none() => {
                title = binding.command.get(index + 1).map(String::as_str);
                index += 2;
            }
            "aikit ui" if index + 1 == binding.command.len() => {
                return auto_close
                    && width == Some("82%")
                    && height == Some("70%")
                    && directory == Some("#{pane_current_path}")
                    && title == Some("AIKit");
            }
            _ => return false,
        }
    }
    false
}

fn conflict(key: &str, binding: &str) -> AikitError {
    AikitError::new(
        "mux.key_conflict",
        format!("tmux root key `{key}` is already bound; AIKit did not replace it"),
    )
    .with("key", key.to_string())
    .with("binding", binding.to_string())
    .with(
        "resolution",
        "choose another key with `--key`, or review and pass `--replace-key`".to_string(),
    )
}

/// Plan the multiplexer integration edit.
pub fn plan(
    service: &Service,
    named: Option<&str>,
    key: &str,
    replace_key: bool,
) -> Result<MuxInstallPlan> {
    let mux = choose(named)?;
    let key = validate_key(key)?.to_string();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let (path, body) = match mux {
        MuxKind::Tmux => (
            home.join(".tmux.conf"),
            format!(
                "# Open AIKit's unified palette/tree surface in a real popup.\n{}\n\
                 set -g @aikit_installed 1\n",
                popup_binding(&key)
            ),
        ),
        MuxKind::Cmux => (
            home.join(".config/cmux/config.toml"),
            "# AIKit renders its palette inline in the focused terminal.\n\
             [keys]\nalt-a = \"aikit ui\"\n"
                .to_string(),
        ),
        MuxKind::Plain => {
            return Err(AikitError::new(
                "mux.nothing_to_install",
                "a plain terminal has no multiplexer configuration to install into",
            ))
        }
    };

    let existing = std::fs::read_to_string(&path).ok();
    let previous_key = (mux == MuxKind::Tmux)
        .then(|| existing.as_deref().and_then(managed_config_key))
        .flatten();
    if mux == MuxKind::Tmux && !replace_key {
        if let Some(binding) = existing
            .as_deref()
            .and_then(|contents| configured_binding_outside_aikit(contents, &key))
        {
            return Err(conflict(&key, &binding));
        }
        let tmux = Tmux::system();
        let presence = tmux.detect()?;
        if presence.server_running {
            if let Some(binding) = tmux.root_binding(&key)? {
                let owned = is_managed_live_binding(&binding, &key);
                if !owned {
                    return Err(conflict(&key, &binding));
                }
            }
        }
    }
    // The comment leader comes from the file type, so a `#`-commented tmux config
    // and a `//`-commented one both get markers their own parser ignores.
    let leader = aikit_store::procedure::comment_leader(&path);
    let updated = splice_marked_block(existing.as_deref().unwrap_or(""), leader, &body);

    let inverse = if existing.is_some() {
        Inverse::Restore {
            blob: aikit_core::procedure::BlobId::deferred(),
        }
    } else {
        Inverse::Remove
    };

    let mut plan = Plan::new().with_note(format!(
        "add AIKit's managed block to {} ({})",
        path.display(),
        mux.as_str()
    ));
    if existing.as_deref() != Some(updated.as_str()) {
        plan = plan.with_edit(WorldEdit::WriteFile {
            path,
            contents: updated.into_bytes(),
            inverse,
        });
    }

    let path = match mux {
        MuxKind::Tmux => home.join(".tmux.conf"),
        MuxKind::Cmux => home.join(".config/cmux/config.toml"),
        MuxKind::Plain => unreachable!("plain returned before planning"),
    };
    let procedure = aikit_store::procedure::plan_procedure(
        service.home(),
        ProcedureKind::MuxInstall { mux },
        plan,
    )?;
    Ok(MuxInstallPlan {
        procedure,
        mux,
        key,
        previous_key,
        path,
    })
}

pub fn activate(plan: &MuxInstallPlan) -> Result<InstallVerification> {
    let contents = std::fs::read_to_string(&plan.path).map_err(|error| {
        AikitError::new(
            "mux.config_unreadable",
            format!("could not verify {}: {error}", plan.path.display()),
        )
    })?;
    if plan.mux == MuxKind::Tmux && !contents.contains(&popup_binding(&plan.key)) {
        return Err(AikitError::new(
            "mux.binding_verification_failed",
            "the written tmux configuration does not contain the reviewed popup binding",
        )
        .with("path", plan.path.display().to_string())
        .with("key", plan.key.clone()));
    }

    if plan.mux != MuxKind::Tmux {
        return Ok(InstallVerification {
            live: false,
            verified: true,
            binding: None,
            warnings: vec![
                "cmux configuration was verified on disk; reload behavior is owned by cmux"
                    .to_string(),
            ],
        });
    }

    let tmux = Tmux::system();
    let presence = tmux.detect()?;
    if !presence.server_running {
        return Ok(InstallVerification {
            live: false,
            verified: true,
            binding: None,
            warnings: vec![
                "tmux is not running; the binding is verified on disk and will load with the next server"
                    .to_string(),
            ],
        });
    }

    tmux.reload_config(&plan.path)?;
    let binding = tmux.root_binding(&plan.key)?.ok_or_else(|| {
        AikitError::new(
            "mux.binding_verification_failed",
            format!("tmux reloaded but root key `{}` is not bound", plan.key),
        )
        .with("key", plan.key.clone())
    })?;
    let verified = is_managed_live_binding(&binding, &plan.key);
    if !verified {
        return Err(AikitError::new(
            "mux.binding_verification_failed",
            "tmux reloaded a different command than the reviewed AIKit popup binding",
        )
        .with("key", plan.key.clone())
        .with("binding", binding));
    }
    if let Some(previous) = plan
        .previous_key
        .as_deref()
        .filter(|previous| *previous != plan.key)
    {
        if let Some(old_binding) = tmux.root_binding(previous)? {
            if is_managed_live_binding(&old_binding, previous) {
                tmux.unbind_root(previous)?;
            }
        }
        if tmux
            .root_binding(previous)?
            .is_some_and(|old_binding| is_managed_live_binding(&old_binding, previous))
        {
            return Err(AikitError::new(
                "mux.binding_verification_failed",
                format!("the previous AIKit root key `{previous}` remained live after replacement"),
            )
            .with("key", previous.to_string()));
        }
    }
    Ok(InstallVerification {
        live: true,
        verified: true,
        binding: Some(binding),
        warnings: vec![],
    })
}

fn planned_tmux_binding(procedure: &Procedure) -> Option<(PathBuf, String)> {
    if !matches!(
        procedure.kind,
        ProcedureKind::MuxInstall { mux: MuxKind::Tmux }
    ) {
        return None;
    }
    procedure.plan.edits.iter().find_map(|edit| {
        let WorldEdit::WriteFile { path, contents, .. } = edit else {
            return None;
        };
        let rendered = std::str::from_utf8(contents).ok()?;
        managed_config_key(rendered).map(|key| (path.clone(), key))
    })
}

/// Reconcile a running tmux server after a committed mux Procedure is undone.
pub fn activate_undo(procedure: &Procedure) -> Result<Vec<String>> {
    let Some((path, key)) = planned_tmux_binding(procedure) else {
        return Ok(vec![]);
    };
    let tmux = Tmux::system();
    let presence = tmux.detect()?;
    if !presence.server_running {
        return Ok(vec![
            "tmux is not running; the integration was restored on disk and no live key exists to reconcile"
                .to_string(),
        ]);
    }

    let restored = std::fs::read_to_string(&path).ok();
    if path.exists() {
        tmux.reload_config(&path)?;
    }
    let restored_owns_key = restored
        .as_deref()
        .and_then(managed_config_key)
        .is_some_and(|restored_key| restored_key == key);
    if !restored_owns_key {
        if let Some(binding) = tmux.root_binding(&key)? {
            if is_managed_live_binding(&binding, &key) {
                tmux.unbind_root(&key)?;
            }
        }
        if tmux
            .root_binding(&key)?
            .is_some_and(|binding| is_managed_live_binding(&binding, &key))
        {
            return Err(AikitError::new(
                "mux.binding_verification_failed",
                format!(
                    "tmux undo restored the config but root key `{key}` remained bound to AIKit"
                ),
            )
            .with("key", key));
        }
    }
    Ok(vec![])
}
