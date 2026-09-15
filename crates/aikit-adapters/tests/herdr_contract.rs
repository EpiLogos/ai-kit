//! Herdr, driven by recorded responses.
//!
//! Herdr is a terminal workspace manager whose layout (workspace/tab/pane
//! topology), real panes and recognised coding agents are observed through its
//! public `herdr` CLI/API. The provider in `aikit_adapters::herdr` keeps every
//! one of those ids as provider-native evidence; canonical AIKit Surface,
//! Project and AgentSession refs exist only where a caller binds them
//! explicitly. This suite pins that contract the way `cmux_contract.rs` pins
//! cmux's.
//!
//! The JSON fixtures in `tests/fixtures/herdr/` are the recorded shapes of the
//! protocol this build understands, captured against the pinned upstream
//! revision `herdrdev/herdr@94f6d9c0d9bb9cf9ffae99d8bbfb09e9bf2fc9e0`
//! (`HERDR_UPSTREAM_REVISION`). `session-snapshot.json` and the create/split/
//! start responses are the recorded shapes verbatim; `session-snapshot-wide.json`
//! is a structural variation inside the same recorded grammar (more entries in
//! the same arrays, every status of the agent vocabulary, and the field
//! fallbacks the parser is documented to tolerate) — it is not a new capture.
//! Error envelopes are inline and assert only what the parser actually reads.
//!
//! Two honest boundaries of this suite:
//!
//! * Herdr is a Linux GUI application, so no live daemon can run on every
//!   development host. Everything here drives the provider through
//!   `ScriptedRunner`; a real-process suite needs a Linux host with herdr
//!   installed, in the manner of `tmux_real.rs`.
//! * The upstream pin is **provenance evidence, not a hard gate**: the provider
//!   records the revision and snapshot protocol in every observation rather
//!   than rejecting a snapshot from a different version. The tests pin that
//!   evidence so drift from the pin is discoverable, not silently absorbed.

use std::sync::Arc;

use aikit_adapters::herdr::HerdrSplitDirection;
use aikit_adapters::runner::ScriptedRunner;
use aikit_adapters::{
    HERDR_PROVIDER_VERSION, HERDR_UPSTREAM_REVISION, HerdrAgentStatus, HerdrSnapshot,
    HerdrWorkingEnvironment, NativeBindingKind, WORKING_ENVIRONMENT_PROVIDER_VERSION,
    WorkingEnvironmentHealth, WorkingEnvironmentProvider, parse_herdr_snapshot,
};
use aikit_core::resource::ResourceRef;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/herdr")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} should exist: {e}", path.display()))
}

fn snapshot_runner() -> ScriptedRunner {
    ScriptedRunner::new().on("api snapshot", &fixture("session-snapshot.json"))
}

fn wide_runner() -> ScriptedRunner {
    ScriptedRunner::new().on("api snapshot", &fixture("session-snapshot-wide.json"))
}

fn parse_fixture(name: &str) -> HerdrSnapshot {
    parse_herdr_snapshot(&fixture(name)).unwrap()
}

// ---------------------------------------------------------------------------
// Snapshot parsing: the recorded protocol shapes
// ---------------------------------------------------------------------------

#[test]
fn a_recorded_pinned_revision_snapshot_parses_without_collapsing_native_ids() {
    let snapshot = parse_fixture("session-snapshot.json");

    assert_eq!(snapshot.version, "0.9.0");
    assert_eq!(snapshot.protocol, 7);
    assert_eq!(snapshot.workspace_ids, vec!["w1"]);
    assert_eq!(snapshot.pane_ids, vec!["w1:p1", "w1:p2"]);
    assert_eq!(snapshot.focused_workspace_id.as_deref(), Some("w1"));
    assert_eq!(snapshot.focused_tab_id.as_deref(), Some("w1:t1"));
    assert_eq!(snapshot.focused_pane_id.as_deref(), Some("w1:p2"));

    let agent = &snapshot.agents[0];
    assert_eq!(
        agent.native_id, "term-2",
        "the terminal id is the native id"
    );
    assert_eq!(agent.pane_id, "w1:p2", "the pane stays distinct evidence");
    assert_eq!(agent.name.as_deref(), Some("reviewer"));
    assert_eq!(agent.status, HerdrAgentStatus::Blocked);
}

#[test]
fn every_recognised_agent_status_round_trips_and_an_unknown_word_is_refused() {
    let snapshot = parse_fixture("session-snapshot-wide.json");
    let statuses: Vec<_> = snapshot.agents.iter().map(|a| a.status).collect();
    assert_eq!(
        statuses,
        vec![
            HerdrAgentStatus::Working,
            HerdrAgentStatus::Blocked,
            HerdrAgentStatus::Done,
            HerdrAgentStatus::Idle,
            HerdrAgentStatus::Unknown,
        ],
        "the wide fixture carries the whole agent status vocabulary"
    );

    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p","agent_status":"finished"}]}}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.unknown_agent_status");
}

#[test]
fn an_agent_status_field_takes_precedence_over_the_legacy_status_alias() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p","agent_status":"working","status":"done"}]}}}"#;
    let snapshot = parse_herdr_snapshot(raw).unwrap();
    assert_eq!(snapshot.agents[0].status, HerdrAgentStatus::Working);
}

#[test]
fn a_missing_status_field_means_unknown_rather_than_an_invented_state() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","pane_id":"p"}]}}}"#;
    let snapshot = parse_herdr_snapshot(raw).unwrap();
    assert_eq!(snapshot.agents[0].status, HerdrAgentStatus::Unknown);
}

#[test]
fn an_agents_native_id_prefers_terminal_id_then_name_then_pane() {
    let both = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"term-9","name":"dup","pane_id":"w9:p4","agent_status":"idle"}]}}}"#;
    let snapshot = parse_herdr_snapshot(both).unwrap();
    assert_eq!(snapshot.agents[0].native_id, "term-9");

    let name_only = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"name":"scout","pane_id":"w9:p1","agent_status":"idle"}]}}}"#;
    let snapshot = parse_herdr_snapshot(name_only).unwrap();
    assert_eq!(snapshot.agents[0].native_id, "scout");

    let pane_only = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"pane_id":"w9:p3","agent_status":"idle"}]}}}"#;
    let snapshot = parse_herdr_snapshot(pane_only).unwrap();
    assert_eq!(
        snapshot.agents[0].native_id, "w9:p3",
        "the pane id is the last-resort native id, not a collapse of the two"
    );
}

#[test]
fn an_agent_without_a_pane_is_rejected_rather_than_orphaned() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"x","protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[{"terminal_id":"t","agent_status":"idle"}]}}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.agent_missing_pane");
}

// ---------------------------------------------------------------------------
// Envelope handling: partial and hostile data
// ---------------------------------------------------------------------------

#[test]
fn an_api_error_envelope_surfaces_herdr_s_own_message() {
    let error = parse_herdr_snapshot(&fixture("api-error.json")).unwrap_err();
    assert_eq!(error.code(), "herdr.api_error");
    assert_eq!(
        error.message(),
        "no workspace manager is running on this socket"
    );
}

#[test]
fn a_response_without_a_result_is_rejected() {
    let error = parse_herdr_snapshot(r#"{"id":"x"}"#).unwrap_err();
    assert_eq!(error.code(), "herdr.missing_result");
}

#[test]
fn a_result_of_the_wrong_type_is_rejected_instead_of_force_parsed() {
    let raw = r#"{"id":"x","result":{"type":"event_stream","events":[]}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.unexpected_result");
}

#[test]
fn a_session_snapshot_without_a_body_is_rejected() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot"}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.missing_snapshot");
}

#[test]
fn a_snapshot_without_a_version_is_rejected_rather_than_defaulted() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"protocol":1,"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[]}}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.missing_version");
}

#[test]
fn a_snapshot_without_a_protocol_is_rejected_rather_than_defaulted() {
    let raw = r#"{"id":"x","result":{"type":"session_snapshot","snapshot":{"version":"0.9.0","workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[]}}}"#;
    let error = parse_herdr_snapshot(raw).unwrap_err();
    assert_eq!(error.code(), "herdr.missing_protocol");
}

#[test]
fn unparseable_herdr_output_is_an_invalid_json_error_naming_the_subject() {
    let error = parse_herdr_snapshot("herdr: connection reset").unwrap_err();
    assert_eq!(error.code(), "herdr.invalid_json");
    assert!(
        error.message().contains("Herdr API response"),
        "the error names which response could not be parsed: {}",
        error.message()
    );
}

// ---------------------------------------------------------------------------
// Workspace, split and agent automation
// ---------------------------------------------------------------------------

#[test]
fn workspace_create_sends_the_no_focus_command_and_adopts_only_returned_ids() {
    let runner = Arc::new(
        ScriptedRunner::new()
            .on("workspace create", &fixture("workspace-created.json"))
            .on("workspace focus", "{}")
            .on("api snapshot", &fixture("session-snapshot-wide.json")),
    );
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .with_create("/repo", Some("reference".into()));

    let created = provider.create_workspace().unwrap();
    assert_eq!(created.workspace_id, "w7");
    assert_eq!(created.tab_id, "w7:t1");
    assert_eq!(created.root_pane_id, "w7:p1");

    let calls = runner.call_lines();
    assert!(
        calls
            .iter()
            .any(|call| call == "herdr workspace create --cwd /repo --no-focus --label reference"),
        "creation must carry the cwd and label and must not steal focus: {calls:?}"
    );

    // The adopted workspace id is provider evidence: focusing later uses it.
    provider.focus_workspace().unwrap();
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr workspace focus w7")
    );
}

#[test]
fn a_create_without_a_configured_cwd_refuses_before_any_call() {
    let runner = Arc::new(ScriptedRunner::new());
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"));

    let error = provider.create_workspace().unwrap_err();
    assert_eq!(error.code(), "herdr.workspace_absent");
    assert!(
        runner.calls().is_empty(),
        "a refusal about local configuration must not reach the provider: {:?}",
        runner.calls()
    );
}

#[test]
fn a_create_response_missing_any_id_is_refused_field_by_field() {
    let cases = [
        (r#"{"id":"x"}"#, "herdr.create_missing_result"),
        (
            r#"{"id":"x","result":{"type":"workspace_created"}}"#,
            "herdr.create_missing_workspace",
        ),
        (
            r#"{"id":"x","result":{"workspace":{"workspace_id":"w7"}}}"#,
            "herdr.create_missing_tab",
        ),
        (
            r#"{"id":"x","result":{"workspace":{"workspace_id":"w7"},"tab":{"tab_id":"w7:t1"}}}"#,
            "herdr.create_missing_root_pane",
        ),
    ];
    for (raw, code) in cases {
        let runner = ScriptedRunner::new().on("workspace create", raw);
        let mut provider =
            HerdrWorkingEnvironment::new(runner, r("provider/herdr")).with_create("/repo", None);
        let error = provider.create_workspace().unwrap_err();
        assert_eq!(error.code(), code, "response was {raw}");
    }
}

#[test]
fn a_failed_herdr_command_names_the_command_and_exit() {
    let runner = ScriptedRunner::new().failing(
        "api snapshot",
        1,
        "herdr: no running session manager on the default socket",
    );
    let provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"));

    let error = provider.snapshot().unwrap_err();
    assert_eq!(error.code(), "herdr.command_failed");
    let message = error.message();
    assert!(
        message.contains("`herdr api snapshot`") && message.contains("status 1"),
        "the error names the exact command and its exit: {message}"
    );
    assert!(
        message.contains("no running session manager"),
        "herdr's own stderr reaches the user: {message}"
    );
}

#[test]
fn a_split_uses_herdr_direction_words_and_binds_the_returned_pane_only() {
    let runner = Arc::new(
        ScriptedRunner::new()
            .on("pane split", &fixture("pane-split.json"))
            .on("api snapshot", &fixture("session-snapshot-wide.json")),
    );
    let root = r("surface/reference/root");
    let review = r("surface/reference/review");
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(root.clone(), "w7:p1");

    let pane = provider
        .split_surface(&root, review.clone(), HerdrSplitDirection::Right)
        .unwrap();
    assert_eq!(pane, "w7:p2", "only the pane herdr returned is adopted");
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr pane split w7:p1 --direction right --no-focus"),
        "{:?}",
        runner.call_lines()
    );

    let pane = provider
        .split_surface(
            &root,
            r("surface/reference/logs"),
            HerdrSplitDirection::Down,
        )
        .unwrap();
    assert_eq!(pane, "w7:p2");
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr pane split w7:p1 --direction down --no-focus")
    );

    let observation = provider.observe().unwrap();
    assert_eq!(observation.canonical_native_id(&review), Some("w7:p2"));
}

#[test]
fn each_failed_mutation_carries_its_own_error_code() {
    let surface = r("surface/reference/root");

    let runner = ScriptedRunner::new().failing("workspace create", 1, "herdr: socket refused");
    let mut provider =
        HerdrWorkingEnvironment::new(runner, r("provider/herdr")).with_create("/repo", None);
    assert_eq!(
        provider.create_workspace().unwrap_err().code(),
        "herdr.workspace_create_failed"
    );

    let runner = ScriptedRunner::new().failing("pane split", 1, "herdr: pane is gone");
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(surface.clone(), "w7:p1");
    assert_eq!(
        provider
            .split_surface(
                &surface,
                r("surface/reference/review"),
                HerdrSplitDirection::Right
            )
            .unwrap_err()
            .code(),
        "herdr.command_failed",
        "a split runs through the shared command seam, not a mutation-specific code"
    );

    let runner = ScriptedRunner::new().failing("agent start", 1, "herdr: no such kind");
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(surface.clone(), "w1:p2");
    assert_eq!(
        provider
            .start_agent_session(
                r("agent-session/reference/reviewer"),
                &surface,
                "reviewer",
                "codex",
                None,
                &[],
            )
            .unwrap_err()
            .code(),
        "herdr.agent_start_failed"
    );
}

#[test]
fn a_non_json_mutation_response_is_refused_by_response_kind() {
    let surface = r("surface/reference/root");

    let runner = ScriptedRunner::new().on("workspace create", "created workspace w7");
    let mut provider =
        HerdrWorkingEnvironment::new(runner, r("provider/herdr")).with_create("/repo", None);
    assert_eq!(
        provider.create_workspace().unwrap_err().code(),
        "herdr.invalid_create_response"
    );

    let runner = ScriptedRunner::new().on("pane split", "OK pane:7");
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(surface.clone(), "w7:p1");
    assert_eq!(
        provider
            .split_surface(
                &surface,
                r("surface/reference/review"),
                HerdrSplitDirection::Right
            )
            .unwrap_err()
            .code(),
        "herdr.invalid_split_response"
    );

    let runner = ScriptedRunner::new().on("agent start", "started reviewer in w7:p2");
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(surface.clone(), "w1:p2");
    assert_eq!(
        provider
            .start_agent_session(
                r("agent-session/reference/reviewer"),
                &surface,
                "reviewer",
                "codex",
                None,
                &[],
            )
            .unwrap_err()
            .code(),
        "herdr.invalid_agent_start_response"
    );
}

#[test]
fn a_split_from_an_unbound_surface_refuses_before_any_call() {
    let runner = Arc::new(ScriptedRunner::new());
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"));

    let error = provider
        .split_surface(
            &r("surface/reference/root"),
            r("surface/reference/review"),
            HerdrSplitDirection::Right,
        )
        .unwrap_err();
    assert_eq!(error.code(), "herdr.surface_unbound");
    assert!(
        runner.calls().is_empty(),
        "no topology change may be attempted from an unbound source: {:?}",
        runner.calls()
    );
}

#[test]
fn a_split_response_without_a_pane_is_refused() {
    let runner =
        ScriptedRunner::new().on("pane split", r#"{"id":"x","result":{"type":"pane_info"}}"#);
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(r("surface/reference/root"), "w7:p1");

    let error = provider
        .split_surface(
            &r("surface/reference/root"),
            r("surface/reference/review"),
            HerdrSplitDirection::Right,
        )
        .unwrap_err();
    assert_eq!(error.code(), "herdr.split_missing_pane");
}

#[test]
fn agent_start_argv_carries_kind_pane_timeout_and_passthrough_args() {
    let runner = Arc::new(
        ScriptedRunner::new()
            .on("agent start", &fixture("agent-started.json"))
            .on("pane split", &fixture("pane-split.json")),
    );
    let review = r("surface/reference/review");
    let agent_surface = r("surface/reference/agent");
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(review.clone(), "w7:p1");
    provider
        .split_surface(&review, agent_surface.clone(), HerdrSplitDirection::Right)
        .unwrap();

    let started = provider
        .start_agent_session(
            r("agent-session/reference/reviewer"),
            &agent_surface,
            "reviewer",
            "codex",
            Some(250),
            &["-m".into(), "gpt-5.4".into()],
        )
        .unwrap();
    assert_eq!(started.terminal_id, "term-2");
    assert_eq!(started.pane_id, "w7:p2");
    assert_eq!(started.name.as_deref(), Some("reviewer"));
    assert_eq!(started.status, HerdrAgentStatus::Idle);

    let calls = runner.call_lines();
    assert!(
        calls.iter().any(|call| call
            == "herdr agent start reviewer --kind codex --pane w7:p2 --timeout 250 -- -m gpt-5.4"),
        "kind, bound pane, timeout and agent passthrough args must all be sent: {calls:?}"
    );
}

#[test]
fn agent_start_refuses_a_returned_name_that_drifts_from_the_request() {
    let runner = ScriptedRunner::new().on(
        "agent start",
        r#"{"id":"x","result":{"agent":{"terminal_id":"term-x","name":"someone-else","pane_id":"w1:p2","agent_status":"idle"}}}"#,
    );
    let surface = r("surface/reference/review");
    let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .bind_surface(surface.clone(), "w1:p2");

    let error = provider
        .start_agent_session(
            r("agent-session/reference/reviewer"),
            &surface,
            "reviewer",
            "codex",
            None,
            &[],
        )
        .unwrap_err();
    assert_eq!(error.code(), "herdr.agent_name_drift");
}

#[test]
fn agent_start_binds_the_name_when_returned_and_the_pane_when_it_is_not() {
    let session = r("agent-session/reference/reviewer");
    let surface = r("surface/reference/review");

    let runner = Arc::new(
        ScriptedRunner::new()
            .on("agent start", &fixture("agent-started.json"))
            .on("agent focus", "{}"),
    );
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(surface.clone(), "w7:p2");
    provider
        .start_agent_session(session.clone(), &surface, "reviewer", "codex", None, &[])
        .unwrap();
    provider.focus_agent_session(&session).unwrap();
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr agent focus reviewer")
    );

    let runner = Arc::new(
        ScriptedRunner::new()
            .on(
                "agent start",
                r#"{"id":"x","result":{"agent":{"terminal_id":"term-3","pane_id":"w7:p2","agent_status":"working"}}}"#,
            )
            .on("agent focus", "{}"),
    );
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(surface.clone(), "w7:p2");
    let started = provider
        .start_agent_session(session.clone(), &surface, "reviewer", "codex", None, &[])
        .unwrap();
    assert_eq!(
        started.name, None,
        "a name herdr did not return is never invented"
    );
    provider.focus_agent_session(&session).unwrap();
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr agent focus w7:p2"),
        "with no name, the pane id is the focus handle: {:?}",
        runner.call_lines()
    );
}

#[test]
fn a_start_response_missing_a_field_is_refused() {
    let cases = [
        (
            r#"{"id":"x","result":{}}"#,
            "herdr.agent_start_missing_agent",
        ),
        (
            r#"{"id":"x","result":{"agent":{"terminal_id":"t"}}}"#,
            "herdr.agent_start_missing_pane",
        ),
        (
            r#"{"id":"x","result":{"agent":{"pane_id":"w1:p2","name":"reviewer","agent_status":"idle"}}}"#,
            "herdr.agent_start_missing_terminal",
        ),
    ];
    for (raw, code) in cases {
        let runner = ScriptedRunner::new().on("agent start", raw);
        let surface = r("surface/reference/review");
        let mut provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
            .bind_surface(surface.clone(), "w1:p2");
        let error = provider
            .start_agent_session(
                r("agent-session/reference/reviewer"),
                &surface,
                "reviewer",
                "codex",
                None,
                &[],
            )
            .unwrap_err();
        assert_eq!(error.code(), code, "response was {raw}");
    }
}

#[test]
fn focusing_an_unbound_agent_session_refuses_without_a_call() {
    let runner = Arc::new(ScriptedRunner::new());
    let provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"));

    let error = provider
        .focus_agent_session(&r("agent-session/reference/reviewer"))
        .unwrap_err();
    assert_eq!(error.code(), "herdr.agent_session_unbound");
    assert!(runner.calls().is_empty());
}

// ---------------------------------------------------------------------------
// Observation, mapping and health
// ---------------------------------------------------------------------------

#[test]
fn an_unbound_provider_reports_no_bindings_rather_than_inventing_them() {
    let mut provider = HerdrWorkingEnvironment::new(wide_runner(), r("provider/herdr"));
    let observation = provider.observe().unwrap();

    assert_eq!(observation.health, WorkingEnvironmentHealth::Healthy);
    assert!(
        observation.bindings.is_empty(),
        "workspaces, panes and agents herdr reported stay unbound evidence: {:?}",
        observation.bindings
    );
    assert_eq!(observation.focused_native_id.as_deref(), Some("w7:p2"));
}

#[test]
fn a_bound_workspace_stays_provider_native_session_evidence_with_no_canonical_ref() {
    let mut provider =
        HerdrWorkingEnvironment::new(wide_runner(), r("provider/herdr")).with_workspace("w7");
    let observation = provider.observe().unwrap();

    let session = observation
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::Session)
        .expect("the bound workspace is reported once");
    assert_eq!(session.native_id, "w7");
    assert_eq!(
        session.canonical_ref, None,
        "a Herdr Workspace is provider-native SessionSpace evidence, never minted canonical identity"
    );
    assert_eq!(observation.bindings.len(), 1, "nothing else was bound");
}

#[test]
fn herdr_pane_and_agent_bindings_carry_canonical_refs_only_where_explicitly_bound() {
    let root = r("surface/reference/root");
    let project = r("project/reference");
    let session = r("agent-session/reference/reviewer");
    let mut provider = HerdrWorkingEnvironment::new(wide_runner(), r("provider/herdr"))
        .with_workspace("w7")
        .bind_surface(root.clone(), "w7:p1")
        .bind_project(project.clone(), "w7")
        .bind_agent_session(session.clone(), "reviewer");
    let observation = provider.observe().unwrap();

    let surface = observation
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::Surface)
        .unwrap();
    assert_eq!(surface.native_id, "w7:p1");
    assert_ne!(
        surface.native_id,
        root.to_string(),
        "a Herdr pane id is not a canonical Surface ref"
    );
    assert_eq!(observation.canonical_native_id(&root), Some("w7:p1"));

    let project_binding = observation
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::Project)
        .unwrap();
    assert_eq!(project_binding.native_id, "w7");
    assert_eq!(project_binding.canonical_ref.as_ref(), Some(&project));

    let agent = observation
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::AgentSession)
        .unwrap();
    assert_eq!(agent.native_id, "reviewer");
    assert_eq!(
        observation.canonical_native_id(&session),
        Some("reviewer"),
        "a Herdr agent name is the bound native handle, not a canonical Agent identity"
    );

    // Every other pane and agent in the snapshot stayed unbound evidence.
    assert_eq!(observation.bindings.len(), 4);
    assert_eq!(
        observation
            .bindings
            .iter()
            .filter(|binding| binding.canonical_ref.is_none())
            .count(),
        1,
        "only the Session-kind workspace binding is native-only"
    );
}

#[test]
fn a_snapshot_where_every_bound_id_is_present_is_healthy_and_reports_focus() {
    let mut provider = HerdrWorkingEnvironment::new(snapshot_runner(), r("provider/herdr"))
        .with_workspace("w1")
        .bind_surface(r("surface/reference/root"), "w1:p1")
        .bind_surface(r("surface/reference/review"), "w1:p2");

    let observation = provider.observe().unwrap();
    assert_eq!(observation.health, WorkingEnvironmentHealth::Healthy);
    assert_eq!(observation.focused_native_id.as_deref(), Some("w1:p2"));
    assert_eq!(observation.provider_version.as_deref(), Some("0.9.0"));
}

#[test]
fn a_workspace_that_vanished_from_the_snapshot_degrades_the_observation() {
    let mut provider = HerdrWorkingEnvironment::new(snapshot_runner(), r("provider/herdr"))
        .with_workspace("w-gone");

    let observation = provider.observe().unwrap();
    assert_eq!(observation.health, WorkingEnvironmentHealth::Degraded);
    assert_eq!(observation.focused_native_id.as_deref(), Some("w1:p2"));
}

#[test]
fn a_pane_that_vanished_from_the_snapshot_degrades_the_observation() {
    let mut provider = HerdrWorkingEnvironment::new(snapshot_runner(), r("provider/herdr"))
        .with_workspace("w1")
        .bind_surface(r("surface/reference/root"), "w1:p1")
        .bind_surface(r("surface/reference/review"), "w9:p9");

    let observation = provider.observe().unwrap();
    assert_eq!(
        observation.health,
        WorkingEnvironmentHealth::Degraded,
        "one missing pane degrades the whole observation; the surviving binding still resolves"
    );
    assert_eq!(
        observation.canonical_native_id(&r("surface/reference/root")),
        Some("w1:p1")
    );
}

#[test]
fn open_recreates_a_workspace_that_vanished_instead_of_keeping_a_stale_binding() {
    let stale = r#"{"id":"cli:api:snapshot","result":{"type":"session_snapshot","snapshot":{"version":"0.9.0","protocol":7,"workspaces":[{"workspace_id":"w1"}],"tabs":[],"panes":[{"pane_id":"w1:p1"}],"layouts":[],"agents":[]}}}"#;
    let runner = Arc::new(
        ScriptedRunner::new()
            .sequence(
                "api snapshot",
                &[stale, &fixture("session-snapshot-wide.json")],
            )
            .on("workspace create", &fixture("workspace-created.json")),
    );
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .with_workspace("w-gone")
        .with_create("/repo", None);

    let opened = provider.open().unwrap();
    assert_eq!(
        opened.health,
        WorkingEnvironmentHealth::Healthy,
        "open() adopts the recreated workspace rather than reporting the stale one"
    );
    let session = opened
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::Session)
        .unwrap();
    assert_eq!(
        session.native_id, "w7",
        "the new workspace id is the evidence"
    );

    let creates = runner
        .call_lines()
        .iter()
        .filter(|call| call.contains("workspace create"))
        .count();
    assert_eq!(creates, 1, "recreation happened exactly once");
}

// ---------------------------------------------------------------------------
// The public trait surface
// ---------------------------------------------------------------------------

#[test]
fn capabilities_state_exactly_what_herdr_does_and_does_not_do() {
    let provider = HerdrWorkingEnvironment::new(ScriptedRunner::new(), r("provider/herdr"));
    let caps = provider.capabilities();

    assert!(caps.discover && caps.open && caps.focus && caps.select);
    assert!(caps.multi_project);
    assert!(
        caps.terminal_surface,
        "a Herdr pane is a real terminal locus"
    );
    assert!(
        caps.conversation_surface,
        "recognised agents converse in panes"
    );
    assert!(caps.reconstruct);
    assert!(!caps.editor_surface);
    assert!(!caps.diff_surface);
    assert!(!caps.preview_surface);
    assert!(!caps.test_surface);
    assert!(
        !caps.surface_attach_detach && !caps.agent_session_attach_detach,
        "attaching to foreign Herdr topology is not claimed"
    );
}

#[test]
fn generic_detach_is_refused_as_destructive_provider_local_lifecycle() {
    let runner = Arc::new(ScriptedRunner::new());
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(r("surface/reference/root"), "w1:p1");

    let error = provider
        .detach_surface(&r("surface/reference/root"))
        .unwrap_err();
    assert_eq!(error.code(), "herdr.detach_requires_explicit_operation");
    assert!(
        runner.calls().is_empty(),
        "closing a pane is destructive provider-local lifecycle; generic detach must not issue it: {:?}",
        runner.calls()
    );
}

#[test]
fn focusing_a_surface_addresses_its_explicitly_bound_pane() {
    let runner = Arc::new(snapshot_runner().on("pane focus", "{}"));
    let mut provider = HerdrWorkingEnvironment::new(runner.clone(), r("provider/herdr"))
        .bind_surface(r("surface/reference/root"), "w1:p1");

    provider
        .focus_surface(&r("surface/reference/root"))
        .unwrap();
    assert!(
        runner
            .call_lines()
            .iter()
            .any(|call| call == "herdr pane focus w1:p1")
    );

    let error = provider
        .focus_surface(&r("surface/reference/unbound"))
        .unwrap_err();
    assert_eq!(error.code(), "herdr.surface_unbound");
}

#[test]
fn observation_provenance_carries_the_pinned_revision_protocol_and_schema() {
    let mut provider = HerdrWorkingEnvironment::new(wide_runner(), r("provider/herdr"));
    let observation = provider.observe().unwrap();

    assert_eq!(observation.provider, r("provider/herdr"));
    assert_eq!(
        observation.schema, WORKING_ENVIRONMENT_PROVIDER_VERSION,
        "the observation speaks the public working-environment seam"
    );
    let provenance = observation.provenance.join("\n");
    assert!(
        provenance.contains(&format!("herdrdev/herdr@{HERDR_UPSTREAM_REVISION}")),
        "every observation discloses the pinned upstream revision: {provenance}"
    );
    assert!(
        provenance.contains("Herdr public API snapshot protocol=8"),
        "the snapshot protocol is disclosed so drift from the pin is discoverable: {provenance}"
    );
    assert!(
        provenance.contains(HERDR_PROVIDER_VERSION),
        "the provider's own version is disclosed: {provenance}"
    );
}

#[test]
fn the_provider_participates_through_the_public_trait_object_seam() {
    let runner = Arc::new(
        ScriptedRunner::new()
            .on("api snapshot", &fixture("session-snapshot-wide.json"))
            .on("pane focus", "{}"),
    );
    let root = r("surface/reference/root");
    let provider = HerdrWorkingEnvironment::new(runner, r("provider/herdr"))
        .with_workspace("w7")
        .bind_surface(root.clone(), "w7:p1");
    let mut provider: Box<dyn WorkingEnvironmentProvider> = Box::new(provider);

    let opened = provider.open().unwrap();
    assert_eq!(opened.health, WorkingEnvironmentHealth::Healthy);
    assert_eq!(opened.provider_version.as_deref(), Some("0.9.1"));
    assert_eq!(provider.provider_ref(), &r("provider/herdr"));
    provider.focus_surface(&root).unwrap();
    let observed = provider.observe().unwrap();
    assert_eq!(observed.canonical_native_id(&root), Some("w7:p1"));
}
