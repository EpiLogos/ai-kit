//! The NOW-field provider against a real ground and real ripgrep.
//!
//! This is the acceptance fixture for NOW-field search: an eligible hidden
//! record is found, a private sibling marked `.no-agent-retrieval` is never
//! read, `.git` internals are never traversed, and hits carry canonical
//! Central source identity. Skips honestly when ripgrep is not installed.

use std::fs;
use std::path::PathBuf;

use aikit_adapters::now_field::{NowFieldScope, NowFieldSourcePoolProvider};
use aikit_adapters::ripgrep;
use aikit_core::knowledge_source_pool::{SourcePoolProvider, SourceSearchMode};

fn write(path: &PathBuf, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent exists")).expect("create dirs");
    fs::write(path, contents).expect("write fixture");
}

#[test]
fn the_now_field_answers_from_its_authorised_scope_only() {
    if !ripgrep::available() {
        eprintln!("ripgrep is not installed; the real-scope acceptance test skipped");
        return;
    }
    let ground = tempfile::TempDir::new().expect("temp ground");
    let root = ground.path().to_path_buf();

    write(
        &root.join("Control/agents/now/clearings/abc/now.json"),
        r#"{"schema":"central.now-clearing/v1","task_ref":"control:task:harness-profile-experiment","purpose":"the harness-profile experiment binds models through the roster"}"#,
    );
    write(
        &root.join("Control/user/day/2026-09-17/day.md"),
        "# 2026-09-17\n\n## #2 Operation\n\n- the harness-profile experiment ran against the real roster\n",
    );
    // An eligible hidden record: inside the scope, below a dot-directory.
    write(
        &root.join("Work/Factory/ProjectCentral/now/.archive/2026-09-16.json"),
        r#"{"subject":"superseded harness-profile experiment handoff"}"#,
    );
    write(
        &root.join("Work/Factory/ProjectCentral/now/agents/handoff.json"),
        r#"{"subject":"harness-profile experiment next step"}"#,
    );
    // A private sibling: same record family, owner-marked.
    write(
        &root.join("Work/Factory/ProjectCentral/now/sealed/private.json"),
        r#"{"secret":"harness-profile experiment private-context-never-hit"}"#,
    );
    fs::write(
        root.join("Work/Factory/ProjectCentral/now/sealed/.no-agent-retrieval"),
        b"",
    )
    .expect("write marker");
    // Git internals with the same needle must never be traversed.
    write(
        &root.join("Work/Factory/.git/HARNESS-NEEDLE"),
        "harness-profile experiment private-context-never-hit\n",
    );
    // Outside every record family: never read.
    write(
        &root.join("Control/user/scratch/loose.md"),
        "harness-profile experiment private-context-never-hit\n",
    );

    let provider = NowFieldSourcePoolProvider::connect(
        aikit_adapters::now_field::default_runner(&root),
        ripgrep::executable(),
        NowFieldScope::standard(&root),
    )
    .expect("provider connects where ripgrep exists");
    assert!(provider.status().available);

    let hits = provider
        .search(
            "harness-profile experiment",
            SourceSearchMode::Fulltext,
            &[],
            20,
        )
        .expect("search runs");
    let sources: Vec<String> = hits
        .iter()
        .map(|hit| hit.source.as_str().to_string())
        .collect();
    let expected = [
        "central:source:control:root:Control/agents/now/clearings/abc/now.json",
        "central:source:control:root:Control/user/day/2026-09-17/day.md",
        "central:source:control:root:Work/Factory/ProjectCentral/now/.archive/2026-09-16.json",
        "central:source:control:root:Work/Factory/ProjectCentral/now/agents/handoff.json",
    ];
    let mut found = sources.clone();
    let mut wanted = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    found.sort();
    wanted.sort();
    assert_eq!(
        found, wanted,
        "the eligible hidden record is found; private siblings are not"
    );

    // Every hit is live-readable through the same authorisation, with a real
    // content revision.
    for hit in &hits {
        let material = provider
            .read(&hit.source)
            .expect("hit is readable")
            .expect("material");
        assert!(material
            .binding
            .revision
            .as_str()
            .starts_with("central.content-fnv1a64/v1:"));
    }

    // A private source ref is refused even when named directly.
    let refused = aikit_core::SourceRef::parse(
        "central:source:control:root:Work/Factory/ProjectCentral/now/sealed/private.json",
    )
    .unwrap();
    let error = provider
        .read(&refused)
        .expect_err("private sibling is not readable");
    assert_eq!(error.code(), "now_field.source_unauthorised");

    // A marked path is also invisible to a direct regex search.
    let regex_hits = provider
        .search_regex("private-context-never-hit", &[], 20)
        .expect("regex search runs");
    assert!(
        regex_hits.is_empty(),
        "the marked subtree must not leak through an explicit regex either"
    );
}
