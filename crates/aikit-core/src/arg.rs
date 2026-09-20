//! Manifest-driven argument specifications.
//!
//! The palette renders forms straight from these, and the runner turns filled
//! forms into argv. Keeping both on one declaration is what stops the TUI from
//! growing its own notion of what a script accepts.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::duration::HumanDuration;
use crate::error::{err, AikitError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArgType {
    String,
    Path,
    Integer,
    Float,
    Bool,
    Enum,
    Multiselect,
    Duration,
    Secret,
    KeyValue,
}

/// Where a default comes from when it is not a literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultSource {
    ProjectRoot,
    Cwd,
    SessionId,
    ContextId,
    GitBranch,
    TaskName,
}

/// Restrict a `path` argument to a file or a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum PathKind {
    #[default]
    Any,
    File,
    Directory,
}

/// A literal default value as written in TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Literal {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<String>),
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Bool(b) => write!(f, "{b}"),
            Literal::Integer(i) => write!(f, "{i}"),
            Literal::Float(x) => write!(f, "{x}"),
            Literal::String(s) => f.write_str(s),
            Literal::List(v) => f.write_str(&v.join(",")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgSpec {
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(rename = "type")]
    pub ty: ArgType,
    /// Positional index (1-based). Mutually exclusive with `flag`.
    #[serde(default)]
    pub position: Option<u8>,
    /// Flag form, e.g. `--changed`.
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub default: Option<Literal>,
    #[serde(default)]
    pub default_from: Option<DefaultSource>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub must_exist: bool,
    #[serde(default)]
    pub path_kind: PathKind,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub repeatable: bool,
    /// Secrets are masked in previews and never written to the event log.
    #[serde(default)]
    pub secret: bool,
}

impl ArgSpec {
    pub fn display_label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }

    /// An argument is required when it says so, or when it is positional with no
    /// default of any kind.
    pub fn is_required(&self) -> bool {
        if let Some(explicit) = self.required {
            return explicit;
        }
        self.position.is_some() && self.default.is_none() && self.default_from.is_none()
    }

    pub fn is_secret(&self) -> bool {
        self.secret || self.ty == ArgType::Secret
    }

    /// Structural validation of the specification itself (not of a value).
    pub fn validate_spec(&self) -> Result<()> {
        if self.name.is_empty() {
            return err("arg.invalid_spec", "an argument must have a name");
        }
        if self.position.is_some() && self.flag.is_some() {
            return err(
                "arg.invalid_spec",
                format!("`{}` declares both a position and a flag", self.name),
            );
        }
        if matches!(self.ty, ArgType::Enum | ArgType::Multiselect) && self.choices.is_empty() {
            return err(
                "arg.invalid_spec",
                format!("`{}` is an enum but declares no choices", self.name),
            );
        }
        if self.is_secret() && self.default.is_some() {
            return err(
                "arg.invalid_spec",
                format!(
                    "`{}` is a secret and must not carry a literal default",
                    self.name
                ),
            );
        }
        if let Some(p) = &self.pattern {
            regex::Regex::new(p).map_err(|e| {
                AikitError::new(
                    "arg.invalid_spec",
                    format!("`{}` has an invalid pattern: {e}", self.name),
                )
            })?;
        }
        Ok(())
    }

    /// Validate and normalize a user-supplied value.
    pub fn coerce(&self, raw: &str) -> Result<ArgValue> {
        let invalid =
            |msg: String| AikitError::new("arg.invalid_value", msg).with("arg", &self.name);

        if let Some(p) = &self.pattern {
            let re = regex::Regex::new(p).map_err(|e| invalid(format!("invalid pattern: {e}")))?;
            if !re.is_match(raw) {
                return Err(invalid(format!(
                    "`{}` does not match the required pattern {p}",
                    self.display_label()
                )));
            }
        }

        let value = match self.ty {
            ArgType::Bool => ArgValue::Bool(match raw {
                "true" | "1" | "yes" | "on" => true,
                "false" | "0" | "no" | "off" | "" => false,
                other => return Err(invalid(format!("`{other}` is not a boolean"))),
            }),
            ArgType::Integer => {
                let n: i64 = raw
                    .parse()
                    .map_err(|_| invalid(format!("`{raw}` is not an integer")))?;
                self.check_range(n as f64).map_err(invalid)?;
                ArgValue::Integer(n)
            }
            ArgType::Float => {
                let n: f64 = raw
                    .parse()
                    .map_err(|_| invalid(format!("`{raw}` is not a number")))?;
                self.check_range(n).map_err(invalid)?;
                ArgValue::Float(n)
            }
            ArgType::Duration => ArgValue::Duration(
                HumanDuration::parse(raw).map_err(|e| invalid(e.message().into()))?,
            ),
            ArgType::Enum => {
                if !self.choices.iter().any(|c| c == raw) {
                    return Err(invalid(format!(
                        "`{raw}` is not one of: {}",
                        self.choices.join(", ")
                    )));
                }
                ArgValue::String(raw.to_string())
            }
            ArgType::Multiselect => {
                let picks: Vec<String> = raw
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                for pick in &picks {
                    if !self.choices.contains(pick) {
                        return Err(invalid(format!(
                            "`{pick}` is not one of: {}",
                            self.choices.join(", ")
                        )));
                    }
                }
                ArgValue::List(picks)
            }
            ArgType::KeyValue => {
                let mut map = BTreeMap::new();
                for pair in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let (k, v) = pair
                        .split_once('=')
                        .ok_or_else(|| invalid(format!("`{pair}` is not `key=value`")))?;
                    map.insert(k.trim().to_string(), v.trim().to_string());
                }
                ArgValue::KeyValue(map)
            }
            ArgType::Secret => ArgValue::Secret(raw.to_string()),
            ArgType::Path | ArgType::String => ArgValue::String(raw.to_string()),
        };
        Ok(value)
    }

    fn check_range(&self, n: f64) -> std::result::Result<(), String> {
        if let Some(min) = self.min {
            if n < min {
                return Err(format!("{n} is below the minimum {min}"));
            }
        }
        if let Some(max) = self.max {
            if n > max {
                return Err(format!("{n} is above the maximum {max}"));
            }
        }
        Ok(())
    }
}

/// A validated argument value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub enum ArgValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Duration(HumanDuration),
    List(Vec<String>),
    KeyValue(BTreeMap<String, String>),
    Secret(String),
}

impl ArgValue {
    /// How the value appears in a preview or log. Secrets never leak here.
    pub fn redacted(&self) -> String {
        match self {
            ArgValue::Secret(_) => "••••••".to_string(),
            other => other.to_argv_string(),
        }
    }

    pub fn to_argv_string(&self) -> String {
        match self {
            ArgValue::String(s) | ArgValue::Secret(s) => s.clone(),
            ArgValue::Integer(n) => n.to_string(),
            ArgValue::Float(n) => n.to_string(),
            ArgValue::Bool(b) => b.to_string(),
            ArgValue::Duration(d) => d.to_string(),
            ArgValue::List(v) => v.join(","),
            ArgValue::KeyValue(m) => m
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ArgValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// A filled-in form: argument name to value.
pub type ArgValues = BTreeMap<String, ArgValue>;

/// Turn a specification plus filled values into argv, in declaration order.
///
/// Positional arguments come first ordered by `position`; flagged arguments
/// follow in declaration order. A `bool` flag emits the flag only when true.
pub fn build_argv(specs: &[ArgSpec], values: &ArgValues) -> Result<Vec<String>> {
    for spec in specs {
        if spec.is_required() && !values.contains_key(&spec.name) {
            return err(
                "arg.missing_required",
                format!("`{}` is required", spec.display_label()),
            );
        }
    }

    let mut positional: Vec<(u8, String)> = Vec::new();
    let mut flagged: Vec<String> = Vec::new();

    for spec in specs {
        let Some(value) = values.get(&spec.name) else {
            continue;
        };
        match (&spec.flag, spec.position) {
            (Some(flag), _) => {
                if spec.ty == ArgType::Bool {
                    if value.as_bool().unwrap_or(false) {
                        flagged.push(flag.clone());
                    }
                } else if spec.repeatable {
                    if let ArgValue::List(items) = value {
                        for item in items {
                            flagged.push(flag.clone());
                            flagged.push(item.clone());
                        }
                    } else {
                        flagged.push(flag.clone());
                        flagged.push(value.to_argv_string());
                    }
                } else {
                    flagged.push(flag.clone());
                    flagged.push(value.to_argv_string());
                }
            }
            (None, Some(pos)) => positional.push((pos, value.to_argv_string())),
            (None, None) => {
                // Neither positional nor flagged: passed through as `--name value`.
                flagged.push(format!("--{}", spec.name));
                flagged.push(value.to_argv_string());
            }
        }
    }

    positional.sort_by_key(|(p, _)| *p);
    let mut argv: Vec<String> = positional.into_iter().map(|(_, v)| v).collect();
    argv.extend(flagged);
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, ty: ArgType) -> ArgSpec {
        ArgSpec {
            name: name.to_string(),
            label: None,
            help: None,
            ty,
            position: None,
            flag: None,
            required: None,
            default: None,
            default_from: None,
            choices: vec![],
            must_exist: false,
            path_kind: PathKind::Any,
            min: None,
            max: None,
            pattern: None,
            repeatable: false,
            secret: false,
        }
    }

    #[test]
    fn a_positional_argument_without_a_default_is_required() {
        let mut s = spec("path", ArgType::Path);
        s.position = Some(1);
        assert!(s.is_required());
        s.default_from = Some(DefaultSource::ProjectRoot);
        assert!(!s.is_required());
    }

    #[test]
    fn boolean_flags_are_emitted_only_when_true() {
        let mut changed = spec("changed", ArgType::Bool);
        changed.flag = Some("--changed".into());
        let specs = vec![changed];

        let mut values = ArgValues::new();
        values.insert("changed".into(), ArgValue::Bool(false));
        assert_eq!(build_argv(&specs, &values).unwrap(), Vec::<String>::new());

        values.insert("changed".into(), ArgValue::Bool(true));
        assert_eq!(build_argv(&specs, &values).unwrap(), vec!["--changed"]);
    }

    #[test]
    fn positional_arguments_precede_flags_and_sort_by_position() {
        let mut second = spec("second", ArgType::String);
        second.position = Some(2);
        let mut first = spec("first", ArgType::String);
        first.position = Some(1);
        let mut opt = spec("opt", ArgType::String);
        opt.flag = Some("--opt".into());

        // Declared out of order on purpose.
        let specs = vec![second, opt, first];
        let mut values = ArgValues::new();
        values.insert("first".into(), ArgValue::String("a".into()));
        values.insert("second".into(), ArgValue::String("b".into()));
        values.insert("opt".into(), ArgValue::String("c".into()));

        assert_eq!(
            build_argv(&specs, &values).unwrap(),
            vec!["a", "b", "--opt", "c"]
        );
    }

    #[test]
    fn a_repeatable_flag_emits_once_per_value() {
        let mut inc = spec("include", ArgType::String);
        inc.flag = Some("-I".into());
        inc.repeatable = true;
        let mut values = ArgValues::new();
        values.insert(
            "include".into(),
            ArgValue::List(vec!["src".into(), "tests".into()]),
        );
        assert_eq!(
            build_argv(&[inc], &values).unwrap(),
            vec!["-I", "src", "-I", "tests"]
        );
    }

    #[test]
    fn a_missing_required_argument_fails_before_execution() {
        let mut s = spec("path", ArgType::Path);
        s.position = Some(1);
        let err = build_argv(&[s], &ArgValues::new()).unwrap_err();
        assert_eq!(err.code(), "arg.missing_required");
    }

    #[test]
    fn enum_values_are_checked_against_the_declared_choices() {
        let mut s = spec("profile", ArgType::Enum);
        s.choices = vec!["ci".into(), "local".into()];
        assert!(s.coerce("ci").is_ok());
        assert_eq!(s.coerce("prod").unwrap_err().code(), "arg.invalid_value");
    }

    #[test]
    fn numeric_ranges_are_enforced() {
        let mut s = spec("jobs", ArgType::Integer);
        s.min = Some(1.0);
        s.max = Some(16.0);
        assert!(s.coerce("8").is_ok());
        assert!(s.coerce("0").is_err());
        assert!(s.coerce("32").is_err());
        assert!(s.coerce("many").is_err());
    }

    #[test]
    fn secrets_are_redacted_in_previews() {
        let v = ArgValue::Secret("hunter2".into());
        assert_eq!(v.redacted(), "••••••");
        assert!(!v.redacted().contains("hunter2"));
    }

    #[test]
    fn a_secret_argument_may_not_carry_a_literal_default() {
        let mut s = spec("token", ArgType::Secret);
        s.default = Some(Literal::String("oops".into()));
        assert_eq!(s.validate_spec().unwrap_err().code(), "arg.invalid_spec");
    }

    #[test]
    fn an_argument_cannot_be_both_positional_and_flagged() {
        let mut s = spec("path", ArgType::Path);
        s.position = Some(1);
        s.flag = Some("--path".into());
        assert_eq!(s.validate_spec().unwrap_err().code(), "arg.invalid_spec");
    }

    #[test]
    fn key_value_arguments_parse_into_a_sorted_map() {
        let s = spec("env", ArgType::KeyValue);
        let v = s.coerce("B=2, A=1").unwrap();
        assert_eq!(v.to_argv_string(), "A=1,B=2");
    }
}
