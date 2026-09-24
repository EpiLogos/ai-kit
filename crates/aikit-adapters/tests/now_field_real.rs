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
        assert!(
            std::env::var_os("AIKIT_REQUIRE_RIPGREP_REAL").is_none(),
            "real NOW-field conformance requires ripgrep"
        );
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

#[test]
fn project_scope_uses_literal_work_name_and_keeps_common_control_with_real_ripgrep() {
    if !ripgrep::available() {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_RIPGREP_REAL").is_none(),
            "real NOW-field conformance requires ripgrep"
        );
        return;
    }
    let ground = tempfile::TempDir::new().expect("temp ground");
    let root = ground.path().to_path_buf();
    write(
        &root.join("Work/fee*box/ProjectCentral/now/agents/own.json"),
        "literalGlobNeedle\n",
    );
    write(
        &root.join("Work/feeeeeebox/ProjectCentral/now/agents/sibling.json"),
        "literalGlobNeedle\n",
    );
    write(
        &root.join("Control/agents/now/clearings/common/note.json"),
        "literalGlobNeedle\n",
    );
    let broad = NowFieldScope::standard(&root);
    let provider = NowFieldSourcePoolProvider::connect(
        aikit_adapters::now_field::default_runner(&root),
        ripgrep::executable(),
        broad.for_project(Some("fee*box")),
    )
    .expect("scoped provider connects");
    let hits = provider
        .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
        .expect("real scoped ripgrep search");
    let refs: Vec<&str> = hits.iter().map(|hit| hit.source.as_str()).collect();
    assert!(refs
        .contains(&"central:source:control:root:Work/fee*box/ProjectCentral/now/agents/own.json"));
    assert!(
        refs.contains(&"central:source:control:root:Control/agents/now/clearings/common/note.json")
    );
    assert!(!refs
        .iter()
        .any(|source| source.contains("Work/feeeeeebox/")));
    let sibling = aikit_core::SourceRef::parse(
        "central:source:control:root:Work/feeeeeebox/ProjectCentral/now/agents/sibling.json",
    )
    .unwrap();
    assert_eq!(
        provider.read(&sibling).unwrap_err().code(),
        "now_field.source_unauthorised"
    );

    let unknown = NowFieldSourcePoolProvider::connect(
        aikit_adapters::now_field::default_runner(&root),
        ripgrep::executable(),
        broad.for_project(None),
    )
    .expect("Control-only provider connects");
    let unknown_refs: Vec<String> = unknown
        .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
        .unwrap()
        .into_iter()
        .map(|hit| hit.source.as_str().to_owned())
        .collect();
    assert_eq!(
        unknown_refs,
        vec!["central:source:control:root:Control/agents/now/clearings/common/note.json"]
    );

    let own = aikit_core::SourceRef::parse(
        "central:source:control:root:Work/fee*box/ProjectCentral/now/agents/own.json",
    )
    .unwrap();
    let common = aikit_core::SourceRef::parse(
        "central:source:control:root:Control/agents/now/clearings/common/note.json",
    )
    .unwrap();
    let traversal = aikit_core::SourceRef::parse(
        "central:source:control:root:Work/fee*box/ProjectCentral/now/agents/../../../../feeeeeebox/ProjectCentral/now/agents/sibling.json",
    )
    .unwrap();
    assert!(aikit_adapters::now_field::glob_match(
        "Work/fee\\*box/ProjectCentral/now/**/*.json",
        "Work/fee*box/ProjectCentral/now/agents/../../../../feeeeeebox/ProjectCentral/now/agents/sibling.json"
    ));
    assert_eq!(
        provider.read(&traversal).unwrap_err().code(),
        "now_field.source_unauthorised",
        "a syntactically eligible ref must not traverse into a sibling Project"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let link = root.join("Work/fee*box/ProjectCentral/now/agents/linked.json");
        symlink(
            root.join("Work/feeeeeebox/ProjectCentral/now/agents/sibling.json"),
            &link,
        )
        .unwrap();
        let link_ref = aikit_core::SourceRef::parse(
            "central:source:control:root:Work/fee*box/ProjectCentral/now/agents/linked.json",
        )
        .unwrap();
        assert_eq!(
            provider.read(&link_ref).unwrap_err().code(),
            "now_field.source_unauthorised",
            "a direct owner read must not follow a symlink into a sibling Project"
        );
        assert!(!provider
            .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
            .unwrap()
            .iter()
            .any(|hit| hit.source == link_ref));
        fs::remove_file(link).unwrap();
    }
    // Ripgrep's `**` has no eight-level cap. A marker below that old walk
    // limit must still be discovered before the real provider searches it.
    let deep_root = root.join("Work/fee*box/ProjectCentral/now/deep");
    let deep_dir = (0..10).fold(deep_root.clone(), |path, level| {
        path.join(format!("l{level}"))
    });
    let deep_file = deep_dir.join("private.json");
    write(&deep_file, "literalGlobNeedle\n");
    fs::write(deep_dir.join(".no-agent-retrieval"), b"").unwrap();
    let deep_scope = NowFieldScope::standard(&root).for_project(Some("fee*box"));
    assert!(deep_scope
        .pruned
        .iter()
        .any(|path| path == deep_dir.strip_prefix(&root).unwrap().to_str().unwrap()));
    let deep_provider = NowFieldSourcePoolProvider::connect(
        aikit_adapters::now_field::default_runner(&root),
        ripgrep::executable(),
        deep_scope,
    )
    .unwrap();
    let deep_ref = aikit_core::SourceRef::parse(format!(
        "central:source:control:root:{}",
        deep_file.strip_prefix(&root).unwrap().display()
    ))
    .unwrap();
    assert!(!deep_provider
        .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
        .unwrap()
        .iter()
        .any(|hit| hit.source == deep_ref));
    assert_eq!(
        deep_provider.read(&deep_ref).unwrap_err().code(),
        "now_field.source_unauthorised"
    );
    fs::remove_dir_all(deep_root).unwrap();

    for (marked_dir, own_visible) in [
        ("Work", false),
        ("Work/fee*box", false),
        ("Work/fee*box/ProjectCentral", false),
        ("Control", true),
    ] {
        let marker = root.join(marked_dir).join(".no-agent-retrieval");
        fs::write(&marker, b"").expect("mark an ancestor of an authorised record");
        if marked_dir == "Work" {
            let live_refs: Vec<String> = provider
                .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
                .expect("a newly added marker must fence an attached provider")
                .into_iter()
                .map(|hit| hit.source.as_str().to_owned())
                .collect();
            assert_eq!(live_refs, vec![common.as_str().to_owned()]);
            assert_eq!(
                provider.read(&own).unwrap_err().code(),
                "now_field.source_unauthorised"
            );
        }
        let marked = NowFieldScope::standard(&root).for_project(Some("fee*box"));
        assert!(
            marked.pruned.iter().any(|path| path == marked_dir),
            "marker at {marked_dir} must be carried into ripgrep exclusions"
        );
        let marked_provider = NowFieldSourcePoolProvider::connect(
            aikit_adapters::now_field::default_runner(&root),
            ripgrep::executable(),
            marked,
        )
        .expect("marked provider connects");
        let expected = if own_visible {
            own.as_str()
        } else {
            common.as_str()
        };
        let refs: Vec<String> = marked_provider
            .search("literalGlobNeedle", SourceSearchMode::Fulltext, &[], 20)
            .expect("real ripgrep honours ancestor marker")
            .into_iter()
            .map(|hit| hit.source.as_str().to_owned())
            .collect();
        assert_eq!(
            refs,
            vec![expected.to_owned()],
            "search crossed marker at {marked_dir}"
        );
        let regex_refs: Vec<String> = marked_provider
            .search_regex("literalGlobNeedle", &[], 20)
            .expect("real regex ripgrep honours ancestor marker")
            .into_iter()
            .map(|hit| hit.source.as_str().to_owned())
            .collect();
        assert_eq!(regex_refs, vec![expected.to_owned()]);
        let withheld = if own_visible { &common } else { &own };
        assert_eq!(
            marked_provider.read(withheld).unwrap_err().code(),
            "now_field.source_unauthorised",
            "direct owner read crossed marker at {marked_dir}"
        );
        assert!(marked_provider.descriptors().iter().all(|material| material
            .binding
            .source
            .as_str()
            != withheld.as_str()));
        fs::remove_file(marker).unwrap();
    }
}
