//! Reading registries off disk.
//!
//! ## The revision is the whole payload, not the manifest
//!
//! Trust is keyed on `(source, capsule, revision)`. If the revision were a hash
//! of the manifest alone, then editing `payload/run.sh` — the part that actually
//! executes — would leave a reviewed capsule reviewed. So [`compute_revision`]
//! folds in every file under the capsule directory, path *and* contents, in
//! sorted order. Moving a byte, adding a file, or renaming one all move the
//! revision, and a moved revision drops the capsule back to `Unseen`.
//!
//! ## One bad manifest must not blind the registry
//!
//! A registry is a tree of independently authored files. Returning
//! `Result<Catalog>` would mean a single stray character in one manifest hides
//! the other four hundred capsules and gives the user no way to find out which
//! file was at fault. So loading returns a [`RegistryLoad`]: everything that
//! parsed, plus a list of [`RegistryProblem`]s naming the exact path and a stable
//! error code. `doctor` prints the list; the palette keeps working.
//!
//! ## The project-local registry is a separate source, deliberately
//!
//! `<repo>/.aikit/capsules/` loads under [`RegistrySource::project_local`]. Since
//! the trust key includes the source, a capsule that is trusted in the personal
//! registry is *unseen* when the identical bytes arrive by way of a cloned
//! repository. That is the point: `git clone` must not be able to hand you a
//! reviewed hook.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::catalog::Catalog;
use aikit_core::profile::{Profile, ProfileFile};
use aikit_core::{AikitError, Capsule, CapsuleId, ProfileId, RegistrySource, Result, Revision};

use crate::home::io_error;

/// A capsule directory is one that contains this file.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// An in-memory catalog loaded from one or more registries.
///
/// Implements [`Catalog`] so it can be handed straight to `aikit_core::resolve`.
/// Ordering is by [`CapsuleId`], which sorts on the rendered string, so listings
/// and the catalog revision are deterministic regardless of walk order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    capsules: BTreeMap<CapsuleId, Capsule>,
    profiles: BTreeMap<ProfileId, Profile>,
}

impl Snapshot {
    /// Insert a capsule, returning the id when it displaced an existing one.
    ///
    /// The return value is not decoration: shadowing is a thing the user needs
    /// told about ("this project overrides your personal `script/test/nt`"), and
    /// a silent overwrite would make that unreportable.
    pub fn insert(&mut self, capsule: Capsule) -> Option<CapsuleId> {
        let id = capsule.id.clone();
        self.capsules.insert(id.clone(), capsule).map(|_| id)
    }

    pub fn insert_profile(&mut self, profile: Profile) -> Option<ProfileId> {
        let id = profile.id.clone();
        self.profiles.insert(id.clone(), profile).map(|_| id)
    }

    pub fn len(&self) -> usize {
        self.capsules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capsules.is_empty() && self.profiles.is_empty()
    }

    /// Where each capsule's files live, in the shape adapters need.
    pub fn capsule_roots(&self) -> BTreeMap<CapsuleId, PathBuf> {
        self.capsules
            .iter()
            .filter_map(|(id, c)| c.root.clone().map(|root| (id.clone(), root)))
            .collect()
    }
}

impl Catalog for Snapshot {
    fn get(&self, id: &CapsuleId) -> Option<&Capsule> {
        self.capsules.get(id)
    }

    fn profile(&self, id: &ProfileId) -> Option<&Profile> {
        self.profiles.get(id)
    }

    fn capsules(&self) -> Vec<&Capsule> {
        self.capsules.values().collect()
    }

    fn profiles(&self) -> Vec<&Profile> {
        self.profiles.values().collect()
    }
}

/// One file that could not be loaded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryProblem {
    pub path: PathBuf,
    pub source: RegistrySource,
    pub error: AikitError,
}

/// The result of loading: what was understood, and what was not.
#[derive(Debug, Clone, Default)]
pub struct RegistryLoad {
    pub catalog: Snapshot,
    pub problems: Vec<RegistryProblem>,
}

impl RegistryLoad {
    /// Layer `other` on top of this load; `other` wins on collisions.
    ///
    /// Returns the ids that were shadowed, so the caller can surface them rather
    /// than leaving a user to wonder why their personal script stopped being the
    /// one that runs.
    pub fn merge(&mut self, other: RegistryLoad) -> Vec<CapsuleId> {
        let mut shadowed = Vec::new();
        let RegistryLoad { catalog, problems } = other;
        for capsule in catalog.capsules.into_values() {
            if let Some(id) = self.catalog.insert(capsule) {
                shadowed.push(id);
            }
        }
        for profile in catalog.profiles.into_values() {
            self.catalog.insert_profile(profile);
        }
        self.problems.extend(problems);
        shadowed
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load `<root>/capsules/**/manifest.toml` and `<root>/profiles/**/*.toml`.
///
/// A missing root is an empty catalog, not an error: a fresh install has no
/// registries, and refusing to start would be a worse answer than an empty
/// palette.
pub fn load_registry(root: &Path, source: RegistrySource) -> Result<RegistryLoad> {
    let mut load = RegistryLoad::default();
    load_capsules(root, &source, &mut load)?;
    load_profiles(root, &source, &mut load)?;
    Ok(load)
}

// ---------------------------------------------------------------------------
// The retained catalogue's change signal
// ---------------------------------------------------------------------------

/// The most threads the signal's walk and status reads run on.
pub const SIGNAL_THREADS: usize = 8;
/// How many entries one thread reads the status of at a time; the signal is
/// folded chunk by chunk in order, so it is the same on any number of threads.
const SIGNAL_CHUNK: usize = 128;

/// The entries of one registry root a walk found, by their paths under it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RootWalk {
    pub root: std::path::PathBuf,
    pub entries: Vec<String>,
}

/// Every entry [`load_registry`] can read under each root's `capsules/` and
/// `profiles/`, in order: each section itself (named even where it is absent,
/// so its arrival is seen), every directory under it, and every other entry.
/// The roots are walked on up to [`SIGNAL_THREADS`] threads at once.
pub fn registry_walk(roots: &[&Path]) -> Vec<RootWalk> {
    let walk = |root: &Path| {
        let mut entries = Vec::new();
        for section in ["capsules", "profiles"] {
            let top = root.join(section);
            entries.push(section.to_owned());
            for entry in walkdir::WalkDir::new(&top)
                .follow_links(false)
                .sort_by_file_name()
                .min_depth(1)
                .into_iter()
                .flatten()
            {
                if let Ok(under) = entry.path().strip_prefix(root) {
                    entries.push(under.to_string_lossy().into_owned());
                }
            }
        }
        RootWalk {
            root: root.to_path_buf(),
            entries,
        }
    };
    in_parallel(roots, |root| walk(root))
}

/// The change signal of the entries [`registry_walk`] found: each by path,
/// kind, length, permissions, inode, and modification and status-change times,
/// or as absent. A write to a file moves its status-change time, which no
/// ordinary call sets back, and adding, removing or renaming an entry moves its
/// directory's modification time; so an equal signal over the entries a walk
/// found says nothing the load reads has moved since that walk (DLW build
/// ruling 2026-11-05: a reading is retained only while a change signal vouches
/// for it). Reading it is one status read per entry, with no directory listed.
pub fn status_signal(walks: &[RootWalk]) -> String {
    let chunks: Vec<(&Path, &[String])> = walks
        .iter()
        .flat_map(|walk| {
            walk.entries
                .chunks(SIGNAL_CHUNK)
                .map(move |chunk| (walk.root.as_path(), chunk))
        })
        .collect();
    let mut hasher = blake3::Hasher::new();
    for walk in walks {
        hasher.update(walk.root.as_os_str().as_encoded_bytes());
        hasher.update(&(walk.entries.len() as u64).to_le_bytes());
    }
    for signal in in_parallel(&chunks, |(root, chunk)| chunk_signal(root, chunk)) {
        hasher.update(signal.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn chunk_signal(root: &Path, entries: &[String]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    for entry in entries {
        hasher.update(entry.as_bytes());
        hasher.update(b"\0");
        // A path the walk did not write is never read: an entry names a place under its root.
        let under = Path::new(entry);
        if under.is_absolute()
            || under
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            hasher.update(b"\0refused");
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(root.join(under)) else {
            hasher.update(b"\0absent");
            continue;
        };
        let kind: u8 = if meta.is_dir() {
            b'd'
        } else if meta.is_file() {
            b'f'
        } else {
            b'l'
        };
        hasher.update(&[0, kind]);
        hasher.update(&meta.len().to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            for value in [
                u64::from(meta.mode()),
                meta.ino(),
                meta.dev(),
                meta.mtime() as u64,
                meta.mtime_nsec() as u64,
                meta.ctime() as u64,
                meta.ctime_nsec() as u64,
            ] {
                hasher.update(&value.to_le_bytes());
            }
        }
        #[cfg(not(unix))]
        if let Ok(modified) = meta.modified() {
            let since = modified
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            hasher.update(&since.as_nanos().to_le_bytes());
        }
    }
    hasher.finalize()
}

/// `work` over every item, on up to [`SIGNAL_THREADS`] threads, the answers in
/// the items' order. A person's sources hold thousands of entries, and reading
/// them one after another costs a hook dispatch most of its time.
fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, SIGNAL_THREADS)
        .min(items.len());
    if threads < 2 {
        return items.iter().map(&work).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut answers: Vec<(usize, R)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut taken = Vec::new();
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(at) else {
                            return taken;
                        };
                        taken.push((at, work(item)));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| match worker.join() {
                Ok(taken) => taken,
                Err(panic) => std::panic::resume_unwind(panic),
            })
            .collect()
    });
    answers.sort_by_key(|(at, _)| *at);
    answers.into_iter().map(|(_, answer)| answer).collect()
}

/// A catalogue as it is retained between processes: what a load with no
/// problems understood, with the change signal it was read under and the
/// entries that signal is read over ([`registry_walk`], [`status_signal`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RetainedCatalogue {
    pub schema: String,
    pub signal: String,
    pub entries: Vec<RootWalk>,
    pub capsules: Vec<Capsule>,
    pub profiles: Vec<Profile>,
}

pub const RETAINED_CATALOGUE_SCHEMA: &str = "aikit.retained-catalogue/v2";

impl RetainedCatalogue {
    /// The retained form of a load, or `None` when the load had problems (a
    /// problem is reported fresh, so such a load is read fresh) or does not
    /// survive the round trip unchanged.
    pub fn of(load: &RegistryLoad, signal: String, entries: Vec<RootWalk>) -> Option<Self> {
        if !load.problems.is_empty() {
            return None;
        }
        let retained = Self {
            schema: RETAINED_CATALOGUE_SCHEMA.into(),
            signal,
            entries,
            capsules: load.catalog.capsules.values().cloned().collect(),
            profiles: load.catalog.profiles.values().cloned().collect(),
        };
        let text = serde_json::to_vec(&retained).ok()?;
        let back: Self = serde_json::from_slice(&text).ok()?;
        (back == retained).then_some(retained)
    }

    /// The load again, when it was taken under `signal`.
    pub fn load_under(self, signal: &str) -> Option<RegistryLoad> {
        if self.schema != RETAINED_CATALOGUE_SCHEMA || self.signal != signal {
            return None;
        }
        let mut load = RegistryLoad::default();
        for capsule in self.capsules {
            load.catalog.insert(capsule);
        }
        for profile in self.profiles {
            load.catalog.insert_profile(profile);
        }
        Some(load)
    }
}

/// Load `<repo>/.aikit/` as the project-local registry.
pub fn load_project_local(repo: &Path) -> Result<RegistryLoad> {
    load_registry(&repo.join(".aikit"), RegistrySource::project_local())
}

fn load_capsules(root: &Path, source: &RegistrySource, load: &mut RegistryLoad) -> Result<()> {
    let capsules_root = root.join("capsules");
    if !capsules_root.is_dir() {
        return Ok(());
    }

    // The capsules are found in order, then each read - its manifest parsed and its files hashed
    // for the revision - at once with the others; what is loaded, and what is a problem, is
    // applied in the order they were found.
    let manifests: Vec<PathBuf> = walkdir::WalkDir::new(&capsules_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == MANIFEST_FILE)
        .map(|entry| entry.path().to_path_buf())
        .filter(|manifest_path| manifest_path.parent().is_some())
        .collect();
    let loaded = crate::parallel::in_parallel(&manifests, |manifest_path| {
        let dir = manifest_path
            .parent()
            .expect("a manifest path with no parent was filtered out");
        load_capsule(&capsules_root, dir, manifest_path, source)
    });
    for (manifest_path, loaded) in manifests.into_iter().zip(loaded) {
        match loaded {
            Ok(capsule) => {
                load.catalog.insert(capsule);
            }
            Err(error) => load.problems.push(RegistryProblem {
                path: manifest_path,
                source: source.clone(),
                error,
            }),
        }
    }
    Ok(())
}

fn load_capsule(
    capsules_root: &Path,
    dir: &Path,
    manifest_path: &Path,
    source: &RegistrySource,
) -> Result<Capsule> {
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|e| io_error("registry.read_failed", manifest_path, &e))?;
    let text = String::from_utf8(manifest_bytes.clone()).map_err(|_| {
        AikitError::new(
            "manifest.parse_error",
            format!("{} is not valid UTF-8", manifest_path.display()),
        )
        .with("path", manifest_path.display().to_string())
    })?;

    let mut capsule = Capsule::from_toml_str(&text)?;

    // The directory and the manifest must agree. Trusting the manifest alone
    // would let a capsule masquerade as one the user already reviewed; trusting
    // the path alone would silently rename the thing the author wrote.
    let relative = dir.strip_prefix(capsules_root).unwrap_or(dir);
    let on_disk = relative.to_string_lossy().replace('\\', "/");
    let declared = format!("{}/{}", capsule.id.kind().as_str(), capsule.id.path());
    if on_disk != declared {
        return Err(AikitError::new(
            "registry.id_path_mismatch",
            format!(
                "the manifest at capsules/{on_disk} declares the id `{declared}`; a capsule's \
                 directory and its id must agree"
            ),
        )
        .with("path", manifest_path.display().to_string())
        .with("declared", declared)
        .with("on_disk", on_disk));
    }

    capsule.revision = Some(compute_revision(dir, &manifest_bytes)?);
    capsule.source = Some(source.clone());
    capsule.root = Some(dir.to_path_buf());
    Ok(capsule)
}

fn load_profiles(root: &Path, source: &RegistrySource, load: &mut RegistryLoad) -> Result<()> {
    let profiles_root = root.join("profiles");
    if !profiles_root.is_dir() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(&profiles_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|e| e.to_str()) != Some("toml")
        {
            continue;
        }
        let path = entry.path().to_path_buf();
        match load_profile(&profiles_root, &path) {
            Ok(profile) => {
                load.catalog.insert_profile(profile);
            }
            Err(error) => load.problems.push(RegistryProblem {
                path,
                source: source.clone(),
                error,
            }),
        }
    }
    Ok(())
}

fn load_profile(profiles_root: &Path, path: &Path) -> Result<Profile> {
    let text =
        std::fs::read_to_string(path).map_err(|e| io_error("registry.read_failed", path, &e))?;
    let file: ProfileFile = toml::from_str(&text).map_err(|e| {
        AikitError::new(
            "profile.parse_error",
            format!("could not parse {}: {e}", path.display()),
        )
        .with("path", path.display().to_string())
    })?;
    let profile = file.into_profile()?;

    let relative = path.strip_prefix(profiles_root).unwrap_or(path);
    let on_disk = relative
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/");
    if on_disk != profile.id.path() {
        return Err(AikitError::new(
            "registry.id_path_mismatch",
            format!(
                "the profile at profiles/{on_disk}.toml declares the id `{}`; a profile's path \
                 and its id must agree",
                profile.id
            ),
        )
        .with("path", path.display().to_string())
        .with("declared", profile.id.to_string())
        .with("on_disk", on_disk));
    }
    Ok(profile)
}

// ---------------------------------------------------------------------------
// The content revision
// ---------------------------------------------------------------------------

/// Hash the manifest plus every other file and its relevant permissions in the capsule directory.
///
/// Paths are included alongside contents so that moving a file — same bytes, new
/// name — is a new revision, and lengths are included so that concatenation
/// cannot be made ambiguous by a crafted filename.
pub fn compute_revision(dir: &Path, manifest_bytes: &[u8]) -> Result<Revision> {
    let manifest_mode = file_mode(&dir.join(MANIFEST_FILE))?;

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.parent() == Some(dir) && entry.file_name() == MANIFEST_FILE {
            continue;
        }
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        files.push((relative, path.to_path_buf()));
    }
    files.sort();

    let mut entries = Vec::with_capacity(files.len());
    for (relative, path) in files {
        let contents =
            std::fs::read(&path).map_err(|e| io_error("registry.read_failed", &path, &e))?;
        entries.push(RevisionEntry {
            path: relative,
            mode: file_mode(&path)?,
            contents,
        });
    }
    Ok(revision_of_entries(manifest_bytes, manifest_mode, &entries))
}

/// One non-manifest file of a capsule as the revision reads it: its
/// `/`-separated path relative to the capsule directory, its permission bits
/// (`mode & 0o7777` on Unix) and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionEntry {
    pub path: String,
    pub mode: u32,
    pub contents: Vec<u8>,
}

/// The capsule revision over files already in memory — the one algorithm
/// [`compute_revision`] applies to a directory, so a capsule carried as data
/// (an exported archive) proves the same revision its directory would.
/// `entries` excludes the top-level manifest; they are hashed in path order
/// whatever order they arrive in.
pub fn revision_of_entries(
    manifest_bytes: &[u8],
    manifest_mode: u32,
    entries: &[RevisionEntry],
) -> Revision {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-capsule-revision-v2\n");
    hasher.update(&(manifest_bytes.len() as u64).to_le_bytes());
    hasher.update(manifest_bytes);
    hasher.update(&manifest_mode.to_le_bytes());

    let mut sorted: Vec<&RevisionEntry> = entries.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    for entry in sorted {
        hasher.update(&(entry.path.len() as u64).to_le_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update(&entry.mode.to_le_bytes());
        hasher.update(&(entry.contents.len() as u64).to_le_bytes());
        hasher.update(&entry.contents);
    }
    Revision::from_hash(hasher.finalize())
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .map_err(|error| io_error("registry.read_failed", path, &error))
}

#[cfg(not(unix))]
fn file_mode(path: &Path) -> Result<u32> {
    std::fs::metadata(path)
        .map(|metadata| u32::from(metadata.permissions().readonly()))
        .map_err(|error| io_error("registry.read_failed", path, &error))
}
