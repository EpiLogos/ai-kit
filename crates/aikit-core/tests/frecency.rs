//! Frecency and `z` (SPEC-III §3).
//!
//! Two properties carry the section: **score is match quality alone** (usage only
//! ever breaks a tie), and **`z` never activates anything**.

mod common;
use common::*;

use std::time::Duration;

use aikit_core::frecency::{self, Candidate, Jump, Tiebreak, DEFAULT_HALF_LIFE};
use aikit_core::search::UsageStats;

fn used(runs: u32, days_ago: u64) -> UsageStats {
    UsageStats {
        successful_runs: runs,
        failed_runs: 0,
        last_success_age: Some(Duration::from_secs(days_ago * 24 * 60 * 60)),
    }
}

#[test]
fn usage_never_outranks_a_better_match() {
    // The correction the section exists for. A heavily-used capsule with a weaker
    // match must not beat a direct hit, because a blended number would be unstable
    // between keystrokes and unexplainable when asked.
    let mut worse_match_heavily_used = Candidate::new(cid("script/test/nextest-helper"), 0.7);
    worse_match_heavily_used.usage = used(500, 0);

    let exact = Candidate::new(cid("script/test/nextest"), 1.0);

    let mut candidates = vec![worse_match_heavily_used, exact];
    frecency::rank(&mut candidates, DEFAULT_HALF_LIFE);

    assert_eq!(
        candidates[0].id,
        cid("script/test/nextest"),
        "match quality decides; 500 successful runs cannot buy a better rank"
    );
}

#[test]
fn usage_breaks_a_tie_between_equally_good_matches() {
    let mut stale = Candidate::new(cid("script/test/a"), 0.9);
    stale.usage = used(3, 400);
    let mut fresh = Candidate::new(cid("script/test/b"), 0.9);
    fresh.usage = used(3, 1);

    let mut candidates = vec![stale, fresh];
    frecency::rank(&mut candidates, DEFAULT_HALF_LIFE);

    assert_eq!(
        candidates[0].id,
        cid("script/test/b"),
        "same match quality, so recency-decayed usage decides"
    );
}

#[test]
fn scope_beats_globality() {
    // A match in the current project outranks a more frecent match from elsewhere:
    // `CurrentProject` sits ABOVE `Frecency` on the ladder, rather than being a
    // term added to it.
    let mut elsewhere_frecent = Candidate::new(cid("script/test/a"), 0.9);
    elsewhere_frecent.usage = used(100, 0);

    let mut here_unused = Candidate::new(cid("script/test/b"), 0.9);
    here_unused.in_current_project = true;

    let mut candidates = vec![elsewhere_frecent, here_unused];
    frecency::rank(&mut candidates, DEFAULT_HALF_LIFE);

    assert_eq!(candidates[0].id, cid("script/test/b"));
    assert_eq!(
        candidates[0].deciding_tiebreak(&candidates[1], DEFAULT_HALF_LIFE),
        Some(Tiebreak::CurrentProject),
        "and the UI can say which rung decided it"
    );
}

#[test]
fn the_order_is_total_so_results_never_jitter() {
    // Without a total order at the bottom, equally-scored equally-used candidates
    // swap places between keystrokes, which reads as a broken UI.
    let make = || {
        vec![
            Candidate::new(cid("script/test/c"), 0.5),
            Candidate::new(cid("script/test/a"), 0.5),
            Candidate::new(cid("script/test/b"), 0.5),
        ]
    };
    let mut first = make();
    let mut second = make();
    second.reverse();

    frecency::rank(&mut first, DEFAULT_HALF_LIFE);
    frecency::rank(&mut second, DEFAULT_HALF_LIFE);

    let ids = |c: &[Candidate]| c.iter().map(|x| x.id.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ids(&first),
        ids(&second),
        "input order must not affect output"
    );
    assert_eq!(
        ids(&first),
        vec!["script/test/a", "script/test/b", "script/test/c"],
        "and the total order is the capsule id"
    );
}

#[test]
fn matching_prefers_the_tail_of_the_id() {
    // `nextest` beats a capsule whose *group* is nextest, the way `z docs` prefers
    // a directory named docs over one merely containing it.
    let leaf = frecency::match_quality("nextest", &cid("script/test/nextest"));
    let group = frecency::match_quality("nextest", &cid("script/nextest/runner"));
    assert!(
        leaf > group,
        "an exact leaf match ({leaf}) must beat a group match ({group})"
    );

    // Matching the END of the leaf is tighter than matching its middle: typing
    // `nextest` means `cargo-nextest`, not `cargo-nextest-helper`. Without this,
    // `z nextest` cannot decide between them and has to ask.
    let suffix = frecency::match_quality("nextest", &cid("script/test/cargo-nextest"));
    let middle = frecency::match_quality("nextest", &cid("script/test/cargo-nextest-helper"));
    assert!(
        suffix > middle,
        "a tail match ({suffix}) must beat a mid-leaf match ({middle})"
    );

    assert_eq!(
        frecency::match_quality("nextest", &cid("script/test/nextest")),
        1.0
    );
    assert!(frecency::match_quality("next", &cid("script/test/nextest")) > 0.0);
    assert_eq!(
        frecency::match_quality("nothing-like-it", &cid("script/test/nextest")),
        0.0
    );
}

#[test]
fn z_acts_when_there_is_one_clear_winner() {
    let mut candidates = vec![
        Candidate::new(cid("script/test/nextest"), 1.0),
        Candidate::new(cid("script/test/nextest-helper"), 0.6),
    ];
    frecency::rank(&mut candidates, DEFAULT_HALF_LIFE);

    assert_eq!(
        frecency::decide(&candidates),
        Jump::Act {
            capsule: cid("script/test/nextest")
        }
    );
}

#[test]
fn z_disambiguates_rather_than_guessing_when_the_top_is_contested() {
    // Ambiguity is never an error message — it is the interactive case, one
    // keystroke from resolved. Running one of two equally-good matches on a coin
    // toss would be the worst possible answer.
    let mut candidates = vec![
        Candidate::new(cid("script/test/alpha"), 0.7),
        Candidate::new(cid("script/test/beta"), 0.7),
    ];
    frecency::rank(&mut candidates, DEFAULT_HALF_LIFE);

    match frecency::decide(&candidates) {
        Jump::Disambiguate { candidates } => {
            assert_eq!(
                candidates.len(),
                2,
                "the palette opens pre-filtered to both"
            );
        }
        other => panic!("a contested top must not be acted on: {other:?}"),
    }
}

#[test]
fn z_finding_nothing_says_nothing_rather_than_running_something() {
    let candidates = vec![Candidate::new(cid("script/test/a"), 0.0)];
    assert_eq!(frecency::decide(&candidates), Jump::Nothing);
    assert_eq!(frecency::decide(&[]), Jump::Nothing);
}

#[test]
fn frecency_counts_successes_not_invocations() {
    // A script you run and abort five times a day must not become your top match.
    let mut aborted = Candidate::new(cid("script/test/a"), 0.9);
    aborted.usage = UsageStats {
        successful_runs: 0,
        failed_runs: 50,
        last_success_age: None,
    };
    assert_eq!(
        aborted.frecency(DEFAULT_HALF_LIFE),
        0.0,
        "fifty failures earn no rank at all"
    );

    let mut succeeded = Candidate::new(cid("script/test/b"), 0.9);
    succeeded.usage = used(1, 0);
    assert!(succeeded.frecency(DEFAULT_HALF_LIFE) > 0.0);
}
