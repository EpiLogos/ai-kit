//! Matching is the palette's, ranking is core's.
//!
//! The seam is the whole point of these tests. nucleo runs in this process — the
//! palette never spawns `fzf`, because a subprocess per keystroke cannot meet a
//! 16 ms budget and would make the ranking policy live in a shell pipeline. What
//! nucleo produces is a *text relevance number*, and that number is handed to
//! `aikit_core::search::score`, which owns everything else. The test that matters
//! most below re-derives every row's score from core and asserts the palette's
//! order is exactly core's order.

use std::time::Duration;

use aikit_core::capsule::Capsule;
use aikit_core::catalog::MemoryCatalog;
use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{CapsuleId, ContextId, RegistrySource, Revision, SessionId};
use aikit_core::platform::{Platform, TargetId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::PoolPatch;
use aikit_core::resolve::{resolve, ResolveRequest, ResolvedView};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::search::{
    parse_query, score, RankingSignals, SearchDoc, UsageStats,
};
use aikit_core::trust::{MemoryTrust, TrustState};
use aikit_tui::search::{rank, Matcher};

fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

/// A real capsule, parsed from real manifest text and stamped as a registry
/// would stamp it.
fn script(id: &str, description: &str, exports: &[&str]) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    let rendered = exports
        .iter()
        .map(|e| format!("\"{e}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!(
        r#"schema = 1
id = "{id}"
kind = "script"
name = "{leaf}"
description = "{description}"

[script]
entry = "payload/run.sh"
exports = [{rendered}]
"#
    );
    let mut capsule = Capsule::from_toml_str(&src).expect("fixture manifest must parse");
    capsule.revision = Some(Revision::from_raw(format!("rev-{id}")));
    capsule.source = Some(RegistrySource::personal());
    capsule
}

fn skill(id: &str, description: &str) -> Capsule {
    let leaf = id.rsplit('/').next().unwrap();
    let src = format!(
        r#"schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "{description}"

[skill]
root = "payload"
"#
    );
    let mut capsule = Capsule::from_toml_str(&src).expect("fixture manifest must parse");
    capsule.revision = Some(Revision::from_raw(format!("rev-{id}")));
    capsule.source = Some(RegistrySource::personal());
    capsule
}

fn descriptor() -> ContextDescriptor {
    ContextDescriptor {
        context_id: ContextId::parse("ctx_TESTCONTEXT000000000000").unwrap(),
        session_id: Some(SessionId::parse("ses_TESTSESSION000000000000").unwrap()),
        project_id: None,
        project_root: Some("/work/payments".into()),
        task: None,
        isolation: Isolation::Shared,
        platform: Platform::Linux,
        targets: vec![TargetId::shell(), TargetId::claude_code()],
        mux: None,
        host: "test-host".into(),
    }
}

/// Resolve a real view over these capsules, enabling `enabled` at the project scope.
fn view_of(capsules: Vec<Capsule>, enabled: &[&str]) -> ResolvedView {
    let mut catalog = MemoryCatalog::default();
    let mut trust = MemoryTrust::default();
    for capsule in capsules {
        trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            TrustState::Reviewed,
        );
        catalog.insert(capsule);
    }
    let layers = vec![ScopeLayer {
        kind: ScopeKind::Project,
        depth: 0,
        origin: LayerOrigin::new("test:project"),
        patch: PoolPatch {
            enable: enabled.iter().map(|s| cid(s)).collect(),
            ..Default::default()
        },
    }];
    resolve(
        &catalog,
        &trust,
        &ResolveRequest {
            context: descriptor(),
            layers,
            policy: ManagedPolicy::default(),
        },
    )
    .expect("the fixture must resolve")
}

fn docs_of(view: &ResolvedView) -> Vec<SearchDoc> {
    view.catalog_index
        .keys()
        .filter_map(|id| SearchDoc::from_view(view, id, UsageStats::default()))
        .collect()
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

#[test]
fn an_empty_query_keeps_every_document_rather_than_showing_nothing() {
    let view = view_of(
        vec![
            script("script/test/cargo-nextest", "Runs the test suite.", &["nt"]),
            skill("skill/rust/review", "Reviews Rust code."),
        ],
        &["script/test/cargo-nextest"],
    );
    let rows = rank(&parse_query(""), &docs_of(&view));
    assert_eq!(rows.len(), 2);
}

#[test]
fn a_query_that_matches_nothing_produces_no_rows_rather_than_a_full_list() {
    let view = view_of(
        vec![script("script/test/cargo-nextest", "Runs tests.", &["nt"])],
        &[],
    );
    let rows = rank(&parse_query("zzzzzzqqqq"), &docs_of(&view));
    assert!(rows.is_empty(), "got {:?}", rows.iter().map(|r| r.doc.id.to_string()).collect::<Vec<_>>());
}

#[test]
fn fuzzy_matching_finds_a_command_from_its_initials() {
    let view = view_of(
        vec![
            script("script/test/cargo-nextest", "Runs the test suite.", &["nt"]),
            script("script/ops/deploy", "Deploys the service.", &["deploy"]),
        ],
        &[],
    );
    let rows = rank(&parse_query("crgnxt"), &docs_of(&view));
    assert_eq!(rows.first().map(|r| r.doc.id.to_string()), Some("script/test/cargo-nextest".to_string()));
}

#[test]
fn matching_is_case_insensitive_when_the_query_is_lowercase() {
    let view = view_of(
        vec![script("script/ops/deploy-all", "Ships The Whole Thing.", &["da"])],
        &[],
    );
    assert_eq!(rank(&parse_query("ships the"), &docs_of(&view)).len(), 1);
}

#[test]
fn a_text_score_is_normalised_into_the_unit_interval() {
    let view = view_of(
        vec![script("script/test/cargo-nextest", "Runs the test suite.", &["nt"])],
        &[],
    );
    let docs = docs_of(&view);
    let mut matcher = Matcher::default();
    for query in ["nt", "cargo", "cargo-nextest", "c", "test suite"] {
        let s = matcher
            .text_score(&parse_query(query), &docs[0])
            .unwrap_or_else(|| panic!("`{query}` should match the fixture"));
        assert!(
            (0.0..=1.0).contains(&s),
            "`{query}` produced a text score of {s}, outside [0, 1]"
        );
    }
}

#[test]
fn a_hit_in_a_command_name_outweighs_the_same_hit_buried_in_a_description() {
    // Core documents the field order as descending weight; this is the palette
    // honouring it. Without it, any capability whose blurb happens to mention
    // "deploy" would sit level with the command actually called `deploy`.
    let view = view_of(
        vec![
            script("script/a/deploy", "Unrelated words here.", &["deploy"]),
            script("script/b/other", "Deploy the payments service.", &["other"]),
        ],
        &[],
    );
    let docs = docs_of(&view);
    let mut matcher = Matcher::default();
    let query = parse_query("deploy");

    let by_command = docs.iter().find(|d| d.id == cid("script/a/deploy")).unwrap();
    let by_description = docs.iter().find(|d| d.id == cid("script/b/other")).unwrap();

    let command = matcher.text_score(&query, by_command).unwrap();
    let description = matcher.text_score(&query, by_description).unwrap();
    assert!(
        command > description,
        "a command hit ({command}) must outweigh a description hit ({description})"
    );
}

#[test]
fn a_document_the_query_cannot_reach_is_dropped_rather_than_scored_zero() {
    let view = view_of(
        vec![
            script("script/test/cargo-nextest", "Runs the test suite.", &["nt"]),
            script("script/ops/rollback", "Undoes a release.", &["rollback"]),
        ],
        &[],
    );
    let docs = docs_of(&view);
    let mut matcher = Matcher::default();
    let query = parse_query("nextest");
    let unreachable = docs.iter().find(|d| d.id == cid("script/ops/rollback")).unwrap();
    assert_eq!(matcher.text_score(&query, unreachable), None);
}

// ---------------------------------------------------------------------------
// The ranking policy lives in core
// ---------------------------------------------------------------------------

#[test]
fn every_rows_score_is_exactly_what_core_would_compute_for_it() {
    let view = view_of(
        vec![
            script("script/test/cargo-nextest", "Runs the test suite.", &["nt"]),
            script("script/ops/deploy", "Deploys the payments service.", &["deploy"]),
            script("script/ops/deploy-canary", "Deploys a canary.", &["deploy-canary"]),
            skill("skill/rust/review", "Reviews Rust code before deploy."),
        ],
        &["script/ops/deploy"],
    );
    let docs = docs_of(&view);
    let query = parse_query("deploy");
    let signals = RankingSignals::default();

    let rows = rank(&query, &docs);
    assert!(rows.len() >= 3);

    for row in &rows {
        let expected = score(&query, &row.doc, row.text_score, &signals);
        assert_eq!(
            row.score, expected,
            "{} was ranked by something other than core's policy",
            row.doc.id
        );
    }
    for pair in rows.windows(2) {
        assert!(
            pair[0].score >= pair[1].score,
            "{} ({}) sorted above {} ({})",
            pair[0].doc.id,
            pair[0].score,
            pair[1].doc.id,
            pair[1].score
        );
    }
}

#[test]
fn a_capability_already_active_in_this_context_is_lifted_by_cores_weighting() {
    // Same text, same name shape; only the resolved view differs.
    let capsules = vec![
        script("script/ops/deploy-alpha", "Deploys alpha.", &["deploy-alpha"]),
        script("script/ops/deploy-beta", "Deploys beta.", &["deploy-beta"]),
    ];
    let idle = view_of(capsules.clone(), &[]);
    let live = view_of(capsules, &["script/ops/deploy-beta"]);

    let query = parse_query("deploy-beta");
    let idle_score = rank(&query, &docs_of(&idle))
        .into_iter()
        .find(|r| r.doc.id == cid("script/ops/deploy-beta"))
        .unwrap()
        .score;
    let live_score = rank(&query, &docs_of(&live))
        .into_iter()
        .find(|r| r.doc.id == cid("script/ops/deploy-beta"))
        .unwrap()
        .score;
    assert!(live_score > idle_score);
}

#[test]
fn recent_successful_use_orders_near_ties_without_overturning_the_query() {
    let view = view_of(
        vec![
            script("script/ops/deploy-alpha", "Deploys alpha.", &["deploy-alpha"]),
            script("script/ops/deploy-beta", "Deploys beta.", &["deploy-beta"]),
        ],
        &[],
    );
    let mut docs = docs_of(&view);
    for doc in &mut docs {
        if doc.id == cid("script/ops/deploy-beta") {
            doc.usage = UsageStats {
                successful_runs: 20,
                failed_runs: 0,
                last_success_age: Some(Duration::from_secs(60)),
            };
        }
    }
    let rows = rank(&parse_query("deploy"), &docs);
    assert_eq!(rows[0].doc.id, cid("script/ops/deploy-beta"));

    // But typing the other one's name still wins.
    let rows = rank(&parse_query("deploy-alpha"), &docs);
    assert_eq!(rows[0].doc.id, cid("script/ops/deploy-alpha"));
}

#[test]
fn ties_are_broken_by_capsule_id_so_the_list_never_shuffles_between_keystrokes() {
    let view = view_of(
        vec![
            script("script/a/twin", "Identical.", &["twin-a"]),
            script("script/b/twin", "Identical.", &["twin-b"]),
        ],
        &[],
    );
    let docs = docs_of(&view);
    let first = rank(&parse_query("twin"), &docs);
    let again = rank(&parse_query("twin"), &docs);
    let ids: Vec<String> = first.iter().map(|r| r.doc.id.to_string()).collect();
    assert_eq!(ids, again.iter().map(|r| r.doc.id.to_string()).collect::<Vec<_>>());
    assert_eq!(ids, vec!["script/a/twin", "script/b/twin"]);
}

// ---------------------------------------------------------------------------
// Filters come from core, before matching
// ---------------------------------------------------------------------------

#[test]
fn a_kind_filter_removes_documents_before_they_are_ever_matched() {
    let view = view_of(
        vec![
            script("script/rust/review", "Reviews Rust code.", &["review"]),
            skill("skill/rust/review", "Reviews Rust code."),
        ],
        &[],
    );
    let rows = rank(&parse_query("kind:skill review"), &docs_of(&view));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].doc.id, cid("skill/rust/review"));
}

#[test]
fn the_run_lane_hides_a_capability_that_cannot_be_run() {
    let view = view_of(
        vec![
            script("script/rust/review", "Reviews Rust code.", &["review"]),
            skill("skill/rust/review", "Reviews Rust code."),
        ],
        &[],
    );
    let rows = rank(&parse_query(">review"), &docs_of(&view));
    assert_eq!(rows.len(), 1, "a skill is not something you run");
    assert_eq!(rows[0].doc.id, cid("script/rust/review"));
}

#[test]
fn a_status_filter_with_no_free_text_still_returns_the_matching_rows() {
    let view = view_of(
        vec![
            script("script/ops/deploy", "Deploys.", &["deploy"]),
            script("script/ops/rollback", "Rolls back.", &["rollback"]),
        ],
        &["script/ops/deploy"],
    );
    let rows = rank(&parse_query("status:active"), &docs_of(&view));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].doc.id, cid("script/ops/deploy"));
}

// ---------------------------------------------------------------------------
// Scale
// ---------------------------------------------------------------------------

#[test]
fn five_thousand_documents_still_produce_a_correctly_ordered_result() {
    let mut capsules: Vec<Capsule> = (0..5000)
        .map(|i| {
            script(
                &format!("script/bulk/item-{i:04}"),
                "A bulk fixture capability.",
                &[&format!("item-{i:04}")],
            )
        })
        .collect();
    capsules.push(script(
        "script/ops/deploy-payments",
        "Deploys the payments service.",
        &["deploy-payments"],
    ));

    let view = view_of(capsules, &["script/ops/deploy-payments"]);
    let docs = docs_of(&view);
    assert_eq!(docs.len(), 5001);

    let query = parse_query("deploy-payments");
    let rows = rank(&query, &docs);

    assert!(!rows.is_empty(), "5000 documents must not starve the query");
    assert_eq!(
        rows[0].doc.id,
        cid("script/ops/deploy-payments"),
        "the thing the user named must be first"
    );

    let signals = RankingSignals::default();
    for pair in rows.windows(2) {
        assert!(pair[0].score >= pair[1].score);
    }
    for row in rows.iter().take(50) {
        assert_eq!(row.score, score(&query, &row.doc, row.text_score, &signals));
    }
}

#[test]
fn an_empty_query_over_five_thousand_documents_returns_all_of_them_in_a_stable_order() {
    let capsules: Vec<Capsule> = (0..5000)
        .map(|i| {
            script(
                &format!("script/bulk/item-{i:04}"),
                "A bulk fixture capability.",
                &[&format!("item-{i:04}")],
            )
        })
        .collect();
    let view = view_of(capsules, &[]);
    let docs = docs_of(&view);

    let rows = rank(&parse_query(""), &docs);
    assert_eq!(rows.len(), 5000);
    let again = rank(&parse_query(""), &docs);
    assert_eq!(
        rows.iter().map(|r| r.doc.id.to_string()).collect::<Vec<_>>(),
        again.iter().map(|r| r.doc.id.to_string()).collect::<Vec<_>>()
    );
}
