//! Procedures on a real filesystem (Spec II §1).
//!
//! The sentence these tests exist to make true is **a failed procedure leaves the
//! world intact**, and its companion **every committed procedure is undoable**.
//! Both are properties of real files, real symlinks and real backups, so nothing
//! here is simulated.

use std::fs;
use std::path::Path;

use aikit_core::procedure::{
    Inverse, MutationIsolation, Plan, Procedure, ProcedureKind, WorldEdit,
};
use aikit_store::home::AikitHome;
use aikit_store::procedure::{git_repo_of, plan_procedure, ProcedureRunner};

fn home(dir: &Path) -> AikitHome {
    let home = AikitHome::at(dir.join("aikit-home"));
    home.ensure_layout().unwrap();
    home
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A procedure that rewrites one existing file and creates one new file.
fn two_edit_plan(existing: &Path, fresh: &Path) -> Plan {
    Plan::new()
        .with_edit(WorldEdit::WriteFile {
            path: existing.to_path_buf(),
            contents: b"rewritten\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::new("unused-computed-at-stage"),
            },
        })
        .with_edit(WorldEdit::WriteFile {
            path: fresh.to_path_buf(),
            contents: b"created\n".to_vec(),
            inverse: Inverse::Remove,
        })
        .with_note("surveyed two paths")
}

// ---------------------------------------------------------------------------
// Plan and diff: nothing is written before a human has seen it
// ---------------------------------------------------------------------------

#[test]
fn a_diff_reads_the_real_world_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let existing = tmp.path().join("world/existing.txt");
    let fresh = tmp.path().join("world/fresh.txt");
    write(&existing, "original\n");

    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        two_edit_plan(&existing, &fresh),
        MutationIsolation::Staged {
            shadow: tmp.path().join("shadow"),
        },
    )
    .unwrap();

    let diff = ProcedureRunner::new(&home).diff(&procedure).unwrap();

    assert_eq!(diff.edits.len(), 2);
    let rewrite = &diff.edits[0];
    assert_eq!(rewrite.before.as_deref(), Some("original\n"), "before is read from disk");
    assert_eq!(rewrite.after.as_deref(), Some("rewritten\n"));
    assert!(!rewrite.creates);
    let create = &diff.edits[1];
    assert!(create.creates, "a path that does not exist is marked as created");
    assert_eq!(create.before, None);

    // Diffing wrote nothing.
    assert_eq!(fs::read_to_string(&existing).unwrap(), "original\n");
    assert!(!fresh.exists());

    let rendered = diff.render();
    assert!(rendered.contains("undo:"), "the diff states how it undoes: {rendered}");
}

// ---------------------------------------------------------------------------
// Reversibility
// ---------------------------------------------------------------------------

#[test]
fn a_committed_procedure_is_fully_reversible() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let existing = tmp.path().join("world/existing.txt");
    let fresh = tmp.path().join("world/fresh.txt");
    write(&existing, "original\n");

    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        two_edit_plan(&existing, &fresh),
        MutationIsolation::Staged {
            shadow: tmp.path().join("shadow"),
        },
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    let outcome = runner.run(&procedure).unwrap();
    assert_eq!(outcome.applied, 2);
    assert!(!outcome.already_satisfied);

    // The world changed.
    assert_eq!(fs::read_to_string(&existing).unwrap(), "rewritten\n");
    assert_eq!(fs::read_to_string(&fresh).unwrap(), "created\n");

    // And it changes back, exactly.
    let undone = runner.undo(&procedure.id).unwrap();
    assert_eq!(undone, 2);
    assert_eq!(
        fs::read_to_string(&existing).unwrap(),
        "original\n",
        "the overwritten file is restored byte for byte"
    );
    assert!(!fresh.exists(), "the created file is removed");
}

#[test]
fn a_failed_procedure_leaves_the_world_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let good = tmp.path().join("world/good.txt");
    write(&good, "original\n");

    // The second edit cannot succeed: its path's parent is an existing FILE, so
    // creating a directory for it fails. The first edit will already have applied.
    let blocker = tmp.path().join("world/blocker");
    write(&blocker, "i am a file\n");

    let plan = Plan::new()
        .with_edit(WorldEdit::WriteFile {
            path: good.clone(),
            contents: b"rewritten\n".to_vec(),
            inverse: Inverse::Remove,
        })
        .with_edit(WorldEdit::WriteFile {
            path: blocker.join("child.txt"),
            contents: b"never\n".to_vec(),
            inverse: Inverse::Remove,
        });

    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        plan,
        MutationIsolation::Staged {
            shadow: tmp.path().join("shadow"),
        },
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    let error = runner.run(&procedure).unwrap_err();
    assert!(
        error.code().starts_with("procedure."),
        "a real failure is reported: {}",
        error.code()
    );

    assert_eq!(
        fs::read_to_string(&good).unwrap(),
        "original\n",
        "the edit that DID apply was rolled back, so the world is intact"
    );
}

#[test]
fn re_running_a_satisfied_plan_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let target = tmp.path().join("world/thing.txt");
    write(&target, "original\n");

    let make = || {
        Procedure::new(
            ProcedureKind::DoctorFix { checks: vec![] },
            Plan::new().with_edit(WorldEdit::WriteFile {
                path: target.clone(),
                contents: b"fixed\n".to_vec(),
                inverse: Inverse::Remove,
            }),
            MutationIsolation::Staged {
                shadow: tmp.path().join("shadow"),
            },
        )
        .unwrap()
    };

    let runner = ProcedureRunner::new(&home);
    let first = runner.run(&make()).unwrap();
    assert_eq!(first.applied, 1);

    // A second, separately-minted procedure with the SAME plan: the digest is the
    // identity, so the work is already done. This is what makes `doctor --fix`
    // safe to run twice.
    let second = runner.run(&make()).unwrap();
    assert_eq!(second.applied, 0);
    assert!(second.already_satisfied);
    assert_eq!(fs::read_to_string(&target).unwrap(), "fixed\n");
}

// ---------------------------------------------------------------------------
// Marked blocks: editing a file AIKit does not own
// ---------------------------------------------------------------------------

#[test]
fn a_marked_block_edit_is_idempotent_and_leaves_human_prose_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let agents_md = tmp.path().join("world/AGENTS.md");
    write(&agents_md, "# My project\n\nHand-written guidance I care about.\n");

    let procedure = |body: &str| {
        Procedure::new(
            ProcedureKind::ClientInstall {
                client: aikit_core::TargetId::codex(),
            },
            Plan::new().with_edit(WorldEdit::MarkedBlock {
                path: agents_md.clone(),
                marker: "aikit".to_string(),
                contents: body.to_string(),
            }),
            MutationIsolation::Direct,
        )
        .unwrap()
    };

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure("first body")).unwrap();
    let once = fs::read_to_string(&agents_md).unwrap();
    assert!(once.contains("Hand-written guidance I care about."));
    assert!(once.contains("first body"));
    assert!(
        once.contains("<!--"),
        "a markdown file gets an HTML comment leader, not a heading: {once}"
    );

    runner.run(&procedure("second body")).unwrap();
    let twice = fs::read_to_string(&agents_md).unwrap();
    assert_eq!(
        twice.matches(">>> aikit >>>").count(),
        1,
        "applying twice replaces the block, never appends a second: {twice}"
    );
    assert!(twice.contains("second body"));
    assert!(!twice.contains("first body"));
    assert!(twice.contains("Hand-written guidance I care about."));
}

// ---------------------------------------------------------------------------
// The safety rule
// ---------------------------------------------------------------------------

#[test]
fn direct_isolation_is_refused_for_an_edit_that_cannot_be_undone() {
    let plan = Plan::new().with_edit(WorldEdit::RunCommand {
        argv: vec!["rm".to_string(), "-rf".to_string(), "/important".to_string()],
        cwd: std::path::PathBuf::from("/"),
        undo: None,
    });
    let error = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        plan,
        MutationIsolation::Direct,
    )
    .unwrap_err();
    assert_eq!(error.code(), "procedure.not_reversible");
}

#[test]
fn isolation_selection_uses_a_git_branch_for_a_real_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let inside = repo.join("file.txt");
    write(&inside, "x\n");

    assert_eq!(git_repo_of(&inside).as_deref(), Some(repo.as_path()));

    let plan = Plan::new().with_edit(WorldEdit::WriteFile {
        path: inside,
        contents: b"y\n".to_vec(),
        inverse: Inverse::Remove,
    });
    let procedure = plan_procedure(&home, ProcedureKind::DoctorFix { checks: vec![] }, plan).unwrap();
    assert!(
        matches!(procedure.isolation, MutationIsolation::GitBranch { .. }),
        "a plan wholly inside one repository stages on a branch, got {:?}",
        procedure.isolation
    );

    // A path outside any repository stages in a shadow tree instead.
    let loose = tmp.path().join("loose/file.txt");
    write(&loose, "x\n");
    let plan = Plan::new().with_edit(WorldEdit::WriteFile {
        path: loose,
        contents: b"y\n".to_vec(),
        inverse: Inverse::Remove,
    });
    let staged = plan_procedure(&home, ProcedureKind::DoctorFix { checks: vec![] }, plan).unwrap();
    assert!(matches!(staged.isolation, MutationIsolation::Staged { .. }));
}

// ---------------------------------------------------------------------------
// Symlinks: the shape the machine's real skill trees are made of
// ---------------------------------------------------------------------------

#[test]
fn replacing_a_symlink_restores_the_original_link_on_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let real = tmp.path().join("world/real-target");
    write(&real, "target\n");
    let other = tmp.path().join("world/other-target");
    write(&other, "other\n");

    let link = tmp.path().join("world/link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let procedure = Procedure::new(
        ProcedureKind::Adopt { capsules: vec![] },
        Plan::new().with_edit(WorldEdit::CreateLink {
            path: link.clone(),
            target: other.clone(),
            inverse: Inverse::Recreate {
                target: real.clone(),
            },
        }),
        MutationIsolation::Staged {
            shadow: tmp.path().join("shadow"),
        },
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    assert_eq!(fs::read_link(&link).unwrap(), other, "the link was repointed");

    runner.undo(&procedure.id).unwrap();
    assert_eq!(
        fs::read_link(&link).unwrap(),
        real,
        "undo recreates the original link rather than leaving a copy of its target"
    );
}

#[test]
fn listing_reports_the_procedures_that_have_an_undo_record() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let target = tmp.path().join("world/a.txt");
    write(&target, "x\n");

    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target,
            contents: b"y\n".to_vec(),
            inverse: Inverse::Remove,
        }),
        MutationIsolation::Staged {
            shadow: tmp.path().join("shadow"),
        },
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    assert!(runner.list().unwrap().is_empty());
    runner.run(&procedure).unwrap();
    assert_eq!(runner.list().unwrap(), vec![procedure.id.clone()]);
}
