//! Existing-index admission uses the real installed CLI for capability discovery
//! and real owner metadata. It must never index, migrate, or prune a repository.
use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::SystemRunner;
use aikit_core::knowledge_code::CodeIndexProvider;
use aikit_core::resource::SourceRef;
use std::collections::BTreeMap;
use std::path::Path;

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, current: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        if !current.exists() {
            return;
        }
        for entry in std::fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else if path.is_file() {
                result.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn absent_index_is_not_created_by_native_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = GitNexusCodeIndexProvider::new(
        SystemRunner::probe().with_cwd(dir.path()),
        "native-absent",
        SourceRef::parse("source:project-code:native-absent").unwrap(),
        None,
    );
    assert!(provider.open_existing(dir.path()).is_err());
    assert!(!provider.status().indexed);
    assert!(!dir.path().join(".gitnexus").exists());
    assert!(provider
        .status()
        .detail
        .contains("explicit indexing required"));
}

#[test]
fn actual_owner_index_metadata_is_read_without_rebuild_or_rewrite() {
    let Some(root) = std::env::var_os("AIKIT_TEST_EXISTING_GITNEXUS_ROOT") else {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_GITNEXUS_EXISTING").is_none(),
            "strict existing-index gate requires a real already indexed repository"
        );
        eprintln!("existing-index native case omitted: AIKIT_TEST_EXISTING_GITNEXUS_ROOT absent");
        return;
    };
    let root = std::path::PathBuf::from(root);
    // Compare metadata, registry, and source Git status. The graph database may
    // be large: admission never opens it, and we compare its filesystem metadata.
    let storage = root.join(".gitnexus");
    let native_metadata: BTreeMap<_, _> = ["gitnexus.json", "meta.json"]
        .into_iter()
        .filter_map(|name| {
            std::fs::read(storage.join(name))
                .ok()
                .map(|bytes| (name, bytes))
        })
        .collect();
    assert!(
        !native_metadata.is_empty(),
        "test requires actual native index metadata"
    );
    let graph = ["lbug", "kuzu"]
        .into_iter()
        .find_map(|name| {
            std::fs::metadata(storage.join(name))
                .ok()
                .map(|meta| (name, meta.len(), meta.modified().unwrap()))
        })
        .expect("test requires existing native graph");
    let registry = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap()
        .join(".gitnexus/registry.json");
    let registry_before = std::fs::read(&registry).ok();
    let source_before = snapshot(&root.join(".git/refs"));
    let mut provider = GitNexusCodeIndexProvider::new(
        SystemRunner::probe().with_cwd(&root),
        "existing-native",
        SourceRef::parse("source:project-code:existing-native").unwrap(),
        None,
    );
    let status = provider
        .open_existing(&root)
        .expect("admit actual existing index");
    assert!(status.indexed);
    assert!(status
        .detail
        .contains("freshness against current source and branch is unverified"));
    assert!(status.detail.contains("no rebuild performed"));
    for (name, bytes) in native_metadata {
        assert_eq!(std::fs::read(storage.join(name)).unwrap(), bytes);
    }
    let after = std::fs::metadata(storage.join(graph.0)).unwrap();
    assert_eq!((after.len(), after.modified().unwrap()), (graph.1, graph.2));
    assert_eq!(std::fs::read(registry).ok(), registry_before);
    assert_eq!(snapshot(&root.join(".git/refs")), source_before);
}

#[test]
fn actual_native_query_uses_private_graph_and_registry() {
    use std::io::Read;
    fn digest(path: &Path) -> blake3::Hash {
        let mut file = std::fs::File::open(path).unwrap();
        let mut hash = blake3::Hasher::new();
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let n = file.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        hash.finalize()
    }
    let Some(root) = std::env::var_os("AIKIT_TEST_EXISTING_GITNEXUS_ROOT") else {
        assert!(std::env::var_os("AIKIT_REQUIRE_GITNEXUS_EXISTING").is_none());
        return;
    };
    let root = std::path::PathBuf::from(root);
    let storage = root.join(".gitnexus");
    let graph_before = digest(&storage.join("lbug"));
    let entries_before: std::collections::BTreeSet<_> = std::fs::read_dir(&storage)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let registry = std::env::var_os("GITNEXUS_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".gitnexus")
        })
        .join("registry.json");
    let registry_before = std::fs::read(&registry).ok();
    let mut provider = GitNexusCodeIndexProvider::new(
        SystemRunner::probe().with_cwd(&root),
        "real-query",
        SourceRef::parse("source:project-code:real-query").unwrap(),
        None,
    );
    provider
        .open_existing(&root)
        .expect("existing actual native graph");
    let hits = provider
        .search("action", 5)
        .expect("actual installed GitNexus query on isolated snapshot");
    assert!(
        !hits.is_empty(),
        "actual repository graph must return action search hits"
    );
    assert_eq!(digest(&storage.join("lbug")), graph_before);
    let entries_after: std::collections::BTreeSet<_> = std::fs::read_dir(&storage)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries_before, entries_after);
    assert_eq!(std::fs::read(registry).ok(), registry_before);
}
