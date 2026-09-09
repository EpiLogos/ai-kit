//! W4 / CASE 06 (minimal half) — the wiki's own authoring conventions,
//! declared as data, arrive before a write to wiki Markdown.
//!
//! The delivery mechanism is CASE 05's (`file_context::run` over any
//! PreToolUse that carries an explicit `tool_input.file_path`, Write and
//! Edit included). What is under test here is the *declaration*:
//! `.aikit/domains/wiki-inhabitation.toml`, this repository's own project
//! layer, is a valid `aikit.knowledge-domain/v1`, is path-addressed with no
//! prompt trigger, carries provenance on every rule, and is delivered for a
//! wiki/NOW Markdown path and for nothing else.
//!
//! The declaration under test is the shipped file, not a fixture copy — a
//! test that invented its own domain would prove the engine and leave the
//! declaration unpinned.

use std::{fs, path::PathBuf};

use aikit_cli::domain_activation::load_domains;
use aikit_cli::file_context::{load_project_wiki, run};
use aikit_core::domain::{KnowledgeDomain, PressureClass};
use aikit_store::index::Index;

/// CASE 04 landed between this slice's branch point and main: the reaction now
/// returns classified `Block`s (header / standing / ordinary) so context
/// pressure can bound ordinary payload without touching standing guidance.
/// These tests assert on what a session actually sees, which is the rendered
/// block — the classification itself is asserted separately, from the
/// declaration.
fn rendered(
    result: (Vec<aikit_core::pressure::Block>, Vec<String>),
) -> (Vec<String>, Vec<String>) {
    (
        result
            .0
            .iter()
            .map(aikit_core::pressure::Block::render)
            .collect(),
        result.1,
    )
}


/// The shipped declaration, read from this repository's project layer.
fn declaration_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.aikit/domains/wiki-inhabitation.toml")
}

fn declaration() -> KnowledgeDomain {
    let text = fs::read_to_string(declaration_path()).expect("the declaration ships in-repo");
    KnowledgeDomain::from_toml_str(&text).expect("the shipped declaration parses and validates")
}

/// A throwaway project carrying the *shipped* declaration and no wiki, so
/// the only thing that can arrive is the domain guidance.
fn project() -> (PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap().keep();
    let domains = root.join(".aikit/domains");
    fs::create_dir_all(&domains).unwrap();
    fs::copy(declaration_path(), domains.join("wiki-inhabitation.toml")).unwrap();
    let db = root.join("state/aikit.sqlite3");
    drop(Index::open(&db).unwrap());
    (root, db)
}

#[test]
fn the_shipped_declaration_is_valid_path_addressed_and_carries_provenance() {
    let domain = declaration();
    assert_eq!(domain.id, "domain/wiki-inhabitation");
    assert!(
        domain.triggers.is_empty(),
        "the wiki-inhabitation domain is addressed by path, not by prompt"
    );
    assert!(!domain.path_patterns.is_empty());
    assert!(!domain.guidance.is_empty());
    for rule in &domain.guidance {
        assert!(
            !rule.provenance.trim().is_empty(),
            "every rule names the file it was restated from: {}",
            rule.rule
        );
    }
    // PROGRAMME §6: a domain cannot address material outside its horizon
    // range. Wiki authoring is @2 readings in @3 document form.
    let range = domain.horizon_range.expect("the domain declares its horizon");
    assert!(range.admits(2) && range.admits(3));
    assert!(!range.admits(1), "@1 personal ground is out of this domain's reach");
    assert!(!range.admits(5));
}

/// Standing is the unrecoverable-violation exemption, not a way to shout.
/// One rule holds it: the authorship boundary, whose violation writes
/// generated material into the human's authored account and cannot be
/// undone from inside the wiki.
#[test]
fn exactly_one_rule_is_standing_and_it_is_the_authorship_boundary() {
    let domain = declaration();
    let standing: Vec<_> = domain
        .guidance
        .iter()
        .filter(|rule| rule.pressure_class == PressureClass::Standing)
        .collect();
    assert_eq!(
        standing.len(),
        1,
        "a file where everything is standing is a file where nothing is: {:?}",
        standing.iter().map(|rule| &rule.rule).collect::<Vec<_>>()
    );
    assert!(
        standing[0].rule.contains("authored source"),
        "{}",
        standing[0].rule
    );
    assert!(
        standing[0].rule.contains("Recognition"),
        "the standing rule names the only promotion door: {}",
        standing[0].rule
    );
}

#[test]
fn a_write_to_wiki_markdown_arrives_with_the_authoring_conventions() {
    let (root, db) = project();
    let index = Index::open(&db).unwrap();
    let (domains, warnings) = load_domains(&root);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(domains.len(), 1);

    let path = root
        .join("ProjectCentral/agents/wiki/returns/a-learning-2026-09-09.md")
        .to_string_lossy()
        .into_owned();
    let (blocks, warnings) = rendered(run(
        &index,
        "session-wiki-inhabitation",
        &root,
        &path,
        &domains,
        load_project_wiki(&root).0,
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    let block = &blocks[0];
    assert!(
        block.starts_with(
            "[continuity/file-context] domain domain/wiki-inhabitation armed for \
             ProjectCentral/agents/wiki/returns/a-learning-2026-09-09.md"
        ),
        "{block}"
    );
    assert!(block.contains("horizon: @2–@3"), "{block}");
    assert!(
        block.contains("source: central:source:project:ai-kit:.aikit/domains/wiki-inhabitation.toml"),
        "{block}"
    );
    assert!(block.contains("standing rules reasserted"), "{block}");
    assert!(
        block.contains("[standing — dedup-exempt by classification]"),
        "{block}"
    );
    assert!(block.contains("[ordinary]"), "{block}");
    assert!(
        block.contains("becomes authored source only through human Recognition"),
        "{block}"
    );
}

/// The NOW field is addressed too — W10 rev 4 §3 makes it a localised wiki
/// inside the same system, and `**` crossing `/` is why the direct-child
/// patterns are declared separately.
#[test]
fn the_now_field_and_direct_children_are_addressed_as_well() {
    let (root, db) = project();
    let index = Index::open(&db).unwrap();
    let (domains, _) = load_domains(&root);
    for relative in [
        "ProjectCentral/now/flows/a-flow-2026-09-09-1200.md",
        "ProjectCentral/agents/wiki/notes.md",
    ] {
        let path = root.join(relative).to_string_lossy().into_owned();
        let (blocks, _) = rendered(run(
            &index,
            &format!("session-{relative}"),
            &root,
            &path,
            &domains,
            Vec::new(),
        ));
        assert_eq!(blocks.len(), 1, "{relative}: {blocks:?}");
        assert!(blocks[0].contains(&format!("armed for {relative}")), "{:?}", blocks[0]);
    }
}

/// The negative half: a path the patterns do not address gets nothing. A
/// Rust source file, a Markdown file outside the two fields, and the wiki's
/// own JSON carrier are all silent.
#[test]
fn a_write_outside_the_declared_patterns_gets_nothing() {
    let (root, db) = project();
    let index = Index::open(&db).unwrap();
    let (domains, _) = load_domains(&root);
    for relative in [
        "crates/aikit-core/src/domain.rs",
        "docs/v2/21-PROJECT-REFLECTION-AND-LOCAL-ARTICULATION.md",
        "README.md",
        "ProjectCentral/user/capability-matrix.md",
        "ProjectCentral/agents/wiki/wiki.json",
    ] {
        let path = root.join(relative).to_string_lossy().into_owned();
        let (blocks, warnings) = rendered(run(
            &index,
            &format!("session-negative-{relative}"),
            &root,
            &path,
            &domains,
            Vec::new(),
        ));
        assert!(
            blocks.is_empty(),
            "{relative} is not wiki Markdown and must receive nothing: {blocks:?}"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

/// The dedup law holds for this declaration like any other: the ordinary
/// conventions arrive once per file, the authorship boundary every time.
#[test]
fn the_conventions_dedup_but_the_authorship_boundary_reasserts() {
    let (root, db) = project();
    let index = Index::open(&db).unwrap();
    let (domains, _) = load_domains(&root);
    let scope = "session-dedup";
    let path = root
        .join("ProjectCentral/agents/wiki/returns/a-learning-2026-09-09.md")
        .to_string_lossy()
        .into_owned();

    let (first, _) = rendered(run(&index, scope, &root, &path, &domains, Vec::new()));
    assert!(first[0].contains("[ordinary]"), "{:?}", first[0]);

    let (second, _) = rendered(run(&index, scope, &root, &path, &domains, Vec::new()));
    assert_eq!(second.len(), 1, "{second:?}");
    assert!(second[0].contains("ordinary payload deduped"), "{:?}", second[0]);
    assert!(!second[0].contains("[ordinary]"), "{:?}", second[0]);
    assert!(
        second[0].contains("becomes authored source only through human Recognition"),
        "the authorship boundary is never deduped away: {:?}",
        second[0]
    );
}

/// The classification earns its second exemption.
///
/// When this declaration was written, `standing` meant one thing: exempt from
/// dedup. CASE 04 landed while the branch waited, and it now means two —
/// exempt from dedup *and* from context pressure, so the rule arrives even at
/// CRITICAL when every ordinary line has been withheld. The authorship
/// boundary was classified `standing` on exactly that reasoning (generated
/// prose in the human's authored account cannot be undone), so the reasoning
/// is worth holding to evidence rather than leaving as an argument.
#[test]
fn the_authorship_boundary_survives_critical_pressure_and_the_conventions_do_not() {
    use aikit_core::pressure::{bound, Pressure};

    let (root, db) = project();
    let index = Index::open(&db).unwrap();
    let (domains, _) = load_domains(&root);
    let path = root.join("ProjectCentral/agents/wiki/returns/a-learning.md");
    let (blocks, _) = run(
        &index,
        "pressure-scope",
        &root,
        &path.to_string_lossy(),
        &domains,
        Vec::new(),
    );

    let critical = bound(&blocks, Pressure::Critical);
    let text = critical.blocks.join("\n");
    assert!(
        text.contains("becomes authored source only through human Recognition"),
        "the authorship boundary must reach a session whose window is nearly full: {text}"
    );
    assert!(
        !text.contains("[ordinary]"),
        "the ordinary conventions are bounded away at CRITICAL: {text}"
    );
    assert!(
        text.contains("withheld"),
        "and the bound is disclosed rather than silent: {text}"
    );

    // FRESH is the control: with room to spare, the whole declaration arrives.
    let fresh = bound(&blocks, Pressure::Fresh).blocks.join("\n");
    assert!(fresh.contains("[ordinary]"), "{fresh}");
    assert!(fresh.contains("becomes authored source only through human Recognition"));
}
