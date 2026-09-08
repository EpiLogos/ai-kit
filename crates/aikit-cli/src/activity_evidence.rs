//! W4 project activity evidence: a composed PostToolUse reaction.
//!
//! A completed tool use is evidence that its project is active. The receipt is
//! append-only, attributed to the real context and tool, and carries an
//! explicit touched path when the event supplied one. No prompt or tool-input
//! content is persisted.

use std::path::Path;

use aikit_core::hooks::HookEvent;
use aikit_core::ContextId;
use aikit_store::{Index, ProjectActivityEvidence, Timestamp};

/// Record this completed tool use and return the receipt that was stored.
pub fn record(
    index: &Index,
    context_id: &ContextId,
    project_root: &Path,
    event: &HookEvent,
) -> aikit_core::Result<ProjectActivityEvidence> {
    let touched_path = crate::file_context::file_path_of(event).map(|raw| {
        let path = Path::new(&raw);
        path.strip_prefix(project_root)
            .unwrap_or(path)
            .to_path_buf()
    });
    let evidence = ProjectActivityEvidence::new(
        project_root,
        Timestamp::now(),
        context_id.clone(),
        event.tool_name.clone(),
        touched_path,
    );
    index.record_project_activity(&evidence)?;
    Ok(evidence)
}

/// Stable JSON for inspection surfaces. The evidence id makes this a receipt,
/// not a bare timestamp claim.
pub fn describe(evidence: &ProjectActivityEvidence) -> serde_json::Value {
    serde_json::json!({
        "evidence_id": evidence.evidence_id.to_string(),
        "occurred_at": evidence.occurred_at.to_string(),
        "context_id": evidence.context_id.to_string(),
        "tool": evidence.tool,
        "touched_path": evidence.touched_path.as_deref().map(|p| p.display().to_string()),
    })
}
