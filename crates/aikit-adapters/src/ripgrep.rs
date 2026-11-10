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
    /// are always emitted after the includes and always win.
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
        )
    }

    fn argv_over(&self, executable: &Path, paths: impl IntoIterator<Item = String>) -> Vec<String> {
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
        for glob in &self.exclude_globs {
            argv.push("--glob".into());
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
    /// it is the same program wherever that pass starts.
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
                argv: self.argv_over(&started_in_a_root, [".".to_string()]),
            })
            .collect();
        if !files.is_empty() {
            passes.push(RipgrepPass {
                root: None,
                argv: self.argv_over(
                    executable,
                    files
                        .into_iter()
                        .map(|file| file.to_string_lossy().into_owned()),
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
/// place of match events, every file counted.
fn counted(mut argv: Vec<String>) -> Vec<String> {
    if let Some(json) = argv.iter().position(|arg| arg == "--json") {
        argv.splice(json..=json, ["--count".to_string(), "--null".to_string()]);
    }
    if let Some(cap) = argv.iter().position(|arg| arg == "--max-count") {
        argv.drain(cap..=cap + 1);
    }
    argv
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
}

impl RipgrepOutcome {
    /// One pass's outcome added to the request's, its paths named under the
    /// pass's root.
    fn add(&mut self, pass: &RipgrepPass, part: RipgrepOutcome) {
        self.files_searched += part.files_searched;
        self.truncated |= part.truncated;
        self.matches
            .extend(part.matches.into_iter().map(|matched| RipgrepMatch {
                path: pass.named(matched.path),
                ..matched
            }));
    }
}

/// The one wall-clock budget every pass of a request shares.
struct Deadline(Option<std::time::Instant>);

impl Deadline {
    fn new(budget: Option<Duration>) -> Self {
        Self(budget.and_then(|budget| std::time::Instant::now().checked_add(budget)))
    }

    /// What is left of the budget for the next pass, or a timeout once it is
    /// spent, as the runner names one.
    fn remaining(&self, argv: &[String]) -> Result<Option<Duration>> {
        let Some(deadline) = self.0 else {
            return Ok(None);
        };
        let left = deadline.saturating_duration_since(std::time::Instant::now());
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
        let deadline = Deadline::new(request.timeout);
        let mut outcome = RipgrepOutcome::default();
        for pass in request.passes(&self.executable) {
            let timeout = deadline.remaining(&pass.argv)?;
            let output = match &pass.root {
                Some(root) => self.runner.run_in(root, &pass.argv, timeout)?,
                None => match timeout {
                    Some(budget) => self.runner.run_with_timeout(&pass.argv, budget)?,
                    None => self.runner.run(&pass.argv)?,
                },
            };
            let kept = request.limit.saturating_sub(outcome.matches.len());
            outcome.add(&pass, Self::outcome_of(kept, &pass.argv, &output)?);
        }
        Ok(outcome)
    }

    /// [`RipgrepSearcher::search`] with what is read bounded as well as what
    /// is kept: the stream may carry `request.limit` matches' worth of events
    /// (`MATCH_EVENT_BYTES` each), and a stream past that is not read on. The
    /// runner stops it at that capacity and refuses the output whole (no
    /// truncated success), so the pass answers truncated with no sample and
    /// a ranking caller rescores from its exact counts. A runner that cannot
    /// bound one read runs the pass unbounded, as [`RipgrepSearcher::search`].
    pub fn search_bounded(&self, request: &SearchRequest) -> Result<RipgrepOutcome> {
        if request.is_no_match() || request.roots.is_empty() || request.limit == usize::MAX {
            return self.search(request);
        }
        let deadline = Deadline::new(Some(request.timeout.unwrap_or(BOUNDED_SEARCH_TIMEOUT)));
        let mut outcome = RipgrepOutcome::default();
        for pass in request.passes(&self.executable) {
            let kept = request.limit.saturating_sub(outcome.matches.len());
            // Both streams share the allowance half and half: stdout's half is
            // the match budget, what this pass may still keep.
            let allowance = kept
                .saturating_add(1)
                .saturating_mul(MATCH_EVENT_BYTES)
                .saturating_mul(2);
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
                Ok(output) => outcome.add(&pass, Self::outcome_of(kept, &pass.argv, &output)?),
                Err(error) if error.code() == "mux.command_output_limit" => {
                    return Ok(RipgrepOutcome {
                        truncated: true,
                        ..RipgrepOutcome::default()
                    })
                }
                Err(error) if error.code() == "mux.command_limits_unsupported" => {
                    return self.search(request)
                }
                Err(error) => return Err(error),
            }
        }
        Ok(outcome)
    }

    /// One pass's answer, keeping at most `kept` matches.
    fn outcome_of(
        kept: usize,
        argv: &[String],
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
        Err(AikitError::new(
            "search.ripgrep_failed",
            format!("ripgrep exited with status {}: {detail}", output.status),
        )
        .with("command", argv.join(" "))
        .with("status", output.status.to_string()))
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
        let deadline = Deadline::new(request.timeout);
        let mut counts = Vec::new();
        for pass in request.passes(&self.executable) {
            let argv = counted(pass.argv.clone());
            let timeout = deadline.remaining(&argv)?;
            let output = match &pass.root {
                Some(root) => self.runner.run_in(root, &argv, timeout)?,
                None => match timeout {
                    Some(budget) => self.runner.run_with_timeout(&argv, budget)?,
                    None => self.runner.run(&argv)?,
                },
            };
            if output.status != 0 && output.status != 1 {
                return Err(AikitError::new(
                    "search.ripgrep_failed",
                    format!("ripgrep count exited with status {}", output.status),
                )
                .with("command", argv.join(" ")));
            }
            for line in output.stdout.lines() {
                let Some((path, count)) = line.split_once('\0') else {
                    // rg names paths only when it walks a scope; over a single
                    // file root it prints the bare count, and that count is that
                    // file's.
                    if request.roots.len() == 1 && pass.root.is_none() {
                        if let Ok(count) = line.trim().parse::<u64>() {
                            counts.push((request.roots[0].clone(), count));
                        }
                    }
                    continue;
                };
                if let Ok(count) = count.trim().parse::<u64>() {
                    counts.push((pass.named(PathBuf::from(path)), count));
                }
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
        let cut = searcher.search_bounded(&narrow).unwrap();
        assert_eq!((cut.matches.len(), cut.truncated), (0, true));
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
}
