//! Procedures on a real filesystem (Spec II §1).
//!
//! The sentence these tests exist to make true is **a failed procedure leaves the
//! world intact**, and its companion **every committed procedure is undoable**.
//! Both are properties of real files, real symlinks and real backups, so nothing
//! here is simulated.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

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

fn staged(home: &AikitHome, label: &str) -> MutationIsolation {
    MutationIsolation::Staged {
        shadow: home.state().join("procedures").join(label),
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn real_git_repo(root: &Path) -> std::path::PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "AIKit Test"]);
    git(&repo, &["config", "user.email", "aikit-test@localhost"]);
    write(&repo.join("file.txt"), "original\n");
    git(&repo, &["add", "file.txt"]);
    git(&repo, &["commit", "-m", "initial"]);
    repo
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
        staged(&home, "diff-stage"),
    )
    .unwrap();

    let diff = ProcedureRunner::new(&home).diff(&procedure).unwrap();

    assert_eq!(diff.edits.len(), 2);
    let rewrite = &diff.edits[0];
    assert_eq!(
        rewrite.before.as_deref(),
        Some("original\n"),
        "before is read from disk"
    );
    assert_eq!(rewrite.after.as_deref(), Some("rewritten\n"));
    assert!(!rewrite.creates);
    let create = &diff.edits[1];
    assert!(
        create.creates,
        "a path that does not exist is marked as created"
    );
    assert_eq!(create.before, None);

    // Diffing wrote nothing.
    assert_eq!(fs::read_to_string(&existing).unwrap(), "original\n");
    assert!(!fresh.exists());

    let rendered = diff.render();
    assert!(
        rendered.contains("undo:"),
        "the diff states how it undoes: {rendered}"
    );
    assert!(
        rendered.contains("before:\n    original\n")
            && rendered.contains("after:\n    rewritten\n"),
        "the human-readable diff includes the actual before and after content: {rendered}"
    );
    assert!(
        rendered.contains("before: <absent>"),
        "new files state that the reviewed prior state is absence: {rendered}"
    );
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
        staged(&home, "reversible-stage"),
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
fn undo_refuses_drift_before_touching_any_path() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let existing = tmp.path().join("world/existing.txt");
    let fresh = tmp.path().join("world/fresh.txt");
    write(&existing, "original\n");

    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        two_edit_plan(&existing, &fresh),
        staged(&home, "drift-stage"),
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();

    // A person changes one result after the Procedure. Undo must not erase that
    // work, and it must preflight the whole journal before restoring anything
    // else.
    write(&fresh, "user changed this after apply\n");
    let error = runner.undo(&procedure.id).unwrap_err();
    assert_eq!(error.code(), "procedure.undo_drift");
    assert_eq!(
        fs::read_to_string(&existing).unwrap(),
        "rewritten\n",
        "preflight means an earlier inverse was not partially applied"
    );
    assert_eq!(
        fs::read_to_string(&fresh).unwrap(),
        "user changed this after apply\n",
        "the user's later work is preserved"
    );
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
        staged(&home, "failure-stage"),
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
fn a_failure_to_record_satisfaction_rolls_back_the_committed_world() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let existing = tmp.path().join("world/existing.txt");
    let fresh = tmp.path().join("world/fresh.txt");
    write(&existing, "original\n");
    // A file where the marker directory must be makes the final audit write
    // fail after the world edits. The Procedure contract still requires the
    // world to look exactly as it did before the run.
    write(
        &home.state().join("procedures/.satisfied"),
        "not a directory\n",
    );
    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        two_edit_plan(&existing, &fresh),
        staged(&home, "satisfaction-stage"),
    )
    .unwrap();

    let error = ProcedureRunner::new(&home).run(&procedure).unwrap_err();

    assert_eq!(error.code(), "home.create_failed");
    assert_eq!(fs::read_to_string(&existing).unwrap(), "original\n");
    assert!(!fresh.exists());
}

#[test]
fn a_run_command_executes_its_recorded_undo_command() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let marker = tmp.path().join("command-effect");
    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::RunCommand {
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!("printf applied > '{}'", marker.display()),
            ],
            cwd: tmp.path().to_path_buf(),
            undo: Some(vec![
                "sh".into(),
                "-c".into(),
                format!("rm -f '{}'", marker.display()),
            ]),
        }),
        MutationIsolation::Direct,
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    assert_eq!(fs::read_to_string(&marker).unwrap(), "applied");
    assert_eq!(runner.undo(&procedure.id).unwrap(), 1);
    assert!(!marker.exists(), "the declared inverse command ran");
}

#[test]
fn marked_block_undo_refuses_to_erase_later_human_prose() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let file = tmp.path().join("world/AGENTS.md");
    write(&file, "# Project\n\nOriginal prose.\n");
    let procedure = Procedure::new(
        ProcedureKind::ClientInstall {
            client: aikit_core::TargetId::codex(),
        },
        Plan::new().with_edit(WorldEdit::MarkedBlock {
            path: file.clone(),
            marker: "aikit".into(),
            contents: "managed".into(),
        }),
        MutationIsolation::Direct,
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(b"\nLater human prose.\n")
        .unwrap();

    let error = runner.undo(&procedure.id).unwrap_err();
    assert_eq!(error.code(), "procedure.undo_drift");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("Later human prose."));
    assert!(text.contains("managed"));
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
            staged(&home, "idempotent-stage"),
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
    write(
        &agents_md,
        "# My project\n\nHand-written guidance I care about.\n",
    );

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
        argv: vec![
            "rm".to_string(),
            "-rf".to_string(),
            "/important".to_string(),
        ],
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
    let procedure =
        plan_procedure(&home, ProcedureKind::DoctorFix { checks: vec![] }, plan).unwrap();
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

#[test]
fn git_branch_isolation_commits_the_procedure_and_undo_reverts_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let target = repo.join("file.txt");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"rewritten by procedure\n".to_vec(),
            inverse: Inverse::Restore {
                // The runner replaces this sentinel with the real pre-edit bytes
                // while staging the Procedure.
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    let MutationIsolation::GitBranch { branch, .. } = &procedure.isolation else {
        panic!("a real repository must select branch isolation");
    };

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();

    assert_eq!(git(&repo, &["branch", "--show-current"]), *branch);
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
    assert_eq!(
        git(&repo, &["log", "-1", "--pretty=%s"]),
        format!("aikit procedure {}", procedure.id)
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "rewritten by procedure\n"
    );
    assert_eq!(
        git(&repo, &["show", "main:file.txt"]),
        "original",
        "the original branch remains untouched"
    );

    assert_eq!(runner.undo(&procedure.id).unwrap(), 1);
    assert_eq!(fs::read_to_string(&target).unwrap(), "original\n");
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
    assert!(
        git(&repo, &["log", "-1", "--pretty=%s"]).starts_with("Revert "),
        "undo is an auditable git revert"
    );

    let reapplied = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"rewritten by procedure\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    runner.run(&reapplied).unwrap();
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "rewritten by procedure\n",
        "the same plan can be applied again after undo on its own fresh branch"
    );
}

#[test]
fn git_branch_isolation_refuses_a_dirty_repo_before_touching_the_target() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let target = repo.join("file.txt");
    write(&target, "human work in progress\n");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"procedure output\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();

    let error = ProcedureRunner::new(&home).run(&procedure).unwrap_err();
    assert_eq!(error.code(), "procedure.git_dirty");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "human work in progress\n"
    );
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo, &["log", "--oneline"]).lines().count(), 1);
}

#[test]
fn an_inverse_capture_failure_restores_the_original_git_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let file = repo.join("file.txt");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: file.clone(),
            contents: b"changed\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);
    let undo = runner.procedure_dir(&procedure.id).join("undo");
    fs::create_dir_all(&undo).unwrap();
    fs::set_permissions(&undo, fs::Permissions::from_mode(0o000)).unwrap();

    let error = runner.run(&procedure).unwrap_err();

    fs::set_permissions(&undo, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(error.code(), "procedure.write_failed");
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert!(
        !git(&repo, &["branch", "--list", "aikit/*"]).contains("aikit/"),
        "a failure before the first edit must not strand its isolation branch"
    );
}

#[test]
fn a_rejecting_commit_hook_leaves_the_original_branch_and_index_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let hook = repo.join(".git/hooks/pre-commit");
    write(&hook, "#!/bin/sh\nexit 1\n");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let existing = repo.join("file.txt");
    let fresh = repo.join("new.txt");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        two_edit_plan(&existing, &fresh),
    )
    .unwrap();
    let MutationIsolation::GitBranch { branch, .. } = &procedure.isolation else {
        panic!("the repository must select branch isolation");
    };
    let branch = branch.clone();

    let error = ProcedureRunner::new(&home).run(&procedure).unwrap_err();
    assert_eq!(error.code(), "procedure.git_commit_failed");
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert!(
        git(&repo, &["status", "--porcelain"]).is_empty(),
        "neither the working tree nor index may carry the rejected Procedure"
    );
    assert_eq!(fs::read_to_string(&existing).unwrap(), "original\n");
    assert!(!fresh.exists());
    assert!(
        git(&repo, &["branch", "--list", &branch]).is_empty(),
        "the failed isolated branch is deleted after verified cleanup"
    );
}

#[test]
fn a_post_commit_metadata_failure_drops_the_commit_and_restores_the_original_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let target = repo.join("file.txt");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"procedure edit\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    // The commit itself can succeed, but the required audit record cannot be
    // written over a directory. This exercises the boundary after commit.
    fs::create_dir_all(
        ProcedureRunner::new(&home)
            .procedure_dir(&procedure.id)
            .join("git.json"),
    )
    .unwrap();

    let error = ProcedureRunner::new(&home).run(&procedure).unwrap_err();

    assert_eq!(error.code(), "procedure.write_failed");
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert_eq!(git(&repo, &["log", "--oneline"]).lines().count(), 1);
    assert_eq!(fs::read_to_string(target).unwrap(), "original\n");
    assert!(
        !git(&repo, &["branch", "--list", "aikit/*"]).contains("aikit/"),
        "the unrecordable commit must not survive on a stranded branch"
    );
}

#[test]
fn git_undo_refuses_when_the_recorded_branch_has_advanced() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let repo = real_git_repo(tmp.path());
    let target = repo.join("file.txt");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"procedure state\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();

    write(&repo.join("later.txt"), "intentional later commit\n");
    git(&repo, &["add", "later.txt"]);
    git(&repo, &["commit", "-m", "later work"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);

    let error = runner.undo(&procedure.id).unwrap_err();
    assert_eq!(error.code(), "procedure.undo_drift");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());
    assert_eq!(fs::read_to_string(&target).unwrap(), "procedure state\n");
    assert!(
        !repo.join(".git/REVERT_HEAD").exists(),
        "a refused undo never starts a revert"
    );
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
        ProcedureKind::Adopt {
            source: tmp.path().join("world"),
            namespace: "test".to_string(),
            capsules: vec![],
        },
        Plan::new().with_edit(WorldEdit::CreateLink {
            path: link.clone(),
            target: other.clone(),
            inverse: Inverse::Recreate {
                target: real.clone(),
            },
        }),
        staged(&home, "symlink-stage"),
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    assert_eq!(
        fs::read_link(&link).unwrap(),
        other,
        "the link was repointed"
    );

    runner.undo(&procedure.id).unwrap();
    assert_eq!(
        fs::read_link(&link).unwrap(),
        real,
        "undo recreates the original link rather than leaving a copy of its target"
    );
}

#[test]
fn writing_through_a_symlink_replaces_only_the_declared_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let external = tmp.path().join("external/authority.txt");
    write(&external, "external authority\n");
    let link = tmp.path().join("world/declaration.toml");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&external, &link).unwrap();
    let procedure = Procedure::new(
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: link.clone(),
            contents: b"owned declaration\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
        staged(&home, "symlink-write-stage"),
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    assert!(
        !link.is_symlink(),
        "the declared directory entry was replaced"
    );
    assert_eq!(fs::read_to_string(&link).unwrap(), "owned declaration\n");
    assert_eq!(
        fs::read_to_string(&external).unwrap(),
        "external authority\n",
        "apply never followed the link into an external authority"
    );

    runner.undo(&procedure.id).unwrap();
    assert_eq!(fs::read_link(&link).unwrap(), external);
    assert_eq!(
        fs::read_to_string(&external).unwrap(),
        "external authority\n"
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
        staged(&home, "listing-stage"),
    )
    .unwrap();

    let runner = ProcedureRunner::new(&home);
    assert!(runner.list().unwrap().is_empty());
    runner.run(&procedure).unwrap();
    assert_eq!(runner.list().unwrap(), vec![procedure.id.clone()]);
}

#[test]
fn apply_refuses_when_a_file_changes_after_the_plan_was_created() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let target = tmp.path().join("world/concurrent.txt");
    write(&target, "reviewed state\n");
    let plan = Plan::new().with_edit(WorldEdit::WriteFile {
        path: target.clone(),
        contents: b"planned state\n".to_vec(),
        inverse: Inverse::Restore {
            blob: aikit_core::procedure::BlobId::deferred(),
        },
    });
    let procedure =
        plan_procedure(&home, ProcedureKind::DoctorFix { checks: vec![] }, plan).unwrap();

    write(&target, "newer concurrent work\n");
    let error = ProcedureRunner::new(&home).run(&procedure).unwrap_err();

    assert_eq!(error.code(), "procedure.precondition_failed");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "newer concurrent work\n",
        "the newer bytes must never be backed up and overwritten"
    );
    assert!(
        ProcedureRunner::new(&home).list().unwrap().is_empty(),
        "a refused apply is not a committed Procedure"
    );
}

#[test]
fn a_satisfaction_marker_never_hides_drift_in_the_applied_result() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    let target = tmp.path().join("world/result.txt");
    write(&target, "before\n");
    let procedure = plan_procedure(
        &home,
        ProcedureKind::DoctorFix { checks: vec![] },
        Plan::new().with_edit(WorldEdit::WriteFile {
            path: target.clone(),
            contents: b"expected result\n".to_vec(),
            inverse: Inverse::Restore {
                blob: aikit_core::procedure::BlobId::deferred(),
            },
        }),
    )
    .unwrap();
    let runner = ProcedureRunner::new(&home);
    runner.run(&procedure).unwrap();
    write(&target, "newer work after apply\n");

    let error = runner.run(&procedure).unwrap_err();

    assert_eq!(error.code(), "procedure.satisfied_drift");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "newer work after apply\n"
    );
}
