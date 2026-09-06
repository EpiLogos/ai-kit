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
    let mut service = Service::open(home, &project, |_| None).unwrap();
    for query in [
        "gateway",
        "@5 gateway",
        "@ skill/parity/gateway",
        "@# @5",
        "absent-resource",
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
