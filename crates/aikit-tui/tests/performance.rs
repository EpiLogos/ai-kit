//! Release-build responsiveness gates for the real popup controller.
//!
//! Discovery and fixture construction happen outside the clock. The first-frame
//! samples cover controller creation, initial ranking, tree construction, and a
//! real Ratatui draw. Search samples exercise the production matcher over 5,000
//! documents while reusing its scratch storage exactly as the popup does.

mod common;

use std::time::{Duration, Instant};

use aikit_core::capsule::Kind;
use aikit_core::scope::ScopeKind;
use aikit_core::search::{parse_query, DocStatus, SearchDoc, UsageStats};
use aikit_core::trust::TrustState;
use aikit_core::Result;
use aikit_tui::host::UiHost;
use aikit_tui::search::Matcher;
use aikit_tui::surface::{SurfaceBackend, SurfaceController, SurfaceRequest};
use aikit_tui::tree::{Node, NodeKind, Root, TreeEffect, TreeState};
use aikit_tui::PaletteBackend;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use common::{cid, script, Fixture};

const COLD_BUDGET: Duration = Duration::from_millis(150);
const WARM_BUDGET: Duration = Duration::from_millis(60);
const SEARCH_BUDGET: Duration = Duration::from_millis(16);

impl SurfaceBackend for Fixture {
    fn surface_tree(&self) -> Result<TreeState> {
        let children = self
            .view()
            .catalog_index
            .keys()
            .map(|id| {
                Node::leaf(
                    NodeKind::Capability { id: id.clone() },
                    id.leaf().to_string(),
                )
            })
            .collect();
        Ok(TreeState::new(vec![Node::branch(
            NodeKind::Root(Root::Kinds),
            "performance catalog",
            children,
        )]))
    }

    fn apply_tree_effect(&mut self, _effect: TreeEffect) -> Result<()> {
        Ok(())
    }
}

fn percentile(samples: &[Duration], numerator: usize, denominator: usize) -> Duration {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let index = (ordered.len() * numerator).div_ceil(denominator) - 1;
    ordered[index]
}

fn report(name: &str, samples: &[Duration]) {
    eprintln!(
        "{name}: samples={}, p50={:?}, p95={:?}, worst={:?}",
        samples.len(),
        percentile(samples, 50, 100),
        percentile(samples, 95, 100),
        samples.iter().max().unwrap()
    );
}

fn release_only() -> bool {
    if cfg!(debug_assertions) {
        eprintln!("performance budgets are enforced by the release-build gate");
        false
    } else {
        true
    }
}

#[test]
fn popup_first_frame_meets_cold_and_warm_budgets() {
    if !release_only() {
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let capsules = (0..250)
        .map(|index| script(&format!("script/performance/tool-{index:04}")))
        .collect();
    let mut backend = Fixture::new(directory.path(), capsules);
    let request = SurfaceRequest::new(UiHost::TmuxPopup).with_query("tool");

    let cold_started = Instant::now();
    let mut surface = SurfaceController::new(&mut backend, request.clone()).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    let cold = cold_started.elapsed();

    let mut warm = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
        surface.draw_terminal(&mut terminal).unwrap();
        warm.push(started.elapsed());
    }
    report("warm popup first frame", &warm);
    eprintln!("cold popup first frame: {cold:?}");

    assert!(
        cold < COLD_BUDGET,
        "cold popup first frame took {cold:?}; budget is {COLD_BUDGET:?}"
    );
    let warm_p95 = percentile(&warm, 95, 100);
    assert!(
        warm_p95 < WARM_BUDGET,
        "warm popup p95 took {warm_p95:?}; budget is {WARM_BUDGET:?}"
    );
}

#[test]
fn five_thousand_document_search_meets_the_keystroke_budget() {
    if !release_only() {
        return;
    }

    let docs: Vec<SearchDoc> = (0..5_000)
        .map(|index| SearchDoc {
            id: cid(&format!("script/performance/tool-{index:04}")),
            kind: Kind::Script,
            name: format!("tool-{index:04}"),
            description: format!("Operate production service shard {index:04} safely"),
            tags: vec!["performance".into(), format!("shard-{}", index % 100)],
            exports: vec![format!("operate-{index:04}")],
            status: DocStatus::Inactive,
            scope: Some(ScopeKind::Project),
            trust: TrustState::Reviewed,
            in_current_project: true,
            in_active_context: false,
            runnable: true,
            usage: UsageStats::default(),
        })
        .collect();
    let queries = [
        "operate-0042",
        "production 281",
        "tool-49",
        "shard-17",
        "service shard 8",
    ];
    let mut matcher = Matcher::new();
    let mut samples = Vec::with_capacity(50);

    for query in queries.into_iter().cycle().take(50) {
        let parsed = parse_query(query);
        let started = Instant::now();
        let rows = matcher.rank(&parsed, &docs);
        samples.push(started.elapsed());
        assert!(
            !rows.is_empty(),
            "performance query `{query}` matched nothing"
        );
    }
    report("5,000-document search step", &samples);

    let search_p95 = percentile(&samples, 95, 100);
    assert!(
        search_p95 < SEARCH_BUDGET,
        "search p95 took {search_p95:?}; budget is {SEARCH_BUDGET:?}"
    );
}
