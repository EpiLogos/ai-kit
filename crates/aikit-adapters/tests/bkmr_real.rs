use std::collections::BTreeMap;

use aikit_adapters::bkmr::BkmrSourcePoolProvider;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::knowledge_source_pool::{
    material_for_actor, SourceBinding, SourceMaterial, SourcePool, SourcePoolProvider,
    SourceSearchMode, SourceVisibility, BKMR_GLADE_CONFORMANCE_VERSION,
};
use aikit_core::resource::{SourceRef, SourceRevision};
use tempfile::tempdir;

fn source(
    id: &str,
    title: &str,
    body: &str,
    tags: &[&str],
    visibility: SourceVisibility,
    owners: &[&str],
) -> SourceMaterial {
    SourceMaterial {
        binding: SourceBinding {
            source: SourceRef::parse(id).expect("fixture source ref"),
            revision: SourceRevision::parse(format!("revision:{id}"))
                .expect("fixture revision"),
            title: title.into(),
            tags: tags.iter().map(|tag| (*tag).into()).collect(),
            visibility,
            owners: owners.iter().map(|owner| (*owner).into()).collect(),
            media_type: "text/markdown".into(),
            locator: None,
            metadata: BTreeMap::new(),
        },
        body: body.into(),
    }
}

#[test]
fn real_bkmr_767_preserves_refs_capabilities_and_privacy_membrane() {
    let dir = tempdir().expect("temporary provider directory");
    let shared = source(
        "source:astronomy",
        "Astronomy",
        "Astronomy uses a telescope to observe distant galaxies and quasars.",
        &["astronomy", "science"],
        SourceVisibility::Team,
        &[],
    );
    let private = source(
        "source:private",
        "Private",
        "The private obsidian narwhal phrase belongs only to Alex.",
        &["private"],
        SourceVisibility::Personal,
        &["alex"],
    );
    let pool = SourcePool::new(
        "source-pool:real-bkmr",
        vec![shared.binding.clone(), private.binding.clone()],
    )
    .expect("valid source pool");
    let material = vec![shared.clone(), private.clone()];

    let mut provider = BkmrSourcePoolProvider::new(SystemRunner::new(), dir.path().join("bkmr.db"), false);
    let status = provider.status();
    if !status.available {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_none(),
            "AIKIT_REQUIRE_BKMR_REAL is set but bkmr is unavailable: {}",
            status.detail
        );
        return;
    }

    // Detection truth: the reported version must be what the installed CLI
    // itself reports. A real-environment test never mandates a pinned version;
    // it proves detection is truthful on whatever is installed.
    let probe = SystemRunner::new()
        .run(&["bkmr".into(), "--version".into()])
        .expect("bkmr --version probe");
    let reported = format!("{} {}", probe.stdout, probe.stderr);
    let version = status
        .version
        .as_deref()
        .expect("status reports the installed bkmr version");
    assert!(
        reported.contains(version),
        "detected version {version} must come from the CLI's own --version output"
    );
    // The conformance pin is tested-version metadata, not a mandate: drift is
    // reported, never enforced.
    assert_eq!(
        status.tested_version.as_deref(),
        Some(BKMR_GLADE_CONFORMANCE_VERSION)
    );
    assert_eq!(status.version_drift, version != BKMR_GLADE_CONFORMANCE_VERSION);

    // The behavioural contract below runs fulltext JSON search with tag
    // filtering; both are detected from the installed CLI. When either is
    // absent there is nothing honest to verify, so the test skips like any
    // unavailable provider. The rest of the capability matrix is a detected
    // report, not a law.
    let missing: Vec<&str> = [
        ("fulltext", status.capabilities.fulltext),
        ("tags", status.capabilities.tags),
    ]
    .iter()
    .filter(|(_, present)| !*present)
    .map(|(faculty, _)| *faculty)
    .collect();
    if !missing.is_empty() {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_none(),
            "AIKIT_REQUIRE_BKMR_REAL is set but the installed bkmr lacks required faculties {missing:?}: {}",
            status.detail
        );
        return;
    }

    // The strict conformance matrix is opt-in: set AIKIT_BKMR_CONFORMANCE in an
    // environment pinned to exactly the tested version (e.g. CI).
    if std::env::var_os("AIKIT_BKMR_CONFORMANCE").is_some() {
        assert_eq!(version, BKMR_GLADE_CONFORMANCE_VERSION);
        assert!(!status.version_drift);
        assert!(status.capabilities.fuzzy_interactive);
        assert!(!status.capabilities.semantic);
        assert!(!status.capabilities.hybrid);
    }

    let frank_material = material_for_actor(&pool, &material, Some("frank"), true)
        .expect("privacy-filtered provider material");
    assert_eq!(frank_material.len(), 1);
    assert_eq!(frank_material[0].binding.source.as_str(), "source:astronomy");
    provider.rebuild(&frank_material).expect("build frank provider view");

    let hits = provider
        .search("quasars", SourceSearchMode::Fulltext, &[], 20)
        .expect("fulltext search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source.as_str(), "source:astronomy");
    assert_eq!(hits[0].provider.as_str(), "provider/source-pool/bkmr");
    assert!(hits[0].provider_binding.is_some());
    assert!(provider
        .search("narwhal", SourceSearchMode::Fulltext, &[], 20)
        .expect("private search")
        .is_empty());

    let tagged = provider
        .search(
            "telescope",
            SourceSearchMode::Fulltext,
            &["astronomy".into()],
            20,
        )
        .expect("tagged search");
    assert_eq!(tagged.len(), 1);
    assert_eq!(tagged[0].source.as_str(), "source:astronomy");
    assert!(provider
        .search(
            "telescope",
            SourceSearchMode::Fulltext,
            &["private".into()],
            20,
        )
        .expect("non-matching tag search")
        .is_empty());

    provider.rebuild(&frank_material).expect("rebuild provider view");
    let rebuilt = provider
        .search("quasars", SourceSearchMode::Fulltext, &[], 20)
        .expect("search after rebuild");
    assert_eq!(rebuilt[0].source.as_str(), "source:astronomy");

    let alex_material = material_for_actor(&pool, &material, Some("alex"), true)
        .expect("alex provider material");
    assert_eq!(alex_material.len(), 2);
    provider.rebuild(&alex_material).expect("build alex provider view");
    let private_hits = provider
        .search("narwhal", SourceSearchMode::Fulltext, &[], 20)
        .expect("alex private search");
    assert_eq!(private_hits.len(), 1);
    assert_eq!(private_hits[0].source.as_str(), "source:private");
}
