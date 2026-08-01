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

/// Load `<repo>/.aikit/` as the project-local registry.
pub fn load_project_local(repo: &Path) -> Result<RegistryLoad> {
    load_registry(&repo.join(".aikit"), RegistrySource::project_local())
}

fn load_capsules(root: &Path, source: &RegistrySource, load: &mut RegistryLoad) -> Result<()> {
    let capsules_root = root.join("capsules");
    if !capsules_root.is_dir() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(&capsules_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() || entry.file_name() != MANIFEST_FILE {
            continue;
        }
        let manifest_path = entry.path().to_path_buf();
        let dir = match manifest_path.parent() {
            Some(dir) => dir.to_path_buf(),
            None => continue,
        };
        match load_capsule(&capsules_root, &dir, &manifest_path, source) {
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
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-capsule-revision-v2\n");
    hasher.update(&(manifest_bytes.len() as u64).to_le_bytes());
    hasher.update(manifest_bytes);
    hasher.update(&file_mode(&dir.join(MANIFEST_FILE))?.to_le_bytes());

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

    for (relative, path) in files {
        let contents =
            std::fs::read(&path).map_err(|e| io_error("registry.read_failed", &path, &e))?;
        hasher.update(&(relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(&file_mode(&path)?.to_le_bytes());
        hasher.update(&(contents.len() as u64).to_le_bytes());
        hasher.update(&contents);
    }

    Ok(Revision::from_hash(hasher.finalize()))
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
