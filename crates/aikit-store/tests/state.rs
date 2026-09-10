//! Contexts, sessions and their bindings.
//!
//! The property worth the most here is the one about identity. A tmux server
//! restart or a cmux workspace restore hands back *different* multiplexer ids for
//! what the user considers the same session. If AIKit keyed on those ids, every
//! restart would orphan the session's overlay and the user would silently lose
//! their session-scoped capabilities. So AIKit's own ids are authoritative and
//! the mux id is a rebindable attribute — which is what
//! `a_restored_session_is_rebound_by_name_and_project_not_by_mux_id` checks.

mod common;

use std::time::Duration;

use aikit_core::{ContextId, Isolation, MuxKind, ProjectId, SessionId};
use aikit_store::events::Timestamp;
use aikit_store::index::Index;
use aikit_store::state::{ContextRecord, SessionRecord, SessionState, StateStore};

fn index(dir: &std::path::Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

fn session(name: &str, project: &ProjectId, mux_session: &str) -> SessionRecord {
    SessionRecord {
        session_id: SessionId::generate(),
        name: name.to_string(),
        project_root: Some(format!("/work/{name}").into()),
        project_marker: Some(project.clone()),
        mux: MuxKind::Tmux,
        mux_session: Some(mux_session.to_string()),
        state: SessionState::Live,
        created_at: Timestamp::now(),
        last_seen: Timestamp::now(),
    }
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[test]
fn a_session_record_round_trips_through_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);
    let record = session("payments", &ProjectId::generate(), "$3");

    state.put_session(&record).unwrap();
    let restored = state.session(&record.session_id).unwrap().unwrap();

    assert_eq!(restored, record);
}

#[test]
fn a_restored_session_is_rebound_by_name_and_project_not_by_mux_id() {
    // tmux restarted; `$3` is now `$7`. The AIKit session id — and therefore the
    // session's overlay, contexts and generations — must survive that.
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let project = ProjectId::generate();
    let original = session("payments", &project, "$3");
    state.put_session(&original).unwrap();

    let rebound = state
        .rebind_session("payments", Some(&project), MuxKind::Tmux, Some("$7"))
        .unwrap()
        .expect("the restored session should have been recognized");

    assert_eq!(
        rebound.session_id, original.session_id,
        "AIKit's own id is authoritative; the mux id is a binding"
    );
    assert_eq!(rebound.mux_session.as_deref(), Some("$7"));
    assert_eq!(
        state.sessions().unwrap().len(),
        1,
        "no duplicate was created"
    );

    let reloaded = state.session(&original.session_id).unwrap().unwrap();
    assert_eq!(reloaded.mux_session.as_deref(), Some("$7"));
    assert_eq!(reloaded.state, SessionState::Live);
}

#[test]
fn rebinding_does_not_reach_across_projects_that_happen_to_share_a_name() {
    // "payments" is not an unusual session name. Matching on it alone would let a
    // restored session in one repository adopt another repository's overlay.
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let mine = ProjectId::generate();
    let theirs = ProjectId::generate();
    state
        .put_session(&session("payments", &mine, "$3"))
        .unwrap();

    assert!(state
        .rebind_session("payments", Some(&theirs), MuxKind::Tmux, Some("$9"))
        .unwrap()
        .is_none());
    assert_eq!(state.sessions().unwrap().len(), 1);
}

#[test]
fn rebinding_does_not_reach_across_multiplexers() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);
    let project = ProjectId::generate();
    state
        .put_session(&session("payments", &project, "$3"))
        .unwrap();

    assert!(
        state
            .rebind_session("payments", Some(&project), MuxKind::Cmux, Some("ws-1"))
            .unwrap()
            .is_none(),
        "a cmux workspace is not a restored tmux session"
    );
}

#[test]
fn rebinding_an_unknown_session_reports_nothing_rather_than_inventing_one() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    assert!(state
        .rebind_session("never-existed", None, MuxKind::Tmux, Some("$1"))
        .unwrap()
        .is_none());
    assert!(state.sessions().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Staleness
// ---------------------------------------------------------------------------

#[test]
fn a_session_not_seen_for_a_while_is_reported_stale_without_being_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let now = Timestamp::now();
    let long_ago = Timestamp::from_nanos(now.as_nanos() - 3_600_000_000_000);

    let mut old = session("abandoned", &ProjectId::generate(), "$1");
    old.last_seen = long_ago;
    let fresh = session("active", &ProjectId::generate(), "$2");
    state.put_session(&old).unwrap();
    state.put_session(&fresh).unwrap();

    let stale = state.stale_sessions(Duration::from_secs(600), now).unwrap();

    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].session_id, old.session_id);
    assert_eq!(
        state.sessions().unwrap().len(),
        2,
        "detecting staleness is not the same as deciding to delete"
    );
}

#[test]
fn touching_a_session_clears_its_staleness() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let now = Timestamp::now();
    let mut record = session("payments", &ProjectId::generate(), "$3");
    record.last_seen = Timestamp::from_nanos(now.as_nanos() - 3_600_000_000_000);
    state.put_session(&record).unwrap();
    assert_eq!(
        state
            .stale_sessions(Duration::from_secs(60), now)
            .unwrap()
            .len(),
        1
    );

    state.touch_session(&record.session_id, now).unwrap();
    assert!(state
        .stale_sessions(Duration::from_secs(60), now)
        .unwrap()
        .is_empty());
}

#[test]
fn a_closed_session_is_not_reported_as_stale() {
    // Stale means "we think this should still be here and it is not". A session
    // the user closed on purpose is neither.
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let now = Timestamp::now();
    let mut record = session("finished", &ProjectId::generate(), "$4");
    record.last_seen = Timestamp::from_nanos(now.as_nanos() - 3_600_000_000_000);
    record.state = SessionState::Closed;
    state.put_session(&record).unwrap();

    assert!(state
        .stale_sessions(Duration::from_secs(60), now)
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// Contexts and bindings
// ---------------------------------------------------------------------------

#[test]
fn a_context_record_remembers_its_isolation_choice() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let shared = ContextRecord {
        context_id: ContextId::generate(),
        session_id: Some(SessionId::generate()),
        project_id: Some(ProjectId::generate()),
        project_root: Some("/work/payments".into()),
        task: Some("migration-review".into()),
        isolation: Isolation::Shared,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    };
    state.put_context(&shared).unwrap();

    let restored = state.context(&shared.context_id).unwrap().unwrap();
    assert_eq!(
        restored.isolation,
        Isolation::Shared,
        "a task shares the tree unless it was asked not to, and the record must say which"
    );
    assert!(!restored.isolation.is_isolated());
    assert_eq!(restored.task.as_deref(), Some("migration-review"));
}

#[test]
fn one_session_can_own_several_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);
    let session_id = SessionId::generate();

    for (task, isolation) in [
        (None, Isolation::Shared),
        (Some("review"), Isolation::Shared),
        (Some("migrate"), Isolation::Worktree),
    ] {
        state
            .put_context(&ContextRecord {
                context_id: ContextId::generate(),
                session_id: Some(session_id.clone()),
                project_id: None,
                project_root: Some("/work/payments".into()),
                task: task.map(ToString::to_string),
                isolation,
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
            })
            .unwrap();
    }

    let contexts = state.contexts_of_session(&session_id).unwrap();
    assert_eq!(contexts.len(), 3);
    assert_eq!(
        contexts
            .iter()
            .filter(|c| c.isolation.is_isolated())
            .count(),
        1
    );
}

#[test]
fn a_context_binding_survives_being_rewritten_when_the_pane_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    let context = ContextId::generate();
    let session_id = SessionId::generate();
    let mut binding = aikit_core::ContextBinding {
        context_id: context.clone(),
        session_id: session_id.clone(),
        mux: MuxKind::Tmux,
        mux_session: Some("payments".into()),
        mux_surface: Some("%3".into()),
        project_root: Some("/work/payments".into()),
        isolation: Isolation::Shared,
    };
    state.bind_context(&binding).unwrap();

    binding.mux_surface = Some("%11".into());
    state.bind_context(&binding).unwrap();

    let bindings = state.bindings().unwrap();
    assert_eq!(
        bindings.len(),
        1,
        "rebinding replaces rather than accumulates"
    );
    assert_eq!(bindings[0].mux_surface.as_deref(), Some("%11"));
}

// ---------------------------------------------------------------------------
// The promotion queue
// ---------------------------------------------------------------------------

#[test]
fn the_promotion_queue_holds_candidates_with_the_reason_they_were_queued() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    state
        .queue_promotion("cnd_01ABC", "run three times in two days")
        .unwrap();
    state
        .queue_promotion("cnd_01DEF", "captured from a hook")
        .unwrap();

    let queued = state.promotion_queue().unwrap();
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].reason, "run three times in two days");

    state.dequeue_promotion("cnd_01ABC").unwrap();
    assert_eq!(state.promotion_queue().unwrap().len(), 1);
}

#[test]
fn queueing_the_same_candidate_twice_updates_the_reason_rather_than_duplicating() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let state = StateStore::new(&index);

    state.queue_promotion("cnd_01ABC", "first reason").unwrap();
    state.queue_promotion("cnd_01ABC", "better reason").unwrap();

    let queued = state.promotion_queue().unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].reason, "better reason");
}
