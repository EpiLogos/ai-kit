//! What the generic discovery walk leaves unlisted.
//!
//! The walk lists a horizon to find the material it carries (SourcePool
//! shards, and outside a Central World SemanticWiki JSON). Some directories
//! below a horizon are never that material, and listing them only costs: the
//! walk spends its candidate budget there, and the discovery ledger observes
//! every entry, so a directory that churns discards the kept walk on every
//! change. Each skip is a rule about what a directory *is* — named by the
//! world's own shape or a published tag — never a list of one machine's paths.
//!
//! A rule decided by an entry's name is observed through the listing that
//! holds the entry (adding, removing or renaming an entry changes that
//! directory's stamps). The cache tag is decided by a file inside the
//! directory, so the tag the walk read is observed itself.

use std::fs;
use std::io::Read;
use std::path::Path;

use super::ledger;

/// Why the walk leaves a directory unlisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WalkSkip {
    /// Central's own machine state (`.central`: reconcile horizons, file
    /// history, the bkmr file-map index). Central rewrites it as it works; it
    /// is never knowledge material. What Knowledge needs from it is read by
    /// name (the file-map freshness note), never found by walking it.
    CentralMachineState,
    /// The live NOW field of a register — the root register's
    /// `Control/agents/now` and a Project's `ProjectCentral/now`. Inside a
    /// Central World the NOW provider searches and reads it in place
    /// (`NowFieldScope::standard`); the walk adds nothing there and its
    /// records change many times a day.
    LiveNowField,
    /// A World root's members other than its root register, `Control` (the
    /// root binding's constituent): `Work` holds the Project registers, each
    /// walked from its own root; everything else at the World root — seats,
    /// harness and machine state — belongs to no register.
    OutsideRootRegister,
    /// A directory tagged by the Cache Directory Tagging Specification
    /// (cargo tags every target directory, whatever it is named).
    CacheTagged,
}

/// Where a walk stands in a Central World.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorldPosition {
    /// No Central World: a standalone horizon, which no NOW provider serves.
    Standalone,
    /// A horizon inside a Central World (a Project register, a seat).
    Member,
    /// The World root itself.
    Root,
}

/// The root register a World root's walk descends into.
const ROOT_REGISTER: &str = "Control";

/// The cache tag's file name and the signature it must begin with
/// (https://bford.info/cachedir/).
pub(super) const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
pub(super) const CACHEDIR_TAG_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// The skip an entry's own name and place decide: `path` is a directory
/// entry met while listing `listing`, which belongs to the walk of `root`.
pub(super) fn named_skip(
    path: &Path,
    listing: &Path,
    root: &Path,
    position: WorldPosition,
) -> Option<WalkSkip> {
    let name = path.file_name()?.to_str()?;
    if name == ".central" {
        return Some(WalkSkip::CentralMachineState);
    }
    if position == WorldPosition::Standalone {
        return None;
    }
    if position == WorldPosition::Root && listing == root && name != ROOT_REGISTER {
        return Some(WalkSkip::OutsideRootRegister);
    }
    let parent = path.parent().and_then(Path::file_name);
    let grandparent = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name);
    let root_now = parent.is_some_and(|parent| parent == "agents")
        && grandparent.is_some_and(|grandparent| grandparent == ROOT_REGISTER);
    let project_now = parent.is_some_and(|parent| parent == "ProjectCentral");
    (name == "now" && (root_now || project_now)).then_some(WalkSkip::LiveNowField)
}

/// [`WalkSkip::CacheTagged`] when `directory` carries a valid cache tag. A
/// tag file that exists — valid or not — decided the answer by its bytes, so
/// it is observed; an absent tag is covered by the directory's own stamps,
/// which the walk records as it lists the directory next.
pub(super) fn cache_skip(
    directory: &Path,
    recorder: ledger::SharedRecorder<'_>,
) -> Option<WalkSkip> {
    let tag = directory.join(CACHEDIR_TAG);
    let metadata = fs::symlink_metadata(&tag);
    if metadata.is_ok() {
        if let Some(recorder) = recorder {
            recorder.borrow_mut().record(
                &tag,
                ledger::Reading::Linked,
                ledger::Outcome::of(&metadata),
            );
        }
    }
    if !metadata.is_ok_and(|metadata| metadata.is_file()) {
        return None;
    }
    let mut head = [0u8; CACHEDIR_TAG_SIGNATURE.len()];
    fs::File::open(&tag)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == CACHEDIR_TAG_SIGNATURE)
        .then_some(WalkSkip::CacheTagged)
}
