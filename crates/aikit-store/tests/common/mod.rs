//! Fixtures that build **real** registries in **real** temporary directories.
//!
//! Nothing here fakes a filesystem. Every helper writes actual `manifest.toml`
//! files and actual payload trees, because the properties the store has to get
//! right — a content revision that moves when a payload byte moves, a bad
//! manifest that does not blind its neighbours, a generation that survives a
//! crash between materialization and the pointer swap — are all properties of
//! files, and a mock would assert only that this crate calls itself the way this
//! crate expects to be called.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::catalog::Catalog;
use aikit_core::trust::MemoryTrust;
use aikit_core::{
    CapsuleId, ContextDescriptor, LayerOrigin, PoolPatch, ProfileId, RegistrySource, ResolveRequest,
    ResolvedContext, ScopeKind, ScopeLayer, TrustState,
};

/// A registry under construction on disk.
pub struct RegistryFixture {
    root: PathBuf,
}

impl RegistryFixture {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        fs::create_dir_all(root.join("capsules")).unwrap();
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn capsule_dir(&self, id: &str) -> PathBuf {
        let id = CapsuleId::parse(id).unwrap();
        self.root.join(id.registry_path())
    }

    /// Write a script capsule with a single payload file.
    pub fn script(&self, id: &str) -> PathBuf {
        self.capsule(
            id,
            "script",
            "entry = \"payload/run.sh\"",
            "",
            &[("payload/run.sh", "#!/bin/sh\necho hi\n")],
        )
    }

    pub fn skill(&self, id: &str) -> PathBuf {
        self.capsule(
            id,
            "skill",
            "root = \"payload\"",
            "",
            &[("payload/SKILL.md", "# skill\n")],
        )
    }

    pub fn hook(&self, id: &str) -> PathBuf {
        self.capsule(
            id,
            "hook",
            "entry = \"payload/check\"\nevents = [\"PreToolUse\"]",
            "",
            &[("payload/check", "#!/bin/sh\nexit 0\n")],
        )
    }

    pub fn guidance(&self, id: &str) -> PathBuf {
        self.capsule(
            id,
            "guidance",
            "entry = \"payload/guidance.md\"",
            "",
            &[("payload/guidance.md", "Read the tests first.\n")],
        )
    }

    /// The general form. `top` is spliced above the kind table.
    pub fn capsule(
        &self,
        id: &str,
        kind: &str,
        section: &str,
        top: &str,
        files: &[(&str, &str)],
    ) -> PathBuf {
        let parsed = CapsuleId::parse(id).unwrap();
        let dir = self.root.join(parsed.registry_path());
        fs::create_dir_all(&dir).unwrap();
        let manifest = format!(
            "schema = 1\nid = \"{id}\"\nkind = \"{kind}\"\nname = \"{leaf}\"\n\
             description = \"Fixture {kind} {leaf}.\"\n{top}\n\n[{kind}]\n{section}\n",
            leaf = parsed.leaf()
        );
        fs::write(dir.join("manifest.toml"), manifest).unwrap();
        for (relative, contents) in files {
            let path = dir.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    /// Write a raw manifest, valid or not. Used to prove one broken file does not
    /// take the rest of the registry down with it.
    pub fn raw_capsule(&self, relative_dir: &str, manifest: &str) -> PathBuf {
        let dir = self.root.join("capsules").join(relative_dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.toml"), manifest).unwrap();
        dir
    }

    pub fn profile(&self, id: &str, body: &str) -> PathBuf {
        let parsed = ProfileId::parse(id).unwrap();
        let path = self.root.join(format!("profiles/{}.toml", parsed.path()));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("schema = 1\nid = \"{id}\"\n{body}\n")).unwrap();
        path
    }

    pub fn write_payload(&self, id: &str, relative: &str, contents: &str) {
        let path = self.capsule_dir(id).join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

/// Load the fixture registry and resolve a view with the named capsules enabled.
///
/// Uses the real loader and the real resolver, so a view built here has real
/// revisions, real capsule roots and a real resolution hash — which is what makes
/// the generation tests meaningful rather than a test of struct construction.
pub fn resolve_fixture(fixture: &RegistryFixture, enable: &[&str]) -> ResolvedContext {
    let load =
        aikit_store::registry::load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);

    let mut trust = MemoryTrust::default();
    for capsule in Catalog::capsules(&load.catalog) {
        trust.set(
            capsule.source.clone().unwrap(),
            capsule.id.clone(),
            capsule.revision.clone().unwrap(),
            TrustState::Reviewed,
        );
    }

    let request = ResolveRequest {
        context: ContextDescriptor::for_project(fixture.root()),
        layers: vec![ScopeLayer::new(
            ScopeKind::Project,
            LayerOrigin::new("tests/common"),
            PoolPatch {
                enable: enable.iter().map(|s| cid(s)).collect(),
                ..Default::default()
            },
        )],
        policy: Default::default(),
    };
    let view = aikit_core::resolve(&load.catalog, &trust, &request).unwrap();
    ResolvedContext {
        view,
        capsule_roots: load.catalog.capsule_roots(),
    }
}

pub fn cid(s: &str) -> CapsuleId {
    CapsuleId::parse(s).unwrap()
}

pub fn pid(s: &str) -> ProfileId {
    ProfileId::parse(s).unwrap()
}

/// Every regular file under `root`, as `(relative path, contents)`, sorted.
///
/// Used to compare two materialized trees for *logical* equality: a link-mode
/// generation and a copy-mode generation must present the same relative paths
/// with the same bytes, whatever the inodes say.
pub fn walk_contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(true)
        .sort_by_file_name()
    {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() && !entry.path_is_symlink() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(bytes) = fs::read(entry.path()) {
            out.push((relative, bytes));
        }
    }
    out.sort();
    out
}
