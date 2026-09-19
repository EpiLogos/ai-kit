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

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::runner::CommandRunner;
use aikit_core::{AikitError, Result};

/// Per-file byte budget handed to `--max-filesize`. Files above it are skipped
/// by ripgrep itself, which keeps a stray binary or log from dominating a scan.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Long matched lines are previewed, not returned whole: a minified asset must
/// not be able to push megabytes through one match event.
pub const MAX_COLUMNS: u32 = 240;
/// Bounded parallelism; the NOW field is small and this is not a fleet scan.
pub const SEARCH_THREADS: u32 = 2;

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
    pub timeout: Option<Duration>,
}

impl SearchRequest {
    pub fn new(pattern: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        Self {
            pattern: pattern.into(),
            regex: false,
            roots,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            hidden: false,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            limit: usize::MAX,
            timeout: Some(Duration::from_secs(30)),
        }
    }

    /// The exact argv this request runs. Tests assert on this so the flags the
    /// contract depends on are pinned in one place.
    pub fn argv(&self, executable: &Path) -> Vec<String> {
        let mut argv = vec![
            executable.to_string_lossy().into_owned(),
            "--json".into(),
            "--no-messages".into(),
            "--max-filesize".into(),
            format_filesize(self.max_file_bytes),
            "--max-columns".into(),
            MAX_COLUMNS.to_string(),
            "--max-columns-preview".into(),
            "--threads".into(),
            SEARCH_THREADS.to_string(),
        ];
        if !self.regex {
            argv.push("--fixed-strings".into());
        }
        if self.hidden {
            argv.push("--hidden".into());
            argv.push("--no-ignore".into());
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
        for root in &self.roots {
            argv.push(root.to_string_lossy().into_owned());
        }
        argv
    }

    fn is_no_match(&self) -> bool {
        self.pattern.is_empty()
    }
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
        let argv = request.argv(&self.executable);
        let output = self.runner.run(&argv)?;
        // 0 = matches, 1 = clean no-match. Both parse their stream; only a
        // ripgrep failure (2) becomes an error.
        let outcome = parse_events(&output.stdout, request.limit)?;
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
    use crate::runner::ScriptedRunner;

    fn rg_json_line(path: &str, line_number: u64, text: &str) -> String {
        format!(
            r#"{{"type":"match","data":{{"path":{{"text":"{path}"}},"lines":{{"text":"{text}\n"}},"line_number":{line_number},"submatches":[]}}}}"#
        )
    }

    fn request() -> SearchRequest {
        SearchRequest {
            pattern: "harness gate".into(),
            regex: false,
            roots: vec![PathBuf::from("/ground")],
            include_globs: vec!["Control/agents/now/**".into()],
            exclude_globs: vec![".git/**".into()],
            hidden: true,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            limit: 10,
            timeout: Some(Duration::from_secs(30)),
        }
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

    #[test]
    fn filesize_budgets_render_in_ripgrep_units() {
        assert_eq!(format_filesize(DEFAULT_MAX_FILE_BYTES), "1M");
        assert_eq!(format_filesize(512), "512");
        assert_eq!(format_filesize(64 * 1024), "64K");
    }
}
