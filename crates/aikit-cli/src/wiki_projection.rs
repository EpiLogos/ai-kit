//! A selected, live Markdown projection in the existing Agent Wiki.
//!
//! The Markdown body is operational guidance, not human governance. Feedback
//! changes that source through an explicit revision-checked update. A selected
//! continuity capability rereads it at the next prompt; nothing scans the Wiki
//! for instructions, silently promotes inference, or claims a harness loaded it.
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::{ContextLock, LockOptions};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CAPABILITY: &str = "hook/continuity/wiki-projection";
const MARKER: &str = "<!-- aikit:wiki-projection-history\n";
const END: &str = "\n-->\n";
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
    /// Replace the body from stdin, retaining feedback provenance in the file.
    /// Requires an existing Agent Wiki Markdown file and its exact read revision.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackBasis {
    pub previous_revision: String,
    pub body_revision: String,
    pub evidence_ref: ResourceRef,
    pub actor_ref: ResourceRef,
    pub reason: String,
    pub recorded_at_unix_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct ProjectionReading {
    pub source: String,
    pub revision: String,
    pub body: String,
    pub feedback: Vec<FeedbackBasis>,
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
    let (feedback, body) = if let Some(rest) = raw.strip_prefix(MARKER) {
        let (history, body) = rest.split_once(END).ok_or_else(|| {
            error(
                "wiki_projection.invalid_history",
                "unfinished feedback history",
            )
        })?;
        let history: Vec<FeedbackBasis> = serde_json::from_str(history)
            .map_err(|e| error("wiki_projection.invalid_history", e))?;
        (history, body.to_string())
    } else {
        (Vec::new(), raw.clone())
    };
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
        feedback,
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
    evidence: &str,
    actor: &str,
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
        || body.contains(MARKER)
        || reason.trim().is_empty()
        || reason.len() > 4096
    {
        return Err(error(
            "wiki_projection.invalid_update",
            "invalid body or missing/oversized feedback reason",
        ));
    }
    let evidence_ref = ResourceRef::parse(evidence)?;
    let actor_ref = ResourceRef::parse(actor)?;
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
    let mut current = read(&path)?;
    if current.revision != expected {
        return Err(error(
            "wiki_projection.conflict",
            "projection changed since the supplied revision; reread and reconcile",
        ));
    }
    current.feedback.push(FeedbackBasis {
        previous_revision: current.revision.clone(),
        body_revision: digest(body.as_bytes()),
        evidence_ref,
        actor_ref,
        reason: reason.to_string(),
        recorded_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    let history = serde_json::to_string(&current.feedback)
        .map_err(|e| error("wiki_projection.invalid_history", e))?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    let rendered = format!("{MARKER}{history}{END}{body}");
    if rendered.len() as u64 > MAX_BYTES {
        return Err(error("wiki_projection.history_budget", "feedback history exceeds source budget; retain it in source history before an explicit consolidation"));
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
    temp.write_all(rendered.as_bytes())
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
    current.revision = digest(rendered.as_bytes());
    current.body = body.to_string();
    Ok(current)
}

pub fn run(command: ProjectionCmd) -> Result<crate::wiki::WikiOutcome> {
    let (reading, state) = match command.command {
        ProjectionSub::Read { file } => (read(&file)?, "read"),
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
            (
                update(&file, &expected_revision, &body, &evidence, &actor, &reason)?,
                "stored",
            )
        }
    };
    Ok(crate::wiki::WikiOutcome {
        data: serde_json::json!({"projection":reading,"state":state,
            "source_kind":"agent-maintained-operational-projection", "governance_changed":false,
            "harness_loaded":false}),
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
        let text = format!("[Wiki projection unavailable] {address}: unavailable or beyond context budget. Do not substitute a cached projection; resolve the current source before relying on it.");
        let cost = aikit_core::estimate_tokens(&text).saturating_add(1);
        (text, cost)
    }).collect();
    let reserved = notices
        .iter()
        .fold(0_u32, |sum, (_, cost)| sum.saturating_add(*cost));
    if reserved > MAX_CONTEXT_TOKENS {
        return Err(error(
            "wiki_projection.source_budget",
            "declared source addresses exceed the disclosure budget; select a smaller bounded set",
        ));
    }
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let mut remaining = MAX_CONTEXT_TOKENS - reserved;
    for (source, (notice, reserved_cost)) in sources.iter().zip(notices) {
        let address = source.as_str().unwrap();
        let result = address_path(address, project, central).and_then(|p| read(&p));
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
