//! The context's environment projection.
//!
//! This is what replaces a global mutable "current project" file. A tool whose
//! only notion of scope is *which file did you point me at* becomes context-scoped
//! by resolving its pointer per context: two projects hold two values of one
//! variable, and **there is nothing shared for them to race over**
//! (`docs/integrations/bkmr.md` §4).
//!
//! ## Why an integration gets a named binding rather than a generic passthrough
//!
//! A capsule could read its config itself, but then every consumer — a bare
//! `bkmr` typed by a human, a script, an editor plugin — would need to know
//! AIKit's config shape. Exporting the tool's *own* variable as well means a
//! person typing the bare command in that pane gets the same database the capsules
//! do. That is the only way to make the three-way disagreement in §2 of that
//! document structurally impossible: there is no second answer to give.

use std::path::{Path, PathBuf};

use aikit_core::id::CapsuleId;
use aikit_core::projection::ProjectionItem;
use aikit_core::{AikitError, ResolvedView, Result};

/// The bkmr integration's capsule id: the config section that binds a database.
const BKMR_TOOL: &str = "tool/search/bkmr";

/// Expand a leading `~` against `home`. Config is hand-written, and a path that
/// silently did not expand would point at a directory literally named `~`.
fn expand(raw: &str, home: &Path) -> PathBuf {
    match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(raw),
    }
}

/// Every environment variable this context exports, as projection items.
///
/// Derived from the *resolved* config, so it obeys scope precedence and the merge
/// algebra like everything else: a session overlay can rebind the database for one
/// pane without touching the project's declaration.
pub fn project(view: &ResolvedView, home: &Path) -> Result<Vec<ProjectionItem>> {
    let mut items = Vec::new();

    if let Some(bkmr) = view
        .active
        .get(&CapsuleId::parse(BKMR_TOOL)?)
        .map(|c| &c.config)
    {
        items.extend(bkmr_env(bkmr, home)?);
    }

    Ok(items)
}

/// The four documented bkmr variables (`docs/integrations/bkmr.md` §4).
fn bkmr_env(
    config: &aikit_core::profile::ConfigTable,
    home: &Path,
) -> Result<Vec<ProjectionItem>> {
    let Some(db) = config.get("db").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
        // The tool is active but no database is bound. That is not an error — the
        // user may be using bkmr's own config — so nothing is exported and nothing
        // is claimed.
        return Ok(Vec::new());
    };

    let dir = config
        .get("dir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("~/.config/bkmr/projects");
    let dir = expand(dir, home);
    let path = dir.join(format!("{db}.db"));

    // `also` is the DECLARED cross-search set, never a directory glob: globbing
    // would sweep in the `<name>_backup_YYYYMMDD.db` files bkmr 7.x writes beside
    // each database during migration and search them as if they were projects.
    let mut set: Vec<String> = vec![path.display().to_string()];
    if let Some(also) = config.get("also").and_then(|v| v.as_array()) {
        for entry in also {
            if let Some(name) = entry.as_str().filter(|s| !s.is_empty()) {
                set.push(dir.join(format!("{name}.db")).display().to_string());
            }
        }
    }

    Ok(vec![
        // What the capsules read.
        ProjectionItem::env("AIKIT_BKMR_DB", path.display().to_string())?,
        ProjectionItem::env("AIKIT_BKMR_DB_DIR", dir.display().to_string())?,
        ProjectionItem::env("AIKIT_BKMR_DB_SET", set.join(":"))?,
        // …and the tool's own variable, so a bare `bkmr` in this pane is already
        // correct. Two names is deliberate: the capsules keep working if bkmr
        // renames its variable, and a human typing the bare command is never
        // looking at a different database from the capsules.
        ProjectionItem::env("BKMR_DB_URL", path.display().to_string())?,
    ])
}

/// Render environment items as shell `export` lines.
///
/// Values are single-quoted with embedded quotes escaped, so a path containing a
/// space, a `$` or a quote cannot become shell syntax.
pub fn render_shell(items: &[ProjectionItem], shell: &str) -> Result<String> {
    let mut out = String::new();
    for item in items {
        let ProjectionItem::Env { name, value } = item else {
            continue;
        };
        let quoted = value.replace('\'', r"'\''");
        match shell {
            "fish" => out.push_str(&format!("set -gx {name} '{quoted}';\n")),
            "bash" | "zsh" | "sh" => out.push_str(&format!("export {name}='{quoted}';\n")),
            other => {
                return Err(AikitError::new(
                    "cli.usage",
                    format!("`{other}` is not a supported shell; use bash, zsh, fish or sh"),
                )
                .with("shell", other.to_string()))
            }
        }
    }
    Ok(out)
}
