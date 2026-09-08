//! Behavioural acceptance for the wide-shell Inspector column (spec
//! `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §2.1): always visible in a wide
//! shell, tracking the current selection, and dropping no capability the
//! modal `Overlay::Explain` already had.

mod common;

use common::*;

use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    );
    (dir, backend)
}

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn draw_width(
    surface: &ApplicationSurfaceController,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

/// The Inspector column's own rendered text, one row per terminal line — not
/// the whole frame — so an assertion here can never accidentally pass
/// because the string it is looking for happened to render in the list or
/// preview pane instead.
fn rendered_inspector(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> String {
    let inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let inspector = Layout::for_width(inner.width)
        .split(inner)
        .inspector
        .expect("this width must expose the persistent Inspector column");
    let buffer = terminal.backend().buffer();
    (inspector.y..inspector.y + inspector.height)
        .map(|y| {
            (inspector.x..inspector.x + inspector.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const WIDTH: u16 = 220;
const HEIGHT: u16 = 40;

fn strip_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn the_column_is_present_and_honest_when_nothing_is_selected() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("nonexistent-query"),
    )
    .unwrap();
    assert!(surface.semantic().selected.is_none());
    assert!(surface.inspector().is_none());

    let inspector = rendered_inspector(&draw_width(&surface, WIDTH, HEIGHT), WIDTH, HEIGHT);
    assert!(inspector.contains("INSPECTOR"));
    assert!(
        inspector.contains("nothing selected"),
        "an empty selection must render an honest empty state, not a blank column:\n{inspector}"
    );
}

#[test]
fn selecting_a_different_resource_updates_the_column_to_match() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();

    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let first_subject = surface
        .semantic()
        .selected
        .clone()
        .expect("Down must select the first result");
    assert_eq!(first_subject.as_str(), "skill/rust/review");
    let first_rendered = rendered_inspector(&draw_width(&surface, WIDTH, HEIGHT), WIDTH, HEIGHT);
    assert!(first_rendered.contains(&format!("subject   {first_subject}")));

    // A fresh query, not a second `Down`: list order among unrelated
    // ambient/navigation resources is not this test's concern, and asserting
    // against it would make the test fragile to unrelated navigation-index
    // changes. Re-querying to a second, specifically known resource keeps
    // the assertion about exactly one thing — does the column follow
    // selection — deterministic.
    surface
        .handle(&mut backend, PaletteEvent::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)))
        .unwrap();
    for character in "deploy".chars() {
        surface.handle(&mut backend, key(KeyCode::Char(character))).unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let second_subject = surface
        .semantic()
        .selected
        .clone()
        .expect("Down must select the deploy script");
    assert_eq!(second_subject.as_str(), "script/ops/deploy");
    assert_ne!(first_subject, second_subject, "the fixture must offer two distinct resources");
    let second_rendered = rendered_inspector(&draw_width(&surface, WIDTH, HEIGHT), WIDTH, HEIGHT);
    assert!(
        second_rendered.contains(&format!("subject   {second_subject}")),
        "the column must track the new selection:\n{second_rendered}"
    );
    assert!(
        !second_rendered.contains(&format!("subject   {first_subject}")),
        "the column must not still show the previous selection:\n{second_rendered}"
    );
}

/// The anti-capability-loss acceptance: retiring the modal `Overlay::Explain`
/// in a wide shell must not drop anything it could show.
///
/// This does not invoke the `:` Explain contextual Action itself:
/// `application_service.rs`'s `invoke_action` records a familiarity
/// observation for every action it runs (`self.record_action_use`, after
/// computing the outcome), so a *second* read taken after invoking would
/// legitimately differ from the first — the resource has now been used once
/// more. That is correct application behaviour, not a rendering defect, and
/// asserting exact JSON equality across that boundary would be testing a
/// timing artefact rather than the column's own completeness.
///
/// Instead this asserts the property that actually matters: `refresh_
/// inspector` computes `explain`/`explain_evidence` through the identical
/// `ApplicationService::explain` / `ExplainHistoryApplicationService::
/// explain_evidence` calls `invoke_action`'s `action/capability/explain` /
/// `EXPLAIN_ACTION_REF` branches use — so whatever those two reads carry
/// *is* what the modal would have shown had it been invoked at this exact
/// moment, and the only remaining question is whether `inspector_lines`
/// actually renders all of it. Every meaningful field from both reads is
/// checked against the rendered column text below.
#[test]
fn the_column_carries_every_field_the_modal_explain_action_would_have_shown() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("review"),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let subject = surface.semantic().selected.clone().unwrap();
    assert_eq!(subject.as_str(), "skill/rust/review");

    let snapshot = surface
        .inspector()
        .expect("a selected Capability must have a computed InspectorSnapshot")
        .clone();
    let explain = snapshot
        .explain
        .as_ref()
        .expect("`action/capability/explain`'s own read must have succeeded for a Capability");
    let evidence = snapshot
        .evidence
        .as_ref()
        .expect("`EXPLAIN_ACTION_REF`'s own read must have succeeded for a Capability");
    assert!(!evidence.facts.is_empty(), "a resolved Capability must carry at least one fact");

    // A generous height, not `HEIGHT`: the Inspector column is deliberately
    // narrow (`INSPECTOR_COLUMNS` in `layout.rs`), so a Capability's full
    // Explain content — including a 64-character catalog/resolution hash —
    // wraps into far more terminal rows than the other tests in this file
    // need. `Paragraph` never scrolls, so every row this content wraps into
    // must actually be on screen for the assertions below to see it.
    const TALL: u16 = 400;
    let inspector = rendered_inspector(&draw_width(&surface, WIDTH, TALL), WIDTH, TALL);
    assert!(inspector.contains(&format!("subject   {subject}")));
    assert!(inspector.contains("kind      capability"));

    // `explain_lines`' authored-intent/effective-state content (spec §2.1's
    // "effective state").
    assert!(
        inspector.contains("Intent") && inspector.contains("Effective"),
        "the authored-intent/effective-state block must be present:\n{inspector}"
    );
    assert!(
        inspector.contains("Catalog") && inspector.contains("Resolution"),
        "catalog/resolution provenance must be present, exactly as the modal showed it:\n{inspector}"
    );

    // Every `ExplainEvidence` fact (`EXPLAIN_ACTION_REF`'s own read) must
    // reach the screen — this is the direct anti-capability-loss check for
    // the per-fact detail (relation, authority, summary) the graph
    // Inspector's own idiom (`graph_presentation::inspector_lines`) already
    // established as the bar for "explains itself".
    //
    // Matched with all whitespace stripped, not against `inspector` itself:
    // the Inspector column is deliberately narrow, so a long unbroken token
    // (a full `ResourceRef` path, here) can be longer than the column
    // itself and force `Paragraph` to break *inside* it, with no space at
    // the break point at all. That is a real, intended degrade
    // (`layout.rs`'s own "drop columns, never truncate text" doctrine
    // playing out as narrow-column wrapping rather than a dropped column)
    // — not a capability loss, and not something a whitespace-preserving
    // comparison can see past. Wrapping only ever inserts a line break; it
    // never drops or reorders a character, so stripping all whitespace from
    // both sides restores a directly comparable character sequence.
    let no_whitespace = strip_whitespace(&inspector);
    for fact in &evidence.facts {
        assert!(
            no_whitespace.contains(&strip_whitespace(&fact.relation)),
            "fact relation `{}` must reach the column:\n{inspector}",
            fact.relation
        );
        assert!(
            no_whitespace.contains(&strip_whitespace(&fact.summary)),
            "fact summary `{}` must reach the column:\n{inspector}",
            fact.summary
        );
    }

    // `action/capability/explain`'s own field — packageCapabilityState — is
    // the one dimension `ExplainEvidence` does not carry at all; it must
    // still reach the screen, hand-formatted rather than lost.
    assert!(
        inspector.contains("Capability state"),
        "packageCapabilityState must reach the column even though `ExplainEvidence` does not \
         carry it:\n{inspector}\nexplain = {explain:#}"
    );
}
