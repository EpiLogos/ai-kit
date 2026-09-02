//! Search is where the palette's promise ("tap a key, find the thing, run it")
//! either holds or does not. The TUI owns fuzzy matching; this module owns the
//! *policy* — what a query means, and what beats what.
//!
//! The property that gets its own test is that usage never becomes destiny. A
//! command run four hundred times last quarter must not permanently sit above the
//! one the user ran ten minutes ago, or the palette slowly becomes a museum.

mod common;

use common::*;

use std::time::Duration;

use aikit_core::capsule::Kind;
use aikit_core::scope::ScopeKind;
use aikit_core::search::{
    parse_query, score, DocStatus, FastPrefix, RankingSignals, SearchDoc, StatusFilter, UsageStats,
};
use aikit_core::trust::TrustState;

const DAY: u64 = 60 * 60 * 24;

fn doc(id: &str) -> SearchDoc {
    SearchDoc {
        id: cid(id),
        kind: cid(id).kind(),
        name: id.rsplit('/').next().unwrap().to_string(),
        description: "A test capability.".into(),
        tags: vec![],
        exports: vec![id.rsplit('/').next().unwrap().to_string()],
        status: DocStatus::Inactive,
        scope: None,
        trust: TrustState::Reviewed,
        in_current_project: false,
        in_active_context: false,
        runnable: matches!(cid(id).kind(), Kind::Script | Kind::Tool | Kind::Template),
        usage: UsageStats::default(),
    }
}

fn used(successes: u32, age_days: u64) -> UsageStats {
    UsageStats {
        successful_runs: successes,
        failed_runs: 0,
        last_success_age: Some(Duration::from_secs(age_days * DAY)),
    }
}

// ---------------------------------------------------------------------------
// Query parsing
// ---------------------------------------------------------------------------

#[test]
fn a_query_splits_filters_from_free_text() {
    let q = parse_query("kind:script tag:rust cargo test");
    assert_eq!(q.kinds, vec![Kind::Script]);
    assert_eq!(q.tags, vec!["rust".to_string()]);
    assert_eq!(q.text, "cargo test");
    assert!(q.prefix.is_none());
}

#[test]
fn every_documented_filter_key_parses() {
    let q = parse_query("kind:hook scope:session status:unavailable trust:quarantined tag:ci");
    assert_eq!(q.kinds, vec![Kind::Hook]);
    assert_eq!(q.scopes, vec![ScopeKind::Session]);
    assert_eq!(q.status, Some(StatusFilter::Unavailable));
    assert_eq!(q.trust, vec![TrustState::Quarantined]);
    assert_eq!(q.tags, vec!["ci".to_string()]);
    assert!(q.text.is_empty());
}

#[test]
fn an_unknown_filter_key_is_free_text_rather_than_an_error() {
    // A palette that rejects what a user typed mid-keystroke is a palette people
    // stop using. `note:` is simply not a filter, so it is what they typed.
    let q = parse_query("note:remember cargo");
    assert_eq!(q.text, "note:remember cargo");
    assert!(q.kinds.is_empty());
}

#[test]
fn an_unknown_value_for_a_known_filter_key_is_free_text_too() {
    let q = parse_query("kind:banana bread");
    assert_eq!(q.text, "kind:banana bread");
    assert!(q.kinds.is_empty());
}

#[test]
fn repeating_a_key_widens_it_while_different_keys_narrow_together() {
    let q = parse_query("kind:script kind:tool tag:rust");
    assert_eq!(q.kinds, vec![Kind::Script, Kind::Tool]);

    let mut script = doc("script/test/cargo-nextest");
    script.tags = vec!["rust".into()];
    let mut tool = doc("tool/rust/cargo");
    tool.tags = vec!["rust".into()];
    let mut skill = doc("skill/rust/review");
    skill.tags = vec!["rust".into()];
    let untagged = doc("script/test/other");

    assert!(q.matches_filters(&script));
    assert!(q.matches_filters(&tool));
    assert!(!q.matches_filters(&skill), "kind must still narrow");
    assert!(!q.matches_filters(&untagged), "tag must still narrow");
}

#[test]
fn filters_are_case_insensitive_on_their_keys_and_values() {
    let q = parse_query("KIND:Script STATUS:Active");
    assert_eq!(q.kinds, vec![Kind::Script]);
    assert_eq!(q.status, Some(StatusFilter::Active));
}

// ---------------------------------------------------------------------------
// Fast prefixes
// ---------------------------------------------------------------------------

#[test]
fn each_fast_prefix_maps_to_its_documented_lane() {
    assert_eq!(parse_query(">cargo").prefix, Some(FastPrefix::Run));
    assert_eq!(parse_query("+rust").prefix, Some(FastPrefix::Capabilities));
    assert_eq!(parse_query("@payments").prefix, Some(FastPrefix::Sessions));
    assert_eq!(parse_query(":apply").prefix, Some(FastPrefix::Manage));
}

#[test]
fn a_fast_prefix_is_stripped_from_the_free_text() {
    let q = parse_query("> cargo nextest");
    assert_eq!(q.prefix, Some(FastPrefix::Run));
    assert_eq!(q.text, "cargo nextest");
}

#[test]
fn a_prefix_character_in_the_middle_of_a_query_is_just_text() {
    let q = parse_query("cargo > out.txt");
    assert!(q.prefix.is_none());
    assert_eq!(q.text, "cargo > out.txt");
}

#[test]
fn a_colon_filter_is_not_mistaken_for_the_management_prefix() {
    let q = parse_query("kind:script");
    assert!(q.prefix.is_none());
    assert_eq!(q.kinds, vec![Kind::Script]);
}

#[test]
fn the_run_prefix_only_matches_things_that_can_actually_be_run() {
    let q = parse_query(">cargo");
    assert!(q.matches_filters(&doc("script/test/cargo-nextest")));
    assert!(q.matches_filters(&doc("tool/rust/cargo")));
    assert!(
        !q.matches_filters(&doc("skill/rust/review")),
        "a skill is not something you run"
    );
}

#[test]
fn the_run_prefix_respects_a_capability_that_is_currently_unrunnable() {
    let mut blocked = doc("script/test/cargo-nextest");
    blocked.runnable = false;
    assert!(!parse_query(">cargo").matches_filters(&blocked));
}

#[test]
fn the_session_prefix_only_matches_session_capsules() {
    assert!(parse_query("@dev").matches_filters(&doc("session/work/dev")));
    assert!(!parse_query("@dev").matches_filters(&doc("script/test/dev")));
}

#[test]
fn the_capability_and_management_prefixes_do_not_narrow_the_capsule_list() {
    // They select a different palette source, which is the TUI's job. Core records
    // the intent without inventing a capsule filter for it.
    let capability = doc("skill/rust/review");
    assert!(parse_query("+review").matches_filters(&capability));
    assert!(parse_query(":review").matches_filters(&capability));
}

// ---------------------------------------------------------------------------
// Filters against documents
// ---------------------------------------------------------------------------

#[test]
fn a_status_filter_distinguishes_active_inactive_and_unavailable() {
    let mut active = doc("script/a/one");
    active.status = DocStatus::Active;
    let mut unavailable = doc("script/a/two");
    unavailable.status = DocStatus::Unavailable;
    let inactive = doc("script/a/three");

    assert!(parse_query("status:active").matches_filters(&active));
    assert!(!parse_query("status:active").matches_filters(&inactive));
    assert!(parse_query("status:unavailable").matches_filters(&unavailable));
    assert!(parse_query("status:inactive").matches_filters(&inactive));
}

#[test]
fn a_scope_filter_only_matches_a_capability_declared_at_that_scope() {
    let mut session = doc("script/a/one");
    session.scope = Some(ScopeKind::Session);
    let undeclared = doc("script/a/two");

    assert!(parse_query("scope:session").matches_filters(&session));
    assert!(!parse_query("scope:project").matches_filters(&session));
    assert!(!parse_query("scope:session").matches_filters(&undeclared));
}

#[test]
fn free_text_is_not_a_filter_and_never_excludes_a_document() {
    // Text relevance is nucleo's job. If this module also filtered on text, a
    // fuzzy match would be silently overruled by a substring check.
    let q = parse_query("something entirely unrelated");
    assert!(q.matches_filters(&doc("script/test/cargo-nextest")));
}

// ---------------------------------------------------------------------------
// Building documents from a resolved view
// ---------------------------------------------------------------------------

#[test]
fn a_search_doc_is_built_from_a_real_resolved_view() {
    let f = Fixture::new(vec![
        script_exporting("script/test/cargo-nextest", &["cargo-nextest", "nt"]),
        skill("skill/rust/review"),
    ])
    .with_layers(vec![layer(
        ScopeKind::Project,
        &["script/test/cargo-nextest"],
        &[],
    )]);
    let view = f.resolve().unwrap();

    let active = SearchDoc::from_view(
        &view,
        &cid("script/test/cargo-nextest"),
        UsageStats::default(),
    )
    .expect("a catalogued capsule must produce a document");
    assert_eq!(active.status, DocStatus::Active);
    assert_eq!(active.scope, Some(ScopeKind::Project));
    assert!(active.in_current_project);
    assert!(active.in_active_context);
    assert!(active.runnable);
    assert_eq!(active.exports, vec!["cargo-nextest", "nt"]);

    let idle =
        SearchDoc::from_view(&view, &cid("skill/rust/review"), UsageStats::default()).unwrap();
    assert_eq!(idle.status, DocStatus::Inactive);
    assert!(!idle.in_active_context);
    assert!(!idle.runnable, "a skill is never runnable, active or not");
    assert_eq!(idle.scope, None);
}

#[test]
fn a_capability_held_back_by_trust_is_a_document_with_unavailable_status() {
    let f = Fixture::new(vec![hook("hook/gate/boundary")])
        .with_layers(vec![layer(
            ScopeKind::Project,
            &["hook/gate/boundary"],
            &[],
        )])
        .untrust("hook/gate/boundary");
    let view = f.resolve().unwrap();

    let doc =
        SearchDoc::from_view(&view, &cid("hook/gate/boundary"), UsageStats::default()).unwrap();
    assert_eq!(doc.status, DocStatus::Unavailable);
    assert!(parse_query("status:unavailable").matches_filters(&doc));
}

#[test]
fn a_capsule_absent_from_the_catalog_produces_no_document() {
    let f = Fixture::new(vec![script("script/a/one")]);
    let view = f.resolve().unwrap();
    assert!(SearchDoc::from_view(&view, &cid("script/a/ghost"), UsageStats::default()).is_none());
}

// ---------------------------------------------------------------------------
// Ranking policy
// ---------------------------------------------------------------------------

#[test]
fn an_old_frequently_used_action_does_not_permanently_outrank_a_recent_one() {
    let signals = RankingSignals::default();
    let q = parse_query("deploy");

    let mut veteran = doc("script/ops/deploy-legacy");
    veteran.usage = used(400, 90);
    let mut newcomer = doc("script/ops/deploy-new");
    newcomer.usage = used(2, 0);

    // Identical text relevance: only the usage signal can separate them.
    let veteran_score = score(&q, &veteran, 0.5, &signals);
    let newcomer_score = score(&q, &newcomer, 0.5, &signals);

    assert!(
        newcomer_score > veteran_score,
        "a 90-day-old habit ({veteran_score}) must not outrank today's ({newcomer_score})"
    );
}

#[test]
fn with_equal_recency_more_successful_runs_still_ranks_higher() {
    let signals = RankingSignals::default();
    let q = parse_query("deploy");

    let mut often = doc("script/ops/a");
    often.usage = used(50, 1);
    let mut rarely = doc("script/ops/b");
    rarely.usage = used(1, 1);

    assert!(score(&q, &often, 0.5, &signals) > score(&q, &rarely, 0.5, &signals));
}

#[test]
fn the_usage_boost_halves_at_the_documented_half_life() {
    let signals = RankingSignals::default();
    let half_life_days = signals.usage_half_life.as_secs() / DAY;

    let fresh = signals.usage_boost(&used(10, 0));
    let aged = signals.usage_boost(&used(10, half_life_days));

    assert!(
        (aged / fresh - 0.5).abs() < 0.01,
        "expected a halving at {half_life_days} days, got {fresh} -> {aged}"
    );
}

#[test]
fn a_never_used_capability_receives_no_usage_boost_at_all() {
    let signals = RankingSignals::default();
    assert_eq!(signals.usage_boost(&UsageStats::default()), 0.0);

    // Same query, same text relevance, same name shape: only usage differs.
    let q = parse_query("deploy");
    let unused = doc("script/ops/deploy-a");
    let mut used_once = doc("script/ops/deploy-b");
    used_once.usage = used(1, 0);
    assert!(score(&q, &used_once, 0.5, &signals) > score(&q, &unused, 0.5, &signals));
}

#[test]
fn a_failed_run_does_not_earn_the_boost_a_successful_one_does() {
    let signals = RankingSignals::default();
    let failing = UsageStats {
        failed_runs: 100,
        ..Default::default()
    };
    assert_eq!(signals.usage_boost(&failing), 0.0);
}

#[test]
fn no_amount_of_usage_can_outweigh_a_direct_text_match() {
    let signals = RankingSignals::default();
    let q = parse_query("review");

    let mut habit = doc("script/ops/deploy");
    habit.usage = used(100_000, 0);
    habit.in_current_project = true;
    habit.in_active_context = true;

    let wanted = doc("skill/rust/review");

    assert!(
        score(&q, &wanted, 1.0, &signals) > score(&q, &habit, 0.0, &signals),
        "what the user typed has to win"
    );
}

#[test]
fn an_exact_command_match_beats_a_merely_similar_name() {
    let signals = RankingSignals::default();
    let q = parse_query("nt");

    let exact = doc("script/test/cargo-nextest");
    let mut exact = exact;
    exact.exports = vec!["nt".into()];
    let similar = doc("script/test/nightly");

    assert!(score(&q, &exact, 0.8, &signals) > score(&q, &similar, 0.8, &signals));
}

#[test]
fn project_and_context_relevance_both_lift_a_capability() {
    let signals = RankingSignals::default();
    let q = parse_query("review");
    let plain = doc("skill/rust/review");

    let mut project = plain.clone();
    project.in_current_project = true;
    let mut context = plain.clone();
    context.in_active_context = true;

    let base = score(&q, &plain, 0.5, &signals);
    assert!(score(&q, &project, 0.5, &signals) > base);
    assert!(score(&q, &context, 0.5, &signals) > base);
}

#[test]
fn scoring_is_deterministic_for_the_same_inputs() {
    let signals = RankingSignals::default();
    let q = parse_query("cargo");
    let mut d = doc("script/test/cargo-nextest");
    d.usage = used(7, 3);
    assert_eq!(score(&q, &d, 0.42, &signals), score(&q, &d, 0.42, &signals));
}
