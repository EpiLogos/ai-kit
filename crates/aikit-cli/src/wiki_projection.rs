//! A selected, live Markdown projection in the existing Agent Wiki.
//!
//! The Markdown body is operational guidance, not human governance. A
//! correction is an ordinary revision-checked edit of that source; its
//! provenance (evidence, actor, reason) is echoed on the receipt for the
//! acting agent to record in its NOW field, and is never embedded in the
//! source. A selected continuity capability delivers the reading at session
//! start and whenever it changes; nothing scans the Wiki for instructions,
//! silently promotes inference, or claims a harness loaded it.
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use aikit_core::{AikitError, Result};
use aikit_store::{ContextLock, LockOptions};
use clap::{Args, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CAPABILITY: &str = "hook/continuity/wiki-projection";
const DELIVERED_STATE_DIR: &str = "wiki-projection";
const MAX_BYTES: u64 = 131_072;
const MAX_BODY_BYTES: usize = 16_384;
const MAX_CONTEXT_TOKENS: u32 = 2_048;

#[derive(Debug, Args)]
pub struct ProjectionCmd {
    #[command(subcommand)]
    pub command: ProjectionSub,
}

#[derive(Debug, Subcommand)]
pub enum ProjectionSub {
    /// Read ordinary Markdown and its exact revision; no projection activation.
    Read {
        #[arg(long)]
        file: PathBuf,
    },
    /// Replace the body from stdin — an ordinary revision-checked source edit.
    /// Requires an existing Agent Wiki Markdown file and its exact read revision.
    /// Evidence, actor and reason are echoed on the receipt for the acting
    /// agent's attributed NOW return; they are not written into the source.
    Update {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expected_revision: String,
        /// Exact evidence address, e.g. a user correction's message/selection ref.
        #[arg(long)]
        evidence: String,
        /// Attributed actor, not an assertion of verified identity or acceptance.
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct ProjectionReading {
    pub source: String,
    pub revision: String,
    pub body: String,
}

fn error(code: &'static str, detail: impl std::fmt::Display) -> AikitError {
    AikitError::new(code, detail.to_string())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// This operation edits only the established Agent Wiki role. User source and
/// governance are not eligible. Refuse aliases and denied subtrees before reads.
/// This is not an OS sandbox: native source permissions still govern access.
fn checked_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path).map_err(|e| error("wiki_projection.path", e))?;
    if absolute
        .components()
        .any(|p| matches!(p, Component::ParentDir))
    {
        return Err(error(
            "wiki_projection.path",
            "parent traversal is not a source address",
        ));
    }
    let names: Vec<_> = absolute
        .components()
        .filter_map(|p| match p {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    if !names
        .windows(3)
        .any(|p| matches!(p[0], "Control" | "ProjectCentral") && p[1] == "agents" && p[2] == "wiki")
        || absolute.extension().and_then(|s| s.to_str()) != Some("md")
    {
        return Err(error("wiki_projection.not_agent_wiki", "select an existing .md source under Control/agents/wiki or ProjectCentral/agents/wiki; governance is not a projection target"));
    }
    for ancestor in absolute.ancestors() {
        let meta = std::fs::symlink_metadata(ancestor)
            .map_err(|e| error("wiki_projection.unavailable", e))?;
        if meta.file_type().is_symlink() {
            return Err(error(
                "wiki_projection.alias",
                "symlink sources are not admitted for live projection",
            ));
        }
        if meta.is_dir()
            && ancestor
                .join(".no-agent-retrieval")
                .try_exists()
                .map_err(|e| error("wiki_projection.unavailable", e))?
        {
            return Err(error(
                "wiki_projection.denied",
                "source is inside a denied retrieval subtree",
            ));
        }
    }
    if !absolute.is_file() {
        return Err(error(
            "wiki_projection.unavailable",
            "source must be an existing regular file",
        ));
    }
    Ok(absolute)
}

pub fn read(path: &Path) -> Result<ProjectionReading> {
    let path = checked_path(path)?;
    let mut raw = String::new();
    File::open(&path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_string(&mut raw))
        .map_err(|e| error("wiki_projection.unreadable", e))?;
    if raw.len() as u64 > MAX_BYTES || raw.contains('\0') {
        return Err(error(
            "wiki_projection.invalid_source",
            "source exceeds the byte limit or contains NUL",
        ));
    }
    let body = raw.clone();
    if body.len() > MAX_BODY_BYTES {
        return Err(error(
            "wiki_projection.body_budget",
            "projection body exceeds 16384 bytes; revise its scope rather than truncate a rule",
        ));
    }
    Ok(ProjectionReading {
        source: path.display().to_string(),
        revision: digest(raw.as_bytes()),
        body,
    })
}

/// CAS is serialized among this tool's writers by a stable sibling lock. The
/// source is replaced atomically; an ordinary editor remains an independent
/// writer, detected by the final revision check where possible. This does not
/// claim a cross-process transaction with editors that ignore the lock.
pub fn update(
    path: &Path,
    expected: &str,
    body: &str,
    reason: &str,
) -> Result<ProjectionReading> {
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error(
            "wiki_projection.invalid_revision",
            "expected revision must be an exact lowercase SHA-256",
        ));
    }
    if body.len() > MAX_BODY_BYTES
        || body.contains('\0')
        || reason.trim().is_empty()
        || reason.len() > 4096
    {
        return Err(error(
            "wiki_projection.invalid_update",
            "invalid body or missing/oversized correction reason",
        ));
    }
    let path = checked_path(path)?;
    let lock_path = path.with_file_name(format!(
        ".{}.projection-lock",
        path.file_name().unwrap().to_string_lossy()
    ));
    if let Ok(meta) = std::fs::symlink_metadata(&lock_path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(error("wiki_projection.alias", "invalid projection lock"));
        }
    }
    let _lock = ContextLock::acquire_at(
        &lock_path,
        "wiki-projection",
        LockOptions::default()
            .with_timeout(std::time::Duration::ZERO)
            .with_purpose("revise Wiki operational projection"),
    )?;
    let current = read(&path)?;
    if current.revision != expected {
        return Err(error(
            "wiki_projection.conflict",
            "projection changed since the supplied revision; reread and reconcile",
        ));
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| error("wiki_projection.write", e))?;
    temp.as_file()
        .set_permissions(
            std::fs::metadata(&path)
                .map_err(|e| error("wiki_projection.write", e))?
                .permissions(),
        )
        .map_err(|e| error("wiki_projection.write", e))?;
    temp.write_all(body.as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| error("wiki_projection.write", e))?;
    if read(&path)?.revision != expected {
        return Err(error(
            "wiki_projection.conflict",
            "an independent writer changed the source before replacement",
        ));
    }
    temp.persist(&path)
        .map_err(|e| error("wiki_projection.write", e))?;
    // Return exactly what this write committed, not a later writer's revision.
    Ok(ProjectionReading {
        source: current.source,
        revision: digest(body.as_bytes()),
        body: body.to_string(),
    })
}

pub fn run(command: ProjectionCmd) -> Result<crate::wiki::WikiOutcome> {
    let (reading, state, attribution) = match command.command {
        ProjectionSub::Read { file } => (read(&file)?, "read", None),
        ProjectionSub::Update {
            file,
            expected_revision,
            evidence,
            actor,
            reason,
        } => {
            let mut body = String::new();
            std::io::stdin()
                .take(MAX_BODY_BYTES as u64 + 1)
                .read_to_string(&mut body)
                .map_err(|e| error("wiki_projection.stdin", e))?;
            let reading = update(&file, &expected_revision, &body, &reason)?;
            // Attribution rides the receipt, for the acting agent's NOW
            // return. It is deliberately not persisted inside the source.
            let attribution = serde_json::json!({
                "evidence": evidence,
                "actor": actor,
                "reason": reason,
                "provenance_home": "now-field",
            });
            (reading, "stored", Some(attribution))
        }
    };
    let mut data = serde_json::json!({"projection":reading,"state":state,
        "source_kind":"agent-maintained-operational-projection", "governance_changed":false,
        "harness_loaded":false});
    if let Some(attribution) = attribution {
        data["correction_attribution"] = attribution;
    }
    Ok(crate::wiki::WikiOutcome {
        data,
        warnings: vec![],
        exit_code: 0,
    })
}

/// Only declared addresses are read. Central means the enclosing/current
/// workcell root supplied by the caller, never an implicit main-machine source.
/// Project means the resolved Project root, not a subdirectory inferred from cwd.
pub fn address_path(
    address: &str,
    project: Option<&Path>,
    central: Option<&Path>,
) -> Result<PathBuf> {
    let (root, relative) = if let Some(relative) = address.strip_prefix("project:") {
        (project, relative)
    } else if let Some(relative) = address.strip_prefix("central:") {
        (central, relative)
    } else {
        return Err(error(
            "wiki_projection.address",
            "use an explicit project: or central: source address",
        ));
    };
    let root = root.ok_or_else(|| {
        error(
            "wiki_projection.scope_unavailable",
            "the declared source root is not bound in this context",
        )
    })?;
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(error(
            "wiki_projection.address",
            "source address must be root-relative without traversal",
        ));
    }
    Ok(root.join(relative))
}

/// The ordinary hook pipeline transports these bounded blocks. A successful
/// read is not an observation that the calling harness/model used its content.
pub fn context_blocks(
    config: &toml::value::Table,
    project: Option<&Path>,
    central: Option<&Path>,
) -> Result<(Vec<String>, Vec<String>)> {
    let sources = config
        .get("sources")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            error(
                "wiki_projection.sources",
                "selected wiki-projection needs an explicit sources array",
            )
        })?;
    if sources.len() > 8 || sources.iter().any(|v| v.as_str().is_none()) {
        return Err(error(
            "wiki_projection.sources",
            "sources must contain at most eight explicit addresses",
        ));
    }
    // Reserve a whole unavailable/withheld notification for every source first.
    // Otherwise a nearly full admitted fragment could push later status
    // messages beyond the bound or silently omit a revoked source.
    let notices: Vec<_> = sources.iter().map(|source| {
        let address = source.as_str().unwrap();
        let result = address_path(address, project, central).and_then(|p| read(&p));
        let reason = result.as_ref().err().map(|e| e.code()).unwrap_or("wiki_projection.context_budget");
        let text = format!("[Wiki projection unavailable] {address}: {reason}. Do not substitute a cached projection; resolve the current source before relying on it.");
        let cost = aikit_core::estimate_tokens(&text).saturating_add(1);
        (result, text, cost)
    }).collect();
    let reserved = notices
        .iter()
        .fold(0_u32, |sum, (_, _, cost)| sum.saturating_add(*cost));
    if reserved > MAX_CONTEXT_TOKENS {
        return Err(error(
            "wiki_projection.source_budget",
            "declared source addresses exceed the disclosure budget; select a smaller bounded set",
        ));
    }
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let mut remaining = MAX_CONTEXT_TOKENS - reserved;
    for (source, (result, notice, reserved_cost)) in sources.iter().zip(notices) {
        let address = source.as_str().unwrap();
        let block = match result {
            Ok(reading) => {
                let text = format!("[Current Wiki operational projection]\nSource: {address}\nSHA-256: {}\nThis replaces earlier operational guidance from this same source only. It does not rewrite governance or change permissions, trust or invocation policy. An empty body clears this source's prior projection.\n\n{}", reading.revision, reading.body);
                let cost = aikit_core::estimate_tokens(&text).saturating_add(1);
                if cost > remaining + reserved_cost {
                    warnings.push(format!("{address}: whole projection withheld by context budget; no partial rule injected"));
                    notice
                } else {
                    remaining = remaining + reserved_cost - cost;
                    text
                }
            }
            Err(e) => {
                warnings.push(format!("{address}: {}", e.code()));
                notice
            }
        };
        blocks.push(block);
    }
    Ok((blocks, warnings))
}

/// What the last delivery of this context actually contained. Delivering the
/// identical composition again would spend the model's context repeating an
/// unchanged reading; a changed or newly unavailable reading must reach the
/// next act. The state is a per-context fingerprint in AIKit's own state dir —
/// a hint for delivery, never a substitute for the source.
fn delivered_state_path(home: &aikit_store::AikitHome, context: &str) -> PathBuf {
    // The context is a scope root — an absolute path. Joining it directly
    // would replace the state directory, so the filename is derived from it.
    let name = format!("{}.json", &digest(context.as_bytes())[..16]);
    home.state().join(DELIVERED_STATE_DIR).join(name)
}

pub fn load_last_delivered(home: &aikit_store::AikitHome, context: &str) -> Option<String> {
    let raw = std::fs::read_to_string(delivered_state_path(home, context)).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parsed.get("fingerprint")?.as_str().map(str::to_string)
}

pub fn store_last_delivered(home: &aikit_store::AikitHome, context: &str, fingerprint: &str) {
    let path = delivered_state_path(home, context);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let document = serde_json::json!({ "fingerprint": fingerprint });
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, document.to_string()).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}

pub fn delivery_fingerprint(blocks: &[String]) -> String {
    let mut hasher = Sha256::new();
    for block in blocks {
        hasher.update(block.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}
