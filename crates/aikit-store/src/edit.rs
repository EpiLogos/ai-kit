//! Format-preserving edits to the files people write by hand.
//!
//! `<repo>/.aikit/profile.toml` is committed, reviewed and argued about. If
//! AIKit round-tripped it through a serializer, one toggle would reorder the
//! keys, unwrap the hand-formatted arrays and delete the comment explaining why
//! the regression hook is off. The diff would be unreviewable, and the team would
//! quite rightly stop letting AIKit near the file.
//!
//! So every edit here is surgical: `toml_edit` keeps the original tokens and this
//! module changes only the array entries it must. The new entry even inherits the
//! whitespace style of its neighbours, so a multi-line array stays multi-line.
//!
//! Two smaller promises fall out of the same principle:
//!
//! * A file that does not parse is **left exactly as it was** and reported as
//!   `edit.parse_error`. Overwriting something we could not understand is how a
//!   tool destroys work.
//! * A save is a write-then-rename, so an interrupted save cannot truncate a
//!   profile that took a team a month to agree on.

use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Value};

use aikit_core::error::err;
use aikit_core::profile::{
    PoolPatch, ProjectProfileFile, SessionOverlayFile, SkillUsageOverlayPatch,
};
use aikit_core::{AikitError, CapsuleId, GenerationId, ProfileId, Result, SessionId};

use crate::home::{create_dir_all, io_error};

const ENABLE: &str = "enable";
const DISABLE: &str = "disable";
const PROFILES: &str = "profiles";
const CONFIG: &str = "config";
const SKILL_OVERLAYS: &str = "skill-overlays";

// ---------------------------------------------------------------------------
// Profile documents
// ---------------------------------------------------------------------------

/// A `profile.toml`, `profile.local.toml` or registry profile, open for editing.
#[derive(Debug, Clone)]
pub struct ProfileDocument {
    doc: DocumentMut,
    path: Option<PathBuf>,
}

impl ProfileDocument {
    /// Parse text without associating it with a file.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self {
            doc: parse_document(text, None)?,
            path: None,
        })
    }

    /// Open a file, or start a minimal one if it does not exist yet.
    pub fn open(path: &Path) -> Result<Self> {
        let doc = match std::fs::read_to_string(path) {
            Ok(text) => parse_document(&text, Some(path))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                "schema = 1\n".parse::<DocumentMut>().map_err(|e| {
                    AikitError::new("edit.parse_error", format!("internal template: {e}"))
                })?
            }
            Err(e) => return Err(io_error("edit.unreadable", path, &e)),
        };
        Ok(Self {
            doc,
            path: Some(path.to_path_buf()),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Declare a capsule enabled here, removing any contrary declaration.
    pub fn enable(&mut self, id: &CapsuleId) {
        self.remove_from(DISABLE, id);
        self.add_to(ENABLE, &id.to_string());
    }

    /// Declare a capsule disabled here, removing any contrary declaration.
    pub fn disable(&mut self, id: &CapsuleId) {
        self.remove_from(ENABLE, id);
        self.add_to(DISABLE, &id.to_string());
    }

    /// Remove every declaration for a capsule, letting lower scopes decide again.
    pub fn clear(&mut self, id: &CapsuleId) {
        self.remove_from(ENABLE, id);
        self.remove_from(DISABLE, id);
        if let Some(config) = self.doc.get_mut(CONFIG).and_then(Item::as_table_like_mut) {
            config.remove(&id.to_string());
        }
    }

    /// Reference a profile from this scope.
    pub fn use_profile(&mut self, id: &ProfileId) {
        self.add_to(PROFILES, &id.to_string());
    }

    pub fn drop_profile(&mut self, id: &ProfileId) {
        let rendered = id.to_string();
        if let Some(array) = self.doc.get_mut(PROFILES).and_then(Item::as_array_mut) {
            array.retain(|v| v.as_str() != Some(rendered.as_str()));
        }
    }

    /// Set one key of a capsule's `[config."<id>"]` table.
    pub fn set_config(&mut self, id: &CapsuleId, key: &str, value: Item) {
        if self.doc.get(CONFIG).is_none() {
            let mut table = toml_edit::Table::new();
            // Implicit, so it renders as `[config."script/x"]` rather than
            // emitting a bare `[config]` header the author never wrote.
            table.set_implicit(true);
            self.doc[CONFIG] = Item::Table(table);
        }
        self.doc[CONFIG][&id.to_string()][key] = value;
    }

    /// Replace this scope's additive guidance for one skill while leaving every
    /// unrelated token and comment in the hand-written profile untouched.
    pub fn set_skill_overlay(&mut self, id: &CapsuleId, overlay: &SkillUsageOverlayPatch) {
        if self.doc.get(SKILL_OVERLAYS).is_none() {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            self.doc[SKILL_OVERLAYS] = Item::Table(table);
        }
        let rendered = id.to_string();
        if let Some(overlays) = self
            .doc
            .get_mut(SKILL_OVERLAYS)
            .and_then(Item::as_table_like_mut)
        {
            overlays.remove(&rendered);
        }
        self.doc[SKILL_OVERLAYS][&rendered]["inherit"] = toml_edit::value(overlay.inherit);
        if let Some(description) = &overlay.description {
            self.doc[SKILL_OVERLAYS][&rendered]["description"] =
                toml_edit::value(description.clone());
        }
        if let Some(guidance) = &overlay.guidance {
            self.doc[SKILL_OVERLAYS][&rendered]["guidance"] =
                toml_edit::value(guidance.clone());
        }
        if let Some(revision) = &overlay.reviewed_against {
            self.doc[SKILL_OVERLAYS][&rendered]["reviewed_against"] =
                toml_edit::value(revision.as_str());
        }
    }

    pub fn clear_skill_overlay(&mut self, id: &CapsuleId) {
        let rendered = id.to_string();
        let empty = self
            .doc
            .get_mut(SKILL_OVERLAYS)
            .and_then(Item::as_table_like_mut)
            .map(|overlays| {
                overlays.remove(&rendered);
                overlays.is_empty()
            })
            .unwrap_or(false);
        if empty {
            self.doc.remove(SKILL_OVERLAYS);
        }
    }

    /// The declarations as the resolver would read them.
    ///
    /// Deliberately goes back through the *rendered text*: that proves the edited
    /// document is still a document the rest of AIKit can parse, rather than
    /// reporting what this module intended to write.
    pub fn patch(&self) -> Result<PoolPatch> {
        let text = self.to_string();
        let file: ProjectProfileFile = toml::from_str(&text).map_err(|e| {
            AikitError::new(
                "edit.parse_error",
                format!("the edited profile is no longer valid: {e}"),
            )
        })?;
        file.patch.validate()?;
        Ok(file.patch)
    }

    /// Write the document back, replacing the file atomically.
    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return err(
                "edit.no_path",
                "this profile document was parsed from text and has no file to save to",
            );
        };
        write_atomically(path, self.to_string().as_bytes())
    }

    fn add_to(&mut self, field: &str, rendered: &str) {
        if self
            .doc
            .get(field)
            .and_then(Item::as_array)
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(rendered)))
        {
            return; // Already declared: leave the file byte-identical.
        }
        if self.doc.get(field).and_then(Item::as_array).is_none() {
            self.doc[field] = Item::Value(Value::Array(Array::new()));
        }
        if let Some(array) = self.doc.get_mut(field).and_then(Item::as_array_mut) {
            push_in_style(array, rendered);
        }
    }

    fn remove_from(&mut self, field: &str, id: &CapsuleId) {
        let rendered = id.to_string();
        if let Some(array) = self.doc.get_mut(field).and_then(Item::as_array_mut) {
            array.retain(|v| v.as_str() != Some(rendered.as_str()));
        }
    }
}

impl std::fmt::Display for ProfileDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.doc.to_string())
    }
}

// ---------------------------------------------------------------------------
// Session overlays
// ---------------------------------------------------------------------------

/// `~/.aikit/state/sessions/<session-id>/overlay.toml`.
///
/// The same editing discipline as a profile, plus one invariant: the file names
/// the session it belongs to, and opening it for a different session is refused.
/// Two sessions sharing an overlay would be precisely the global mutable active
/// set the architecture exists to avoid.
#[derive(Debug, Clone)]
pub struct OverlayDocument {
    inner: ProfileDocument,
    session: SessionId,
}

impl OverlayDocument {
    pub fn open(path: &Path, session: &SessionId) -> Result<Self> {
        let mut inner = ProfileDocument::open(path)?;
        match inner.doc.get("session_id").and_then(Item::as_str) {
            Some(existing) if existing != session.as_str() => {
                return Err(AikitError::new(
                    "edit.session_mismatch",
                    format!(
                        "{} belongs to session {existing}, not {session}",
                        path.display()
                    ),
                )
                .with("path", path.display().to_string())
                .with("expected", session.to_string())
                .with("found", existing.to_string()));
            }
            Some(_) => {}
            None => {
                inner.doc["schema"] = toml_edit::value(1);
                inner.doc["session_id"] = toml_edit::value(session.as_str());
            }
        }
        inner.path = Some(path.to_path_buf());
        Ok(Self {
            inner,
            session: session.clone(),
        })
    }

    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The generation this overlay was last resolved against.
    ///
    /// This is the compare-and-swap token: an apply that presents a stale base is
    /// refused rather than allowed to discard another pane's change.
    pub fn set_base_generation(&mut self, generation: Option<&GenerationId>) {
        match generation {
            Some(id) => self.inner.doc["base_generation"] = toml_edit::value(id.as_str()),
            None => {
                self.inner.doc.remove("base_generation");
            }
        }
    }

    pub fn base_generation(&self) -> Option<GenerationId> {
        self.inner
            .doc
            .get("base_generation")
            .and_then(Item::as_str)
            .and_then(|raw| GenerationId::parse(raw).ok())
    }

    pub fn enable(&mut self, id: &CapsuleId) {
        self.inner.enable(id);
    }

    pub fn disable(&mut self, id: &CapsuleId) {
        self.inner.disable(id);
    }

    pub fn clear(&mut self, id: &CapsuleId) {
        self.inner.clear(id);
    }

    pub fn use_profile(&mut self, id: &ProfileId) {
        self.inner.use_profile(id);
    }

    pub fn set_config(&mut self, id: &CapsuleId, key: &str, value: Item) {
        self.inner.set_config(id, key, value);
    }

    pub fn set_skill_overlay(&mut self, id: &CapsuleId, overlay: &SkillUsageOverlayPatch) {
        self.inner.set_skill_overlay(id, overlay);
    }

    pub fn clear_skill_overlay(&mut self, id: &CapsuleId) {
        self.inner.clear_skill_overlay(id);
    }

    /// The declarations as the resolver would read them.
    ///
    /// Parsed back as a [`SessionOverlayFile`], **not** through the project-profile
    /// parser [`ProfileDocument::patch`] uses: an overlay carries `session_id` and
    /// `base_generation` that a project profile does not, and the strict
    /// project-profile schema rejects those very fields. Going through the rendered
    /// text still proves the edited overlay is one the rest of AIKit can read back.
    pub fn patch(&self) -> Result<PoolPatch> {
        let text = self.inner.to_string();
        let file: SessionOverlayFile = toml::from_str(&text).map_err(|e| {
            AikitError::new(
                "edit.parse_error",
                format!("the edited session overlay is no longer valid: {e}"),
            )
        })?;
        file.patch.validate()?;
        Ok(file.patch)
    }

    pub fn save(&self) -> Result<()> {
        self.inner.save()
    }
}

impl std::fmt::Display for OverlayDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.inner, f)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_document(text: &str, path: Option<&Path>) -> Result<DocumentMut> {
    text.parse::<DocumentMut>().map_err(|e| {
        let where_ = path
            .map(|p| format!("{}: ", p.display()))
            .unwrap_or_default();
        let mut error = AikitError::new("edit.parse_error", format!("{where_}{e}"));
        if let Some(p) = path {
            error = error.with("path", p.display().to_string());
        }
        error
    })
}

/// Append to an array while keeping the author's layout.
///
/// A one-line array stays on one line; a hand-wrapped array gets the new entry on
/// its own line with the same indent and a trailing comma, so the diff is a
/// single added line rather than a reflow of the whole value.
fn push_in_style(array: &mut Array, rendered: &str) {
    let existing_prefix = array
        .iter()
        .find_map(|v| v.decor().prefix().and_then(|p| p.as_str()))
        .filter(|p| p.contains('\n'))
        .map(ToString::to_string);

    array.push(rendered);
    let last = array.len().saturating_sub(1);

    if let (Some(prefix), Some(value)) = (existing_prefix, array.get_mut(last)) {
        value.decor_mut().set_prefix(prefix);
        value.decor_mut().set_suffix("");
        array.set_trailing_comma(true);
    }
}

/// Write via a sibling temporary file and a rename.
///
/// The temporary lives beside the target so the rename stays within one
/// filesystem — a `/tmp` staging file would make it a copy, and a copy is not
/// atomic.
fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.aikit-tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("toml")
    ));
    std::fs::write(&temporary, contents).map_err(|e| io_error("edit.write_failed", &temporary, &e))?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&temporary);
            Err(io_error("edit.write_failed", path, &e))
        }
    }
}
