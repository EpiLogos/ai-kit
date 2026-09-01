//! Release-build responsiveness gates for the final V2 application surface.
//!
//! Fixture construction happens outside the clock. First-frame samples cover
//! ApplicationSurface creation, ResourceRef-native ranking, Project-world
//! composition and a real Ratatui draw. Search samples exercise the canonical
//! ApplicationService over 5,000 resources.

mod common;

use std::time::{Duration, Instant};

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::host::UiHost;
use aikit_tui::{ApplicationService, TuiApplicationService};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use common::{script, Fixture};

const COLD_BUDGET: Duration = Duration::from_millis(150);
const WARM_BUDGET: Duration = Duration::from_millis(60);
const SEARCH_BUDGET: Duration = Duration::from_millis(16);

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
fn application_first_frame_meets_cold_and_warm_budgets() {
    if !release_only() {
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let capsules = (0..250)
        .map(|index| script(&format!("script/performance/tool-{index:04}")))
        .collect();
    let mut backend = Fixture::new(directory.path(), capsules);
    let request = ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("tool");

    let cold_started = Instant::now();
    let surface = ApplicationSurfaceController::new(&mut backend, request).unwrap();
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
    report("warm application first frame", &warm);
    eprintln!("cold application first frame: {cold:?}");

    assert!(
        cold < COLD_BUDGET,
        "cold application first frame took {cold:?}; budget is {COLD_BUDGET:?}"
    );
    let warm_p95 = percentile(&warm, 95, 100);
    assert!(
        warm_p95 < WARM_BUDGET,
        "warm application p95 took {warm_p95:?}; budget is {WARM_BUDGET:?}"
    );
}

#[test]
fn five_thousand_resource_search_meets_the_keystroke_budget() {
    if !release_only() {
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let capsules = (0..5_000)
        .map(|index| script(&format!("script/performance/tool-{index:04}")))
        .collect();
    let mut backend = Fixture::new(directory.path(), capsules);
    let service = ApplicationService::new(&mut backend);
    let queries = [
        "tool-0042",
        "tool-0281",
        "tool-49",
        "performance/tool-17",
        "tool-0008",
    ];
    let mut samples = Vec::with_capacity(50);

    for query in queries.into_iter().cycle().take(50) {
        let started = Instant::now();
        let rows = service.search(query).unwrap();
        samples.push(started.elapsed());
        assert!(
            !rows.resources.is_empty(),
            "performance query `{query}` matched nothing"
        );
    }
    report("5,000-resource application search step", &samples);

    let search_p95 = percentile(&samples, 95, 100);
    assert!(
        search_p95 < SEARCH_BUDGET,
        "search p95 took {search_p95:?}; budget is {SEARCH_BUDGET:?}"
    );
}
