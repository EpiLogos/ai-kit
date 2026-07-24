//! The built-in secret scanner.
//!
//! ## Why it is built in
//!
//! Capture runs the scanner over content that has not been reviewed, in order to
//! decide whether it may be stored. If the scanner were itself a capsule, the
//! thing deciding whether unreviewed content is safe would be unreviewed content.
//! So this module depends on nothing but `regex` and refuses to grow a plugin
//! interface beyond a list of extra patterns the *user* configures.
//!
//! ## The two failure modes are not symmetrical in visibility
//!
//! A missed secret is loud when it eventually surfaces. A false positive is
//! quiet: the user shrugs, clicks past the quarantine, and within a fortnight has
//! learned to ignore every warning the tool produces. At that point the scanner
//! has negative value. Hence the shape of the rules below:
//!
//! * **Shape rules** (token prefixes, PEM markers) fire on structure that has no
//!   innocent reading. `ghp_` followed by 36 base62 characters is a GitHub token
//!   and nothing else.
//! * **Context rules** (`.env` assignments, `Authorization:` headers) require a
//!   secret-*sounding* name as well as a plausible value, and skip the
//!   placeholders that fill every `.env.example`.
//! * **The entropy rule** fires only inside an assignment, only on values of
//!   credential length, and only above 4.2 bits per character. That last number
//!   is chosen structurally: hexadecimal cannot exceed 4.0 bits per character, so
//!   **no commit sha, blob id or checksum can ever reach the threshold**, however
//!   long it is. Data URIs and oversized blobs are excluded for the same reason —
//!   an asset is not a credential, and treating one as such is how a scanner
//!   loses its audience.
//!
//! A [`Finding`] deliberately does not carry the matched text. Findings are
//! logged, rendered in previews and passed between modules; a finding that
//! contained the secret would be a second copy of it in exactly the places the
//! secret must not reach.

use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

use aikit_core::{AikitError, Result};

/// The kind of evidence a rule is based on. Ordered from most to least certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// A token whose shape is unmistakable.
    TokenPrefix,
    /// A PEM private key block.
    PrivateKey,
    /// An `Authorization:` (or `Proxy-Authorization:`) header with a credential.
    AuthorizationHeader,
    /// A `KEY=value` line whose key names a secret.
    EnvAssignment,
    /// A high-entropy value assigned to a secret-sounding name.
    HighEntropy,
    /// A pattern the user configured.
    Custom,
}

impl Family {
    pub fn as_str(self) -> &'static str {
        match self {
            Family::TokenPrefix => "token-prefix",
            Family::PrivateKey => "private-key",
            Family::AuthorizationHeader => "authorization-header",
            Family::EnvAssignment => "env-assignment",
            Family::HighEntropy => "high-entropy",
            Family::Custom => "custom",
        }
    }

    /// How sure the scanner is. Used to decide between quarantine and a warning.
    pub fn is_certain(self) -> bool {
        matches!(self, Family::TokenPrefix | Family::PrivateKey)
    }
}

/// One suspected secret. Carries where it is and what it looked like — never what
/// it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: String,
    pub family: Family,
    /// Byte range within the scanned text.
    pub range: Range<usize>,
    /// A masked hint, e.g. `ghp_…8 chars…`. Safe to print.
    pub preview: String,
    /// What to tell the user.
    pub description: String,
}

impl Finding {
    fn new(
        rule: impl Into<String>,
        family: Family,
        range: Range<usize>,
        matched: &str,
        description: impl Into<String>,
    ) -> Self {
        Self {
            rule: rule.into(),
            family,
            range,
            preview: mask(matched),
            description: description.into(),
        }
    }
}

/// A safe hint: the first few characters of structure, then a length.
///
/// Four leading characters is enough to say "this is a GitHub token" and far too
/// few to be useful to anyone who obtains the log.
fn mask(matched: &str) -> String {
    let head: String = matched.chars().take(4).collect();
    format!("{head}… ({} characters)", matched.chars().count())
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

struct ShapeRule {
    name: &'static str,
    family: Family,
    pattern: &'static str,
    description: &'static str,
}

/// Shapes with no innocent reading.
const SHAPE_RULES: &[ShapeRule] = &[
    ShapeRule {
        name: "github-token",
        family: Family::TokenPrefix,
        pattern: r"\bgh[pousr]_[A-Za-z0-9]{30,}",
        description: "a GitHub access token",
    },
    ShapeRule {
        name: "github-fine-grained-token",
        family: Family::TokenPrefix,
        pattern: r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        description: "a fine-grained GitHub token",
    },
    ShapeRule {
        name: "openai-key",
        family: Family::TokenPrefix,
        pattern: r"\bsk-[A-Za-z0-9]*-?[A-Za-z0-9]{16,}",
        description: "an OpenAI-style API key",
    },
    ShapeRule {
        name: "slack-token",
        family: Family::TokenPrefix,
        pattern: r"\bxox[baprs]-[A-Za-z0-9-]{10,}",
        description: "a Slack token",
    },
    ShapeRule {
        name: "aws-access-key-id",
        family: Family::TokenPrefix,
        pattern: r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
        description: "an AWS access key id",
    },
    ShapeRule {
        name: "google-api-key",
        family: Family::TokenPrefix,
        pattern: r"\bAIza[0-9A-Za-z_\-]{35}",
        description: "a Google API key",
    },
    ShapeRule {
        name: "gitlab-token",
        family: Family::TokenPrefix,
        pattern: r"\bglpat-[0-9A-Za-z_\-]{18,}",
        description: "a GitLab personal access token",
    },
    ShapeRule {
        name: "npm-token",
        family: Family::TokenPrefix,
        pattern: r"\bnpm_[A-Za-z0-9]{30,}",
        description: "an npm access token",
    },
    ShapeRule {
        name: "docker-token",
        family: Family::TokenPrefix,
        pattern: r"\bdckr_pat_[A-Za-z0-9_\-]{20,}",
        description: "a Docker Hub access token",
    },
    ShapeRule {
        name: "private-key",
        family: Family::PrivateKey,
        // The whole block when there is an END marker, the header alone when the
        // block is truncated — a truncated key is still a disclosed key.
        pattern: r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY[A-Z ]*-----(?:.*?-----END [A-Z0-9 ]*PRIVATE KEY[A-Z ]*-----)?",
        description: "a private key block",
    },
    ShapeRule {
        name: "authorization-header",
        family: Family::AuthorizationHeader,
        pattern: r"(?i)\b(?:proxy-)?authorization\s*:\s*(?:bearer|basic|token|digest)\s+[A-Za-z0-9+/=._\-]{16,}",
        description: "an Authorization header carrying a credential",
    },
];

/// Key names that make a value a secret whatever it looks like.
const SECRET_NAMES: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "pwd",
    "apikey",
    "api_key",
    "accesskey",
    "access_key",
    "privatekey",
    "private_key",
    "credential",
    "auth",
    "session_key",
    "signing_key",
    "client_secret",
];

/// Key names that make a high-entropy value *expected*, and therefore not a
/// finding: hashes, ids and versions are supposed to look random.
const BENIGN_NAMES: &[&str] = &[
    "hash", "sha", "sha1", "sha256", "digest", "checksum", "etag", "uuid", "guid", "revision",
    "commit", "blob", "nonce_len", "version", "url", "uri", "path", "id",
];

/// Values that are obviously not a real credential.
const PLACEHOLDERS: &[&str] = &[
    "changeme",
    "change-me",
    "your-secret-here",
    "yoursecrethere",
    "placeholder",
    "todo",
    "none",
    "null",
    "example",
    "redacted",
    "secret",
    "password",
    "hunter2",
];

/// Below this many characters a value cannot carry a useful credential; above the
/// upper bound it is an asset, not a token.
const MIN_SECRET_LEN: usize = 8;
const MAX_ENTROPY_VALUE_LEN: usize = 200;
const MIN_ENTROPY_VALUE_LEN: usize = 16;

/// Bits per character. Hexadecimal tops out at 4.0, so this threshold structurally
/// excludes every commit sha, blob id and checksum.
const ENTROPY_THRESHOLD: f64 = 4.2;

// ---------------------------------------------------------------------------
// The scanner
// ---------------------------------------------------------------------------

/// A configured scanner. [`Scanner::new`] carries the built-in rules; user
/// patterns are added on top and never replace them.
#[derive(Debug, Clone, Default)]
pub struct Scanner {
    custom: Vec<(String, Regex)>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a user-configured pattern.
    pub fn with_custom_rule(mut self, name: impl Into<String>, pattern: &str) -> Result<Self> {
        let name = name.into();
        let compiled = Regex::new(pattern).map_err(|e| {
            AikitError::new(
                "scan.invalid_pattern",
                format!("the custom secret pattern `{name}` is not a valid regex: {e}"),
            )
            .with("rule", name.clone())
            .with("pattern", pattern)
        })?;
        self.custom.push((name, compiled));
        Ok(self)
    }

    /// Findings in document order.
    pub fn scan(&self, text: &str) -> Vec<Finding> {
        let mut findings = Vec::new();

        for (rule, regex) in shape_regexes() {
            for m in regex.find_iter(text) {
                findings.push(Finding::new(
                    rule.name,
                    rule.family,
                    m.range(),
                    m.as_str(),
                    rule.description,
                ));
            }
        }

        findings.extend(scan_assignments(text));

        for (name, regex) in &self.custom {
            for m in regex.find_iter(text) {
                findings.push(Finding::new(
                    name.clone(),
                    Family::Custom,
                    m.range(),
                    m.as_str(),
                    "matched a locally configured secret pattern",
                ));
            }
        }

        findings.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then_with(|| b.range.end.cmp(&a.range.end))
                .then_with(|| a.rule.cmp(&b.rule))
        });
        findings
    }

    /// Replace every finding with a marker, merging overlaps.
    ///
    /// Merging matters: an AWS key inside `AWS_SECRET_ACCESS_KEY=` matches two
    /// rules, and splicing both replacements in would corrupt the text around it.
    pub fn redact(&self, text: &str) -> String {
        let findings = self.scan(text);
        if findings.is_empty() {
            return text.to_string();
        }

        let mut merged: Vec<(Range<usize>, Family)> = Vec::new();
        for finding in findings {
            match merged.last_mut() {
                Some((range, family)) if finding.range.start <= range.end => {
                    range.end = range.end.max(finding.range.end);
                    // The more certain family names the redaction.
                    if finding.family < *family {
                        *family = finding.family;
                    }
                }
                _ => merged.push((finding.range.clone(), finding.family)),
            }
        }

        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        for (range, family) in merged {
            let start = floor_boundary(text, range.start);
            let end = ceil_boundary(text, range.end);
            if start < cursor {
                continue;
            }
            out.push_str(&text[cursor..start]);
            out.push_str(&format!("[redacted:{}]", family.as_str()));
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        out
    }
}

/// Scan with the built-in rules only.
pub fn scan(text: &str) -> Vec<Finding> {
    Scanner::new().scan(text)
}

/// Redact with the built-in rules only.
pub fn redact(text: &str) -> String {
    Scanner::new().redact(text)
}

// ---------------------------------------------------------------------------
// Assignment rules
// ---------------------------------------------------------------------------

/// `KEY=value`, `key: "value"`, `let key = "value"`, `"key": "value"`.
fn assignment_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?m)["']?(?P<key>[A-Za-z_][A-Za-z0-9_.\-]{1,64})["']?\s*(?:=|:)\s*["']?(?P<value>[^"'\s,;)\]}]*)["']?"#,
        )
        .expect("the assignment pattern is a literal and is checked by the tests")
    })
}

fn scan_assignments(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for caps in assignment_regex().captures_iter(text) {
        let (Some(key), Some(value)) = (caps.name("key"), caps.name("value")) else {
            continue;
        };
        let whole = caps.get(0).expect("group 0 always exists");
        let key_text = key.as_str();
        let value_text = value.as_str();
        let lowered_key = key_text.to_ascii_lowercase();

        // `https://host/path` looks like `key: value` to any pattern that treats
        // a colon as an assignment. A scheme is not an assignment, and a URL path
        // is not a credential — this is what stops a commit sha in a link from
        // being read as a high-entropy secret.
        if whole.as_str().contains("://") || value_text.starts_with("//") {
            continue;
        }
        if is_placeholder(value_text) {
            continue;
        }

        // A secret-sounding name plus any plausible value.
        if names_a_secret(&lowered_key) && value_text.len() >= MIN_SECRET_LEN {
            let family = if is_high_entropy(value_text) {
                Family::HighEntropy
            } else {
                Family::EnvAssignment
            };
            findings.push(Finding::new(
                match family {
                    Family::HighEntropy => "secret-named-high-entropy-value",
                    _ => "secret-named-assignment",
                },
                family,
                whole.range(),
                whole.as_str(),
                format!("`{key_text}` names a credential"),
            ));
            continue;
        }

        // A value that looks like a credential even though the name is neutral.
        if !is_benign_name(&lowered_key) && is_high_entropy(value_text) {
            findings.push(Finding::new(
                "high-entropy-value",
                Family::HighEntropy,
                value.range(),
                value_text,
                "a high-entropy value in an assignment",
            ));
        }
    }
    findings
}

fn names_a_secret(lowered_key: &str) -> bool {
    if is_benign_name(lowered_key) {
        return false;
    }
    SECRET_NAMES.iter().any(|needle| lowered_key.contains(needle))
}

fn is_benign_name(lowered_key: &str) -> bool {
    BENIGN_NAMES
        .iter()
        .any(|needle| lowered_key == *needle || lowered_key.ends_with(&format!("_{needle}")))
}

fn is_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Shell and template interpolation is a reference, not a value.
    if trimmed.starts_with("${") || trimmed.starts_with("$(") || trimmed.starts_with('<') {
        return true;
    }
    if trimmed.starts_with("{{") || trimmed.starts_with("%{") {
        return true;
    }
    let squashed: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    if PLACEHOLDERS.iter().any(|p| squashed == *p) {
        return true;
    }
    // `xxxxxxxx`, `--------`, `********`: one repeated character is nobody's key.
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) => chars.all(|c| c == first),
        None => true,
    }
}

/// Shannon entropy per character, with the guards that keep assets out.
fn is_high_entropy(value: &str) -> bool {
    let value = value.trim();
    if value.len() < MIN_ENTROPY_VALUE_LEN || value.len() > MAX_ENTROPY_VALUE_LEN {
        return false;
    }
    // A data URI or a URL is a location, not a credential.
    if value.starts_with("data:") || value.contains("://") {
        return false;
    }
    // A UUID is structurally hex plus dashes; say so explicitly rather than
    // relying on the threshold alone, because the dashes raise the alphabet.
    if is_uuid(value) {
        return false;
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "+/=_-.".contains(c))
    {
        return false;
    }
    shannon_bits_per_char(value) > ENTROPY_THRESHOLD
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
        && value.matches('-').count() == 4
}

fn shannon_bits_per_char(value: &str) -> f64 {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for b in bytes {
        counts[*b as usize] += 1;
    }
    let total = bytes.len() as f64;
    -counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / total;
            p * p.log2()
        })
        .sum::<f64>()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn shape_regexes() -> &'static [(&'static ShapeRule, Regex)] {
    static COMPILED: OnceLock<Vec<(&'static ShapeRule, Regex)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        SHAPE_RULES
            .iter()
            .map(|rule| {
                let regex = Regex::new(rule.pattern).unwrap_or_else(|e| {
                    // These are literals in this file; a failure here is a bug
                    // caught by the tests, not a runtime condition.
                    panic!("built-in secret rule `{}` is not a valid regex: {e}", rule.name)
                });
                (rule, regex)
            })
            .collect()
    })
}

/// Nudge an index left to a character boundary, so redaction cannot split a
/// multi-byte character and produce invalid UTF-8 slicing.
fn floor_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}
