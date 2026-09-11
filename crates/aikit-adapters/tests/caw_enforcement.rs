//! Controlled placement owner; tests adapter law, not an unpublished Central API
//! or installation of hooks/material confinement.
use aikit_adapters::placement_enforcement::*;
use aikit_core::{ResourceRef, Result, SourceRevision};
use serde_json::json;
use std::{cell::Cell, path::PathBuf};
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn rev(s: &str) -> SourceRevision {
    SourceRevision::parse(s).unwrap()
}
struct Owner {
    // canonical: guard() canonicalises every attempt path, so the fixture's
    // comparisons must live in the same (/var vs /private/var on macOS) space.
    root: PathBuf,
    revision: &'static str,
    calls: Cell<usize>,
}
impl Owner {
    fn new(root: &std::path::Path) -> Self {
        Self { root: root.canonicalize().unwrap(), revision: "rev/1", calls: Cell::new(0) }
    }
}
impl PlacementOwner for Owner {
    fn resolve_and_allocate(&self, _: &ResourceRef) -> Result<PlacementBasis> {
        let now = self.root.join("NOW/task");
        std::fs::create_dir_all(&now).unwrap();
        Ok(PlacementBasis {
            policy_ref: r("central:policy:fixture"),
            policy_revision: rev("rev/1"),
            allocation_ref: r("central:now:fixture"),
            now: now.canonicalize().unwrap(),
            requirement: EnforcementRequirement::NativeWriteEvents,
        })
    }
    fn validate_write(&self, _: &PlacementBasis, a: &WriteAttempt) -> Result<PlacementDecision> {
        self.calls.set(self.calls.get() + 1);
        Ok(
            if a.target.starts_with(self.root.join("source"))
                || a.target.starts_with(self.root.join("NOW"))
            {
                PlacementDecision::Allow {
                    decision_ref: r("decision/fixture"),
                    policy_revision: rev(self.revision),
                    canonical_target: a.target.clone(),
                }
            } else {
                PlacementDecision::Deny {
                    decision_ref: r("decision/fixture"),
                    policy_revision: rev(self.revision),
                    reason: "Root scratch is not an authorised source write".into(),
                }
            },
        )
    }
}
fn coverage() -> EnforcementCoverage {
    EnforcementCoverage {
        harness: "claude-code".into(),
        descriptor_revision: Some(1),
        native_file_events_blockable: true,
        material_receipt: None,
        limitations: vec!["fixture descriptor, not installed hook proof".into()],
    }
}
#[test]
fn native_source_writes_and_now_work_succeed_but_root_scratch_returns_usable_destination() {
    let t = tempfile::tempdir().unwrap();
    let o = Owner::new(t.path());
    for path in ["source/README.md", "NOW/task/draft.md"] {
        assert!(
            guard(
                &o,
                &r("task/a"),
                &WriteAttempt {
                    cwd: t.path().into(),
                    target: path.into(),
                    exposure: WriteExposure::NativeFileOperation
                },
                &coverage()
            )
            .unwrap()
            .allowed
        );
    }
    let result = guard(
        &o,
        &r("task/a"),
        &WriteAttempt {
            cwd: t.path().into(),
            target: "scratch.md".into(),
            exposure: WriteExposure::NativeFileOperation,
        },
        &coverage(),
    )
    .unwrap();
    assert!(!result.allowed);
    assert!(result.basis.now.is_dir());
    let native = claude_pre_tool_response(&result).unwrap();
    assert_eq!(native.exit_code, 2);
    assert!(native.stderr.contains("NOW destination"));
}
#[test]
fn opaque_shell_and_unsupported_body_cannot_borrow_native_blocking() {
    let t = tempfile::tempdir().unwrap();
    let o = Owner::new(t.path());
    let a = WriteAttempt {
        cwd: t.path().into(),
        target: "source/lib.rs".into(),
        exposure: WriteExposure::OpaqueProcess,
    };
    assert!(!guard(&o, &r("task/a"), &a, &coverage()).unwrap().allowed);
    assert_eq!(o.calls.get(), 0);
    let mut codex = coverage();
    codex.harness = "codex".into();
    codex.native_file_events_blockable = false;
    let out = guard(&o, &r("task/a"), &a, &codex).unwrap();
    assert!(!out.allowed);
    assert!(claude_pre_tool_response(&out).is_err());
    assert!(!codex.satisfies(
        EnforcementRequirement::MaterialConfinement,
        WriteExposure::NativeFileOperation
    ));
}
#[test]
fn stale_policy_and_parent_traversal_fail_closed() {
    let t = tempfile::tempdir().unwrap();
    let o = Owner {
        root: t.path().into(),
        revision: "rev/2",
        calls: Cell::new(0),
    };
    assert_eq!(
        guard(
            &o,
            &r("task/a"),
            &WriteAttempt {
                cwd: t.path().into(),
                target: "source/a".into(),
                exposure: WriteExposure::NativeFileOperation
            },
            &coverage()
        )
        .unwrap_err()
        .code(),
        "placement.policy_changed"
    );
    assert!(canonical_write_target(t.path(), std::path::Path::new("../escape")).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_redirect_resolves_to_actual_owner_decision_not_lexical_prefix() {
    let t = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(other.path(), t.path().join("source")).unwrap();
    let o = Owner::new(t.path());
    let out = guard(
        &o,
        &r("task/a"),
        &WriteAttempt {
            cwd: t.path().into(),
            target: "source/file".into(),
            exposure: WriteExposure::NativeFileOperation,
        },
        &coverage(),
    )
    .unwrap();
    assert!(!out.allowed);
}
#[test]
fn hook_install_and_remove_preserve_foreign_siblings_even_empty_entries() {
    let source = json!({"other":true,"hooks":{"Stop":[{"custom":1}],"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"foreign"}]},{"hooks":[]},{"custom":"opaque"}]}});
    let once = project_claude_hook(&source, "aikit guard-owned", true).unwrap();
    assert_eq!(
        once,
        project_claude_hook(&once, "aikit guard-owned", true).unwrap()
    );
    assert_eq!(
        source,
        project_claude_hook(&once, "aikit guard-owned", false).unwrap()
    );
    assert!(project_claude_hook(&json!({"hooks":"foreign-unparsed"}), "owned", true).is_err());
}
