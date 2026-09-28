//! Real disk-backed Service parity across the compatibility API and TUI resolver.
use std::fs;

use aikit_cli::app::{AikitApplication, SearchRequest, Service};
use aikit_core::CapsuleId;
use aikit_store::home::AikitHome;
use aikit_tui::application_service::ApplicationService;

#[test]
fn headless_and_interactive_search_share_typed_expressions_and_order() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("store"));
    home.ensure_layout().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    for name in ["gateway", "gateway-guide", "garden"] {
        let root = home
            .registry("personal")
            .join(format!("capsules/skill/parity/{name}"));
        fs::create_dir_all(root.join("payload")).unwrap();
        fs::write(root.join("manifest.toml"), format!(
            "schema = 1\nid = \"skill/parity/{name}\"\nkind = \"skill\"\nname = \"{name}\"\ndescription = \"Operate {name}.\"\n[skill]\nroot = \"payload\"\n"
        )).unwrap();
        fs::write(root.join("payload/SKILL.md"), format!(
            "---\nname: {name}\ndescription: Operate {name}.\n---\n\nRead the source, inspect its state, and explain the result.\n"
        )).unwrap();
    }
    // The verification/close-out practice task language must be able to find.
    let verification = home
        .registry("personal")
        .join("capsules/skill/parity/verification-closeout");
    fs::create_dir_all(verification.join("payload")).unwrap();
    fs::write(
        verification.join("manifest.toml"),
        "schema = 1\nid = \"skill/parity/verification-closeout\"\nkind = \"skill\"\nname = \"verification-before-completion\"\ndescription = \"Verify the implementation before claiming completion and close out with evidence.\"\n[skill]\nroot = \"payload\"\n",
    )
    .unwrap();
    fs::write(
        verification.join("payload/SKILL.md"),
        "---\nname: verification-before-completion\ndescription: Verify the implementation before claiming completion and close out with evidence.\n---\n\nRun the checks and read the evidence back.\n",
    )
    .unwrap();
    let mut service = Service::open(home, &project, |_| None).unwrap();
    for query in [
        "gateway",
        "@5 gateway",
        "@ skill/parity/gateway",
        "@# @5",
        "absent-resource",
        "verify this implementation",
    ] {
        let canonical = ApplicationService::new(&mut service)
            .resolve_search(query)
            .unwrap();
        let expected: Vec<_> = canonical
            .resources
            .resources
            .into_iter()
            .filter_map(|row| {
                let id = CapsuleId::parse(row.resource.as_str()).ok()?;
                service
                    .resolved()
                    .catalog_index
                    .contains_key(&id)
                    .then_some(id)
            })
            .collect();
        let actual = AikitApplication::search(
            &service,
            SearchRequest {
                query: query.to_string(),
                limit: 256,
            },
        )
        .unwrap();
        assert_eq!(
            actual
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            expected,
            "{query}"
        );
        let limited = AikitApplication::search(
            &service,
            SearchRequest {
                query: query.to_string(),
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(
            limited
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            expected.into_iter().take(1).collect::<Vec<_>>(),
            "limit after package filtering: {query}"
        );
    }
    let exact = AikitApplication::search(
        &service,
        SearchRequest {
            query: "gateway".to_string(),
            limit: 1,
        },
    )
    .unwrap();
    assert_eq!(exact.rows[0].id.to_string(), "skill/parity/gateway");

    // X2: task language finds the applicable registered practice, and the
    // exact invocation is learned from ordinary describe — no archaeology.
    let phrase = AikitApplication::search(
        &service,
        SearchRequest {
            query: "verify this implementation".to_string(),
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(
        phrase.rows[0].id.to_string(),
        "skill/parity/verification-closeout",
        "the verification/close-out practice leads the task-phrase answer"
    );
    assert_eq!(phrase.rows[0].name, "verification-before-completion");

    // A garbage query is an answered absence, never a bare expression dump:
    // what was searched, nearby suggestions from the same field, next routes.
    let inert_before = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
    let disclosure = aikit_cli::act::empty_query_disclosure(&service, "zzqx wobble flurb", 3)
        .expect("the empty-state disclosure reads the same field");
    assert_eq!(disclosure["schema"], "aikit.search-empty/v1");
    assert!(disclosure["query"] == "zzqx wobble flurb");
    let searched = disclosure["searched"]
        .as_array()
        .expect("the disclosure names the corpora that were searched");
    assert!(
        searched.iter().any(|corpus| corpus["corpus"] == "resource-field"),
        "the resource field corpus is named"
    );
    let suggestions = disclosure["suggestions"].as_array().unwrap();
    assert!(
        !suggestions.is_empty() && suggestions.len() <= 3,
        "1-3 nearby suggestions, bounded"
    );
    for suggestion in suggestions {
        assert!(suggestion["ref"].as_str().is_some());
        assert!(suggestion["kind"].as_str().is_some());
        assert!(
            suggestion["next"]["describe"]
                .as_str()
                .is_some_and(|route| route.starts_with("aikit act describe ")),
            "every suggestion carries its next describe route"
        );
    }
    let garbage = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .current_dir(&project)
        .env("AIKIT_HOME", temp.path().join("store"))
        .env("AIKIT_CONTEXT_ID", "ctx_SEARCHPARITY0000000000")
        .args(["--json", "search", "zzqx wobble flurb"])
        .output()
        .unwrap();
    assert!(
        garbage.status.success(),
        "{}",
        String::from_utf8_lossy(&garbage.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&garbage.stdout).unwrap();
    let empty = &envelope["data"];
    assert!(empty["rows"].as_array().unwrap().is_empty());
    assert_eq!(empty["empty_query"]["schema"], "aikit.search-empty/v1");
    assert!(
        empty["empty_query"]["suggestions"]
            .as_array()
            .is_some_and(|s| !s.is_empty() && s.len() <= 3),
        "the printed search answer carries the bounded helpful form"
    );

    // Search stays inert across the whole new surface: no familiarity or
    // observation event is recorded by a reading (the standing invariant the
    // gateway block above asserts for display).
    let inert_after = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
    assert_eq!(
        format!("{inert_before:?}"),
        format!("{inert_after:?}"),
        "search and its empty state record nothing"
    );

    let before = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
    AikitApplication::search(
        &service,
        SearchRequest {
            query: "gateway".into(),
            limit: 10,
        },
    )
    .unwrap();
    let after = aikit_tui::backend::PaletteBackend::familiarity(&service).unwrap();
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "display must not teach familiarity"
    );
    let invoke = |command: &str| {
        let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
            .current_dir(&project)
            .env("AIKIT_HOME", temp.path().join("store"))
            .env("AIKIT_CONTEXT_ID", "ctx_SEARCHPARITY0000000000")
            .args(["--json", command, "@5 gateway"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["data"].clone()
    };
    assert_eq!(
        invoke("resolve"),
        invoke("search"),
        "the alias preserves typed refs and path evidence"
    );
}
