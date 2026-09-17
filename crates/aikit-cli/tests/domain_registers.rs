//! Two registers, one law (W1/CASE 03 extension).
//!
//! Domain declarations were project-layer only, which had a consequence nobody
//! chose: a convention could be declared inside one project and be unreachable
//! everywhere else — including at the root register, where the root wiki and
//! all cross-project work live. These assert the personal register exists, that
//! the project's own still wins where the two disagree, and that the
//! replacement is visible rather than silent.

use aikit_cli::domain_activation::{load_domains, load_domains_in};
use std::fs;
use std::path::Path;

fn declare(dir: &Path, id: &str, rule: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join(format!("{id}.toml")),
        format!(
            r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/{id}"
title = "{id}"
revision = "r1"
source = "central:source:test:.aikit/domains/{id}.toml"
triggers = ["release"]

[horizon_range]
min = 2
max = 4

[[guidance]]
rule = "{rule}"
provenance = "central:source:test:conventions.md"
classification = "ordinary"
"#
        ),
    )
    .unwrap();
}

#[test]
fn a_personal_declaration_is_in_force_with_no_project_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home/domains");
    declare(&home, "house-style", "the personal rule");

    let (domains, warnings) = load_domains_in(Some(&home), None);
    assert_eq!(
        domains.len(),
        1,
        "the root register is consulted on its own"
    );
    assert_eq!(domains[0].guidance[0].rule, "the personal rule");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn both_registers_contribute_when_their_ids_differ() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home/domains");
    let project = tmp.path().join("project");
    declare(&home, "house-style", "the personal rule");
    declare(
        &project.join(".aikit/domains"),
        "release",
        "the project rule",
    );

    let (domains, warnings) = load_domains_in(Some(&home), Some(&project));
    let mut ids: Vec<&str> = domains.iter().map(|d| d.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["domain/house-style", "domain/release"]);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn the_project_declaration_replaces_the_personal_one_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home/domains");
    let project = tmp.path().join("project");
    declare(&home, "release", "the personal rule");
    declare(
        &project.join(".aikit/domains"),
        "release",
        "the project rule",
    );

    let (domains, warnings) = load_domains_in(Some(&home), Some(&project));
    assert_eq!(domains.len(), 1, "same id is one domain, not two");
    assert_eq!(
        domains[0].guidance[0].rule, "the project rule",
        "the more specific declaration wins"
    );
    // Replacement, not merge: guidance neither author wrote must not appear.
    assert_eq!(domains[0].guidance.len(), 1);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("replaces the personal declaration")),
        "shadowing must be visible to whoever is debugging what arrived: {warnings:?}"
    );
}

#[test]
fn the_project_only_load_still_behaves_exactly_as_it_did() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    declare(
        &project.join(".aikit/domains"),
        "release",
        "the project rule",
    );

    let (domains, warnings) = load_domains(&project);
    assert_eq!(domains.len(), 1);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn an_absent_register_is_an_honest_absence_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (domains, warnings) = load_domains_in(
        Some(&tmp.path().join("nothing-here")),
        Some(&tmp.path().join("nor-here")),
    );
    assert!(domains.is_empty());
    assert!(warnings.is_empty(), "{warnings:?}");
}
