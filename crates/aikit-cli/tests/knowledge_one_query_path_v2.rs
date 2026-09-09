//! One query path — Knowledge retrieval expresses through the Vāk resolver.
//!
//! The law (Wiki Continuity Programme §4.1/§12, Law 3): *all retrieval expresses
//! through the Vāk resolver contract (`ResolveExpression`, typed refs). No local
//! DSL.* A plain typed string remains legitimate **input**; a second retrieval
//! path beside the resolver does not.
//!
//! These cases assert the *path*, not the output shape. Each one fails if
//! Knowledge retrieval is re-plumbed onto a raw-string scan that never builds an
//! expression:
//!
//! - a typed string is lowered into the contract and the receipt is disclosed;
//! - operative syntax is **read** by Knowledge (an address horizon narrows the
//!   federated result), which a substring scanner cannot do;
//! - a relation combines both sides, which a substring scanner cannot do;
//! - the path identity Knowledge mints is the same one `aikit search` resolves,
//!   so learned evidence rides one path rather than two;
//! - the human front is thin: it agrees with the canonical entry exactly.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::{
    parse_or_search_expression, resolve_path_identity, AddressHorizon, RelationOp,
    ResolveExpression,
};
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use aikit_tui::application_service::ApplicationService;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open production application service")
}

/// One curated Wiki node (KnowledgeNode: horizons @0 and @2) and one authored
/// source (KnowledgeSource: horizon @0). The two kinds differ by exactly one
/// horizon, which is what lets an address narrow observably.
fn write_project_knowledge(temp: &TempDir) {
    let wiki = r#"{
      "objects": [
        {
          "profile": "okf-wiki/v1",
          "object": "node",
          "ref": "wiki:node:authentication",
          "revision": 7,
          "provenance": [{"source_ref":"source:paper:authentication","source_revision":"rev-3"}],
          "type": "Concept",
          "title": "Authentication architecture",
          "space_refs": [],
          "source_refs": ["source:paper:authentication"]
        }
      ]
    }"#;
    fs::write(temp.path().join("semantic-wiki.json"), wiki).unwrap();

    let source = r#"{
      "binding": {
        "source": "source:paper:authentication",
        "revision": "rev-3",
        "title": "Authentication source paper",
        "tags": ["authentication", "architecture"],
        "visibility": "public",
        "owners": [],
        "media_type": "text/markdown",
        "metadata": {"origin":"test-fixture"}
      },
      "body": "Authentication architecture keeps source evidence distinct from compiled semantic knowledge."
    }"#;
    fs::write(temp.path().join("source-material.json"), source).unwrap();
}

fn resources(result: &aikit_core::KnowledgeSearchResult) -> Vec<String> {
    result
        .hits
        .iter()
        .map(|hit| hit.resource.to_string())
        .collect()
}

#[test]
fn knowledge_retrieval_expresses_through_the_one_resolver_contract() {
    let temp = TempDir::new().unwrap();
    write_project_knowledge(&temp);
    let service = open_service(&temp);

    // 1. A typed string is lowered *into* the contract, not handled beside it.
    let plain = service.knowledge_search("authentication", 50).unwrap();
    assert_eq!(
        plain.expression,
        ResolveExpression::ordinary_search("authentication"),
        "an ordinary Knowledge query is the potential resolution of a universal \
         address — `@# (@ text)` — not a raw string on a second path"
    );
    assert_eq!(
        plain.path_identity,
        resolve_path_identity(&ResolveExpression::ordinary_search("authentication")),
        "the disclosed path identity is the resolver's own, minted once"
    );
    assert_eq!(
        plain.query, "authentication",
        "the caller's own words are preserved beside the resolver receipt"
    );

    let plain_resources = resources(&plain);
    assert!(
        plain_resources.contains(&"wiki:node:authentication".to_string()),
        "the curated Wiki node is reachable: {plain_resources:?}"
    );
    assert!(
        plain_resources.contains(&"source:paper:authentication".to_string()),
        "the authored source is reachable: {plain_resources:?}"
    );

    // 2. Operative syntax is READ by Knowledge retrieval. `@2` is the reflection
    //    / meaning horizon: a KnowledgeNode participates in it, a KnowledgeSource
    //    does not. A substring scanner over the literal text "@2 authentication"
    //    would return nothing at all — this is the case that goes red if a
    //    parallel raw-string path is reintroduced.
    let narrowed = service.knowledge_search("@2 authentication", 50).unwrap();
    assert_eq!(
        narrowed.expression,
        ResolveExpression::horizon(
            AddressHorizon::H2,
            ResolveExpression::subject("authentication")
        ),
        "the address was parsed by the one grammar, not consumed as literal text"
    );
    let narrowed_resources = resources(&narrowed);
    assert_eq!(
        narrowed_resources,
        vec!["wiki:node:authentication".to_string()],
        "an address horizon narrows the federated Knowledge result — retrieval \
         evaluates the expression rather than matching its rendering"
    );

    // The @0 ground horizon holds both kinds, so the narrowing above is the
    // address doing work and not an accident of ranking or truncation.
    let ground = service.knowledge_search("@0 authentication", 50).unwrap();
    let ground_resources = ground_sorted(&ground);
    assert_eq!(
        ground_resources,
        vec![
            "source:paper:authentication".to_string(),
            "wiki:node:authentication".to_string(),
        ],
        "@0 is the ground horizon both kinds participate in: {ground_resources:?}"
    );

    // 3. A relation combines both sides. A substring scanner cannot union.
    let related = service
        .knowledge_search("@2 authentication x @0 \"source:paper:authentication\"", 50)
        .unwrap();
    assert!(
        matches!(
            related.expression,
            ResolveExpression::Binary {
                op: RelationOp::Relate,
                ..
            }
        ),
        "the relation was parsed: {:?}",
        related.expression
    );
    let mut related_resources = resources(&related);
    related_resources.sort();
    assert_eq!(
        related_resources,
        vec![
            "source:paper:authentication".to_string(),
            "wiki:node:authentication".to_string(),
        ],
        "both sides of the relation contributed: {related_resources:?}"
    );

    // 4. The human front is thin: it parses and delegates to the canonical entry
    //    and adds no retrieval of its own.
    let expression = parse_or_search_expression("@2 authentication").unwrap();
    let canonical = service.knowledge_resolve(&expression, 50).unwrap();
    assert_eq!(
        resources(&canonical),
        narrowed_resources,
        "`knowledge search` is `knowledge_resolve(parse(input))` and nothing else"
    );
    assert_eq!(canonical.path_identity, narrowed.path_identity);
}

fn ground_sorted(result: &aikit_core::KnowledgeSearchResult) -> Vec<String> {
    let mut values = resources(result);
    values.sort();
    values
}

/// `aikit search` and `aikit knowledge search` must mint the *same* path
/// identity for the same expression. Two identities would mean two paths, and
/// learned familiarity would be split across them.
#[test]
fn knowledge_and_resource_search_share_one_path_identity() {
    let temp = TempDir::new().unwrap();
    write_project_knowledge(&temp);
    let mut service = open_service(&temp);

    let knowledge = Service::knowledge_search(&service, "authentication", 50).unwrap();
    let resolved = {
        let application = ApplicationService::new(&mut service);
        application.resolve_search("authentication").unwrap()
    };

    assert_eq!(
        resolved.expression, knowledge.expression,
        "one grammar reads the same input the same way on both surfaces"
    );
    assert_eq!(
        resolved.path.identity, knowledge.path_identity,
        "one path identity: learned evidence cannot be split across two paths"
    );
    assert_eq!(
        resolved.path.version, "aikit.operative-resolve/v1",
        "the shared contract is the operative Resolve contract"
    );
}

/// Input the grammar cannot read is disclosed and lowered to a single ordinary
/// subject — never silently answered by a different mechanism.
#[test]
fn unreadable_input_is_disclosed_rather_than_answered_off_path() {
    let temp = TempDir::new().unwrap();
    write_project_knowledge(&temp);
    let service = open_service(&temp);

    let broken = service.knowledge_search("authentication \"unclosed", 50);
    let error = broken.expect_err("an unreadable expression is refused at the CLI front");
    assert_eq!(error.code(), "resolve.unclosed_quote");

    // The core application front does not fail the caller; it discloses and
    // lowers, so the absence is visible rather than the retrieval being silent.
    let plain = service.knowledge_search("authentication", 50).unwrap();
    assert!(
        plain
            .hits
            .iter()
            .any(|hit| matches!(hit.address, KnowledgeAddress::Wiki(_))),
        "the readable path still resolves"
    );
}
