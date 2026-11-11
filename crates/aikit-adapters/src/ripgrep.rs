//! The ripgrep fast path: bounded literal/regex content search over explicitly
//! named roots.
//!
//! Ripgrep is the direct content-search provider — the freshness fallback when
//! no index exists and the exact-match floor when no embeddings are wanted. It
//! is deliberately *not* a second retrieval grammar: callers hand it roots and
//! glob-authorized scopes; it answers with structured matches. Three bounds
//! travel with every call:
//!
//! * an explicit argument array (no shell, no inherited `RIPGREP_CONFIG_PATH`),
//! * a per-file byte budget (`--max-filesize`) and column budget,
//! * a wall-clock budget enforced by the runner seam, so a pathological tree
//!   costs one timeout, not an unbounded wait.
//!
//! Exit status is data: 0 means matches, 1 means a clean no-matches answer,
//! 2 means ripgrep itself failed. Only the last is an error.
//!
//! A glob is matched against each root-relative path ([`SearchRequest`]).
//! ripgrep matches `--glob` against a path relative to its working directory,
//! so each root that is a directory is searched by its own process, started in
//! that root, over `.`, and its hits are named under the root again
//! ([`SearchRequest::passes`]). The folders above a root are never part of what
//! a glob sees: an ancestor named `target` or `ProjectCentral` excludes nothing,
//! and an anchored `/secret/**` withholds the root's own `secret` folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::runner::CommandRunner;
use aikit_core::{AikitError, Result};

/// Per-file byte budget handed to `--max-filesize`. Files above it are skipped
/// by ripgrep itself, which keeps a stray binary or log from dominating a scan.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024;
/// `max_file_bytes` for a search that must see every file its globs admit: the
/// flag is left off. A skipped file is invisible, not refused, so a source over
/// the budget could not be found at all.
pub const NO_FILE_BOUND: u64 = u64::MAX;
/// Long matched lines are previewed, not returned whole: a minified asset must
/// not be able to push megabytes through one match event.
pub const MAX_COLUMNS: u32 = 240;
/// Bounded parallelism; the NOW field is small and this is not a fleet scan.
pub const SEARCH_THREADS: u32 = 2;
/// The ripgrep release the knowledge-path searchers were verified against. A
/// different installed release is disclosed as version drift in `status()`,
/// matching the bkmr/GitNexus convention, never refused: literal content
/// search is a stable surface.
/// What one retained match may cost a bounded pass in read bytes
/// ([`RipgrepSearcher::search_bounded`]): its event, the line cut at
/// `MAX_COLUMNS` with its submatches, and its file's begin and end events.
pub const MATCH_EVENT_BYTES: usize = 2048;

/// The wall-clock budget of a bounded pass whose request names none.
const BOUNDED_SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

pub const RIPGREP_TESTED_VERSION: &str = "15.2.0";

pub fn executable() -> PathBuf {
    std::env::var_os("AIKIT_RIPGREP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rg"))
}

/// One content-search request. Every field is explicit; there is no implicit
/// scope and no configuration file can widen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub pattern: String,
    /// Literal by default. Regex is a deliberate caller decision because a
    /// caller-supplied pattern is also caller-supplied syntax.
    pub regex: bool,
    /// Case-fold the match. Off by default: the NOW field answers for exact
    /// ground. A code-search caller opts in, because code spells the same
    /// word in many cases (`mcp`, `Mcp`, `MCP`).
    pub ignore_case: bool,
    pub roots: Vec<PathBuf>,
    /// Include globs, matched against each root-relative path.
    pub include_globs: Vec<String>,
    /// Deny globs; ripgrep applies a later glob over an earlier one, so these
    /// are always emitted after the includes and always win. They apply to
    /// what a root that is a folder holds, never to a root that is a file:
    /// ripgrep searches a file it is named whatever the globs say.
    pub exclude_globs: Vec<String>,
    /// Search dotfiles and gitignored paths *inside the authorised scope*.
    /// Eligibility is carried by the include/deny globs, never by ignore files.
    pub hidden: bool,
    pub max_file_bytes: u64,
    /// Maximum matches retained; matches beyond it are disclosed as truncation.
    pub limit: usize,
    /// Per-file match cap (`--max-count`). A ranked caller that only needs
    /// evidence per file sets it, so one file full of a common word cannot
    /// spend the whole retained budget before other files are seen.
    pub max_count_per_file: Option<u64>,
    pub timeout: Option<Duration>,
}

impl SearchRequest {
    pub fn new(pattern: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        Self {
            pattern: pattern.into(),
            regex: false,
            ignore_case: false,
            roots,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            hidden: false,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            limit: usize::MAX,
            max_count_per_file: None,
            timeout: Some(Duration::from_secs(30)),
        }
    }

    /// The exact argv of this request over its roots as given. Tests assert on
    /// this so the flags the contract depends on are pinned in one place. A
    /// root that is a directory is searched over `.` from inside it instead
    /// ([`SearchRequest::passes`]); the flags are the same.
    pub fn argv(&self, executable: &Path) -> Vec<String> {
        self.argv_over(
            executable,
            self.roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned()),
            false,
        )
    }

    /// The argv over `paths`. On a volume that looks names up without regard
    /// to case (`folded`), the deny globs are matched so too (`--iglob`): there
    /// `Secret` and `secret` name one folder, so a glob that withholds one
    /// withholds the other.
    fn argv_over(
        &self,
        executable: &Path,
        paths: impl IntoIterator<Item = String>,
        folded: bool,
    ) -> Vec<String> {
        let mut argv = vec![
            executable.to_string_lossy().into_owned(),
            "--json".into(),
            "--no-messages".into(),
        ];
        if self.max_file_bytes != NO_FILE_BOUND {
            argv.push("--max-filesize".into());
            argv.push(format_filesize(self.max_file_bytes));
        }
        argv.extend([
            "--max-columns".into(),
            MAX_COLUMNS.to_string(),
            "--max-columns-preview".into(),
            "--threads".into(),
            SEARCH_THREADS.to_string(),
        ]);
        if !self.regex {
            argv.push("--fixed-strings".into());
        }
        if self.ignore_case {
            argv.push("--ignore-case".into());
        }
        if self.hidden {
            argv.push("--hidden".into());
            argv.push("--no-ignore".into());
        }
        if let Some(cap) = self.max_count_per_file {
            argv.push("--max-count".into());
            argv.push(cap.to_string());
        }
        for glob in &self.include_globs {
            argv.push("--glob".into());
            argv.push(glob.clone());
        }
        let deny = if folded { "--iglob" } else { "--glob" };
        for glob in &self.exclude_globs {
            argv.push(deny.into());
            argv.push(format!("!{glob}"));
        }
        argv.push("-e".into());
        argv.push(self.pattern.clone());
        argv.extend(paths);
        argv
    }

    /// The ripgrep processes this request runs. Each root that is a directory
    /// is searched by its own, started in that root, over `.`, so its globs
    /// are matched against root-relative paths, as the contract says, and
    /// never against the folders above it. The roots that are not directories
    /// (single files, where no glob applies) share one, over their paths as
    /// given, started wherever the runner starts it. A relative executable
    /// path (`tools/rg`) is named from here for a pass started in a root, so
    /// it is the same program wherever that pass starts. A root on a volume
    /// that looks names up without regard to case has its deny globs matched
    /// so too ([`folds_case`]). No glob applies to a single file: a caller
    /// withholds a file root before it names one.
    pub fn passes(&self, executable: &Path) -> Vec<RipgrepPass> {
        let (folders, files): (Vec<&PathBuf>, Vec<&PathBuf>) = self
            .roots
            .iter()
            .partition(|root| std::fs::metadata(root).is_ok_and(|meta| meta.is_dir()));
        let started_in_a_root = started_anywhere(executable);
        let mut passes: Vec<RipgrepPass> = folders
            .into_iter()
            .map(|root| RipgrepPass {
                root: Some(root.clone()),
                argv: self.argv_over(&started_in_a_root, [".".to_string()], folds_case(root)),
            })
            .collect();
        if !files.is_empty() {
            passes.push(RipgrepPass {
                root: None,
                argv: self.argv_over(
                    executable,
                    // A relative file named with a leading dash is named from `.`, so ripgrep reads a path and not a flag.
                    files.into_iter().map(|file| {
                        let named = file.to_string_lossy().into_owned();
                        if named.starts_with('-') {
                            format!("./{named}")
                        } else {
                            named
                        }
                    }),
                    false,
                ),
            });
        }
        passes
    }

    /// The exact argv of the count form of this request: the same scope,
    /// budgets and globs, answering `path NUL matching-line-count` per file
    /// instead of match events. Nothing is truncated: every file is counted.
    pub fn count_argv(&self, executable: &Path) -> Vec<String> {
        counted(self.argv(executable))
    }

    fn is_no_match(&self) -> bool {
        self.pattern.is_empty()
    }
}

/// The executable as a pass started in another folder must name it: a bare
/// name is looked up on `PATH` wherever it starts, and an absolute path is the
/// same anywhere, but a relative path with a folder in it is named from here.
fn started_anywhere(executable: &Path) -> PathBuf {
    if executable.is_relative() && executable.components().count() > 1 {
        std::path::absolute(executable).unwrap_or_else(|_| executable.to_path_buf())
    } else {
        executable.to_path_buf()
    }
}

/// Whether the volume `folder` stands on looks names up without regard to case,
/// as a default APFS or HFS+ volume does: an entry of the folder, named with
/// its ASCII letters' case swapped, is the same file. A folder none of whose
/// first entries has an ASCII letter is judged by its own name in its parent,
/// when that parent is on the same volume. Where nothing can be judged (no
/// name with a letter to try, or a parent on another volume) the answer is
/// yes: the caller asks for its deny globs, and a deny glob matched without
/// regard to case withholds no less than one matched with it (round 55 item
/// 8c; the review of #616, F5).
pub(crate) fn folds_case(folder: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let same = |one: &Path, other: &Path| match (
            std::fs::symlink_metadata(one),
            std::fs::symlink_metadata(other),
        ) {
            (Ok(one), Ok(other)) => one.dev() == other.dev() && one.ino() == other.ino(),
            _ => false,
        };
        let swapped = |name: &std::ffi::OsStr| {
            let name = name.to_str()?;
            let other: String = name
                .chars()
                .map(|ch| {
                    if ch.is_ascii_lowercase() {
                        ch.to_ascii_uppercase()
                    } else {
                        ch.to_ascii_lowercase()
                    }
                })
                .collect();
            (other != name).then(|| (name.to_string(), other))
        };
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten().take(64) {
                if let Some((name, other)) = swapped(&entry.file_name()) {
                    return same(&folder.join(name), &folder.join(other));
                }
            }
        }
        let (Some(parent), Some((name, other))) =
            (folder.parent(), folder.file_name().and_then(swapped))
        else {
            return true;
        };
        let on_one_volume = match (std::fs::metadata(parent), std::fs::metadata(folder)) {
            (Ok(parent), Ok(folder)) => parent.dev() == folder.dev(),
            _ => false,
        };
        !on_one_volume || same(&parent.join(name), &parent.join(other))
    }
    #[cfg(not(unix))]
    {
        let _ = folder;
        cfg!(windows)
    }
}

/// One ripgrep process of a request ([`SearchRequest::passes`]): the root it is
/// started in and searches over `.`, or none for the pass over single files
/// named as given, and its argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipgrepPass {
    pub root: Option<PathBuf>,
    pub argv: Vec<String>,
}

impl RipgrepPass {
    /// A path ripgrep named, as the request named it: under the pass's root,
    /// where ripgrep walked `.` from inside it.
    fn named(&self, path: PathBuf) -> PathBuf {
        match &self.root {
            Some(root) if path.is_relative() => root.join(path.strip_prefix(".").unwrap_or(&path)),
            _ => path,
        }
    }
}

/// The count form of a match argv: `path NUL matching-line-count` per file in
/// place of match events, every file counted. Each file is named, a single
/// file as well (`--with-filename`), so every record carries its path and none
/// is told apart by how many roots the request has.
fn counted(mut argv: Vec<String>) -> Vec<String> {
    if let Some(json) = argv.iter().position(|arg| arg == "--json") {
        argv.splice(
            json..=json,
            [
                "--count".to_string(),
                "--null".to_string(),
                "--with-filename".to_string(),
            ],
        );
    }
    if let Some(cap) = argv.iter().position(|arg| arg == "--max-count") {
        argv.drain(cap..=cap + 1);
    }
    argv
}

/// The records of a count pass's stream, each `path NUL count LF`, read where
/// they cannot be split wrongly: a path holds no NUL, and a count is digits
/// alone, so a record ends at the first LF after its NUL, whatever LFs its path
/// holds. A stream that is not such records is refused, not read in part.
fn count_records(stream: &str) -> Result<Vec<(PathBuf, u64)>> {
    let invalid = || {
        AikitError::new(
            "search.ripgrep_invalid_output",
            "ripgrep --count emitted a record that is not a path, a NUL and a count",
        )
    };
    let mut records = Vec::new();
    let mut rest = stream;
    while !rest.is_empty() {
        let (path, after) = rest.split_once('\0').ok_or_else(invalid)?;
        let (count, next) = after.split_once('\n').unwrap_or((after, ""));
        let count = count.parse::<u64>().map_err(|_| invalid())?;
        records.push((PathBuf::from(path), count));
        rest = next;
    }
    Ok(records)
}

fn format_filesize(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{}M", bytes / MIB)
    } else if bytes >= KIB && bytes.is_multiple_of(KIB) {
        format!("{}K", bytes / KIB)
    } else {
        bytes.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipgrepMatch {
    pub path: PathBuf,
    pub line_number: u64,
    pub line: String,
}

/// What one bounded search actually saw. `truncated` is the honest count of
/// matches that existed beyond the retained budget — a caller that must not
/// silently answer a narrower question than it asked reads this.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RipgrepOutcome {
    pub matches: Vec<RipgrepMatch>,
    pub files_searched: usize,
    pub truncated: bool,
    /// How many passes of a bounded search were stopped at their capacity and
    /// asked again for a sample ([`RipgrepSearcher::search_bounded`]). Each
    /// cost up to two more reads of its allowance, so a caller can tell a
    /// search that read once from one that read up to three times a root.
    pub asked_again: usize,
}

impl RipgrepOutcome {
    /// One pass's outcome added to the request's, its paths named under the
    /// pass's root, in path order and then line order: ripgrep walks a root on
    /// more than one thread, so the order of its stream is not the same twice
    /// (round 55 item 8c; the review of #616, F5). Which matches a limit keeps
    /// is still the stream's to say; what is kept is ordered.
    fn add(&mut self, pass: &RipgrepPass, mut part: RipgrepOutcome) {
        self.files_searched += part.files_searched;
        self.truncated |= part.truncated;
        self.asked_again += part.asked_again;
        part.matches.sort_by(|one, other| {
            (&one.path, one.line_number).cmp(&(&other.path, other.line_number))
        });
        self.matches
            .extend(part.matches.into_iter().map(|matched| RipgrepMatch {
                path: pass.named(matched.path),
                ..matched
            }));
    }

    /// The matches in the order the request named its roots: a root that is a
    /// file where the request named it, and not after every folder, though the
    /// single files are searched by one pass after the folders'. A match is its
    /// root's by being that file, or else by standing under that folder; the
    /// order inside a root is kept.
    fn in_request_order(mut self, request: &SearchRequest) -> Self {
        let place = |matched: &RipgrepMatch| {
            request
                .roots
                .iter()
                .position(|root| *root == matched.path)
                .or_else(|| {
                    request
                        .roots
                        .iter()
                        .position(|root| matched.path.starts_with(root))
                })
                .unwrap_or(request.roots.len())
        };
        self.matches.sort_by_key(place);
        self
    }
}

/// The clock a request's budget is read by. A test moves it on by hand
/// (`clock::advance`), so what a shared budget leaves each pass is asserted
/// on a logical clock, not on how long the machine took.
fn now() -> std::time::Instant {
    #[cfg(test)]
    {
        std::time::Instant::now() + clock::skew()
    }
    #[cfg(not(test))]
    {
        std::time::Instant::now()
    }
}

#[cfg(test)]
pub(crate) mod clock {
    use std::cell::Cell;
    use std::time::Duration;

    thread_local! {
        static SKEW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    }

    pub(crate) fn skew() -> Duration {
        SKEW.with(Cell::get)
    }

    /// Move this thread's clock on by `by`, as if that much time had passed.
    pub(crate) fn advance(by: Duration) {
        SKEW.with(|skew| skew.set(skew.get() + by));
    }
}

/// The one wall-clock budget every pass of a request shares.
struct Deadline(Option<std::time::Instant>);

impl Deadline {
    fn new(budget: Option<Duration>) -> Self {
        Self(budget.and_then(|budget| now().checked_add(budget)))
    }

    /// What is left of the budget for the next pass, or a timeout once it is
    /// spent, as the runner names one.
    fn remaining(&self, argv: &[String]) -> Result<Option<Duration>> {
        let Some(deadline) = self.0 else {
            return Ok(None);
        };
        let left = deadline.saturating_duration_since(now());
        if left.is_zero() {
            return Err(AikitError::new(
                "mux.command_timeout",
                "ripgrep's search budget was spent before its next pass",
            )
            .with("command", argv.join(" "))
            .with("execution_started", "false"));
        }
        Ok(Some(left))
    }
}

/// The matches and read bytes the passes of one bounded request share
/// ([`RipgrepSearcher::search_bounded`]). Each pass may keep an equal part of
/// the matches still unkept, rounded up, and read an equal part of the bytes
/// still unread; what a pass leaves unused is carried on to the passes after
/// it, so a first root cannot starve the rest. The first read of every pass is
/// bounded together as one process over every root was: its matches' worth of
/// events and one more, on each of its two streams. A pass stopped at its
/// capacity reads at most its allowance twice more for its sample: three
/// allowances for that pass in the worst case.
struct Share {
    matches: usize,
    bytes: usize,
    passes: usize,
}

impl Share {
    fn new(limit: usize, passes: usize) -> Self {
        Self {
            matches: limit,
            bytes: limit
                .saturating_add(1)
                .saturating_mul(MATCH_EVENT_BYTES)
                .saturating_mul(2),
            passes: passes.max(1),
        }
    }

    /// What the next pass may keep, and what it may read.
    fn next(&self) -> (usize, usize) {
        (self.matches.div_ceil(self.passes), self.bytes / self.passes)
    }

    /// A stopped pass kept `kept` matches after all, as its sample.
    fn kept(&mut self, kept: usize) {
        self.matches = self.matches.saturating_sub(kept);
    }

    /// A pass kept `kept` matches and read `read` bytes.
    fn spent(&mut self, kept: usize, read: usize) {
        self.matches = self.matches.saturating_sub(kept);
        self.bytes = self.bytes.saturating_sub(read);
        self.passes = self.passes.saturating_sub(1).max(1);
    }
}

pub struct RipgrepSearcher<R> {
    runner: R,
    executable: PathBuf,
}

impl<R: CommandRunner> RipgrepSearcher<R> {
    pub fn new(runner: R, executable: impl Into<PathBuf>) -> Self {
        Self {
            runner,
            executable: executable.into(),
        }
    }

    /// The installed ripgrep version's first line, or an error naming the
    /// unavailability. Providers call this at attachment so `status()` can
    /// disclose rather than search failing later.
    pub fn probe(&self) -> Result<String> {
        let argv = vec![
            self.executable.to_string_lossy().into_owned(),
            "--version".into(),
        ];
        let output = self.runner.run(&argv)?;
        if !output.ok() {
            return Err(output
                .require(&argv, "search.ripgrep_unavailable")
                .unwrap_err());
        }
        Ok(output
            .stdout
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    pub fn search(&self, request: &SearchRequest) -> Result<RipgrepOutcome> {
        if request.is_no_match() {
            return Ok(RipgrepOutcome::default());
        }
        if request.roots.is_empty() {
            return Err(AikitError::new(
                "search.scope_empty",
                "ripgrep search requested with no authorised roots",
            ));
        }
        // The declared wall-clock budget is enforced here, at the one place
        // every knowledge-path search travels through: a pathological tree
        // costs one timeout, not an unbounded wait. The runner seam does the
        // killing; a timed-out search is a runner error, not a failed search.
        // Every pass of the request shares the one budget.
        let request = self.rooted(request);
        let deadline = Deadline::new(request.timeout);
        let mut outcome = RipgrepOutcome::default();
        for pass in request.passes(&self.executable) {
            let timeout = deadline.remaining(&pass.argv)?;
            let output = match &pass.root {
                Some(root) => self
                    .runner
                    .run_in(root, &pass.argv, timeout)
                    .map_err(|error| unsearchable(&pass, error))?,
                None => match timeout {
                    Some(budget) => self.runner.run_with_timeout(&pass.argv, budget)?,
                    None => self.runner.run(&pass.argv)?,
                },
            };
            let kept = request.limit.saturating_sub(outcome.matches.len());
            let part = Self::outcome_of(kept, &pass, &output)
                .map_err(|error| never_stood(&request, &pass, error))?;
            outcome.add(&pass, part);
        }
        Ok(outcome.in_request_order(&request))
    }

    /// [`RipgrepSearcher::search`] with what is read bounded as well as what
    /// is kept: the stream may carry `request.limit` matches' worth of events
    /// (`MATCH_EVENT_BYTES` each), and a stream past that is not read on. The
    /// runner stops it at that capacity and refuses the output whole (no
    /// truncated success), so the pass answers truncated with no sample and
    /// a ranking caller rescores from its exact counts. A runner that cannot
    /// bound one read runs the pass unbounded, as [`RipgrepSearcher::search`].
    ///
    /// The passes of a request over several roots share its matches and its
    /// read fairly ([`Share`]): each may keep and read an equal part of what
    /// is left. A folder pass stopped at its capacity answers truncated, and
    /// is asked again for a sample: the files that match, then one match of
    /// each of the first of them ([`RipgrepSearcher::sample_of_stopped`]). So
    /// a root is represented when the limit allows a match a root and the list
    /// of its matching files fits its allowance; where that list does not fit,
    /// and for the pass over single files, a stopped pass keeps no sample.
    /// What the other passes keep stands either way. The worst case of the
    /// read is so: each pass reads its allowance once, and a stopped pass at
    /// most twice more, three allowances in all; the first reads of every pass
    /// together are bounded as one process over every root was. The outcome
    /// counts the passes asked again (`asked_again`). Every read, the two
    /// further ones too, runs under the request's one timeout.
    pub fn search_bounded(&self, request: &SearchRequest) -> Result<RipgrepOutcome> {
        if request.is_no_match() || request.roots.is_empty() || request.limit == usize::MAX {
            return self.search(request);
        }
        let request = self.rooted(request);
        let deadline = Deadline::new(Some(request.timeout.unwrap_or(BOUNDED_SEARCH_TIMEOUT)));
        let passes = request.passes(&self.executable);
        let mut share = Share::new(request.limit, passes.len());
        let mut outcome = RipgrepOutcome::default();
        for pass in &passes {
            // Both streams share the allowance half and half: stdout's half is
            // the match budget, what this pass may keep.
            let (kept, allowance) = share.next();
            let budget = deadline
                .remaining(&pass.argv)?
                .unwrap_or(BOUNDED_SEARCH_TIMEOUT);
            let output = match &pass.root {
                Some(root) => self
                    .runner
                    .run_in_with_limits(root, &pass.argv, budget, allowance, false),
                None => self
                    .runner
                    .run_with_limits(&pass.argv, budget, allowance, false),
            };
            match output {
                Ok(output) => {
                    let part = Self::outcome_of(kept, pass, &output)
                        .map_err(|error| never_stood(&request, pass, error))?;
                    share.spent(
                        part.matches.len(),
                        output.stdout.len().saturating_add(output.stderr.len()),
                    );
                    outcome.add(pass, part);
                }
                Err(error) if error.code() == "mux.command_output_limit" => {
                    share.spent(0, allowance);
                    outcome.truncated = true;
                    let (asked, sample) = self.sample_of_stopped(pass, kept, allowance, &deadline);
                    outcome.asked_again += usize::from(asked);
                    if let Some(sample) = sample {
                        share.kept(sample.matches.len());
                        outcome.add(pass, sample);
                    }
                }
                Err(error) if error.code() == "mux.command_limits_unsupported" => {
                    return self.search(&request)
                }
                Err(error) => return Err(unsearchable(pass, error)),
            }
        }
        Ok(outcome.in_request_order(&request))
    }

    /// A sample of a folder pass that was stopped at its capacity, so its root
    /// is still represented (round 55 item 8c; the review of #616, the share).
    /// The pass is asked again for less, twice, each read under the stopped
    /// pass's own allowance: first the files that match, by name and count,
    /// which is a far smaller stream than their match events; then one match of
    /// each of the first `kept` of them, in path order. Where either read
    /// fails or runs past its allowance too, there is no sample, as before.
    /// Both reads run under what is left of the request's one timeout, and
    /// neither starts once it is spent. Answers whether the pass was asked
    /// again at all, and its sample.
    fn sample_of_stopped(
        &self,
        pass: &RipgrepPass,
        kept: usize,
        allowance: usize,
        deadline: &Deadline,
    ) -> (bool, Option<RipgrepOutcome>) {
        let Some(root) = pass.root.as_ref().filter(|_| kept > 0) else {
            return (false, None);
        };
        let listing = counted(pass.argv.clone());
        let Ok(budget) = deadline.remaining(&listing) else {
            return (false, None);
        };
        (
            true,
            self.sample_within(root, pass, kept, allowance, deadline, &listing, budget),
        )
    }

    /// The two reads of [`RipgrepSearcher::sample_of_stopped`].
    #[allow(clippy::too_many_arguments)]
    fn sample_within(
        &self,
        root: &Path,
        pass: &RipgrepPass,
        kept: usize,
        allowance: usize,
        deadline: &Deadline,
        listing: &[String],
        budget: Option<Duration>,
    ) -> Option<RipgrepOutcome> {
        let listed = self
            .runner
            .run_in_with_limits(
                root,
                listing,
                budget.unwrap_or(BOUNDED_SEARCH_TIMEOUT),
                allowance,
                false,
            )
            .ok()?;
        let mut files: Vec<PathBuf> = count_records(&listed.stdout)
            .ok()?
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(path, _)| path)
            .collect();
        files.sort();
        files.truncate(kept);
        if files.is_empty() {
            return None;
        }
        let sampling = one_match_of_each(&pass.argv, &files);
        let budget = deadline.remaining(&sampling).ok()?;
        let sampled = self
            .runner
            .run_in_with_limits(
                root,
                &sampling,
                budget.unwrap_or(BOUNDED_SEARCH_TIMEOUT),
                allowance,
                false,
            )
            .ok()?;
        let mut sample = Self::outcome_of(kept, pass, &sampled).ok()?;
        // The sample is one match a file: whether more stand is the stopped pass's to say, and it has.
        sample.truncated = true;
        Some(sample)
    }

    /// One pass's answer, keeping at most `kept` matches.
    fn outcome_of(
        kept: usize,
        pass: &RipgrepPass,
        output: &crate::runner::Output,
    ) -> Result<RipgrepOutcome> {
        // 0 = matches, 1 = clean no-match. Both parse their stream; only a
        // ripgrep failure (2) becomes an error.
        let outcome = parse_events(&output.stdout, kept)?;
        if output.status == 0 || output.status == 1 {
            return Ok(outcome);
        }
        let detail = if output.stderr.trim().is_empty() {
            "ripgrep reported no diagnostic".to_string()
        } else {
            output.stderr.trim().to_string()
        };
        Err(failed(
            pass,
            format!(
                "ripgrep exited with status {}{}: {detail}",
                output.status,
                searching(pass)
            ),
        )
        .with("status", output.status.to_string()))
    }

    /// The request with each relative root named from the folder this
    /// runner starts its commands in, when that is not this process's own
    /// ([`CommandRunner::starts_in`]). Every pass is then classified, started
    /// and named by the folder ripgrep reads it in, not by this process's.
    fn rooted<'a>(&self, request: &'a SearchRequest) -> std::borrow::Cow<'a, SearchRequest> {
        let Some(base) = self.runner.starts_in() else {
            return std::borrow::Cow::Borrowed(request);
        };
        if request.roots.iter().all(|root| root.is_absolute()) {
            return std::borrow::Cow::Borrowed(request);
        }
        let base = std::path::absolute(&base).unwrap_or(base);
        let mut named = request.clone();
        named.roots = request
            .roots
            .iter()
            .map(|root| {
                if root.is_relative() {
                    base.join(root)
                } else {
                    root.clone()
                }
            })
            .collect();
        std::borrow::Cow::Owned(named)
    }
}

/// A folder pass's argv turned to its sample pass ([`RipgrepSearcher::sample_of_stopped`]): the same flags over `files`, named
/// from the pass's root in place of `.`, with one match kept a file. A name ripgrep could read as a flag is named from `.`.
fn one_match_of_each(argv: &[String], files: &[PathBuf]) -> Vec<String> {
    let mut argv = argv.to_vec();
    argv.pop();
    match argv.iter().position(|arg| arg == "--max-count") {
        Some(cap) => argv[cap + 1] = "1".into(),
        None => {
            let pattern = argv
                .iter()
                .rposition(|arg| arg == "-e")
                .unwrap_or(argv.len());
            argv.splice(
                pattern..pattern,
                ["--max-count".to_string(), "1".to_string()],
            );
        }
    }
    argv.extend(files.iter().map(|file| {
        let named = file.to_string_lossy().into_owned();
        if named.starts_with('-') {
            format!("./{named}")
        } else {
            named
        }
    }));
    argv
}

/// ` searching <root>`, for a pass started in a root; nothing for the pass over
/// single files, whose command names them.
fn searching(pass: &RipgrepPass) -> String {
    pass.root
        .as_ref()
        .map(|root| format!(" searching {}", root.display()))
        .unwrap_or_default()
}

/// A ripgrep failure over one pass, naming its command and the root it searched.
fn failed(pass: &RipgrepPass, message: String) -> AikitError {
    let error =
        AikitError::new("search.ripgrep_failed", message).with("command", pass.argv.join(" "));
    match &pass.root {
        Some(root) => error.with("root", root.display().to_string()),
        None => error,
    }
}

/// A pass that could not start in its root because the root cannot be read or
/// entered is ripgrep's failure to search it (`search.ripgrep_failed`), as a
/// search over that root answered before each root's pass started in it, in
/// words naming the root. Any other refusal is the runner's own.
fn unsearchable(pass: &RipgrepPass, error: AikitError) -> AikitError {
    let Some(root) = &pass.root else {
        return error;
    };
    if error.code() != "mux.command_spawn_failed" {
        return error;
    }
    let cause = std::fs::read_dir(root)
        .err()
        .or_else(|| std::fs::metadata(root.join(".")).err());
    match cause {
        Some(cause) => failed(
            pass,
            format!("ripgrep could not search {}: {cause}", root.display()),
        ),
        None => error,
    }
}

/// The failure of the pass over single files, when one of the roots it was given is not there: that root is named, in the
/// words an unreadable folder root is named in (`unsearchable`), since ripgrep's own failure over a missing path names nothing
/// under `--no-messages` (round 55 item 8c; the review of #616, F3). A root that is not a folder goes to that pass, a missing
/// one too (`SearchRequest::passes`). A failure ripgrep gave a diagnostic for, and any other, is passed on as it is.
fn never_stood(request: &SearchRequest, pass: &RipgrepPass, error: AikitError) -> AikitError {
    // Only where ripgrep itself said nothing: a diagnostic it gave is the better word.
    let silent = error.message().contains("ripgrep reported no diagnostic")
        || error.message().starts_with("ripgrep count exited");
    if pass.root.is_some() || error.code() != "search.ripgrep_failed" || !silent {
        return error;
    }
    let missing = request.roots.iter().find_map(|root| {
        std::fs::symlink_metadata(root)
            .err()
            .filter(|cause| cause.kind() == std::io::ErrorKind::NotFound)
            .map(|cause| (root, cause))
    });
    match missing {
        Some((root, cause)) => failed(
            pass,
            format!("ripgrep could not search {}: {cause}", root.display()),
        )
        .with("root", root.display().to_string()),
        None => error,
    }
}

impl<R: CommandRunner> RipgrepSearcher<R> {
    /// Count matching lines per file over the request's whole scope. The
    /// exact, untruncated companion of [`RipgrepSearcher::search`] for a
    /// ranking caller whose match pass was truncated.
    pub fn count(&self, request: &SearchRequest) -> Result<Vec<(PathBuf, u64)>> {
        if request.is_no_match() {
            return Ok(Vec::new());
        }
        let request = self.rooted(request);
        let deadline = Deadline::new(request.timeout);
        let mut counts = Vec::new();
        for pass in request.passes(&self.executable) {
            let pass = RipgrepPass {
                argv: counted(pass.argv.clone()),
                ..pass
            };
            let timeout = deadline.remaining(&pass.argv)?;
            let output = match &pass.root {
                Some(root) => self
                    .runner
                    .run_in(root, &pass.argv, timeout)
                    .map_err(|error| unsearchable(&pass, error))?,
                None => match timeout {
                    Some(budget) => self.runner.run_with_timeout(&pass.argv, budget)?,
                    None => self.runner.run(&pass.argv)?,
                },
            };
            if output.status != 0 && output.status != 1 {
                return Err(never_stood(
                    &request,
                    &pass,
                    failed(
                        &pass,
                        format!(
                            "ripgrep count exited with status {}{}",
                            output.status,
                            searching(&pass)
                        ),
                    ),
                ));
            }
            for (path, count) in count_records(&output.stdout)? {
                counts.push((pass.named(path), count));
            }
        }
        counts.sort();
        Ok(counts)
    }
}

fn parse_events(stream: &str, limit: usize) -> Result<RipgrepOutcome> {
    let mut outcome = RipgrepOutcome::default();
    for line in stream.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(trimmed).map_err(|e| {
            AikitError::new(
                "search.ripgrep_invalid_output",
                format!("ripgrep --json emitted a non-JSON line: {e}"),
            )
        })?;
        match event["type"].as_str() {
            Some("begin") => {
                outcome.files_searched += 1;
            }
            Some("match") => {
                let data = &event["data"];
                let Some(path) = data["path"]["text"].as_str() else {
                    // Binary or undecodable path: real ripgrep behaviour, not a
                    // contract breach. It is simply not a textual match.
                    continue;
                };
                let Some(text) = data["lines"]["text"].as_str() else {
                    continue;
                };
                if outcome.matches.len() < limit {
                    outcome.matches.push(RipgrepMatch {
                        path: PathBuf::from(path),
                        line_number: data["line_number"].as_u64().unwrap_or_default(),
                        line: text.trim_end_matches(['\n', '\r']).to_string(),
                    });
                } else {
                    outcome.truncated = true;
                }
            }
            // begin/end/summary/context events carry no matches.
            _ => {}
        }
    }
    Ok(outcome)
}

/// Real-filesystem helper for integration tests and live wiring: does the
/// installed binary answer at all? Not a contract test — a presence probe.
pub fn available() -> bool {
    std::process::Command::new(executable())
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{ScriptedRunner, SystemRunner};

    fn rg_json_line(path: &str, line_number: u64, text: &str) -> String {
        format!(
            r#"{{"type":"match","data":{{"path":{{"text":"{path}"}},"lines":{{"text":"{text}\n"}},"line_number":{line_number},"submatches":[]}}}}"#
        )
    }

    fn request() -> SearchRequest {
        SearchRequest {
            pattern: "harness gate".into(),
            regex: false,
            ignore_case: false,
            roots: vec![PathBuf::from("/ground")],
            include_globs: vec!["Control/agents/now/**".into()],
            exclude_globs: vec![".git/**".into()],
            hidden: true,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            limit: 10,
            max_count_per_file: None,
            timeout: Some(Duration::from_secs(30)),
        }
    }

    #[test]
    fn case_folding_is_a_deliberate_caller_opt_in() {
        assert!(!request()
            .argv(Path::new("rg"))
            .contains(&"--ignore-case".to_string()));
        let mut folded = request();
        folded.ignore_case = true;
        assert!(folded
            .argv(Path::new("rg"))
            .contains(&"--ignore-case".to_string()));
    }

    #[test]
    fn literal_is_the_default_and_regex_is_a_deliberate_flag() {
        let argv = request().argv(Path::new("rg"));
        assert!(argv.contains(&"--fixed-strings".to_string()));
        let mut regex_request = request();
        regex_request.regex = true;
        assert!(!regex_request
            .argv(Path::new("rg"))
            .contains(&"--fixed-strings".to_string()));
    }

    #[test]
    fn deny_globs_are_emitted_last_and_negated_so_they_win() {
        let argv = request().argv(Path::new("rg"));
        let glob_positions: Vec<usize> = argv
            .iter()
            .enumerate()
            .filter(|(_, arg)| arg.as_str() == "--glob")
            .map(|(index, _)| index)
            .collect();
        let last_glob_value = argv[glob_positions[glob_positions.len() - 1] + 1].clone();
        assert_eq!(last_glob_value, "!.git/**");
        assert!(argv.iter().any(|arg| arg == "Control/agents/now/**"));
    }

    #[test]
    fn pattern_travels_through_the_explicit_flag_not_a_positional() {
        let argv = request().argv(Path::new("rg"));
        let pattern_position = argv
            .iter()
            .position(|arg| arg == "harness gate")
            .expect("pattern present");
        assert_eq!(argv[pattern_position - 1], "-e");
        assert!(argv.iter().any(|arg| arg == "--json"));
        assert!(argv.iter().any(|arg| arg == "--hidden"));
        assert!(argv.iter().any(|arg| arg == "--no-ignore"));
        assert!(argv.iter().any(|arg| arg == "--max-filesize"));
    }

    #[test]
    fn a_search_with_no_file_bound_leaves_the_size_flag_off() {
        let mut unbounded = request();
        unbounded.max_file_bytes = NO_FILE_BOUND;
        let argv = unbounded.argv(Path::new("rg"));
        assert!(!argv.iter().any(|arg| arg == "--max-filesize"), "{argv:?}");
        assert!(argv.iter().any(|arg| arg == "--max-columns"));
    }

    #[test]
    fn structured_matches_are_parsed_and_truncation_is_truthful() {
        let runner = ScriptedRunner::new().on(
            "--json",
            &format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n",
                r#"{"type":"begin","data":{"path":{"text":"/ground/a.md"}}}"#,
                rg_json_line("/ground/a.md", 3, "harness gate holds"),
                r#"{"type":"match","data":{"path":{"text":null},"lines":{"text":"x"},"line_number":1}}"#,
                rg_json_line("/ground/b.md", 11, "another harness gate"),
                r#"{"type":"end","data":{"path":{"text":"/ground/b.md"}}}"#,
                r#"{"type":"summary","data":{}}"#
            ),
        );
        let mut bounded = request();
        bounded.limit = 1;
        let outcome = RipgrepSearcher::new(runner, "rg").search(&bounded).unwrap();
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].path, PathBuf::from("/ground/a.md"));
        assert_eq!(outcome.matches[0].line_number, 3);
        assert_eq!(outcome.matches[0].line, "harness gate holds");
        assert!(outcome.truncated);
        assert_eq!(outcome.files_searched, 1);
    }

    #[test]
    fn a_bounded_pass_reads_no_more_than_its_matches_are_worth() {
        let line = rg_json_line("/ground/a.md", 3, "harness gate holds");
        let within = ScriptedRunner::new().on("--json", &format!("{line}\n"));
        let mut bounded = request();
        bounded.limit = 1;
        let outcome = RipgrepSearcher::new(within, "rg")
            .search_bounded(&bounded)
            .unwrap();
        assert_eq!(outcome.matches.len(), 1);
        assert!(!outcome.truncated);
        // Past the two matches' worth its one retained match allows, the
        // stream is refused whole: truncated, no sample.
        let flood = format!("{line}\n").repeat(2 * MATCH_EVENT_BYTES / line.len() + 2);
        let past = ScriptedRunner::new().on("--json", &flood);
        let outcome = RipgrepSearcher::new(past, "rg")
            .search_bounded(&bounded)
            .unwrap();
        assert_eq!(
            outcome,
            RipgrepOutcome {
                truncated: true,
                ..RipgrepOutcome::default()
            }
        );
    }

    #[test]
    fn real_ripgrep_is_stopped_at_the_bounded_pass_s_capacity() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for index in 0..64 {
            std::fs::write(dir.path().join(format!("f{index}.md")), "harness gate\n").unwrap();
        }
        let mut wide = request();
        wide.roots = vec![dir.path().to_path_buf()];
        wide.include_globs = vec![];
        wide.limit = 1000;
        let searcher = RipgrepSearcher::new(SystemRunner::new(), executable());
        let all = searcher.search_bounded(&wide).unwrap();
        assert_eq!((all.matches.len(), all.truncated), (64, false));
        let mut narrow = wide.clone();
        narrow.limit = 2;
        // The pass is stopped at its capacity and says so. Since round 55 item 8c it is asked again and keeps a sample: one
        // match of each of the first two files that match, where before it kept none.
        let cut = searcher.search_bounded(&narrow).unwrap();
        assert_eq!(
            (cut.matches.len(), cut.truncated, cut.asked_again),
            (2, true, 1)
        );
        assert_eq!(all.asked_again, 0);
    }

    #[test]
    fn a_clean_no_match_is_data_not_an_error() {
        let runner = ScriptedRunner::new().on("--json", "");
        let outcome = RipgrepSearcher::new(runner, "rg")
            .search(&request())
            .unwrap();
        assert_eq!(outcome, RipgrepOutcome::default());
    }

    #[test]
    fn a_ripgrep_failure_is_an_error_that_names_the_diagnostic() {
        let runner = ScriptedRunner::new().failing("--json", 2, "unrecognized flag: --bogus");
        let error = RipgrepSearcher::new(runner, "rg")
            .search(&request())
            .unwrap_err();
        assert_eq!(error.code(), "search.ripgrep_failed");
        assert!(error.message().contains("unrecognized flag"));
    }

    /// A root under folders named as globs name them (`target/ProjectCentral/secret/world`), holding a file of its own, its own
    /// `secret`, `target` and `ProjectCentral` folders, and a `secret` folder deeper of the same name.
    fn globbed_tree(at: &Path) -> PathBuf {
        let root = at.join("target/ProjectCentral/secret/world");
        for relative in [
            "e.md",
            "secret/a.md",
            "src/c.md",
            "src/secret/b.md",
            "target/d.md",
            "ProjectCentral/f.md",
        ] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "harness gate\n").unwrap();
        }
        root
    }

    /// What one real search over `roots` finds with `exclude`, each path named under its root.
    fn found(roots: &[&Path], exclude: &[&str]) -> Vec<String> {
        let mut request = request();
        request.roots = roots.iter().map(|root| root.to_path_buf()).collect();
        request.include_globs = vec![];
        request.exclude_globs = exclude.iter().map(|glob| glob.to_string()).collect();
        let searcher = RipgrepSearcher::new(
            SystemRunner::new().with_env_removed("RIPGREP_CONFIG_PATH"),
            executable(),
        );
        let outcome = searcher.search(&request).unwrap();
        let counted = searcher.count(&request).unwrap();
        let mut paths: Vec<String> = outcome
            .matches
            .iter()
            .map(|matched| named_under(roots, &matched.path))
            .collect();
        paths.sort();
        let mut counted: Vec<String> = counted
            .iter()
            .map(|(path, _)| named_under(roots, path))
            .collect();
        counted.sort();
        assert_eq!(paths, counted, "the count names what the search finds");
        paths
    }

    fn named_under(roots: &[&Path], path: &Path) -> String {
        let (index, root) = roots
            .iter()
            .enumerate()
            .find(|(_, root)| path.starts_with(root))
            .unwrap_or_else(|| panic!("{} is under no root", path.display()));
        format!("{index}:{}", path.strip_prefix(root).unwrap().display())
    }

    /// Round 55 item 8 (DLW build ruling 2026-11-09): a glob is matched against each root-relative path, as the contract says, and
    /// never against the folders above the root. A root under folders named `ProjectCentral` and `target` is searched whole; only
    /// its own `ProjectCentral` and `target` folders are withheld.
    #[test]
    fn a_root_under_folders_named_like_its_globs_is_searched_whole() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = globbed_tree(dir.path());
        assert_eq!(
            found(&[&root], &["**/ProjectCentral/**", "**/target/**"]),
            ["0:e.md", "0:secret/a.md", "0:src/c.md", "0:src/secret/b.md"]
        );
        assert_eq!(
            found(&[&root], &["target/**"]).len(),
            5,
            "an anchored target/** names the root's own target alone"
        );
    }

    /// The same ruling: an anchored glob withholds the root's own folder, and an unanchored one each folder of that name inside
    /// the root, and neither anything above it (the root sits under a folder named `secret`). Withholding less than an anchored
    /// glob names is a privacy fault; withholding the whole root is the other.
    #[test]
    fn an_anchored_glob_withholds_the_root_s_own_folder_and_an_unanchored_one_each_namesake_in_it()
    {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = globbed_tree(dir.path());
        let anchored = [
            "0:ProjectCentral/f.md",
            "0:e.md",
            "0:src/c.md",
            "0:src/secret/b.md",
            "0:target/d.md",
        ];
        assert_eq!(found(&[&root], &["secret/**"]), anchored);
        assert_eq!(found(&[&root], &["/secret/**"]), anchored);
        assert_eq!(
            found(&[&root], &["**/secret/**"]),
            [
                "0:ProjectCentral/f.md",
                "0:e.md",
                "0:src/c.md",
                "0:target/d.md"
            ]
        );
    }

    /// The same ruling: two roots of one request are each searched by their own paths, and each answers its own matches under it.
    /// Neither root's place bleeds into the other's: the second root sits inside a folder the first's glob names.
    #[test]
    fn two_roots_are_each_searched_by_their_own_paths() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let first = globbed_tree(&dir.path().join("first"));
        let second = globbed_tree(&dir.path().join("first/secret/second"));
        assert_eq!(
            found(&[&first, &second], &["secret/**", "**/target/**"]),
            [
                "0:ProjectCentral/f.md",
                "0:e.md",
                "0:src/c.md",
                "0:src/secret/b.md",
                "1:ProjectCentral/f.md",
                "1:e.md",
                "1:src/c.md",
                "1:src/secret/b.md",
            ]
        );
    }

    /// Each root that is a folder is searched by its own pass, started in it, over `.`; single files share one pass, as given.
    #[test]
    fn a_folder_root_is_searched_from_inside_it_and_single_files_as_named() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("one.md");
        std::fs::write(&file, "x").unwrap();
        let mut both = request();
        both.roots = vec![dir.path().to_path_buf(), file.clone()];
        let passes = both.passes(Path::new("rg"));
        assert_eq!(passes.len(), 2, "{passes:?}");
        assert_eq!(passes[0].root.as_deref(), Some(dir.path()));
        assert_eq!(passes[0].argv.last().map(String::as_str), Some("."));
        assert_eq!(passes[1].root, None);
        assert_eq!(
            passes[1].argv.last(),
            Some(&file.to_string_lossy().into_owned())
        );
    }

    /// A ripgrep named by a relative path is the same program in a pass started in a root.
    #[test]
    fn a_relative_ripgrep_path_runs_the_same_program_in_a_root() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        let root = dir.path().join("root");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let ran = dir.path().join("ran");
        let program = tools.join("rg");
        std::fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let here = std::env::current_dir().unwrap();
        let up = here.components().count() - 1;
        let relative: PathBuf = std::iter::repeat_n(Path::new(".."), up)
            .collect::<PathBuf>()
            .join(program.strip_prefix("/").unwrap());
        assert!(relative.is_relative() && relative.components().count() > 1);
        let mut folder = request();
        folder.roots = vec![root];
        // Where a relative program is looked up once a command starts in
        // another folder is the platform's choice (std's `current_dir`), so
        // the pass names it from here on every platform.
        let named = &folder.passes(&relative)[0].argv[0];
        assert_eq!(named, &here.join(&relative).to_string_lossy().into_owned());
        let hits = RipgrepSearcher::new(SystemRunner::new(), &relative)
            .search(&folder)
            .expect("the program named from here runs in the root");
        assert!(hits.matches.is_empty());
        assert!(ran.exists(), "the named program ran");
    }

    /// A runner that cannot start a command in a folder runs no search of one, rather than search wherever it stands.
    #[test]
    fn a_runner_that_cannot_start_in_a_root_searches_nothing() {
        struct Nowhere(std::sync::Mutex<usize>);
        impl CommandRunner for Nowhere {
            fn run(&self, _argv: &[String]) -> Result<crate::runner::Output> {
                *self.0.lock().unwrap() += 1;
                Ok(crate::runner::Output::success(""))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut folder = request();
        folder.roots = vec![dir.path().to_path_buf()];
        let runner = Nowhere(std::sync::Mutex::new(0));
        let searcher = RipgrepSearcher::new(&runner, "rg");
        assert_eq!(
            searcher.search(&folder).unwrap_err().code(),
            "mux.command_dir_unsupported"
        );
        assert_eq!(
            searcher.search_bounded(&folder).unwrap_err().code(),
            "mux.command_dir_unsupported"
        );
        assert_eq!(
            searcher.count(&folder).unwrap_err().code(),
            "mux.command_dir_unsupported"
        );
        assert_eq!(*runner.0.lock().unwrap(), 0, "nothing ran");
    }

    #[test]
    fn filesize_budgets_render_in_ripgrep_units() {
        assert_eq!(format_filesize(DEFAULT_MAX_FILE_BYTES), "1M");
        assert_eq!(format_filesize(512), "512");
        assert_eq!(format_filesize(64 * 1024), "64K");
    }

    #[test]
    fn a_search_over_a_runaway_binary_is_killed_by_the_declared_budget() {
        // The real runner seam, not a scripted stand-in: the module contract
        // promises that a pathological search costs one timeout, so the
        // knowledge layer proves the process is actually killed.
        let dir = tempfile::tempdir().unwrap();
        let slow = dir.path().join("slow-rg");
        std::fs::write(&slow, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut request = request();
        request.roots = vec![dir.path().to_path_buf()];
        request.timeout = Some(Duration::from_millis(300));
        let error = RipgrepSearcher::new(SystemRunner::new(), &slow)
            .search(&request)
            .expect_err("a 30s runaway cannot finish inside a 300ms budget");
        assert_eq!(error.code(), "mux.command_timeout");
    }

    /// A real searcher with no inherited ripgrep configuration.
    fn real() -> RipgrepSearcher<SystemRunner> {
        RipgrepSearcher::new(
            SystemRunner::new().with_env_removed("RIPGREP_CONFIG_PATH"),
            executable(),
        )
    }

    /// Round 55 item 8b (DLW build ruling 2026-11-09): a count over a folder and a file counts both. The file is the request's
    /// only file but not its only root, and each is counted where it stands.
    #[test]
    fn a_count_over_a_folder_and_a_file_counts_both() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("d");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "harness gate\nharness gate\n").unwrap();
        let file = dir.path().join("f.md");
        std::fs::write(&file, "harness gate\n").unwrap();
        let mut both = request();
        both.roots = vec![folder.clone(), file.clone()];
        both.include_globs = vec![];
        assert_eq!(
            real().count(&both).unwrap(),
            [(folder.join("a.md"), 2), (file.clone(), 1)]
        );
        let mut alone = both.clone();
        alone.roots = vec![file.clone()];
        assert_eq!(real().count(&alone).unwrap(), [(file, 1)]);
    }

    /// The same ruling: a file whose name holds a newline is counted once, under its own name. No part of its name becomes a
    /// path of its own under the root.
    #[test]
    fn a_file_named_with_a_newline_is_counted_once_where_it_stands() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let odd = root.join("a\nb");
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::write(odd.join("x.md"), "harness gate\n").unwrap();
        std::fs::write(root.join("plain.md"), "harness gate\nharness gate\n").unwrap();
        let mut walked = request();
        walked.roots = vec![root.clone()];
        walked.include_globs = vec![];
        assert_eq!(
            real().count(&walked).unwrap(),
            [(odd.join("x.md"), 1), (root.join("plain.md"), 2)]
        );
        let mut alone = walked.clone();
        alone.roots = vec![odd.join("x.md")];
        assert_eq!(real().count(&alone).unwrap(), [(odd.join("x.md"), 1)]);
    }

    /// What a real bounded search with `limit` keeps from each of its roots, the root at each index holding that many
    /// one-match files, and whether it says it was truncated.
    fn kept_per_root(files: &[usize], limit: usize) -> (Vec<usize>, bool) {
        let dir = tempfile::tempdir().unwrap();
        let roots: Vec<PathBuf> = files
            .iter()
            .enumerate()
            .map(|(index, count)| {
                let root = dir.path().join(format!("root{index}"));
                std::fs::create_dir_all(&root).unwrap();
                for file in 0..*count {
                    std::fs::write(root.join(format!("f{file}.md")), "harness gate\n").unwrap();
                }
                root
            })
            .collect();
        let mut bounded = request();
        bounded.roots = roots.clone();
        bounded.include_globs = vec![];
        bounded.limit = limit;
        let outcome = real().search_bounded(&bounded).unwrap();
        (
            roots
                .iter()
                .map(|root| {
                    outcome
                        .matches
                        .iter()
                        .filter(|matched| matched.path.starts_with(root))
                        .count()
                })
                .collect(),
            outcome.truncated,
        )
    }

    /// The same ruling: a bounded search over two roots keeps a sample of each. A first root with more matches than its share
    /// keeps its share, and the second keeps its own: ten matches, not none. A first root that keeps less leaves the rest of
    /// the limit to the roots after it.
    #[test]
    fn a_bounded_search_over_two_roots_keeps_a_share_of_each() {
        if !available() {
            return;
        }
        assert_eq!(kept_per_root(&[10, 6], 10), (vec![5, 5], true));
        assert_eq!(kept_per_root(&[0, 10], 10), (vec![0, 10], false));
    }

    /// The same ruling: under a limit smaller than its roots' matches, every root of a bounded search is represented.
    #[test]
    fn two_roots_are_each_represented_under_a_small_limit() {
        if !available() {
            return;
        }
        assert_eq!(kept_per_root(&[3, 3], 2), (vec![1, 1], true));
    }

    /// Round 55 item 8c (the review of #616, the share): a root whose matches run past its share of the read still keeps a
    /// sample. Two roots of 40 one-match files under a limit of 10 each keep their five, where both kept none; a second root of
    /// one file keeps its one.
    #[test]
    fn a_root_stopped_at_its_capacity_still_keeps_a_sample() {
        if !available() {
            return;
        }
        assert_eq!(kept_per_root(&[40, 40], 10), (vec![5, 5], true));
        assert_eq!(kept_per_root(&[40, 1], 10), (vec![5, 1], true));
    }

    /// The same ruling, by what each read is given: a stopped pass is asked again twice, each read under the same allowance
    /// and under what the reads before it left of the request's one timeout; the outcome counts the pass. Once the timeout is
    /// spent nothing more is asked, and the pass is not counted.
    #[test]
    fn a_stopped_pass_is_asked_again_within_the_request_s_timeout_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        let whole = 11 * MATCH_EVENT_BYTES * 2;
        let seconds = |given: &[Given]| -> Vec<(usize, u64, Option<usize>)> {
            given
                .iter()
                .map(|given| {
                    (
                        shared
                            .roots
                            .iter()
                            .position(|root| *root == given.root)
                            .unwrap(),
                        // To the nearest second: the logical clock moves by whole seconds, the machine's by a little more.
                        given.timeout.unwrap().as_secs_f64().round() as u64,
                        given.bytes,
                    )
                })
                .collect()
        };
        let ledger = Ledger::new(&shared.roots, &[400, 2], Duration::from_secs(4));
        let outcome = RipgrepSearcher::new(&ledger, "rg")
            .search_bounded(&shared)
            .unwrap();
        assert_eq!(outcome.asked_again, 1);
        let half = Some(whole / 2);
        assert_eq!(
            seconds(&ledger.given()),
            [(0, 30, half), (0, 26, half), (0, 22, half), (1, 18, half)]
        );
        // A pass that takes the whole timeout leaves nothing to ask again with, and the next pass does not start.
        let slow = Ledger::new(&shared.roots, &[400, 2], Duration::from_secs(30));
        let spent = RipgrepSearcher::new(&slow, "rg").search_bounded(&shared);
        assert_eq!(spent.unwrap_err().code(), "mux.command_timeout");
        assert_eq!(seconds(&slow.given()), [(0, 30, half)]);
        // One that is not stopped is not asked again.
        let small = Ledger::new(&shared.roots, &[2, 2], Duration::ZERO);
        let once = RipgrepSearcher::new(&small, "rg")
            .search_bounded(&shared)
            .unwrap();
        assert_eq!((once.asked_again, small.given().len()), (0, 2));
    }

    /// The same ruling, by its argv: the sample pass keeps the stopped pass's flags, names the listed files in place of `.`,
    /// and keeps one match a file, whether the request named a cap of its own or none.
    #[test]
    fn a_sample_pass_names_the_listed_files_and_keeps_one_match_of_each() {
        let mut capped = request();
        capped.max_count_per_file = Some(4);
        for request in [request(), capped] {
            let over_a_folder = request.argv_over(Path::new("rg"), [".".to_string()], false);
            let sampling = one_match_of_each(
                &over_a_folder,
                &[PathBuf::from("./a.md"), PathBuf::from("-b.md")],
            );
            let cap = sampling
                .iter()
                .position(|arg| arg == "--max-count")
                .unwrap();
            assert_eq!(sampling[cap + 1], "1", "{sampling:?}");
            assert_eq!(
                sampling.iter().filter(|arg| *arg == "--max-count").count(),
                1,
                "{sampling:?}"
            );
            let pattern = sampling.iter().position(|arg| arg == "-e").unwrap();
            assert!(cap < pattern, "{sampling:?}");
            assert_eq!(
                sampling[pattern + 2..],
                ["./a.md", "./-b.md"],
                "{sampling:?}"
            );
        }
    }

    /// A runner that answers every folder pass with one stream and the pass over single files with another, and records
    /// which form each pass was run by and the time it was given.
    struct Streams {
        in_a_root: String,
        over_files: String,
        ran: std::sync::Mutex<Vec<(&'static str, Option<Duration>)>>,
    }

    impl Streams {
        fn new(in_a_root: String, over_files: String) -> Self {
            Self {
                in_a_root,
                over_files,
                ran: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn ran(&self) -> Vec<(&'static str, Option<Duration>)> {
            self.ran.lock().unwrap().clone()
        }
    }

    impl CommandRunner for Streams {
        fn run(&self, _argv: &[String]) -> Result<crate::runner::Output> {
            self.ran.lock().unwrap().push(("run", None));
            Ok(crate::runner::Output::success(self.over_files.clone()))
        }

        fn run_with_timeout(
            &self,
            _argv: &[String],
            timeout: Duration,
        ) -> Result<crate::runner::Output> {
            self.ran
                .lock()
                .unwrap()
                .push(("run_with_timeout", Some(timeout)));
            Ok(crate::runner::Output::success(self.over_files.clone()))
        }

        fn run_in(
            &self,
            _dir: &Path,
            _argv: &[String],
            timeout: Option<Duration>,
        ) -> Result<crate::runner::Output> {
            self.ran.lock().unwrap().push(("run_in", timeout));
            Ok(crate::runner::Output::success(self.in_a_root.clone()))
        }
    }

    /// Round 55 item 8c (the review of #616, F5): the matches of a pass are answered in path order, then line order, whatever
    /// order ripgrep's threads streamed them in; and the roots keep the order the request named them in, a file root named
    /// before a folder answering before it.
    #[test]
    fn matches_are_ordered_by_path_inside_a_root_and_roots_as_the_request_named_them() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("d");
        std::fs::create_dir_all(&folder).unwrap();
        let file = dir.path().join("f.md");
        std::fs::write(&file, "harness gate\n").unwrap();
        let in_a_root = [("./z.md", 1), ("./a.md", 7), ("./a.md", 2)]
            .map(|(path, line)| rg_json_line(path, line, "harness gate"))
            .join("\n");
        let over_files = rg_json_line(&file.to_string_lossy(), 1, "harness gate");
        let mut ordered = request();
        ordered.roots = vec![file.clone(), folder.clone()];
        ordered.timeout = None;
        let runner = Streams::new(in_a_root, over_files);
        let outcome = RipgrepSearcher::new(&runner, "rg")
            .search(&ordered)
            .unwrap();
        let answered: Vec<(PathBuf, u64)> = outcome
            .matches
            .iter()
            .map(|matched| (matched.path.clone(), matched.line_number))
            .collect();
        assert_eq!(
            answered,
            [
                (file, 1),
                (folder.join("a.md"), 2),
                (folder.join("a.md"), 7),
                (folder.join("z.md"), 1)
            ]
        );
    }

    /// Round 55 item 8c: a count answers in path order whatever order ripgrep streamed its records in. (The reviewer's mutant
    /// r07 was killed in #616's review by a test over real ripgrep, whose two threads do not always stream out of order; this
    /// one does not lean on that.)
    #[test]
    fn a_count_is_answered_in_path_order_whatever_the_stream_s() {
        let dir = tempfile::tempdir().unwrap();
        let mut counted = request();
        counted.roots = vec![dir.path().to_path_buf()];
        let nul = '\0';
        let runner = Streams::new(
            format!("./z.md{nul}1\n./a.md{nul}2\n./m.md{nul}3\n"),
            String::new(),
        );
        assert_eq!(
            RipgrepSearcher::new(&runner, "rg").count(&counted).unwrap(),
            [("a.md", 2), ("m.md", 3), ("z.md", 1)]
                .map(|(name, count)| (dir.path().join(name), count))
        );
    }

    /// Round 55 item 8c (the reviewer's mutants r08 and r09): the pass over single files runs under the request's budget too, in
    /// a search and in a count; with no budget named it runs with none.
    #[test]
    fn the_pass_over_single_files_runs_under_the_request_s_budget() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.md");
        std::fs::write(&file, "harness gate\n").unwrap();
        let mut single = request();
        single.roots = vec![file];
        for (budget, form) in [
            (Some(Duration::from_secs(30)), "run_with_timeout"),
            (None, "run"),
        ] {
            single.timeout = budget;
            let runner = Streams::new(String::new(), String::new());
            let searcher = RipgrepSearcher::new(&runner, "rg");
            searcher.search(&single).unwrap();
            searcher.count(&single).unwrap();
            let ran = runner.ran();
            assert_eq!(
                ran.iter().map(|(form, _)| *form).collect::<Vec<_>>(),
                [form, form],
                "{budget:?}"
            );
            for (_, given) in ran {
                assert_eq!(given.is_some(), budget.is_some(), "{budget:?}");
                assert!(given <= budget, "{given:?} of {budget:?}");
            }
        }
    }

    /// Round 55 item 8c (the reviewer's mutant r03): a pass stopped at its capacity has spent its allowance of the read. The
    /// pass after it may read what is left, not the whole again.
    #[test]
    fn a_pass_stopped_at_its_capacity_has_spent_its_allowance() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        let ledger = Ledger::new(&shared.roots, &[400, 2], Duration::ZERO);
        RipgrepSearcher::new(&ledger, "rg")
            .search_bounded(&shared)
            .unwrap();
        let whole = 11 * MATCH_EVENT_BYTES * 2;
        let second: Vec<Option<usize>> = ledger
            .given()
            .iter()
            .filter(|given| given.root == shared.roots[1])
            .map(|given| given.bytes)
            .collect();
        assert_eq!(second, [Some(whole / 2)]);
    }

    /// Round 55 item 8c (the reviewer's mutant r05): on a volume that folds case the deny globs are matched without regard to
    /// case and the include globs are passed as they were.
    #[test]
    fn include_globs_are_unchanged_where_deny_globs_fold_case() {
        let mut globbed = request();
        globbed.include_globs = vec!["*.MD".into()];
        globbed.exclude_globs = vec!["secret/**".into()];
        let argv = globbed.argv_over(Path::new("rg"), [".".to_string()], true);
        let flag_of = |glob: &str| {
            let at = argv.iter().position(|arg| arg == glob).unwrap();
            argv[at - 1].clone()
        };
        assert_eq!(flag_of("*.MD"), "--glob", "{argv:?}");
        assert_eq!(flag_of("!secret/**"), "--iglob", "{argv:?}");
    }

    /// Round 55 item 8c (the reviewer's mutant r06): a pass that cannot start for a reason that is not its root is the runner's
    /// own refusal, passed on as it is. A ripgrep that is not there, over a root that reads, is not "could not search <root>".
    #[test]
    fn a_ripgrep_that_is_not_there_is_not_blamed_on_a_root_that_reads() {
        let dir = tempfile::tempdir().unwrap();
        let mut folder = request();
        folder.roots = vec![dir.path().to_path_buf()];
        let searcher =
            RipgrepSearcher::new(SystemRunner::new(), dir.path().join("no-ripgrep-here"));
        for error in [
            searcher.search(&folder).unwrap_err(),
            searcher.search_bounded(&folder).unwrap_err(),
            searcher.count(&folder).unwrap_err(),
        ] {
            assert_eq!(error.code(), "mux.command_spawn_failed", "{error:?}");
            assert!(!error.message().contains("could not search"), "{error:?}");
        }
    }

    /// The same ruling: a root that cannot be read is ripgrep's failure to search it (`search.ripgrep_failed`), as it was before
    /// each root's pass started in it, and the words name the root. A superuser reads any folder, so there it is not run.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_root_is_ripgrep_s_failure_in_words_that_name_it() {
        use std::os::unix::fs::PermissionsExt;
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sealed");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "harness gate\n").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).unwrap();
        let entered = std::fs::read_dir(&root).is_ok();
        let mut sealed = request();
        sealed.roots = vec![root.clone()];
        sealed.include_globs = vec![];
        sealed.limit = 10;
        let errors = if entered {
            Vec::new()
        } else {
            vec![
                real().search(&sealed).unwrap_err(),
                real().search_bounded(&sealed).unwrap_err(),
                real().count(&sealed).unwrap_err(),
            ]
        };
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        for error in errors {
            assert_eq!(error.code(), "search.ripgrep_failed", "{error:?}");
            assert!(
                error.message().contains(&root.display().to_string()),
                "{error:?}"
            );
        }
    }

    /// Round 55 item 8c (DLW build ruling 2026-11-11; the review of #616, F3): a root that never stood is named. Beside a folder
    /// that stands and holds a match, a root that is not there fails the search, the bounded search and the count as ripgrep's
    /// failure, in words that name the missing root, as an unreadable one is named.
    #[test]
    fn a_root_that_never_stood_is_named_in_ripgrep_s_failure() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let stands = dir.path().join("stands");
        std::fs::create_dir_all(&stands).unwrap();
        std::fs::write(stands.join("a.md"), "harness gate\n").unwrap();
        let missing = dir.path().join("never-stood.md");
        let mut asked = request();
        asked.roots = vec![stands, missing.clone()];
        asked.include_globs = vec![];
        asked.limit = 10;
        for error in [
            real().search(&asked).unwrap_err(),
            real().search_bounded(&asked).unwrap_err(),
            real().count(&asked).unwrap_err(),
        ] {
            assert_eq!(error.code(), "search.ripgrep_failed", "{error:?}");
            assert!(
                error
                    .message()
                    .starts_with(&format!("ripgrep could not search {}: ", missing.display())),
                "{error:?}"
            );
        }
    }

    /// The same ruling (F5): a relative file root that begins with a dash is named from `.`, so ripgrep reads it as a path and
    /// never as a flag. A file root named any other way goes as given.
    #[test]
    fn a_file_root_that_begins_with_a_dash_is_named_as_a_path() {
        let mut asked = request();
        asked.roots = vec![PathBuf::from("-x.md"), PathBuf::from("y.md")];
        let passes = asked.passes(Path::new("rg"));
        assert_eq!(passes.len(), 1, "{passes:?}");
        assert_eq!(
            passes[0].argv[passes[0].argv.len() - 2..],
            ["./-x.md".to_string(), "y.md".to_string()],
            "{:?}",
            passes[0].argv
        );
    }

    /// The same ruling (F5): a folder whose volume cannot be judged is taken to fold case, the safe side for a deny glob. Here
    /// no name gives a letter to try: the folder's own name and its one entry's are digits.
    #[test]
    fn a_volume_that_cannot_be_judged_is_taken_to_fold_case() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("123");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("456"), "").unwrap();
        assert!(folds_case(&root));
    }

    /// The same ruling: a relative root, searched by a runner that starts its commands in another folder than this process's,
    /// is named from that folder before any pass. It is classified, searched and named there, so its globs withhold what they
    /// name.
    #[test]
    fn a_relative_root_is_named_from_the_folder_the_runner_starts_in() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let root = globbed_tree(&base.join("proj"));
        let mut relative = request();
        relative.roots = vec![root.strip_prefix(&base).unwrap().to_path_buf()];
        relative.include_globs = vec![];
        relative.exclude_globs = vec!["secret/**".into()];
        let searcher = RipgrepSearcher::new(
            SystemRunner::new()
                .with_env_removed("RIPGREP_CONFIG_PATH")
                .with_cwd(&base),
            executable(),
        );
        let mut found: Vec<PathBuf> = searcher
            .search(&relative)
            .unwrap()
            .matches
            .into_iter()
            .map(|matched| matched.path)
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                "ProjectCentral/f.md",
                "e.md",
                "src/c.md",
                "src/secret/b.md",
                "target/d.md"
            ]
            .map(|path| root.join(path))
        );
    }

    /// The same ruling: on a volume that looks names up without regard to case, as a default APFS volume does, a deny glob
    /// withholds the folder it names in whatever case the folder is written. On one that does not, it withholds that spelling
    /// alone.
    #[test]
    fn a_deny_glob_withholds_its_folder_in_any_case_where_the_volume_folds_case() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("Secret")).unwrap();
        std::fs::write(root.join("Secret/a.md"), "harness gate\n").unwrap();
        std::fs::write(root.join("ok.md"), "harness gate\n").unwrap();
        let folded = root.join("OK.MD").exists();
        let kept = found(&[&root], &["secret/**"]);
        if folded {
            assert_eq!(kept, ["0:ok.md"]);
        } else {
            assert_eq!(kept, ["0:Secret/a.md", "0:ok.md"]);
        }
    }

    /// The same ruling: a runner that starts a command in a folder but cannot bound its read runs a bounded search unbounded,
    /// rather than refuse it.
    #[test]
    fn a_runner_that_cannot_bound_a_read_in_a_root_searches_it_unbounded() {
        struct Unbounded;
        impl CommandRunner for Unbounded {
            fn run(&self, _argv: &[String]) -> Result<crate::runner::Output> {
                panic!("every root here is a folder")
            }

            fn run_in(
                &self,
                _dir: &Path,
                _argv: &[String],
                _timeout: Option<Duration>,
            ) -> Result<crate::runner::Output> {
                Ok(crate::runner::Output::success(format!(
                    "{}\n",
                    rg_json_line("./a.md", 1, "harness gate")
                )))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut folder = request();
        folder.roots = vec![dir.path().to_path_buf()];
        folder.limit = 5;
        let outcome = RipgrepSearcher::new(Unbounded, "rg")
            .search_bounded(&folder)
            .unwrap();
        assert_eq!(outcome.matches.len(), 1, "{outcome:?}");
        assert_eq!(outcome.matches[0].path, dir.path().join("a.md"));
    }

    /// What one pass of a [`Ledger`] was given: the root it was started in, its time and its read allowance.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Given {
        root: PathBuf,
        timeout: Option<Duration>,
        bytes: Option<usize>,
    }

    /// A runner that answers each root's pass as though the root held so many one-match files, records what each pass was
    /// given, and moves this thread's clock on by `takes` for each pass: a logical clock, so a shared budget is asserted by what
    /// it leaves each pass, never by how long a pass took.
    struct Ledger {
        roots: Vec<(PathBuf, usize)>,
        takes: Duration,
        given: std::sync::Mutex<Vec<Given>>,
    }

    impl Ledger {
        fn new(roots: &[PathBuf], files: &[usize], takes: Duration) -> Self {
            Self {
                roots: roots.iter().cloned().zip(files.iter().copied()).collect(),
                takes,
                given: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn stream(&self, dir: &Path, argv: &[String]) -> String {
            let files = self
                .roots
                .iter()
                .find(|(root, _)| root == dir)
                .map_or(0, |(_, files)| *files);
            let counting = argv.iter().any(|arg| arg == "--count");
            (0..files)
                .map(|index| {
                    let path = format!("./f{index}.md");
                    if counting {
                        format!("{path}\u{0}1\n")
                    } else {
                        format!(
                            "{{\"type\":\"begin\",\"data\":{{\"path\":{{\"text\":\"{path}\"}}}}}}\n{}\n",
                            rg_json_line(&path, 1, "harness gate")
                        )
                    }
                })
                .collect()
        }

        fn given(&self) -> Vec<Given> {
            self.given.lock().unwrap().clone()
        }

        fn pass(&self, dir: &Path, timeout: Option<Duration>, bytes: Option<usize>) {
            self.given.lock().unwrap().push(Given {
                root: dir.to_path_buf(),
                timeout,
                bytes,
            });
            clock::advance(self.takes);
        }
    }

    impl CommandRunner for Ledger {
        fn run(&self, _argv: &[String]) -> Result<crate::runner::Output> {
            panic!("every root here is a folder")
        }

        fn run_in(
            &self,
            dir: &Path,
            argv: &[String],
            timeout: Option<Duration>,
        ) -> Result<crate::runner::Output> {
            self.pass(dir, timeout, None);
            Ok(crate::runner::Output::success(self.stream(dir, argv)))
        }

        fn run_in_with_limits(
            &self,
            dir: &Path,
            argv: &[String],
            remaining_timeout: Duration,
            remaining_aggregate_bytes: usize,
            _strict_utf8: bool,
        ) -> Result<crate::runner::Output> {
            self.pass(
                dir,
                Some(remaining_timeout),
                Some(remaining_aggregate_bytes),
            );
            let stream = self.stream(dir, argv);
            if stream.len() > remaining_aggregate_bytes / 2 {
                return Err(AikitError::new(
                    "mux.command_output_limit",
                    "the stream ran past its share of the capacity",
                ));
            }
            Ok(crate::runner::Output::success(stream))
        }
    }

    /// `count` folders under `dir`, each a root of a request with a 30-second budget and a limit of `limit`.
    fn ledger_request(dir: &Path, count: usize, limit: usize) -> SearchRequest {
        let mut shared = request();
        shared.roots = (0..count)
            .map(|index| {
                let root = dir.join(format!("root{index}"));
                std::fs::create_dir_all(&root).unwrap();
                root
            })
            .collect();
        shared.limit = limit;
        shared.timeout = Some(Duration::from_secs(30));
        shared
    }

    /// How many of `outcome`'s matches are under each root of `request`.
    fn per_root(request: &SearchRequest, outcome: &RipgrepOutcome) -> Vec<usize> {
        request
            .roots
            .iter()
            .map(|root| {
                outcome
                    .matches
                    .iter()
                    .filter(|matched| matched.path.starts_with(root))
                    .count()
            })
            .collect()
    }

    type Searched = fn(&RipgrepSearcher<&Ledger>, &SearchRequest) -> Result<usize>;

    /// A search, a bounded search and a count, each answering how many matches or counts it kept.
    fn searches() -> [(&'static str, Searched); 3] {
        [
            ("search", |searcher, request| {
                searcher
                    .search(request)
                    .map(|outcome| outcome.matches.len())
            }),
            ("search_bounded", |searcher, request| {
                searcher
                    .search_bounded(request)
                    .map(|outcome| outcome.matches.len())
            }),
            ("count", |searcher, request| {
                searcher.count(request).map(|counts| counts.len())
            }),
        ]
    }

    /// Round 55 item 8b (DLW build ruling 2026-11-09): the passes of a request share its one timeout, in a search, a bounded
    /// search and a count alike. Each pass runs under what the passes before it left, and none starts once it is spent.
    #[test]
    fn the_passes_of_a_request_share_one_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 3, 10);
        for (name, searched) in searches() {
            let ledger = Ledger::new(&shared.roots, &[1, 1, 1], Duration::from_secs(12));
            searched(&RipgrepSearcher::new(&ledger, "rg"), &shared).unwrap();
            let given = ledger.given();
            assert_eq!(given.len(), 3, "{name}: {given:?}");
            for (pass, most) in [30, 18, 6].into_iter().enumerate() {
                let timeout = given[pass].timeout.expect("each pass is given its time");
                assert!(
                    timeout <= Duration::from_secs(most),
                    "{name} pass {pass}: {given:?}"
                );
            }
            let slow = Ledger::new(&shared.roots, &[1, 1, 1], Duration::from_secs(16));
            let error = searched(&RipgrepSearcher::new(&slow, "rg"), &shared).unwrap_err();
            assert_eq!(error.code(), "mux.command_timeout", "{name}");
            assert_eq!(
                slow.given().len(),
                2,
                "{name}: no pass starts once the time is spent"
            );
        }
    }

    /// The same ruling: the passes of a request share its one limit. A search keeps what the passes before it left; a bounded
    /// search gives each pass an equal share of what is left. Either keeps the limit and says the rest was cut.
    #[test]
    fn the_passes_of_a_request_share_one_limit() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 3, 10);
        let ledger = Ledger::new(&shared.roots, &[6, 6, 6], Duration::ZERO);
        let searcher = RipgrepSearcher::new(&ledger, "rg");
        let all = searcher.search(&shared).unwrap();
        assert_eq!(
            (per_root(&shared, &all), all.truncated),
            (vec![6, 4, 0], true)
        );
        let bounded = searcher.search_bounded(&shared).unwrap();
        assert_eq!(
            (per_root(&shared, &bounded), bounded.truncated),
            (vec![4, 3, 3], true)
        );
    }

    /// The same ruling: a request is truncated when any of its passes is, whichever pass comes last.
    #[test]
    fn a_request_is_truncated_when_any_of_its_passes_is() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        let ledger = Ledger::new(&shared.roots, &[12, 0], Duration::ZERO);
        let searcher = RipgrepSearcher::new(&ledger, "rg");
        let all = searcher.search(&shared).unwrap();
        assert_eq!(
            (per_root(&shared, &all), all.truncated),
            (vec![10, 0], true)
        );
        let bounded = searcher.search_bounded(&shared).unwrap();
        assert_eq!(
            (per_root(&shared, &bounded), bounded.truncated),
            (vec![5, 0], true)
        );
    }

    /// The same ruling: the files a request searched are those every one of its passes searched.
    #[test]
    fn the_files_a_request_searched_are_every_pass_s() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        let ledger = Ledger::new(&shared.roots, &[2, 3], Duration::ZERO);
        let searcher = RipgrepSearcher::new(&ledger, "rg");
        assert_eq!(searcher.search(&shared).unwrap().files_searched, 5);
        assert_eq!(searcher.search_bounded(&shared).unwrap().files_searched, 5);
    }

    /// The same ruling: a bounded pass may read an equal share of what the request may still read, and the whole is what one
    /// process over every root could read: the limit's matches' worth and one more, on each of two streams.
    #[test]
    fn a_bounded_pass_may_read_its_share_of_what_is_left_unread() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        let ledger = Ledger::new(&shared.roots, &[2, 2], Duration::ZERO);
        RipgrepSearcher::new(&ledger, "rg")
            .search_bounded(&shared)
            .unwrap();
        let whole = 11 * MATCH_EVENT_BYTES * 2;
        let first = ledger.stream(&shared.roots[0], &[]).len();
        let given: Vec<Option<usize>> = ledger.given().iter().map(|given| given.bytes).collect();
        assert_eq!(given, [Some(whole / 2), Some(whole - first)]);
    }

    /// The same ruling: a bounded pass stopped at its capacity answers truncated with no sample of its own, and the samples the
    /// request's other passes keep stand, whichever order the passes run in.
    #[test]
    fn a_pass_stopped_at_its_capacity_leaves_the_other_passes_samples() {
        let dir = tempfile::tempdir().unwrap();
        let shared = ledger_request(dir.path(), 2, 10);
        for (files, kept) in [([3, 400], [3, 0]), ([400, 3], [0, 3])] {
            let ledger = Ledger::new(&shared.roots, &files, Duration::ZERO);
            let bounded = RipgrepSearcher::new(&ledger, "rg")
                .search_bounded(&shared)
                .unwrap();
            assert_eq!(
                (per_root(&shared, &bounded), bounded.truncated),
                (kept.to_vec(), true),
                "{files:?}"
            );
        }
    }

    /// The same ruling: a count record ends at the first line feed after its NUL, so a path that holds a line feed is one path.
    /// A stream that is not such records is refused.
    #[test]
    fn a_count_record_ends_at_the_first_line_feed_after_its_nul() {
        let nul = '\0';
        assert_eq!(
            count_records(&format!("./a\nb/x.md{nul}1\n./c.md{nul}12\n")).unwrap(),
            [
                (PathBuf::from("./a\nb/x.md"), 1),
                (PathBuf::from("./c.md"), 12)
            ]
        );
        assert!(count_records("").unwrap().is_empty());
        assert_eq!(
            count_records(&format!("./c.md{nul}3")).unwrap(),
            [(PathBuf::from("./c.md"), 3)]
        );
        for broken in ["./c.md:3\n".to_string(), format!("./c.md{nul}three\n")] {
            assert_eq!(
                count_records(&broken).unwrap_err().code(),
                "search.ripgrep_invalid_output",
                "{broken:?}"
            );
        }
    }

    /// The same ruling: a volume is judged to fold case as a lookup in it does: a name written in one case and looked up in
    /// another. A folder with no entry is judged by its own name in its parent.
    #[test]
    fn a_volume_is_judged_to_fold_case_as_a_lookup_in_it_does() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("probe.md"), "").unwrap();
        let folded = root.join("PROBE.MD").exists();
        assert_eq!(folds_case(&root), folded);
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(folds_case(&empty), folded);
    }
}
