//! AIKit persistence.
//!
//! `aikit-core` is deliberately free of I/O; this crate is where the bytes are.
//! Its responsibility is to make three promises true on a real filesystem:
//!
//! 1. **Canonical files are the truth.** Capsule manifests, payloads, profile
//!    TOML and project declarations are authoritative. The SQLite index, the
//!    search facets and the materialized generations are derived, and
//!    `index::Index::reindex` can rebuild the derived half from the canonical
//!    half without losing the genuinely operational records (usage events, live
//!    bindings, issued bypasses).
//! 2. **A failed apply changes nothing.** Generations are built into a temporary
//!    directory, validated in full, and only then renamed and pointed at. The
//!    `current` symlink is replaced atomically, and a commit against a stale base
//!    is refused (`generation.stale_base`) rather than allowed to clobber.
//! 3. **A captured secret never enters a registry.** The scanner in `scan` is
//!    built in — it depends on no capsule, because a capability that has not been
//!    reviewed yet must not be the thing that decides whether the thing being
//!    reviewed is safe.
//!
//! What this crate refuses to do: **resolve**. There is no capability semantics
//! here. The store loads a catalog, hands it to `aikit_core::resolve`, and writes
//! down what came back. It also refuses to decide *policy*: trust is recorded,
//! never inferred; quarantine is enforced, never overridden; and isolation is
//! carried through and recorded, never assumed. A context whose task shares the
//! session's working tree is written down as `shared`, and the generation
//! metadata says so, so no later reader can mistake it for a task with a tree of
//! its own.
//!
//! ## Where to look
//!
//! | Question | Module |
//! |---|---|
//! | Where does AIKit put things? | [`home`] |
//! | How is a capsule read off disk, and where does its revision come from? | [`registry`] |
//! | What is in SQLite, and what survives a rebuild? | [`index`] |
//! | Who reviewed what? | [`trust`] |
//! | Why did two panes not corrupt each other? | [`locks`], [`generation`] |
//! | What actually lands in `current/`? | [`generation`] |
//! | What was recorded, and what was deliberately not? | [`events`] |
//! | Where is safe credential binding metadata persisted? | [`credentials`] |
//! | How is learned accessibility rebuilt? | [`familiarity`] |
//! | Why was this capture held back? | [`scan`], [`inbox`] |
//! | How does a capture become a capsule? | [`inbox`] |
//! | Why did my hand-formatted profile survive a toggle? | [`edit`] |
//! | Which session is this, after tmux restarted? | [`state`] |
//! | Which SessionSpace semantic state and receipts survive provider loss? | [`session_space_application`] |

#![forbid(unsafe_code)]

pub mod channel;
pub mod credentials;
pub mod curator;
pub mod edit;
pub mod events;
pub mod familiarity;
pub mod generation;
pub mod home;
pub mod inbox;
pub mod index;
pub mod knowledge_application;
pub mod locks;
pub mod procedure;
pub mod registry;
pub mod scan;
pub mod session_space_application;
pub mod session_space_evidence;
pub mod skillsets;
pub mod state;
pub mod template;
pub mod trust;

// The modules stay the documented home of each type; these are the names the
// four consuming crates would otherwise import a dozen `use` lines to reach.
pub use channel::{Evidence, InboxChannel, InboxItem, InboxKind, InboxState, NewItem};
pub use credentials::{CredentialBindingStore, CREDENTIAL_BINDING_STORE_VERSION};
pub use curator::{curate, detect_drift, report_drift, CurationReport, Drift};
pub use edit::{OverlayDocument, ProfileDocument};
pub use events::{Event, EventAction, EventRecorder, Outcome, Timestamp};
pub use familiarity::{
    append_familiarity_observation, append_familiarity_reset, familiarity_observation_event,
    familiarity_reset_event, replay_familiarity, FamiliarityReplay, FAMILIARITY_OBSERVATION_EVENT,
    FAMILIARITY_RESET_EVENT,
};
pub use generation::{
    CommittedGeneration, GenerationBuilder, GenerationMetadata, StagedGeneration,
};
pub use home::AikitHome;
pub use inbox::{Candidate, CandidateState, Capture, Inbox, PromotionEdits, Similarity};
pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};
pub use knowledge_application::{
    KnowledgeApplicationReceipt, KnowledgeApplicationStore, KnowledgeHistoryOperation,
    KNOWLEDGE_APPLICATION_STORE_VERSION,
};
pub use locks::{ContextLock, LockOptions};
pub use procedure::{plan_procedure, EditDiff, ProcedureDiff, ProcedureOutcome, ProcedureRunner};
pub use registry::{load_project_local, load_registry, RegistryLoad, RegistryProblem, Snapshot};
pub use scan::{Finding, Scanner};
pub use session_space_application::{
    SessionSpaceApplicationStore, SessionSpaceHistoryComparison, SessionSpaceReceipt,
    SESSION_SPACE_STORE_VERSION,
};
pub use session_space_evidence::{
    explain_session_space_with_receipts, SessionSpaceExplainEvidence,
};
pub use state::{ContextRecord, SessionRecord, SessionState, StateStore};
pub use template::{plan_instantiation, ParamValues};
pub use trust::{TrustSnapshot, TrustStore};
