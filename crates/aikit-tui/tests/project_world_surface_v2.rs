mod common;

use common::*;

use aikit_tui::application::Overlay;
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::event::PaletteEvent;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Glyphs;
use aikit_tui::project_workspace_render::workspace_section_label;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
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

fn alt(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn ctrl(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
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

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn final_surface_uses_the_shared_project_world_read_model() {
    let (_dir, mut backend) = fixture();
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            // Pinned, not read from the process locale: these assertions
            // name the Unicode separator, and `nextest`'s parallel execution
            // rules out setting `LANG` from a test.
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();

    let world = surface
        .project_world()
        .expect("a Project-bound surface must disclose its Project world");
    assert_eq!(world.project.project.as_str(), "project:payments");
    assert!(world
        .capability_horizon
        .capabilities
        .iter()
        .any(|resource| resource.resource.as_str() == "skill/rust/review"));
    assert!(world.resolution_basis.scopes.is_empty());
    assert!(world
        .warnings
        .iter()
        .any(|warning| warning.contains("scope-layer stack")));
}

#[test]
fn global_surface_remains_first_class_without_inventing_project_world() {
    let dir = tempfile::tempdir().unwrap();
    let mut global = descriptor();
    global.project_root = None;
    global.project_id = None;
    let mut backend = Fixture::new(
        dir.path(),
        vec![script("script/ops/deploy"), skill("skill/rust/review")],
    )
    .with_descriptor(global);
    let surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            // Pinned, not read from the process locale: these assertions
            // name the Unicode separator, and `nextest`'s parallel execution
            // rules out setting `LANG` from a test.
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();

    assert!(surface.project_world().is_none());
    let output = rendered(&draw_width(&surface, 120, 24));
    assert!(output.contains("AIKit · Workspace"));
    assert!(!output.contains("resolved Project world"));
}

/// The Workspace panes under the ASCII set: the same headings, the same
/// resolved values, the same fields — separator swapped and nothing else.
/// These lines are built by `project_workspace_render`, which formatted its
/// own `·` as a literal until the glyph capability was threaded into it, so
/// a terminal that could not draw one got a pane of replacement boxes with
/// the resolution facts still in it.
#[test]
fn the_ascii_workspace_panes_carry_the_same_resolved_world() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            .with_glyphs(Glyphs::ascii()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    let context = rendered(&draw_width(&surface, 220, 30));
    assert!(context.contains("Context - resolved Project world"));
    assert!(context.contains("Project  project:payments"));
    assert!(context.contains("Scopes   not exposed by application boundary"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    let compose = rendered(&draw_width(&surface, 220, 30));
    assert!(compose.contains("Compose - intention to operative world"));
    assert!(compose.contains("Intent        eligibility unresolved"));
    assert!(compose.contains("Effective     available - 0 providers"));
    assert!(
        compose.is_ascii(),
        "an ASCII Workspace pane still emitted non-ASCII"
    );
}

#[test]
// 220 columns rather than 140: the wide-shell Inspector column (spec §2.1)
// now carves a persistent share out of the preview pane's own budget
// (`Layout::split`, `crates/aikit-tui/src/layout.rs`), never out of the list
// pane. At 140 columns several of the single-line assertions below (e.g.
// "Scopes   not exposed by application boundary") would wrap once the
// preview pane gives up part of its width to Inspector; 220 keeps the
// preview pane exactly as roomy as it needs to be for every string this test
// asserts as one contiguous line, so the assertions below are unchanged.
fn wide_workspace_renders_context_compose_and_explain_from_one_world() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            // Pinned, not read from the process locale: these assertions
            // name the Unicode separator, and `nextest`'s parallel execution
            // rules out setting `LANG` from a test.
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    let context = rendered(&draw_width(&surface, 220, 30));
    assert!(context.contains("Context · resolved Project world"));
    assert!(context.contains("Project  project:payments"));
    assert!(context.contains("Scopes   not exposed by application boundary"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Compose");
    let compose = rendered(&draw_width(&surface, 220, 30));
    assert!(compose.contains("Compose · intention to operative world"));
    // §5.1's spine, not the four read-model horizon counts it replaced: a
    // person reads their own progress off the steps, and each step carries
    // strictly more than the count row it retired.
    for step in [
        "Intention", "Identity", "Governance", "Praxis", "Information",
        "Worlds/bounds", "Runtime", "Continuity", "Preview", "Enter work",
    ] {
        assert!(compose.contains(step), "Compose must carry the §5.1 step `{step}`");
    }
    // Step *content* — including Praxis's "no Profile/SkillSet/Skill/Method
    // contract here" — is pinned by `compose_spine`'s unit tests. The pane is
    // narrow and wraps, so only the labels (first token on their line) can be
    // asserted safely from a rendering.
    // The selected Resource's own detail stays in view: it is the most
    // specific thing in the pane and must outrank chrome for the rows.
    assert!(compose.contains("Intent        eligibility unresolved"));
    assert!(compose.contains("Effective     available · 0 providers"));

    // Projection is retired as a top-level Workspace tab (spec §17: "Explain
    // is not a top-level destination in the final IA"); the same authored-
    // intent/effective-state content is now reached through the Explain
    // contextual action, available from any section via `:`.
    surface
        .handle(&mut backend, key(KeyCode::Char(':')))
        .unwrap();
    for character in "explain".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::Explain));
    // The provider Explain evidence (pretty-printed JSON) precedes the
    // project_workspace_render::explain_lines block this overlay now also
    // carries, so the frame needs more rows than the tab views above to keep
    // "Catalog"/"Resolution" on screen.
    let explain = rendered(&draw_width(&surface, 220, 80));
    assert!(explain.contains("Explain · authored intent and effective state"));
    assert!(explain.contains("Catalog"));
    assert!(explain.contains("Resolution"));
}

#[test]
// See the comment on `wide_workspace_renders_context_compose_and_explain_
// from_one_world` above: 220 columns, not 140, keeps the preview pane's
// single-line assertions below intact once the wide-shell Inspector column
// (spec §2.1) takes its own carved share of that pane's width.
fn work_and_system_sections_disclose_real_facts_without_fabricating_factory_or_credential_state() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap(); // Compose
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap(); // Work
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "Work");
    let work = rendered(&draw_width(&surface, 220, 30));
    assert!(work.contains("Work · what is actually running"));
    assert!(work.contains("Factory work    not exposed by application boundary"));
    assert!(
        !work.contains("Journey") && !work.contains("Run "),
        "Work must not fabricate Factory Journey/Run state this application boundary does not expose"
    );

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap(); // Knowledge
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap(); // History
    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap(); // System
    assert_eq!(workspace_section_label(surface.semantic().workspace_section), "System");
    let system = rendered(&draw_width(&surface, 220, 30));
    assert!(system.contains("System · installation and provider disclosure"));
    // The fixture composes no credential input, so the read model carries a
    // `not_attempted` disclosure. System must render that as the open question
    // it is — never as "no credentials" and never as an observed-empty roster.
    assert!(
        system.contains("Credentials   not attempted for this world"),
        "System must disclose an unattempted credential reading as unattempted"
    );
    assert!(
        system.contains("Providers     not observed"),
        "an Unknown provider roster must read as not observed, not as an empty roster"
    );
    assert!(
        !system.contains("none on this machine") && !system.contains("none required"),
        "System must not turn an unread credential world into a confirmed negative"
    );
}

#[test]
fn ctrl_k_navigator_finds_and_opens_a_workspace_destination() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    assert_eq!(surface.semantic().presentation, aikit_tui::PresentationMode::Workspace);
    let before_section = surface.semantic().workspace_section;

    surface
        .handle(
            &mut backend,
            PaletteEvent::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
        )
        .unwrap();
    assert_eq!(surface.semantic().presentation, aikit_tui::PresentationMode::Quick);

    for character in "system".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    let hit = surface
        .semantic()
        .read_model
        .resources
        .iter()
        .find(|item| item.kind == aikit_core::resource::ResourceKind::Surface)
        .expect("a Surface destination hit must be present for query \"system\"")
        .clone();
    assert_eq!(hit.resource.as_str(), "surface/workspace/system");

    let index = surface
        .semantic()
        .read_model
        .position(&hit.resource)
        .expect("the Surface hit must be present in the read model it was read from");
    for _ in 0..=index {
        surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    }
    assert_eq!(surface.semantic().selected.as_ref(), Some(&hit.resource));

    // A destination Surface also carries the global Explain/History contextual
    // actions (every indexed Resource does, via
    // `aikit_core::install_explain_history_actions`), so more than one
    // immediate action is available and `open_selected_action`'s single-
    // immediate-action fast path correctly declines to guess — exactly the
    // same "press : and choose one" fallback the rest of this surface already
    // uses for any multi-action resource. Choose "Open" explicitly.
    surface
        .handle(&mut backend, key(KeyCode::Char(':')))
        .unwrap();
    for character in "open".chars() {
        surface
            .handle(&mut backend, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();

    assert_eq!(surface.semantic().presentation, aikit_tui::PresentationMode::Workspace);
    assert_eq!(
        workspace_section_label(surface.semantic().workspace_section),
        "System"
    );

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.semantic().workspace_section, before_section);
}

#[test]
fn staged_composition_survives_field_navigation() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            // Pinned, not read from the process locale: these assertions
            // name the Unicode separator, and `nextest`'s parallel execution
            // rules out setting `LANG` from a test.
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    let selected = surface
        .semantic()
        .selected
        .clone()
        .expect("explicit selection should choose the review capability");

    surface.handle(&mut backend, ctrl(KeyCode::Char(' '))).unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);
    assert!(surface.semantic().staged.get(&selected).is_some());

    for _ in 0..6 {
        surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
        assert_eq!(surface.semantic().staged.len(), 1);
        assert!(surface.semantic().staged.get(&selected).is_some());
    }
    assert!(backend.applied.is_empty());
}

#[test]
fn narrow_workspace_progressively_discloses_project_world_without_a_second_controller() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("review")
            // Pinned, not read from the process locale: these assertions
            // name the Unicode separator, and `nextest`'s parallel execution
            // rules out setting `LANG` from a test.
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    let selected = surface.semantic().selected.clone();
    let context = rendered(&draw_width(&surface, 60, 24));
    assert!(context.contains("Context · resolved Project world"));
    assert!(context.contains("Project  project:payments"));

    surface.handle(&mut backend, alt(KeyCode::Right)).unwrap();
    let compose = rendered(&draw_width(&surface, 60, 24));
    assert!(compose.contains("Compose · intention to operative world"));
    assert!(compose.contains("Information"));
    assert!(compose.contains("Runtime"));
    assert_eq!(
        surface.semantic().selected,
        selected,
        "responsive Project-world disclosure must not create a second selection state"
    );
}

/// Spec §5.1's Preview answers nine named questions before a person enters
/// work. It rides the existing staging -> preview -> confirm -> apply route
/// (Ctrl+Space stages, Ctrl+S previews), so this drives the real surface
/// rather than the renderer in isolation.
///
/// The overlay pane is narrow and `Wrap` reflows prose, so this asserts the
/// route and the row labels — never wrapped, because they are the first token
/// on their line. Exact row *content* is pinned by
/// `compose_preview`'s own unit tests, where no reflow can hide a wrong fact.
#[test]
fn compose_preview_answers_every_section_5_question_on_the_existing_preview_route() {
    let (_dir, mut backend) = fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("deploy")
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();
    surface
        .handle(&mut backend, ctrl(KeyCode::Char(' ')))
        .unwrap();
    assert_eq!(surface.semantic().staged.len(), 1);

    surface.handle(&mut backend, ctrl(KeyCode::Char('s'))).unwrap();
    assert_eq!(surface.semantic().overlay, Some(Overlay::CompositionPreview));

    let preview = rendered(&draw_width(&surface, 220, 40));

    // Every §5.1 Preview question gets a row, in spec order. A missing row is
    // the failure this test exists to catch: a Preview that silently omits a
    // question reads as "nothing to report" about it.
    for row in [
        "Resolved to",
        "Authored",
        "Effective",
        "Withheld",
        "Carried by",
        "Information",
        "Material",
        "Environment",
        "Activates",
        "Reprojection",
    ] {
        assert!(
            preview.contains(row),
            "§5.1 Preview must answer `{row}` on the preview route"
        );
    }

    // The package-toggle summary is not replaced by the world preview — both
    // answer different questions and the one route carries both.
    assert!(preview.contains("Composition preview"));
    assert!(preview.contains("Ctrl+S proceeds to confirm"));
}
