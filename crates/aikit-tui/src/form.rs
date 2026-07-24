//! Argument forms, built from `[[args]]` and nothing else.
//!
//! The palette has no opinion about what a script accepts. Types, requiredness,
//! defaults, choices, ranges and patterns are all read from
//! [`aikit_core::arg::ArgSpec`], values are validated by
//! [`ArgSpec::coerce`](aikit_core::arg::ArgSpec::coerce), and argv is produced by
//! [`aikit_core::arg::build_argv`]. A form that built argv itself would be a
//! second, quietly divergent implementation of the calling convention, and the
//! divergence would show up as a script invoked wrongly rather than as a test
//! failure.
//!
//! ## What this module adds
//!
//! Exactly two things core cannot do.
//!
//! * **Filesystem checks.** `must_exist` and `path_kind` are declarations about
//!   the world, and `aikit-core` is free of I/O by design. Checking them at the
//!   field, before anything runs, is the difference between "that directory does
//!   not exist" and a script's own error forty lines into its output.
//! * **Presentation**, including redaction.
//!
//! ## Secrets
//!
//! A secret is masked in its field, replaced before argv is built for any
//! preview, and dropped entirely from the intent that goes into the recent list.
//! The last one has a consequence the tests pin down: repeating a run that needed
//! a secret fails the required check and asks again. That is the intended
//! behaviour — a secret is re-entered, never replayed out of a history buffer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::arg::{ArgSpec, ArgType, ArgValues, DefaultSource, PathKind};
use aikit_core::capsule::{Capsule, ExecMode, WorkingDir};
use aikit_core::context::ContextDescriptor;
use aikit_core::error::AikitError;
use aikit_core::Result;

use crate::backend::{RunIntent, REDACTED};

/// The values behind `default_from`.
///
/// Most come straight from the context descriptor. The two that do not — the
/// working directory and the git branch — are supplied by the caller, because the
/// palette does not run `git` and should not be the thing that decides what "the
/// current directory" means for a task with its own tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormContext {
    values: BTreeMap<&'static str, String>,
    project_root: Option<PathBuf>,
    label: String,
    isolation: &'static str,
}

impl FormContext {
    pub fn from_descriptor(descriptor: &ContextDescriptor) -> Self {
        let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
        if let Some(root) = &descriptor.project_root {
            values.insert(key(DefaultSource::ProjectRoot), root.display().to_string());
        }
        if let Some(session) = &descriptor.session_id {
            values.insert(key(DefaultSource::SessionId), session.to_string());
        }
        values.insert(
            key(DefaultSource::ContextId),
            descriptor.context_id.to_string(),
        );
        if let Some(task) = &descriptor.task {
            values.insert(key(DefaultSource::TaskName), task.clone());
        }
        Self {
            values,
            project_root: descriptor.project_root.clone(),
            label: descriptor.label(),
            isolation: descriptor.isolation.as_str(),
        }
    }

    /// Supply a value the context alone cannot produce.
    #[must_use]
    pub fn with_default(mut self, source: DefaultSource, value: impl Into<String>) -> Self {
        self.values.insert(key(source), value.into());
        self
    }

    pub fn value(&self, source: DefaultSource) -> Option<&str> {
        self.values.get(key(source)).map(String::as_str)
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn isolation(&self) -> &'static str {
        self.isolation
    }
}

fn key(source: DefaultSource) -> &'static str {
    match source {
        DefaultSource::ProjectRoot => "project_root",
        DefaultSource::Cwd => "cwd",
        DefaultSource::SessionId => "session_id",
        DefaultSource::ContextId => "context_id",
        DefaultSource::GitBranch => "git_branch",
        DefaultSource::TaskName => "task_name",
    }
}

/// One editable argument.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub spec: ArgSpec,
    input: String,
    /// Where `Space` is in the choice list, for enums and multiselects.
    choice_cursor: usize,
    error: Option<AikitError>,
}

impl Field {
    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn required(&self) -> bool {
        self.spec.is_required()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.message())
    }

    pub fn error_code(&self) -> Option<&'static str> {
        self.error.as_ref().map(|e| e.code())
    }

    /// What the field shows. A secret shows its length and nothing else, so the
    /// user can see they typed something without the value reaching the screen.
    pub fn display(&self) -> String {
        if self.spec.is_secret() {
            return "•".repeat(self.input.chars().count());
        }
        self.input.clone()
    }

    /// The short type hint drawn after the label.
    pub fn type_hint(&self) -> String {
        match self.spec.ty {
            ArgType::Enum | ArgType::Multiselect => self.spec.choices.join("/"),
            ArgType::Bool => "true/false".to_string(),
            ArgType::Path => match self.spec.path_kind {
                PathKind::File => "file".to_string(),
                PathKind::Directory => "directory".to_string(),
                PathKind::Any => "path".to_string(),
            },
            ArgType::Integer | ArgType::Float => match (self.spec.min, self.spec.max) {
                (Some(min), Some(max)) => format!("{min}–{max}"),
                (Some(min), None) => format!("≥ {min}"),
                (None, Some(max)) => format!("≤ {max}"),
                (None, None) => "number".to_string(),
            },
            ArgType::Duration => "e.g. 30s, 5m".to_string(),
            ArgType::Secret => "secret".to_string(),
            ArgType::KeyValue => "k=v,k=v".to_string(),
            ArgType::String => "text".to_string(),
        }
    }

    fn selected(&self) -> Vec<String> {
        self.input
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// The form for one capability.
#[derive(Debug, Clone, PartialEq)]
pub struct ArgForm {
    fields: Vec<Field>,
    focused: usize,
}

impl ArgForm {
    /// Build the form and seed every default.
    ///
    /// A `default_from` that resolves wins over a literal `default`: the literal
    /// was written once for every context, the derived one is about this one.
    pub fn new(capsule: &Capsule, context: &FormContext) -> Self {
        let fields = capsule
            .args
            .iter()
            .map(|spec| {
                let derived = spec.default_from.and_then(|s| context.value(s));
                let input = match (derived, &spec.default) {
                    (Some(value), _) => value.to_string(),
                    (None, Some(literal)) => literal.to_string(),
                    (None, None) if spec.ty == ArgType::Bool => "false".to_string(),
                    // An enum with choices starts on the first one rather than
                    // empty: there is no such thing as "no enum".
                    (None, None) if spec.ty == ArgType::Enum => {
                        spec.choices.first().cloned().unwrap_or_default()
                    }
                    (None, None) => String::new(),
                };
                Field {
                    spec: spec.clone(),
                    input,
                    choice_cursor: 0,
                    error: None,
                }
            })
            .collect();
        Self { fields, focused: 0 }
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn focused(&self) -> usize {
        self.focused
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn focus_next(&mut self) {
        if !self.fields.is_empty() {
            self.focused = (self.focused + 1) % self.fields.len();
        }
    }

    pub fn focus_previous(&mut self) {
        if !self.fields.is_empty() {
            self.focused = (self.focused + self.fields.len() - 1) % self.fields.len();
        }
    }

    pub fn input_char(&mut self, c: char) {
        if let Some(field) = self.fields.get_mut(self.focused) {
            field.input.push(c);
            field.error = None;
        }
    }

    pub fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.focused) {
            field.input.pop();
            field.error = None;
        }
    }

    pub fn set_input(&mut self, index: usize, value: &str) {
        if let Some(field) = self.fields.get_mut(index) {
            field.input = value.to_string();
            field.error = None;
        }
    }

    /// `Space` on a field whose type has discrete states.
    ///
    /// A boolean flips. An enum advances through its choices and wraps. A
    /// multiselect toggles the choice the cursor is on and then advances, so
    /// holding `Space` walks the list turning options on, and a second lap turns
    /// them back off.
    pub fn activate(&mut self, index: usize) {
        let Some(field) = self.fields.get_mut(index) else {
            return;
        };
        field.error = None;
        match field.spec.ty {
            ArgType::Bool => {
                field.input = if field.input == "true" { "false" } else { "true" }.to_string();
            }
            ArgType::Enum => {
                if field.spec.choices.is_empty() {
                    return;
                }
                let current = field
                    .spec
                    .choices
                    .iter()
                    .position(|c| *c == field.input)
                    .unwrap_or(0);
                let next = (current + 1) % field.spec.choices.len();
                field.input = field.spec.choices[next].clone();
                field.choice_cursor = next;
            }
            ArgType::Multiselect => {
                if field.spec.choices.is_empty() {
                    return;
                }
                let cursor = field.choice_cursor % field.spec.choices.len();
                let choice = field.spec.choices[cursor].clone();
                let mut selected = field.selected();
                if let Some(at) = selected.iter().position(|s| *s == choice) {
                    selected.remove(at);
                } else {
                    selected.push(choice);
                }
                // Keep declaration order, so the rendered value and the argv it
                // produces do not depend on the order things were clicked.
                selected.sort_by_key(|s| {
                    field
                        .spec
                        .choices
                        .iter()
                        .position(|c| c == s)
                        .unwrap_or(usize::MAX)
                });
                field.input = selected.join(",");
                field.choice_cursor = (cursor + 1) % field.spec.choices.len();
            }
            _ => {}
        }
    }

    /// Activate the focused field.
    pub fn activate_focused(&mut self) {
        self.activate(self.focused);
    }

    /// Validate every field, recording per-field errors. Returns true when the
    /// form could produce an intent.
    pub fn validate(&mut self) -> bool {
        let mut ok = true;
        for field in &mut self.fields {
            field.error = None;
            if field.input.is_empty() {
                if field.required() {
                    field.error = Some(
                        AikitError::new(
                            "arg.missing_required",
                            format!("`{}` is required", field.spec.display_label()),
                        )
                        .with("arg", field.spec.name.clone()),
                    );
                    ok = false;
                }
                continue;
            }
            if let Err(error) = field.spec.coerce(&field.input) {
                field.error = Some(error);
                ok = false;
                continue;
            }
            if field.spec.ty == ArgType::Path {
                if let Err(error) = check_path(&field.spec, &field.input) {
                    field.error = Some(error);
                    ok = false;
                }
            }
        }
        ok
    }

    /// The coerced values. Fails on the first invalid field.
    pub fn values(&self) -> Result<ArgValues> {
        let mut values = ArgValues::new();
        for field in &self.fields {
            if field.input.is_empty() {
                continue;
            }
            values.insert(field.spec.name.clone(), field.spec.coerce(&field.input)?);
        }
        Ok(values)
    }

    /// The invocation this form describes.
    pub fn intent(&self, capsule: &Capsule, descriptor: &ContextDescriptor) -> Result<RunIntent> {
        self.intent_with_confirmation(capsule, descriptor, false)
    }

    /// The invocation, told whether the capsule's revision has been reviewed.
    ///
    /// Confirmation is not something the form can work out: it is a trust fact,
    /// and trust lives in AIKit's database, not in a manifest.
    pub fn intent_with_confirmation(
        &self,
        capsule: &Capsule,
        descriptor: &ContextDescriptor,
        requires_confirmation: bool,
    ) -> Result<RunIntent> {
        let values = self.values()?;
        // Fail here rather than at execution: `build_argv` is the authority on
        // whether the form is complete, and the palette must not hand back an
        // intent that cannot run.
        aikit_core::arg::build_argv(&capsule.args, &values)?;

        let (mode, cwd, env) = match capsule.script() {
            Some(script) => (script.mode, script.cwd, script.env.clone()),
            None => (ExecMode::default(), WorkingDir::default(), BTreeMap::new()),
        };
        Ok(RunIntent {
            capsule: capsule.id.clone(),
            context: descriptor.context_id.clone(),
            specs: capsule.args.clone(),
            values,
            mode,
            cwd,
            env,
            requires_confirmation,
        })
    }
}

/// The `must_exist` and `path_kind` declarations, against the real filesystem.
fn check_path(spec: &ArgSpec, raw: &str) -> Result<()> {
    if !spec.must_exist && spec.path_kind == PathKind::Any {
        return Ok(());
    }
    let path = Path::new(raw);
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) if !spec.must_exist => return Ok(()),
        Err(e) => {
            return Err(AikitError::new(
                "arg.path_missing",
                format!(
                    "`{}` must name an existing path, and `{raw}` does not exist ({e})",
                    spec.display_label()
                ),
            )
            .with("arg", spec.name.clone())
            .with("path", raw))
        }
    };
    let wrong = match spec.path_kind {
        PathKind::File => !metadata.is_file(),
        PathKind::Directory => !metadata.is_dir(),
        PathKind::Any => false,
    };
    if wrong {
        let wanted = if spec.path_kind == PathKind::File {
            "a file"
        } else {
            "a directory"
        };
        return Err(AikitError::new(
            "arg.path_wrong_kind",
            format!(
                "`{}` must be {wanted}, and `{raw}` is not",
                spec.display_label()
            ),
        )
        .with("arg", spec.name.clone())
        .with("path", raw));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

/// What running this would do, in the words the user needs before saying yes.
///
/// Built from a *redacted* argv rather than by masking a rendered string: there
/// is no intermediate value in which the secret exists as display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPreview {
    rows: Vec<(String, String)>,
}

impl RunPreview {
    pub fn of(capsule: &Capsule, intent: &RunIntent, context: &FormContext) -> Self {
        let mut rows: Vec<(String, String)> = Vec::new();

        let command = capsule
            .exported_commands()
            .first()
            .cloned()
            .unwrap_or_else(|| capsule.id.leaf().to_string());
        rows.push(("Command".into(), command));

        let arguments = match intent.redacted_argv() {
            Ok(argv) if argv.is_empty() => "none".to_string(),
            Ok(argv) => argv.join(" "),
            // A preview that cannot be built says why rather than showing a
            // half-formed command line.
            Err(e) => format!("incomplete — {}", e.message()),
        };
        rows.push(("Arguments".into(), arguments));

        rows.push(("Working directory".into(), working_dir(intent.cwd, context)));
        rows.push((
            "Context".into(),
            format!("{} · {}", context.label(), context.isolation()),
        ));

        let env = if intent.env.is_empty() {
            "none".to_string()
        } else {
            intent
                .env
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        rows.push(("Environment".into(), env));
        rows.push(("Effects".into(), capsule.effects.summary()));

        let mut mode = intent.mode.as_str().to_string();
        if intent.mode.releases_terminal() {
            mode.push_str(" — the palette closes first");
        }
        if intent.mode.needs_mux() {
            mode.push_str(" — needs a multiplexer");
        }
        rows.push(("Mode".into(), mode));

        if intent.requires_confirmation {
            rows.push((
                "Trust".into(),
                "this revision has not been reviewed; running it needs a confirmation".into(),
            ));
        }
        if intent.has_secrets() {
            rows.push((
                "Secrets".into(),
                format!("{REDACTED} — supplied for this run only, never recorded"),
            ));
        }
        Self { rows }
    }

    pub fn rows(&self) -> &[(String, String)] {
        &self.rows
    }

    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn working_dir(cwd: WorkingDir, context: &FormContext) -> String {
    match cwd {
        WorkingDir::Project => match context.project_root() {
            Some(root) => root.display().to_string(),
            None => "project (none here)".to_string(),
        },
        WorkingDir::Cwd => context
            .value(DefaultSource::Cwd)
            .unwrap_or("the invoking directory")
            .to_string(),
        WorkingDir::Capsule => "the capsule's payload directory".to_string(),
    }
}
