//! The knowledge result cache seam: repeat knowledge reads answer from the
//! suite's Redis instead of re-materialising the whole horizon.
//!
//! A Central-backed knowledge runtime is deliberately invalidated between
//! calls, and no index state survives between CLI processes, so every read
//! pays full materialisation. This seam wraps the *pure* compute of the
//! knowledge reads — the parts whose answer is a function of the ground and
//! of what was asked — and remembers the answer under an operation key that
//! already contains the basis digest: the answer is keyed by the state of
//! the world it was computed from, so a changed world is simply a different
//! key. Side effects (learned accessibility, search-hit memory, familiarity
//! observations) stay outside the cached region and run on every call, hit
//! or miss.
//!
//! The basis is a bounded stat digest of the horizon's *source* material —
//! the Control wiki, governance and user ground, each Work project's
//! ProjectCentral, the git HEAD each project stands on and its GitNexus
//! index state, and the bkmr stores. Derived state is excluded on purpose:
//! a basis that watched AIKit's own writes — its caches, or the familiarity
//! index every invocation's usage events land in — would be invalidated by
//! the act of asking, and would never answer twice. Two scopes decide how
//! much of the moving field counts: search and graph answer from everything
//! (`Full`, NOW clearings included); relations, reads and document reads
//! are assembled from standing material only, so they key on the `Ground`
//! scope and stay servable while lanes return to NOW all around them.
//! Live session journals are never watched. Anything the walk cannot read
//! cleanly poisons the basis, and a poisoned basis computes without the
//! cache rather than answering from it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use aikit_core::secret_ref::SecretResolver;
use aikit_core::{AikitError, Result, SecretValue};
use aikit_store::knowledge_cache::KnowledgeCacheStore;
use aikit_store::AikitHome;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::Service;

/// Cached answers live this long. The basis does the real invalidation; the
/// TTL only bounds the damage of an input the basis does not stat.
const KNOWLEDGE_CACHE_TTL_SECONDS: u64 = 15 * 60;
/// The operator's switch: `AIKIT_KNOWLEDGE_RESULT_CACHE=off` disables the
/// cache without a rebuild, for as long as the env var is set.
const DISABLE_ENV: &str = "AIKIT_KNOWLEDGE_RESULT_CACHE";
const BASIS_SCHEMA: &str = "aikit.knowledge-basis/v1";
/// The walk is bounded; exceeding the bound poisons the basis (always-miss)
/// rather than silently answering from a partially observed ground.
const MAX_BASIS_ENTRIES: usize = 50_000;
/// Directory symlinks are not followed this deep; a cycle must not hang a
/// knowledge read.
const MAX_BASIS_DEPTH: usize = 24;

static CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static CACHE_COMPUTES: AtomicUsize = AtomicUsize::new(0);
static CACHE_DEGRADED: OnceLock<String> = OnceLock::new();

fn record_degraded(reason: String) {
    let _ = CACHE_DEGRADED.set(reason);
}

/// The operation key prefix for an address-addressed read: the operation,
/// the address in its canonical serial form, and any op-specific suffix.
pub(super) fn address_cache_key(
    operation: &str,
    address: &aikit_core::KnowledgeAddress,
    suffix: &str,
) -> String {
    let address_key = serde_json::to_string(address).unwrap_or_else(|_| format!("{address:?}"));
    format!("{operation}\x1f{address_key}{suffix}")
}

/// A Redis-backed knowledge cache, if this AIKit home names one and it can
/// be opened. `None` takes the cache out of the path for this invocation;
/// the reason is recorded for `knowledge status`.
struct KnowledgeResultCache {
    store: KnowledgeCacheStore,
    secret: Option<SecretValue>,
}

impl KnowledgeResultCache {
    fn discover(home: &AikitHome) -> Option<Self> {
        let path = crate::inhabitation::world_redis_config_path(None, Some(home))?;
        let config = match crate::inhabitation::load_redis_config(&path) {
            Ok(config) => config,
            Err(error) => {
                record_degraded(format!("config {} unreadable: {error}", path.display()));
                return None;
            }
        };
        let secret = match config.credential_ref.as_ref() {
            Some(reference) => {
                match aikit_adapters::secret_resolver::SuiteSecretResolver::default()
                    .resolve(reference)
                {
                    Ok(secret) => Some(secret),
                    Err(error) => {
                        record_degraded(format!("credential unresolvable: {error}"));
                        return None;
                    }
                }
            }
            None => None,
        };
        match KnowledgeCacheStore::new(config) {
            Ok(store) => Some(Self { store, secret }),
            Err(error) => {
                record_degraded(format!("store refused: {error}"));
                None
            }
        }
    }

    fn get(&self, operation_key: &str) -> Option<String> {
        match self.store.get(self.secret.as_ref(), operation_key) {
            Ok(payload) => payload,
            Err(error) => {
                record_degraded(format!("get failed: {error}"));
                None
            }
        }
    }

    fn put(&self, operation_key: &str, payload: &str) {
        let outcome = self.store.put(
            self.secret.as_ref(),
            operation_key,
            payload,
            KNOWLEDGE_CACHE_TTL_SECONDS,
        );
        if let Err(error) = outcome {
            // An oversized payload is a normal skip, not an outage: the next
            // caller computes and tries again.
            if error.code() != "knowledge_cache.payload_bounds" {
                record_degraded(format!("put failed: {error}"));
            }
        }
    }
}

/// Which slice of the ground an operation's answer can actually see.
///
/// `Full` watches the whole live field, NOW clearings included: a search or
/// a graph answers from everything. `Ground` watches the world's standing
/// material but not the NOW field's moving records: a relation, read or
/// document answer is assembled from wiki, sources and the code index —
/// NOW churn cannot change it, so NOW churn must not invalidate it. On a
/// working machine the NOW field moves many times an hour; a
/// relations call keyed on the full field would recompute for exactly the
/// parallel-work hours this cache exists to serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BasisScope {
    Full,
    Ground,
}

/// One basis digest per (root, central root, scope) per process: several
/// knowledge reads in one invocation answer against one consistent reading
/// of the ground instead of each snapshotting it separately.
fn knowledge_basis(root: &Path, central_root: Option<&Path>, scope: BasisScope) -> Result<String> {
    static BASIS_MEMO: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    let memo_key = format!(
        "{}\x1f{}\x1f{:?}",
        root.display(),
        central_root
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        scope
    );
    let memo = BASIS_MEMO.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(digest) = memo.lock().ok().and_then(|m| m.get(&memo_key).cloned()) {
        return Ok(digest);
    }
    let digest = compute_knowledge_basis(root, central_root, scope)?;
    if let Ok(mut m) = memo.lock() {
        m.insert(memo_key, digest.clone());
    }
    Ok(digest)
}

fn compute_knowledge_basis(
    root: &Path,
    central_root: Option<&Path>,
    scope: BasisScope,
) -> Result<String> {
    let mut entries: Vec<(String, u64, u128)> = Vec::new();
    let mut inputs: Vec<PathBuf> = Vec::new();
    match central_root {
        Some(central) => {
            // The owner's live ground: the agent field (wiki, governance,
            // NOW) and the authored user ground. Absence of a named input is
            // itself a state of the world and hashes as one. The NOW field
            // joins only a Full-basis answer: Ground answers are assembled
            // from standing material, so the field's moving records cannot
            // invalidate them.
            if scope == BasisScope::Full {
                inputs.push(central.join("Control/agents/now"));
            }
            inputs.push(central.join("Control/agents/wiki"));
            inputs.push(central.join("Control/agents/governance"));
            inputs.push(central.join("Control/user"));
            let mut work_projects: Vec<PathBuf> = std::fs::read_dir(central.join("Work"))
                .map(|reads| {
                    reads
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .collect()
                })
                .unwrap_or_default();
            work_projects.sort();
            for project in work_projects {
                inputs.push(project.join("ProjectCentral"));
                let git_dir = project.join(".git");
                match std::fs::symlink_metadata(&git_dir) {
                    // A primary checkout carries its history in a `.git`
                    // directory. What an answer can see is the checked-out
                    // working tree: HEAD, and the branch HEAD names. Sibling
                    // branches and remote refs move with parallel lanes and
                    // fetches and must not invalidate anything.
                    Ok(meta) if meta.is_dir() => {
                        inputs.push(git_dir.join("HEAD"));
                        if let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) {
                            if let Some(branch) = head.trim().strip_prefix("ref: refs/heads/") {
                                inputs.push(git_dir.join("refs/heads").join(branch));
                            }
                            // A detached HEAD is its own state: the sha in
                            // the file is what the tree stands on, and any
                            // move rewrites the file.
                        }
                    }
                    // A linked worktree only carries a pointer file; the
                    // pointer's content is the state visible from here.
                    Ok(_) => inputs.push(git_dir),
                    Err(_) => inputs.push(git_dir.join("HEAD")),
                }
                // The code index state: a re-indexed repository answers
                // differently through the code lens.
                inputs.push(project.join(".gitnexus"));
            }
        }
        None => inputs.push(root.to_path_buf()),
    }
    // bkmr stores and their configuration are live inputs of answers too.
    let bkmr_config_dir = aikit_adapters::bkmr::bkmr_config_dir();
    inputs.push(bkmr_config_dir.clone());
    for (path, len, modified) in bkmr_store_states(&bkmr_config_dir) {
        entries.push((format!("bkmr-store\x00{path}"), len, modified));
    }

    for input in &inputs {
        match std::fs::symlink_metadata(input) {
            Ok(meta) if meta.is_dir() => walk_tree(input, &mut entries, 0)?,
            Ok(meta) => note_entry(input, meta, &mut entries)?,
            Err(_) => entries.push((format!("{}\x00absent", input.display()), 0, 0)),
        }
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(BASIS_SCHEMA.as_bytes());
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(
        std::env::var("AIKIT_GITNEXUS_BIN")
            .unwrap_or_default()
            .as_bytes(),
    );
    entries.sort();
    for (path, len, modified) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(&len.to_le_bytes());
        hasher.update(&modified.to_le_bytes());
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The database files behind the discovered bkmr stores, as (path, size,
/// mtime) — the pool's operative state.
fn bkmr_store_states(config_dir: &Path) -> Vec<(String, u64, u128)> {
    aikit_adapters::bkmr::discover_bkmr_stores(config_dir)
        .into_iter()
        .filter_map(|store| {
            let meta = std::fs::symlink_metadata(&store.path).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Some((store.path.display().to_string(), meta.len(), modified))
        })
        .collect()
}

fn note_entry(
    path: &Path,
    meta: std::fs::Metadata,
    entries: &mut Vec<(String, u64, u128)>,
) -> Result<()> {
    if entries.len() >= MAX_BASIS_ENTRIES {
        return Err(AikitError::new(
            "knowledge.basis_overflow",
            format!("basis walk exceeded {MAX_BASIS_ENTRIES} entries"),
        ));
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    entries.push((path.display().to_string(), meta.len(), modified));
    Ok(())
}

/// Stat-only, sorted, bounded. Derived directories are skipped because a
/// basis that watched AIKit's own caches would never stabilise, and
/// directory symlinks are not followed because a cycle must not hang a
/// knowledge read.
fn walk_tree(dir: &Path, entries: &mut Vec<(String, u64, u128)>, depth: usize) -> Result<()> {
    if depth > MAX_BASIS_DEPTH {
        return Err(AikitError::new(
            "knowledge.basis_unreadable",
            format!("basis walk exceeded depth at {}", dir.display()),
        ));
    }
    let reads = std::fs::read_dir(dir).map_err(|error| {
        AikitError::new(
            "knowledge.basis_unreadable",
            format!("{}: {error}", dir.display()),
        )
    })?;
    let mut children: Vec<PathBuf> = reads
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    children.sort();
    for child in children {
        let name = child
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if matches!(
            name.as_str(),
            "target" | "node_modules" | ".git" | ".DS_Store"
        ) {
            // A repository's head is fed explicitly by the caller; the
            // object database under `.git` is history, not the ground.
            continue;
        }
        if name == "flows" && dir.file_name().is_some_and(|n| n == "now") {
            // Live session journals: telemetry that other agents append to
            // continuously on a working machine. They are deliberately
            // outside the basis — watching them would keep the cache from
            // ever answering while any lane is live. The curated NOW
            // material stays watched in a Full basis; the journals age into
            // answers within the TTL like any other unwatched input, and
            // `AIKIT_KNOWLEDGE_RESULT_CACHE=off` restores strictly-live
            // behaviour when it matters.
            continue;
        }
        let meta = std::fs::symlink_metadata(&child).map_err(|error| {
            AikitError::new(
                "knowledge.basis_unreadable",
                format!("{}: {error}", child.display()),
            )
        })?;
        if meta.is_dir() {
            walk_tree(&child, entries, depth + 1)?;
        } else {
            note_entry(&child, meta, entries)?;
        }
    }
    Ok(())
}

impl Service {
    /// Run one pure knowledge read through the result cache. A hit answers
    /// from Redis without materialising the horizon; a miss computes and
    /// remembers. Every cache failure degrades to direct computation with
    /// the reason disclosed through `knowledge status` — the cache is never
    /// allowed to stand between a caller and an answer.
    pub(super) fn with_knowledge_cached<T, F>(
        &self,
        scope: BasisScope,
        operation_key: &str,
        compute: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Result<T>,
    {
        if std::env::var(DISABLE_ENV).as_deref() == Ok("off") {
            return compute();
        }
        let Some(cache) = KnowledgeResultCache::discover(&self.home) else {
            CACHE_COMPUTES.fetch_add(1, Ordering::Relaxed);
            return compute();
        };
        let root = self
            .descriptor
            .project_root
            .as_deref()
            .unwrap_or(&self.invocation_cwd);
        let central_root = self.knowledge_central_root(root);
        let basis = match knowledge_basis(root, central_root, scope) {
            Ok(basis) => basis,
            Err(error) => {
                record_degraded(format!("basis unavailable: {error}"));
                CACHE_COMPUTES.fetch_add(1, Ordering::Relaxed);
                return compute();
            }
        };
        // The scope rides in every operation key: a world-root reply and a
        // project-scoped reply can share one basis (the same ground) yet
        // must never share an answer.
        let scope = self.knowledge_scope_project().unwrap_or_default();
        let key = format!("{basis}\x1f{scope}\x1f{operation_key}");
        if let Some(payload) = cache.get(&key) {
            match serde_json::from_str::<T>(&payload) {
                Ok(value) => {
                    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                    return Ok(value);
                }
                Err(error) => {
                    record_degraded(format!("cached payload undecodable: {error}"));
                }
            }
        } else {
            CACHE_COMPUTES.fetch_add(1, Ordering::Relaxed);
        }
        let value = compute()?;
        if let Ok(payload) = serde_json::to_string(&value) {
            cache.put(&key, &payload);
        }
        Ok(value)
    }

    /// The disclosure line for `knowledge status`: what the cache is, and
    /// what this invocation did with it.
    pub(super) fn knowledge_result_cache_note(&self) -> String {
        if std::env::var(DISABLE_ENV).as_deref() == Ok("off") {
            return "knowledge result cache: disabled by AIKIT_KNOWLEDGE_RESULT_CACHE".into();
        }
        if crate::inhabitation::world_redis_config_path(None, Some(&self.home)).is_none() {
            return "knowledge result cache: not configured (no Redis config at the AIKit home)"
                .into();
        }
        if let Some(reason) = CACHE_DEGRADED.get() {
            return format!(
                "knowledge result cache unavailable; live computation remains authoritative: {reason}"
            );
        }
        let hits = CACHE_HITS.load(Ordering::Relaxed);
        let computes = CACHE_COMPUTES.load(Ordering::Relaxed);
        format!(
            "knowledge result cache: Redis-backed, basis-invalidated; {hits} answered from cache, {computes} computed this invocation"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A miniature Central ground: Control field, two Work projects with
    /// ProjectCentral, git heads and a GitNexus index state.
    fn miniature_central(root: &Path) {
        fs::create_dir_all(root.join("Control/agents/wiki")).unwrap();
        fs::write(root.join("Control/agents/wiki/projection.md"), "v1").unwrap();
        fs::create_dir_all(root.join("Control/user")).unwrap();
        for project in ["alpha", "beta"] {
            fs::create_dir_all(root.join(format!("Work/{project}/ProjectCentral/user"))).unwrap();
            fs::write(
                root.join(format!("Work/{project}/ProjectCentral/user/note.md")),
                "note",
            )
            .unwrap();
            fs::create_dir_all(root.join(format!("Work/{project}/.git/refs/heads"))).unwrap();
            fs::write(
                root.join(format!("Work/{project}/.git/HEAD")),
                "ref: refs/heads/main\n",
            )
            .unwrap();
            fs::write(
                root.join(format!("Work/{project}/.git/refs/heads/main")),
                "abc123",
            )
            .unwrap();
            fs::create_dir_all(root.join(format!("Work/{project}/.gitnexus"))).unwrap();
            fs::write(
                root.join(format!("Work/{project}/.gitnexus/meta.json")),
                "{}",
            )
            .unwrap();
        }
    }

    #[test]
    fn a_basis_is_deterministic_for_one_reading_of_the_ground() {
        let ground = tempfile::tempdir().unwrap();
        miniature_central(ground.path());
        let first =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        let second =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn a_changed_source_changes_the_basis_and_a_changed_derived_file_does_not() {
        let ground = tempfile::tempdir().unwrap();
        miniature_central(ground.path());
        let before =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();

        // A source edit under a Work project's ground moves the basis.
        fs::write(
            ground.path().join("Work/alpha/ProjectCentral/user/note.md"),
            "note edited",
        )
        .unwrap();
        let after_edit =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        assert_ne!(before, after_edit);

        // A new commit moves the branch ref the basis watches.
        fs::write(
            ground.path().join("Work/alpha/.git/refs/heads/main"),
            "def456",
        )
        .unwrap();
        let after_commit =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        assert_ne!(after_edit, after_commit);

        // Derived state under `.git`'s object store is history, not ground:
        // only HEAD/refs/packed-refs are watched, so this does not move it.
        fs::create_dir_all(ground.path().join("Work/alpha/.git/objects/ab")).unwrap();
        fs::write(ground.path().join("Work/alpha/.git/objects/ab/cd"), "junk").unwrap();
        let after_objects =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        assert_eq!(after_commit, after_objects);
    }

    #[test]
    fn an_absent_input_is_a_state_of_the_world_not_an_error() {
        let ground = tempfile::tempdir().unwrap();
        miniature_central(ground.path());
        let with_wiki =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        fs::remove_dir_all(ground.path().join("Control/agents/wiki")).unwrap();
        let without_wiki =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        assert_ne!(with_wiki, without_wiki);
    }

    #[test]
    fn a_walk_that_cannot_be_completed_poisons_the_basis() {
        let ground = tempfile::tempdir().unwrap();
        miniature_central(ground.path());
        // A directory deeper than the walk bound must be refused, never
        // silently answered from a partially observed ground.
        let mut deep = ground.path().join("Control/user/deep");
        fs::create_dir_all(&deep).unwrap();
        for _ in 0..(MAX_BASIS_DEPTH + 4) {
            deep = deep.join("down");
            fs::create_dir_all(&deep).unwrap();
        }
        fs::write(deep.join("leaf.md"), "beyond the bound").unwrap();
        let poisoned =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full);
        assert!(
            poisoned.is_err(),
            "a walk past its bound must poison the basis"
        );
    }

    #[test]
    fn now_field_churn_invalidates_a_full_answer_but_never_a_ground_answer() {
        let ground = tempfile::tempdir().unwrap();
        miniature_central(ground.path());
        fs::create_dir_all(ground.path().join("Control/agents/now/clearings")).unwrap();
        fs::write(
            ground.path().join("Control/agents/now/clearings/one.json"),
            r#"{"purpose":"before"}"#,
        )
        .unwrap();
        let full_before =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        let ground_before =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Ground)
                .unwrap();

        // Another lane returns to the NOW field: the full basis must move
        // (a search would answer differently), the ground basis must not
        // (relations and reads cannot see the return).
        fs::write(
            ground.path().join("Control/agents/now/clearings/one.json"),
            r#"{"purpose":"after"}"#,
        )
        .unwrap();
        let full_after =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Full).unwrap();
        let ground_after =
            compute_knowledge_basis(ground.path(), Some(ground.path()), BasisScope::Ground)
                .unwrap();
        assert_ne!(full_before, full_after);
        assert_eq!(ground_before, ground_after);
    }
}
