//! Receipt-backed Explain evidence for persisted SessionSpace semantics.
//!
//! Explain remains read-only. It combines the core semantic explanation with the
//! latest immutable receipt held by the same canonical persistence authority so a
//! consumer can answer *what changed this?* without replaying resolver/domain
//! logic or consulting a second history store.

use serde::{Deserialize, Serialize};

use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceExplanation, SessionSpaceReconstructionReport,
};
use aikit_core::Result;

use crate::{SessionSpaceApplicationStore, SessionSpaceReceipt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSpaceExplainEvidence {
    pub explanation: SessionSpaceExplanation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_receipt: Option<SessionSpaceReceipt>,
    pub receipt_count: usize,
}

pub fn explain_session_space_with_receipts(
    store: &SessionSpaceApplicationStore,
    space: &SessionSpaceRef,
    reconstruction: Option<SessionSpaceReconstructionReport>,
) -> Result<SessionSpaceExplainEvidence> {
    let explanation = store.explain(space, reconstruction)?;
    let receipts = store.history(space)?;
    Ok(SessionSpaceExplainEvidence {
        explanation,
        latest_receipt: receipts.last().cloned(),
        receipt_count: receipts.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AikitHome;
    use aikit_core::session_space_application::SessionSpaceMutation;

    #[test]
    fn explain_uses_the_same_receipt_that_changed_canonical_state() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let store = SessionSpaceApplicationStore::new(home);
        let id = SessionSpaceRef::parse("session-space/explain-receipt").unwrap();
        let preview = store
            .stage(
                None,
                SessionSpaceMutation::Create {
                    id: id.clone(),
                    label: Some("explain receipt".into()),
                },
            )
            .unwrap();
        let receipt = store.apply(&preview).unwrap();
        let evidence = explain_session_space_with_receipts(&store, &id, None).unwrap();
        assert_eq!(evidence.receipt_count, 1);
        assert_eq!(evidence.latest_receipt.as_ref(), Some(&receipt));
        assert_eq!(
            evidence.explanation.semantic_revision,
            receipt.after.revision
        );
    }
}
