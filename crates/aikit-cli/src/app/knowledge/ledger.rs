//! The discovery ledger: one Knowledge discovery walk kept between
//! invocations, and reused only while every physical fact it observed still
//! reads the same.
//!
//! A walk records each filesystem observation that decided its outcome — the
//! directories it listed, every entry's followed metadata (or the error it
//! met), the files whose bytes it read, the canonical chains of the aliases it
//! followed and the ancestors its admission policy consulted — together with
//! the material and disclosures it produced. A later walk of the same horizon
//! repeats only those observations. If every one answers exactly as recorded,
//! nothing the walk read or listed has changed, its admission inputs
//! included. Otherwise the ledger is discarded and the walk runs again in
//! full.
//!
//! Listed directories and read files are observed in full: device, inode,
//! mode, size and both nanosecond timestamps (or the same error, for an entry
//! that failed). Adding, removing or renaming an entry — a
//! `.no-agent-retrieval` marker included — changes the directory that holds
//! it, and any write to a file changes its ctime, which no process can set.
//! The ancestors a canonicalisation or an admission policy walks through but
//! never lists are observed for exactly what those consult: their identity
//! and kind (an ancestor replaced by an alias or another directory reads
//! differently) and their own marker. Their listings are not part of the
//! walk, so a sibling written beside the horizon does not discard it.
//!
//! The ledger is never written when an observed timestamp falls inside the
//! racy window before the walk began: a write in the same clock tick as an
//! observation could leave the recorded time unchanged. The executable's own
//! identity is part of the key, so a rebuilt or upgraded AIKit never reuses a
//! walk an older discovery produced.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aikit_core::knowledge_source_pool::SourceMaterial;
use aikit_core::knowledge_wiki::{
    WikiEdge, WikiFrame, WikiNode, WikiObject, WikiReading, WikiSpace,
};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "aikit.knowledge-discovery-ledger/v1";
/// A walk a search's deadline stopped is kept under its own schema: an aikit
/// that predates the flag reads only [`SCHEMA`] and so walks again rather than
/// taking a partial walk for a whole one (several installed builds share one
/// AIKit home).
const STOPPED_SCHEMA: &str = "aikit.knowledge-discovery-ledger/deadline-stopped-v1";

/// The ledger records unix stat facts; elsewhere every walk runs in full.
pub(super) const SUPPORTED: bool = cfg!(unix);

#[cfg(test)]
thread_local! {
    /// Full walks this test thread ran: the proof a ledger answered instead.
    pub(super) static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Observations this close to (or after) the walk's start are racy: a later
/// write in the same timestamp tick could read back identically. Two seconds
/// also covers filesystems with whole-second timestamps.
const RACY_WINDOW: Duration = Duration::from_secs(2);

/// The physical identity and change stamps of one followed or linked path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StatSignature {
    dev: u64,
    ino: u64,
    mode: u32,
    size: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl StatSignature {
    #[cfg(unix)]
    pub(super) fn of(metadata: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: metadata.mode(),
            size: metadata.size(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    #[cfg(not(unix))]
    pub(super) fn of(_metadata: &fs::Metadata) -> Self {
        unreachable!("the discovery ledger is only recorded on unix platforms")
    }

    /// The later of the two change stamps, as a wall-clock instant.
    fn latest_change(&self) -> SystemTime {
        let stamp = |(secs, nanos): (i64, i64)| {
            if secs < 0 {
                UNIX_EPOCH
            } else {
                UNIX_EPOCH + Duration::new(secs as u64, nanos.clamp(0, 999_999_999) as u32)
            }
        };
        stamp(self.mtime).max(stamp(self.ctime))
    }
}

/// What one metadata call answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(super) enum Outcome {
    Present { stat: StatSignature },
    Exists { dev: u64, ino: u64, kind: String },
    Missing,
    Failed { kind: String, errno: Option<i32> },
}

impl Outcome {
    #[cfg(unix)]
    fn identity(result: &std::io::Result<fs::Metadata>) -> Self {
        use std::os::unix::fs::MetadataExt;
        match result {
            Ok(metadata) => {
                let kind = metadata.file_type();
                Self::Exists {
                    dev: metadata.dev(),
                    ino: metadata.ino(),
                    kind: if kind.is_symlink() {
                        "symlink"
                    } else if kind.is_dir() {
                        "directory"
                    } else if kind.is_file() {
                        "file"
                    } else {
                        "other"
                    }
                    .into(),
                }
            }
            Err(_) => Self::of(result),
        }
    }

    #[cfg(not(unix))]
    fn identity(result: &std::io::Result<fs::Metadata>) -> Self {
        Self::of(result)
    }

    pub(super) fn of(result: &std::io::Result<fs::Metadata>) -> Self {
        match result {
            Ok(metadata) => Self::Present {
                stat: StatSignature::of(metadata),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Failed {
                kind: format!("{:?}", error.kind()),
                errno: error.raw_os_error(),
            },
        }
    }
}

/// Which metadata an observation read, and how much of it decided the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reading {
    /// Followed metadata in full: a listed directory or a file whose bytes
    /// were read.
    Followed,
    /// The link itself in full: an alias the walk followed.
    Linked,
    /// Followed metadata's identity and kind: an entry the walk classified
    /// (descend, skip, or read) but whose own stamps did not decide it.
    Kind,
    /// The link itself, identity and kind: an ancestor a canonicalisation or
    /// a policy walked through without listing it.
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Observation {
    path: PathBuf,
    reading: Reading,
    #[serde(flatten)]
    outcome: Outcome,
}

impl Observation {
    fn repeat(&self) -> Outcome {
        match self.reading {
            Reading::Followed => Outcome::of(&fs::metadata(&self.path)),
            Reading::Linked => Outcome::of(&fs::symlink_metadata(&self.path)),
            Reading::Kind => Outcome::identity(&fs::metadata(&self.path)),
            Reading::Identity => Outcome::identity(&fs::symlink_metadata(&self.path)),
        }
    }
}

/// Everything one walk observed, in the order it observed it.
pub(super) struct DiscoveryRecorder {
    observations: Vec<Observation>,
    recorded: BTreeSet<(PathBuf, Reading)>,
    started: SystemTime,
    racy: bool,
}

impl DiscoveryRecorder {
    pub(super) fn new() -> Self {
        Self {
            observations: Vec::new(),
            recorded: BTreeSet::new(),
            started: SystemTime::now(),
            racy: false,
        }
    }

    /// Record an outcome the walk already obtained. The first outcome for a
    /// path is the one that decided the walk; later repeats are not kept.
    pub(super) fn record(&mut self, path: &Path, reading: Reading, outcome: Outcome) {
        if !self.recorded.insert((path.to_path_buf(), reading)) {
            return;
        }
        if let Outcome::Present { stat } = &outcome {
            let floor = self.started.checked_sub(RACY_WINDOW).unwrap_or(UNIX_EPOCH);
            if stat.latest_change() >= floor {
                self.racy = true;
            }
        }
        self.observations.push(Observation {
            path: path.to_path_buf(),
            reading,
            outcome,
        });
    }

    /// Record the identity and kind a followed metadata call answered.
    pub(super) fn record_kind(&mut self, path: &Path, result: &std::io::Result<fs::Metadata>) {
        let outcome = Outcome::identity(result);
        self.record(path, Reading::Kind, outcome);
    }

    /// Observe and record a path's followed metadata.
    pub(super) fn observe(&mut self, path: &Path) {
        if self
            .recorded
            .contains(&(path.to_path_buf(), Reading::Followed))
        {
            return;
        }
        let outcome = Outcome::of(&fs::metadata(path));
        self.record(path, Reading::Followed, outcome);
    }

    /// Observe every ancestor of a path up to the filesystem root for what a
    /// canonicalisation or an admission policy consults there: its identity
    /// and kind, and its own `.no-agent-retrieval` marker.
    pub(super) fn observe_chain(&mut self, path: &Path) {
        for ancestor in path.ancestors() {
            if ancestor.as_os_str().is_empty() {
                break;
            }
            if !self
                .recorded
                .contains(&(ancestor.to_path_buf(), Reading::Identity))
            {
                let outcome = Outcome::identity(&fs::symlink_metadata(ancestor));
                self.record(ancestor, Reading::Identity, outcome);
            }
            self.observe(&ancestor.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER));
        }
    }

    /// Observe an aliased entry: the link itself and the canonical chain it
    /// resolves through, so a retarget or a marker on that chain is seen.
    pub(super) fn observe_alias(&mut self, path: &Path) {
        let outcome = Outcome::of(&fs::symlink_metadata(path));
        self.record(path, Reading::Linked, outcome);
        if let Ok(canonical) = fs::canonicalize(path) {
            self.observe_chain(&canonical);
        }
    }
}

/// The identity of one walk: what it was asked, and by which executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct LedgerKey {
    root: PathBuf,
    home: PathBuf,
    world: Option<PathBuf>,
    discover_wiki: bool,
    limits: String,
    executable: (PathBuf, StatSignature),
}

impl LedgerKey {
    pub(super) fn new(
        root: &Path,
        home: &Path,
        world: Option<&Path>,
        discover_wiki: bool,
        limits: String,
    ) -> Option<Self> {
        // Without its own identity this executable cannot tell its walks from
        // another build's, so it keeps none.
        let executable = std::env::current_exe().and_then(fs::canonicalize).ok()?;
        let stat = StatSignature::of(&fs::metadata(&executable).ok()?);
        Some(Self {
            root: root.to_path_buf(),
            home: home.to_path_buf(),
            world: world.map(Path::to_path_buf),
            discover_wiki,
            limits,
            executable: (executable, stat),
        })
    }

    /// One ledger per horizon, like the SemanticWiki projection: the file is
    /// named by what was walked, and the rest of the key (limits, the
    /// executable) is checked inside it, so a newer AIKit replaces the ledger
    /// an older one kept instead of leaving it behind.
    fn file_name(&self) -> Option<String> {
        let horizon =
            serde_json::to_vec(&(&self.root, &self.home, &self.world, self.discover_wiki)).ok()?;
        Some(format!("{}.json", blake3::hash(&horizon).to_hex()))
    }
}

/// The outcome of one walk, exactly as it was handed to the runtime.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct DiscoveryOutcome {
    pub(super) wiki: Vec<WikiObject>,
    pub(super) sources: Vec<SourceMaterial>,
    pub(super) absences: Vec<String>,
    /// The walk stopped at a search's deadline; only another search reuses it.
    pub(super) deadline_stopped: bool,
}

#[derive(Serialize, Deserialize)]
struct EncodedObject {
    kind: String,
    json: String,
}

#[derive(Serialize, Deserialize)]
struct LedgerFile {
    schema: String,
    key: LedgerKey,
    root_canonical: PathBuf,
    world_canonical: Option<PathBuf>,
    observations: Vec<Observation>,
    wiki: Vec<EncodedObject>,
    sources: Vec<SourceMaterial>,
    absences: Vec<String>,
    #[serde(default)]
    deadline_stopped: bool,
}

fn encode(object: &WikiObject) -> Option<EncodedObject> {
    let (kind, json) = match object {
        WikiObject::Space(value) => ("space", serde_json::to_string(value)),
        WikiObject::Node(value) => ("node", serde_json::to_string(value)),
        WikiObject::Edge(value) => ("edge", serde_json::to_string(value)),
        WikiObject::Frame(value) => ("frame", serde_json::to_string(value)),
        WikiObject::Reading(value) => ("reading", serde_json::to_string(value)),
    };
    Some(EncodedObject {
        kind: kind.into(),
        json: json.ok()?,
    })
}

fn decode(encoded: &EncodedObject) -> Option<WikiObject> {
    let json = encoded.json.as_str();
    Some(match encoded.kind.as_str() {
        "space" => WikiObject::Space(serde_json::from_str::<WikiSpace>(json).ok()?),
        "node" => WikiObject::Node(serde_json::from_str::<WikiNode>(json).ok()?),
        "edge" => WikiObject::Edge(serde_json::from_str::<WikiEdge>(json).ok()?),
        "frame" => WikiObject::Frame(serde_json::from_str::<WikiFrame>(json).ok()?),
        "reading" => WikiObject::Reading(serde_json::from_str::<WikiReading>(json).ok()?),
        _ => return None,
    })
}

/// Where the ledgers of one AIKit home live.
pub(super) struct DiscoveryLedgers {
    directory: PathBuf,
}

impl DiscoveryLedgers {
    pub(super) fn in_cache(cache: &Path) -> Self {
        Self {
            directory: cache.join("knowledge/discovery"),
        }
    }

    /// The recorded outcome, when every observation still reads the same.
    /// An absent, unreadable, foreign or stale ledger answers nothing.
    pub(super) fn current(
        &self,
        key: &LedgerKey,
        root_canonical: &Path,
        world_canonical: Option<&Path>,
    ) -> Option<DiscoveryOutcome> {
        let path = self.directory.join(key.file_name()?);
        let bytes = fs::read(path).ok()?;
        let file: LedgerFile = serde_json::from_slice(&bytes).ok()?;
        let stopped = file.schema == STOPPED_SCHEMA;
        if (file.schema != SCHEMA && !stopped)
            || &file.key != key
            || file.root_canonical != root_canonical
            || file.world_canonical.as_deref() != world_canonical
        {
            return None;
        }
        for observation in &file.observations {
            if observation.repeat() != observation.outcome {
                return None;
            }
        }
        let wiki = file.wiki.iter().map(decode).collect::<Option<Vec<_>>>()?;
        Some(DiscoveryOutcome {
            wiki,
            sources: file.sources,
            absences: file.absences,
            deadline_stopped: file.deadline_stopped || stopped,
        })
    }

    /// Keep a walk for the next invocation. A racy walk, or one whose facts
    /// cannot be written faithfully, is not kept; the next walk runs in full.
    pub(super) fn keep(
        &self,
        key: &LedgerKey,
        root_canonical: &Path,
        world_canonical: Option<&Path>,
        recorder: DiscoveryRecorder,
        outcome: &DiscoveryOutcome,
    ) {
        if recorder.racy {
            return;
        }
        let Some(name) = key.file_name() else {
            return;
        };
        let Some(wiki) = outcome.wiki.iter().map(encode).collect::<Option<Vec<_>>>() else {
            return;
        };
        let file = LedgerFile {
            schema: if outcome.deadline_stopped {
                STOPPED_SCHEMA
            } else {
                SCHEMA
            }
            .into(),
            key: key.clone(),
            root_canonical: root_canonical.to_path_buf(),
            world_canonical: world_canonical.map(Path::to_path_buf),
            observations: recorder.observations,
            wiki,
            sources: outcome.sources.clone(),
            absences: outcome.absences.clone(),
            deadline_stopped: outcome.deadline_stopped,
        };
        let Ok(bytes) = serde_json::to_vec(&file) else {
            return;
        };
        if fs::create_dir_all(&self.directory).is_err() {
            return;
        }
        let target = self.directory.join(&name);
        let staging = self.directory.join(format!(
            ".{name}.{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        if fs::write(&staging, bytes).is_err() || fs::rename(&staging, &target).is_err() {
            let _ = fs::remove_file(&staging);
        }
    }
}

/// Shared recording handle for the walk's observe/commit closures.
pub(super) type SharedRecorder<'a> = Option<&'a RefCell<DiscoveryRecorder>>;
