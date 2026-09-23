//! Foreign harness cron reconciliation readers (parent §2, addendum A-6).
//!
//! Two live harness crons exist on this machine: OpenClaw's (`~/.openclaw/
//! cron/jobs.json`) and Hermes' (`~/.hermes/cron/jobs.json`). These readers
//! are **read-only, always**: AIKit never writes another product's store —
//! retiring a harness timer is an explicit owner act in the harness, and the
//! reconciliation surface says so in plain words. The shapes below are pinned
//! from the live stores on 2026-09-23 (see `routine_import.rs` fixtures).

use std::path::{Path, PathBuf};

use serde_json::Value;

use aikit_core::schedule::ScheduleShape;
use aikit_core::{AikitError, Result};

/// The supported foreign providers and their on-disk stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignProvider {
    OpenClawCron,
    HermesCron,
}

impl ForeignProvider {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "openclaw-cron" => Ok(Self::OpenClawCron),
            "hermes-cron" => Ok(Self::HermesCron),
            other => Err(AikitError::new(
                "routine.import_unknown_provider",
                format!(
                    "unknown foreign provider `{other}`; supported providers are openclaw-cron \
                     and hermes-cron"
                ),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenClawCron => "openclaw-cron",
            Self::HermesCron => "hermes-cron",
        }
    }

    /// The provider ref a reconciled Routine's scheduler binding carries.
    pub fn provider_ref(&self) -> String {
        format!("provider:{}", self.as_str())
    }

    /// The store this provider owns, resolved under the given home directory.
    pub fn store_path(&self, home_dir: &Path) -> PathBuf {
        match self {
            Self::OpenClawCron => home_dir.join(".openclaw/cron/jobs.json"),
            Self::HermesCron => home_dir.join(".hermes/cron/jobs.json"),
        }
    }

    /// The plain-word way to retire a timer in the harness itself.
    pub fn retirement_hint(&self, job_id: &str) -> String {
        match self {
            Self::OpenClawCron => {
                format!("`openclaw cron disable {job_id}` (or `openclaw cron rm {job_id}`)")
            }
            Self::HermesCron => format!("`hermes cron pause {job_id}` (or the Hermes UI)"),
        }
    }
}

/// One foreign harness job, normalised for reconciliation. The original JSON
/// rides along so a report can show the payload honestly.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignJob {
    pub provider: ForeignProvider,
    pub job_id: String,
    pub name: String,
    /// Whether the harness will actually fire this job.
    pub active: bool,
    /// True when the job runs a script with no agent — there is no Method in
    /// it to reconcile.
    pub script_only: bool,
    /// The job's own instruction text (system event text / prompt), used for
    /// Method matching and shown in reports.
    pub payload_text: String,
    /// The time-shape the harness scheduled, when it maps onto the shared
    /// grammar. `None` shapes are reported as unreconcilable timing.
    pub schedule: Option<ScheduleShape>,
    pub raw: Value,
}

impl ForeignJob {
    /// Why this job cannot be imported, when it cannot. `Ok(())` means the
    /// import gate is open: a Method match and a proven basis may reconcile it.
    pub fn refusal_reason(&self) -> Result<()> {
        if self.script_only {
            return Err(AikitError::new(
                "routine.import_script_job_has_no_method",
                format!(
                    "job {} runs a script with no agent, so there is no Method in it to prove \
                     and reconcile",
                    self.job_id
                ),
            ));
        }
        if self.schedule.is_none() {
            return Err(AikitError::new(
                "routine.import_unrepresentable_schedule",
                format!(
                    "job {} carries a schedule shape this import cannot read; reconcile it by \
                     hand if its timing matters",
                    self.job_id
                ),
            ));
        }
        Ok(())
    }
}

/// Read one job from the provider's store, read-only.
pub fn read_job(provider: ForeignProvider, job_id: &str, home_dir: &Path) -> Result<ForeignJob> {
    let jobs = read_store(provider, home_dir)?;
    jobs.into_iter()
        .find(|job| job.job_id == job_id)
        .ok_or_else(|| {
            AikitError::new(
                "routine.import_job_not_found",
                format!(
                    "no job {job_id} exists in the {} store at {}; nothing was read or changed",
                    provider.as_str(),
                    provider.store_path(home_dir).display()
                ),
            )
        })
}

/// Read every job in the provider's store, read-only. A missing store is an
/// empty list, not an error: "no foreign timers" is a normal answer.
pub fn read_store(provider: ForeignProvider, home_dir: &Path) -> Result<Vec<ForeignJob>> {
    let path = provider.store_path(home_dir);
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(&path).map_err(|error| {
        AikitError::new(
            "routine.import_store_read_failed",
            format!("{}: {error}", path.display()),
        )
    })?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|error| {
        AikitError::new(
            "routine.import_store_invalid",
            format!("{}: {error}", path.display()),
        )
    })?;
    let entries = document
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut jobs = Vec::new();
    for entry in entries {
        if let Some(job) = normalise(provider, &entry) {
            jobs.push(job);
        }
    }
    Ok(jobs)
}

fn normalise(provider: ForeignProvider, entry: &Value) -> Option<ForeignJob> {
    let job_id = string_field(entry, &["id"])?;
    let name = string_field(entry, &["name"]).unwrap_or_default();
    let (script_only, payload_text) = match provider {
        ForeignProvider::OpenClawCron => {
            let kind = entry
                .pointer("/payload/kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text = string_field_pointer(entry, "/payload/text").unwrap_or_default();
            (kind == "script", text)
        }
        ForeignProvider::HermesCron => {
            let no_agent = entry
                .get("no_agent")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let has_script = string_field(entry, &["script"]).is_some();
            let text = string_field(entry, &["prompt"]).unwrap_or_default();
            (no_agent || has_script, text)
        }
    };
    let active = match provider {
        ForeignProvider::OpenClawCron => entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ForeignProvider::HermesCron => {
            let enabled = entry
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let paused = entry
                .get("state")
                .and_then(Value::as_str)
                .is_some_and(|state| state == "paused");
            enabled && !paused
        }
    };
    let schedule = read_schedule(provider, entry);
    Some(ForeignJob {
        provider,
        job_id,
        name,
        active,
        script_only,
        payload_text,
        schedule,
        raw: entry.clone(),
    })
}

fn read_schedule(provider: ForeignProvider, entry: &Value) -> Option<ScheduleShape> {
    match provider {
        ForeignProvider::OpenClawCron => {
            let kind = entry
                .pointer("/schedule/kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match kind {
                "every" => {
                    let interval = entry.pointer("/schedule/everyMs").and_then(Value::as_u64)?;
                    Some(ScheduleShape::Every {
                        interval_ms: interval,
                    })
                }
                "cron" => {
                    let expression = entry
                        .pointer("/schedule/cron")
                        .or_else(|| entry.pointer("/schedule/expression"))
                        .and_then(Value::as_str)?;
                    Some(ScheduleShape::Cron {
                        expression: expression.to_owned(),
                    })
                }
                "at" => once_from(entry.pointer("/schedule/at")),
                _ => None,
            }
        }
        ForeignProvider::HermesCron => {
            let kind = entry
                .pointer("/schedule/kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match kind {
                "cron" => {
                    let expression = entry.pointer("/schedule/expr").and_then(Value::as_str)?;
                    Some(ScheduleShape::Cron {
                        expression: expression.to_owned(),
                    })
                }
                "relative" | "every" | "interval" => {
                    relative_to_every(entry.pointer("/schedule/display").and_then(Value::as_str))
                        .or(relative_to_every(
                            entry.pointer("/schedule/expr").and_then(Value::as_str),
                        ))
                }
                "once" | "at" => once_from(entry.pointer("/schedule/at")),
                _ => None,
            }
        }
    }
}

/// Hermes relative schedules ("30m", "every 2h", "90s") map onto `every`.
/// Anything else is refused honestly rather than guessed.
fn relative_to_every(raw: Option<&str>) -> Option<ScheduleShape> {
    let raw = raw?;
    let trimmed = raw.trim().trim_start_matches("every ").trim();
    let (number, unit) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len()),
    );
    let amount: u64 = number.parse().ok()?;
    let interval_ms = match unit {
        "s" => amount.checked_mul(1_000)?,
        "m" => amount.checked_mul(60_000)?,
        "h" => amount.checked_mul(3_600_000)?,
        "d" => amount.checked_mul(86_400_000)?,
        _ => return None,
    };
    if interval_ms == 0 {
        return None;
    }
    Some(ScheduleShape::Every { interval_ms })
}

fn once_from(value: Option<&Value>) -> Option<ScheduleShape> {
    let value = value?;
    if let Some(ms) = value.as_i64() {
        return Some(ScheduleShape::Once {
            due_unix_ms: Some(ms),
            rfc3339: None,
        });
    }
    let text = value.as_str()?;
    Some(ScheduleShape::Once {
        due_unix_ms: None,
        rfc3339: Some(text.to_owned()),
    })
}

fn string_field(entry: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| entry.get(name).and_then(Value::as_str))
        .map(str::to_owned)
}

fn string_field_pointer(entry: &Value, pointer: &str) -> Option<String> {
    entry
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live OpenClaw store shape, pinned 2026-09-23 (tmux watcher job).
    const OPENCLAW_FIXTURE: &str = r#"{
      "version": 1,
      "jobs": [
        {
          "id": "9efb7069-a72b-4ccc-8b4e-4e9134c58b57",
          "agentId": "main",
          "name": "tmux-completion-watcher",
          "enabled": true,
          "schedule": { "kind": "every", "everyMs": 120000 },
          "sessionTarget": "main",
          "wakeMode": "next-heartbeat",
          "payload": {
            "kind": "systemEvent",
            "text": "Tmux completion check: notify on new completions."
          },
          "state": { "lastStatus": "ok" }
        }
      ]
    }"#;

    /// The live Hermes store shape, pinned 2026-09-23 (Nara 06:00 job, the
    /// script-only Sunday archive job is included to pin the refusal path).
    const HERMES_FIXTURE: &str = r#"{
      "jobs": [
        {
          "id": "2e45cb4eef8a",
          "name": "Nara Daily Flow Compose (06:00)",
          "prompt": "You are Hermes-Nara composing the daily Nara flow document.",
          "script": null,
          "no_agent": false,
          "schedule": { "kind": "cron", "expr": "0 6 * * *", "display": "0 6 * * *" },
          "enabled": true,
          "state": "active"
        },
        {
          "id": "22d0554dfc4c",
          "name": "Nara Weekly Flow Archive",
          "prompt": "",
          "script": "archive-weekly-flows.py",
          "no_agent": true,
          "schedule": { "kind": "cron", "expr": "0 7 * * 0", "display": "0 7 * * 0" },
          "enabled": true,
          "state": "active"
        }
      ]
    }"#;

    fn store(dir: &Path, relative: &str, body: &str) {
        std::fs::create_dir_all(dir.join(relative).parent().unwrap()).unwrap();
        std::fs::write(dir.join(relative), body).unwrap();
    }

    #[test]
    fn openclaw_store_reads_in_live_shape() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), ".openclaw/cron/jobs.json", OPENCLAW_FIXTURE);
        let jobs = read_store(ForeignProvider::OpenClawCron, dir.path()).unwrap();
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.job_id, "9efb7069-a72b-4ccc-8b4e-4e9134c58b57");
        assert!(job.active);
        assert_eq!(
            job.schedule,
            Some(ScheduleShape::Every {
                interval_ms: 120_000
            })
        );
        assert!(job.payload_text.contains("Tmux completion check"));
        assert!(!job.script_only);
    }

    #[test]
    fn hermes_store_reads_in_live_shape_and_flags_script_jobs() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), ".hermes/cron/jobs.json", HERMES_FIXTURE);
        let jobs = read_store(ForeignProvider::HermesCron, dir.path()).unwrap();
        assert_eq!(jobs.len(), 2);
        let nara = jobs
            .iter()
            .find(|job| job.job_id == "2e45cb4eef8a")
            .unwrap();
        assert_eq!(
            nara.schedule,
            Some(ScheduleShape::Cron {
                expression: "0 6 * * *".into()
            })
        );
        assert!(!nara.script_only);
        assert_eq!(nara.refusal_reason(), Ok(()));
        let archive = jobs
            .iter()
            .find(|job| job.job_id == "22d0554dfc4c")
            .unwrap();
        assert!(archive.script_only);
        assert_eq!(
            archive.refusal_reason().unwrap_err().code(),
            "routine.import_script_job_has_no_method"
        );
    }

    #[test]
    fn missing_store_is_empty_and_unknown_job_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_store(ForeignProvider::OpenClawCron, dir.path())
            .unwrap()
            .is_empty());
        let error = read_job(ForeignProvider::HermesCron, "nope", dir.path()).unwrap_err();
        assert_eq!(error.code(), "routine.import_job_not_found");
    }

    #[test]
    fn hermes_relative_schedules_map_onto_every() {
        assert_eq!(
            relative_to_every(Some("30m")),
            Some(ScheduleShape::Every {
                interval_ms: 1_800_000
            })
        );
        assert_eq!(
            relative_to_every(Some("every 2h")),
            Some(ScheduleShape::Every {
                interval_ms: 7_200_000
            })
        );
        assert_eq!(relative_to_every(Some("weekly")), None);
    }

    #[test]
    fn provider_parse_and_hints_are_plain_words() {
        assert_eq!(
            ForeignProvider::parse("openclaw-cron").unwrap(),
            ForeignProvider::OpenClawCron
        );
        assert_eq!(
            ForeignProvider::parse("cron").unwrap_err().code(),
            "routine.import_unknown_provider"
        );
        assert!(ForeignProvider::HermesCron
            .retirement_hint("abc123")
            .contains("hermes cron pause abc123"));
    }
}
