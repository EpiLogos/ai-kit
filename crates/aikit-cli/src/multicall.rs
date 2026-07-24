//! The busybox-style multicall shim.
//!
//! A capability's exported command can reach the user two ways: through a shim in
//! the context's `bin/` that execs `aikit run …`, or through a symlink named after
//! the export that points straight at the `aikit` binary. This module is the
//! second path. When `argv[0]`'s basename is not `aikit`, the binary does not
//! parse the CLI at all — it treats the name as an export and runs the capability
//! that owns it.
//!
//! The rule the architecture insists on is *at that exact revision*: the command
//! runs the capsule as it was when the generation was applied, not as the catalog
//! happens to be now. So the flow reads the **committed generation's lock**, finds
//! the export there, and refuses (rather than silently running newer code) if the
//! capsule's payload has moved underneath the applied generation.

use std::path::Path;

use aikit_core::id::{ContextId, Revision};
use aikit_core::trust::TrustOracle;
use aikit_core::{AikitError, Result};

use aikit_store::generation;
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::state::StateStore;
use aikit_store::trust::TrustStore;

use crate::app;
use crate::discover;
use crate::run;

/// The command name a binary was invoked under: the basename of `argv[0]`.
pub fn program_name(argv0: &str) -> String {
    Path::new(argv0)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| argv0.to_string())
}

/// Was the binary invoked as `aikit` (the normal CLI), or under another name (a
/// multicall export)?
pub fn is_aikit(argv0: &str) -> bool {
    let name = program_name(argv0);
    name == "aikit" || name == "aikit.exe"
}

/// Run the capability that owns `export`, resolved from the context's current
/// generation. Returns the child's exit status.
pub fn dispatch<F>(
    export: &str,
    args: &[String],
    home: &AikitHome,
    cwd: &Path,
    env: F,
) -> Result<i32>
where
    F: Fn(&str) -> Option<String>,
{
    let context_dir = locate_context(home, cwd, &env)?;

    let generation_id = generation::current(&context_dir)?.ok_or_else(|| {
        AikitError::new(
            "generation.no_current",
            "this context has no applied generation, so it exports no commands yet",
        )
    })?;
    let generation_dir = context_dir
        .join(generation::GENERATIONS)
        .join(generation_id.as_str());
    let view = generation::read_lock(&generation_dir)?;

    // Find the export in the *applied* view, and the exact revision it was
    // applied at.
    let capsule_id = view.exported_commands().get(export).cloned().ok_or_else(|| {
        AikitError::new(
            "multicall.unknown_export",
            format!("the current generation exports no command named `{export}`"),
        )
        .with("export", export.to_string())
    })?;
    let applied_revision: Option<Revision> = view
        .active
        .get(&capsule_id)
        .and_then(|c| c.revision.clone());

    // Load the capsule's payload from the catalog and insist it is still the
    // revision the generation was built against.
    let project_root = view.context.project_root.clone();
    let load = app::load_catalog(home, project_root.as_deref())?;
    let capsule = aikit_core::catalog::Catalog::get(&load.catalog, &capsule_id)
        .cloned()
        .ok_or_else(|| {
            AikitError::new(
                "multicall.capsule_gone",
                format!("{capsule_id} exports `{export}` but is no longer in any registry"),
            )
            .with("capability", capsule_id.to_string())
        })?;

    if let Some(applied) = &applied_revision {
        if capsule.revision.as_ref() != Some(applied) {
            return Err(AikitError::new(
                "multicall.revision_changed",
                format!(
                    "{capsule_id} has changed since the generation was applied; \
                     re-apply before running `{export}`"
                ),
            )
            .with("capability", capsule_id.to_string()));
        }
    }

    // The multicall shim runs unattended off the PATH, so it must consult trust
    // exactly like the interactive `aikit run` does: an unreviewed executable is
    // refused, not silently run. A script is active-and-exported without a trust
    // check (activation does not gate scripts), which is precisely why *running*
    // one has to. There is no `--confirm` on this path — the shim's arguments
    // belong to the capability — so the escape hatch is the interactive command.
    if capsule.kind.is_executable() {
        let index = Index::open(&home.database())?;
        let trust = TrustStore::new(&index).snapshot()?;
        let state = trust.state_for(
            capsule.source.as_ref(),
            &capsule.id,
            capsule.revision.as_ref(),
        );
        if !state.may_run_unattended() {
            return Err(AikitError::new(
                "trust.required",
                format!(
                    "`{export}` is exported by {capsule_id}, which has not been reviewed; \
                     review it, or run `aikit run {capsule_id} --confirm` to run it once"
                ),
            )
            .with("capability", capsule_id.to_string())
            .with("export", export.to_string())
            .with("trust", state.as_str()));
        }
    }

    let plan = run::plan_script(&capsule, args, project_root.as_deref(), cwd)?;
    let report = run::execute(&plan)?;
    Ok(report.status)
}

/// Decide which context's generation to run against.
///
/// `AIKIT_CONTEXT_ID` is authoritative when present — a session or task exports it
/// precisely so its children run against its own generation. Without it, we fall
/// back to project discovery and the most recently updated context bound to the
/// discovered project.
fn locate_context<F>(home: &AikitHome, cwd: &Path, env: &F) -> Result<std::path::PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(raw) = env("AIKIT_CONTEXT_ID") {
        let context = ContextId::parse(&raw)?;
        return Ok(home.context_dir(&context));
    }

    let project = discover::discover_project(cwd).ok_or_else(|| {
        AikitError::new(
            "context.unknown",
            "no AIKIT_CONTEXT_ID is set and the working directory is not inside a project",
        )
    })?;

    let index = Index::open(&home.database())?;
    let state = StateStore::new(&index);
    let mut candidates: Vec<_> = state
        .contexts()?
        .into_iter()
        .filter(|c| c.project_root.as_deref() == Some(project.root.as_path()))
        .collect();
    candidates.sort_by_key(|c| c.updated_at.as_nanos());
    let chosen = candidates.last().ok_or_else(|| {
        AikitError::new(
            "context.unknown",
            format!(
                "no context is bound to {}; run something in it first",
                project.root.display()
            ),
        )
    })?;
    Ok(home.context_dir(&chosen.context_id))
}
