//! Proof that the Worlds pane actually renders `ProjectWorldReadModel::versioned_world`.
//!
//! Before this, `project_workspace_render::context_lines` (the Worlds-tab
//! renderer) never read the field at all: a Project under real Git material
//! and a Project with no versioned-material provider attached rendered
//! identically, because the pane simply never asked. This drives the public
//! `project_world_lines` entry point -- the same function
//! `crate::v2_render` calls to fill the Worlds pane -- directly against a
//! `ProjectWorldReadModel` built the same way `aikit-core`'s own
//! `with_versioned_world` tests build one, so the proof does not depend on
//! standing up a whole `ApplicationSurfaceController`.

use aikit_core::context::ContextDescriptor;
use aikit_core::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef};
use aikit_core::resource::{
    GitRepositoryRelation, GitWorkingState, GitWorktreeRelation, ProviderRef, VersionRevision,
    VersionedProjectWorld, VersionedWorldCapability, VersionedWorldProviderDescriptor,
    VersionedWorldProviderStatus, VERSIONED_WORLD_VERSION,
};
use aikit_core::ProjectWorldReadModel;

use aikit_tui::application::TuiState;
use aikit_tui::layout::Glyphs;
use aikit_tui::project_workspace_render::{
    project_world_lines, HistoryReading, SessionSpaceRoster, WorkspaceReading,
};

fn binding(project_ref: &str) -> ProjectBinding {
    ProjectBinding::new(
        ProjectRef::parse(project_ref).unwrap(),
        ProjectConstituentRef::parse("source:working-tree").unwrap(),
        ProjectBindingLocator::LocalDirectory { path: "/tmp/worlds-git-probe".into() },
    )
}

fn provider_descriptor() -> VersionedWorldProviderDescriptor {
    VersionedWorldProviderDescriptor {
        provider: ProviderRef::parse("aikit:provider:native-git").unwrap(),
        status: VersionedWorldProviderStatus::Available,
        capabilities: vec![VersionedWorldCapability::Inspect],
        implementation_version: Some("git version 2.43.0".into()),
    }
}

/// A Project with real, dirty Git material and one linked worktree -- enough
/// to exercise every row `git_lines` renders, not just the clean/no-worktree
/// happy path.
fn versioned_world_with_material(project_ref: &str) -> VersionedProjectWorld {
    VersionedProjectWorld {
        version: VERSIONED_WORLD_VERSION.to_string(),
        project: ProjectRef::parse(project_ref).unwrap(),
        provider: provider_descriptor(),
        repository: GitRepositoryRelation {
            repository_root: "/tmp/worlds-git-probe".into(),
            worktree_root: "/tmp/worlds-git-probe".into(),
            head: VersionRevision::new("abcdef0123456789abcdef0123456789abcdef01"),
            branch: Some("trunk".into()),
            detached: false,
            upstream: Some("origin/trunk".into()),
            ahead: 2,
            behind: 1,
        },
        working: GitWorkingState {
            staged: vec!["src/lib.rs".into()],
            unstaged: vec!["src/main.rs".into()],
            untracked: vec!["notes.md".into()],
            conflicted: vec![],
        },
        worktrees: vec![
            GitWorktreeRelation {
                path: "/tmp/worlds-git-probe".into(),
                head: VersionRevision::new("abcdef0123456789abcdef0123456789abcdef01"),
                branch: Some("trunk".into()),
                detached: false,
                locked: false,
                prunable: false,
            },
            GitWorktreeRelation {
                path: "/tmp/worlds-git-probe-review".into(),
                head: VersionRevision::new("1111111111111111111111111111111111111a"),
                branch: Some("review".into()),
                detached: false,
                locked: false,
                prunable: false,
            },
        ],
    }
}

fn render(world: &ProjectWorldReadModel) -> Vec<String> {
    let state = TuiState::default();
    let session_spaces = SessionSpaceRoster::default();
    let history = HistoryReading::default();
    let reading = WorkspaceReading::new(world, &session_spaces, &history);
    project_world_lines(&state, reading, Glyphs::unicode())
}

/// The wiring proof: a Project with real Git material attached shows the
/// branch, a short head revision, the upstream ahead/behind count, the dirty
/// working-tree counts, and the one linked worktree -- none of which
/// `context_lines` read from `versioned_world` before this change.
#[test]
fn the_worlds_pane_renders_attached_git_material() {
    let project_ref = "project:worlds-git-probe";
    let world = ProjectWorldReadModel::empty(binding(project_ref), ContextDescriptor::for_project("/tmp/worlds-git-probe"))
        .with_versioned_world(versioned_world_with_material(project_ref))
        .unwrap();

    let lines = render(&world);

    assert!(
        lines.iter().any(|line| line.contains("Branch") && line.contains("trunk")),
        "expected a Branch row naming trunk, got: {lines:#?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("Head") && line.contains("abcdef012345")),
        "expected a short-head Head row (not the full 40-char SHA), got: {lines:#?}"
    );
    assert!(
        lines.iter().any(|line| !line.contains("abcdef0123456789abcdef0123456789abcdef01")),
        "the head revision must be shortened somewhere, not just echoed in full"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Upstream") && line.contains("origin/trunk") && line.contains("ahead") && line.contains("behind")),
        "expected an Upstream row with ahead/behind counts, got: {lines:#?}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("Working") && line.contains("staged") && line.contains("unstaged") && line.contains("untracked")
        }),
        "expected a Working row summarising the dirty tree, got: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Worktrees") && line.contains("worlds-git-probe-review")),
        "expected the linked worktree to be named, got: {lines:#?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("no versioned material provider")),
        "a Project with real Git material must not render the absence sentence"
    );
}

/// The absence-discipline proof: with no provider attached at all, the Worlds
/// pane must say so plainly rather than rendering a blank/clean-looking
/// section that a reader could mistake for a confirmed clean repository.
#[test]
fn the_worlds_pane_states_plainly_when_no_versioned_provider_is_attached() {
    let project_ref = "project:worlds-git-probe";
    let world = ProjectWorldReadModel::empty(binding(project_ref), ContextDescriptor::for_project("/tmp/worlds-git-probe"));
    assert!(world.versioned_world.is_none());

    let lines = render(&world);

    assert!(
        lines
            .iter()
            .any(|line| line.contains("no versioned material provider attached to this reading")),
        "expected the honest-absence sentence, got: {lines:#?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("clean")),
        "an absent provider must never be worded as though a clean repository was observed"
    );
    assert!(
        !lines.iter().any(|line| line.contains("Branch") || line.contains("Upstream")),
        "no repository rows should appear when nothing was observed, got: {lines:#?}"
    );
}
