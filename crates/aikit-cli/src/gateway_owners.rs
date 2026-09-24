//! The owner reads and acts gateway contact joins, behind one injectable seam.
//!
//! Gateway contact answers "who is here, who holds that address, and what
//! does that occupant carry" without owning any of it: Central defines
//! Positions (`central.position.list|read`, `central.world.here`), Actuation
//! holds occupancy and tenure (`actuation occupancy list|read|verify`), Factory
//! holds custody and current work (`factory development current-work|custody
//! assign`). This module is the only place those owners are spoken to, through
//! their real CLIs, so tests can put fixture owners behind the same seam and
//! production can never grow a private copy of their state.
//!
//! The invariant it owns: an owner that cannot answer is reported as
//! `unavailable` with the exact command that failed and why — never guessed,
//! never papered over with a default, never a crash. A missing verb (an owner
//! not yet upgraded) is the same honest `unavailable`.

use std::path::{Path, PathBuf};

use aikit_adapters::runner::{CommandRunner, SystemRunner};
use serde_json::Value;

/// One owner that could not answer: the command that was run and why it
/// failed, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerUnavailable {
    pub command: String,
    pub reason: String,
}

impl std::fmt::Display for OwnerUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` failed: {}", self.command, self.reason)
    }
}

/// An owner refusal that carries its own three-part explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRefusal {
    pub command: String,
    pub code: String,
    pub fact: String,
    pub consequence: String,
    pub action: String,
}

/// Actuation's answer to `occupancy verify`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OccupancyVerdict {
    /// The generation is the Position's current occupant; carries the tenure.
    Current(Value),
    /// Actuation refused (superseded, vacant, unknown generation, ...).
    Refused(OwnerRefusal),
}

/// Central's answer to `central.position.read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionLookup {
    Found(Value),
    NotFound(OwnerRefusal),
}

/// The owner seam. Every method is one owner verb.
pub trait ContactOwners {
    /// `central.position.list {project?}` — the listing document.
    fn position_list(&self, project: Option<&str>) -> Result<Value, OwnerUnavailable>;
    /// `central.position.read {position_ref}`.
    fn position_read(&self, position_ref: &str) -> Result<PositionLookup, OwnerUnavailable>;
    /// `central.world.here {cwd?}`.
    fn world_here(&self, cwd: &Path) -> Result<Value, OwnerUnavailable>;
    /// `actuation occupancy list`.
    fn occupancy_list(&self) -> Result<Value, OwnerUnavailable>;
    /// `actuation occupancy read --position P`.
    fn occupancy_read(&self, position_ref: &str) -> Result<Value, OwnerUnavailable>;
    /// `actuation occupancy verify --position P --generation G`.
    fn occupancy_verify(
        &self,
        position_ref: &str,
        generation_ref: &str,
    ) -> Result<OccupancyVerdict, OwnerUnavailable>;
    /// `factory development current-work --position P`, run from `cwd`.
    fn current_work(&self, position_ref: &str, cwd: &Path) -> Result<Value, OwnerUnavailable>;
    /// `factory development custody assign ...` — the explicit crossing.
    fn custody_assign(
        &self,
        request: &CustodyAssign,
        cwd: &Path,
    ) -> Result<Result<Value, OwnerRefusal>, OwnerUnavailable>;
}

/// The arguments of one custody assignment, as Factory names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodyAssign {
    pub position_ref: String,
    pub work_ref: String,
    pub reason: String,
    pub run_ref: Option<String>,
    pub journey_ref: Option<String>,
    pub workflow_unit_ref: Option<String>,
    pub origin_communique_ref: String,
}

/// Production owners: the real `ctrl`, `actuation` and `factory` binaries.
/// The executables follow the suite's existing overrides (`CENTRAL_CTRL_BIN`,
/// `ACTUATION_BIN`, `FACTORY_BIN`, with their `OI_*` forms), and reads run
/// under the shared probe budget so a hanging owner costs time, not the turn.
#[derive(Debug, Clone)]
pub struct ProcessOwners {
    pub ctrl: String,
    pub actuation: String,
    pub factory: String,
    pub central_root: Option<PathBuf>,
}

fn env_bin(names: &[&str], default: &str) -> String {
    names
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| default.to_owned())
}

impl ProcessOwners {
    pub fn from_env() -> Self {
        Self {
            ctrl: env_bin(&["CENTRAL_CTRL_BIN", "OI_CENTRAL_CTRL_BIN"], "ctrl"),
            actuation: env_bin(&["ACTUATION_BIN", "OI_ACTUATION_BIN"], "actuation"),
            factory: env_bin(
                &["FACTORY_BIN", "OI_FACTORY_BIN", "AIKIT_FACTORY_BIN"],
                "factory",
            ),
            central_root: std::env::var_os("AIKIT_CENTRAL_ROOT").map(PathBuf::from),
        }
    }

    fn ctrl_action(&self, action: &str, input: &Value) -> Result<Value, CtrlFailure> {
        let mut argv = vec![self.ctrl.clone(), "--json".to_owned()];
        if let Some(root) = &self.central_root {
            argv.push("--root".to_owned());
            argv.push(root.display().to_string());
        }
        argv.extend([
            "action".to_owned(),
            "run".to_owned(),
            action.to_owned(),
            input.to_string(),
        ]);
        let command = argv.join(" ");
        let output = SystemRunner::probe()
            .run(&argv)
            .map_err(|error| CtrlFailure::Unavailable(unavailable(&command, error.to_string())))?;
        let envelope: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
            CtrlFailure::Unavailable(unavailable(
                &command,
                format!(
                    "exit {} with no JSON envelope{}",
                    output.status,
                    stderr_tail(&output.stderr)
                ),
            ))
        })?;
        if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(envelope.get("data").cloned().unwrap_or(Value::Null));
        }
        Err(CtrlFailure::Refused { command, envelope })
    }

    fn json_process(
        &self,
        argv: Vec<String>,
        cwd: Option<&Path>,
    ) -> Result<(i32, Value), OwnerUnavailable> {
        let command = argv.join(" ");
        let mut runner = SystemRunner::probe();
        if let Some(cwd) = cwd {
            runner = runner.with_cwd(cwd);
        }
        let output = runner
            .run(&argv)
            .map_err(|error| unavailable(&command, error.to_string()))?;
        let value: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
            unavailable(
                &command,
                format!(
                    "exit {} with no JSON answer{}",
                    output.status,
                    stderr_tail(&output.stderr)
                ),
            )
        })?;
        Ok((output.status, value))
    }
}

enum CtrlFailure {
    Unavailable(OwnerUnavailable),
    Refused { command: String, envelope: Value },
}

fn unavailable(command: &str, reason: impl Into<String>) -> OwnerUnavailable {
    OwnerUnavailable {
        command: command.to_owned(),
        reason: reason.into(),
    }
}

fn stderr_tail(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        let tail: String = trimmed
            .lines()
            .last()
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect();
        format!(": {tail}")
    }
}

fn ctrl_refusal_reason(envelope: &Value) -> String {
    envelope
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("the action refused without a message")
        .to_owned()
}

/// Read a three-part refusal the way the suite owners print one: either
/// `{error:{code,fact,consequence,action}}` or a bare refusal document.
fn three_part(command: &str, value: &Value) -> OwnerRefusal {
    let error = value.get("error").unwrap_or(value);
    let text = |key: &str| {
        error
            .get(key)
            .or_else(|| error.pointer(&format!("/details/{key}")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    OwnerRefusal {
        command: command.to_owned(),
        code: {
            let code = text("code");
            if code.is_empty() {
                "owner.refused".into()
            } else {
                code
            }
        },
        fact: {
            let fact = text("fact");
            if fact.is_empty() {
                text("message")
            } else {
                fact
            }
        },
        consequence: text("consequence"),
        action: text("action"),
    }
}

impl ContactOwners for ProcessOwners {
    fn position_list(&self, project: Option<&str>) -> Result<Value, OwnerUnavailable> {
        let input = match project {
            Some(project) => serde_json::json!({ "project": project }),
            None => serde_json::json!({}),
        };
        self.ctrl_action("central.position.list", &input)
            .map_err(|failure| match failure {
                CtrlFailure::Unavailable(unavailable) => unavailable,
                CtrlFailure::Refused { command, envelope } => {
                    unavailable(&command, ctrl_refusal_reason(&envelope))
                }
            })
    }

    fn position_read(&self, position_ref: &str) -> Result<PositionLookup, OwnerUnavailable> {
        match self.ctrl_action(
            "central.position.read",
            &serde_json::json!({ "position_ref": position_ref }),
        ) {
            Ok(value) => Ok(PositionLookup::Found(value)),
            Err(CtrlFailure::Unavailable(unavailable)) => Err(unavailable),
            Err(CtrlFailure::Refused { command, envelope }) => {
                let code = envelope
                    .pointer("/error/code")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if code == "central.position_not_found" {
                    let data = envelope.get("data").cloned().unwrap_or(Value::Null);
                    let mut refusal = three_part(&command, &data);
                    refusal.code = code.to_owned();
                    if refusal.fact.is_empty() {
                        refusal.fact = ctrl_refusal_reason(&envelope);
                    }
                    Ok(PositionLookup::NotFound(refusal))
                } else {
                    Err(unavailable(&command, ctrl_refusal_reason(&envelope)))
                }
            }
        }
    }

    fn world_here(&self, cwd: &Path) -> Result<Value, OwnerUnavailable> {
        self.ctrl_action(
            "central.world.here",
            &serde_json::json!({ "cwd": cwd.display().to_string() }),
        )
        .map_err(|failure| match failure {
            CtrlFailure::Unavailable(unavailable) => unavailable,
            CtrlFailure::Refused { command, envelope } => {
                unavailable(&command, ctrl_refusal_reason(&envelope))
            }
        })
    }

    fn occupancy_list(&self) -> Result<Value, OwnerUnavailable> {
        let argv = vec![
            self.actuation.clone(),
            "occupancy".into(),
            "list".into(),
            "--json".into(),
        ];
        let command = argv.join(" ");
        let (status, value) = self.json_process(argv, None)?;
        if status != 0 {
            return Err(unavailable(&command, three_part(&command, &value).fact));
        }
        Ok(value)
    }

    fn occupancy_read(&self, position_ref: &str) -> Result<Value, OwnerUnavailable> {
        let argv = vec![
            self.actuation.clone(),
            "occupancy".into(),
            "read".into(),
            "--position".into(),
            position_ref.into(),
            "--json".into(),
        ];
        let command = argv.join(" ");
        let (status, value) = self.json_process(argv, None)?;
        if status != 0 {
            return Err(unavailable(&command, three_part(&command, &value).fact));
        }
        Ok(value)
    }

    fn occupancy_verify(
        &self,
        position_ref: &str,
        generation_ref: &str,
    ) -> Result<OccupancyVerdict, OwnerUnavailable> {
        let argv = vec![
            self.actuation.clone(),
            "occupancy".into(),
            "verify".into(),
            "--position".into(),
            position_ref.into(),
            "--generation".into(),
            generation_ref.into(),
            "--json".into(),
        ];
        let command = argv.join(" ");
        let (status, value) = self.json_process(argv, None)?;
        if status == 0 && value.get("ok").and_then(Value::as_bool) != Some(false) {
            return Ok(OccupancyVerdict::Current(
                value.get("current").cloned().unwrap_or(value),
            ));
        }
        let refusal = three_part(&command, &value);
        // Only an owner-coded occupancy refusal is a verdict; anything else
        // (a missing verb, a usage error) means Actuation could not answer.
        if refusal.code.starts_with("occupancy.") {
            Ok(OccupancyVerdict::Refused(refusal))
        } else {
            Err(unavailable(&command, refusal.fact))
        }
    }

    fn current_work(&self, position_ref: &str, cwd: &Path) -> Result<Value, OwnerUnavailable> {
        let argv = vec![
            self.factory.clone(),
            "development".into(),
            "current-work".into(),
            "--position".into(),
            position_ref.into(),
            "--json".into(),
        ];
        let command = argv.join(" ");
        let (status, value) = self.json_process(argv, Some(cwd))?;
        if status != 0 {
            return Err(unavailable(&command, three_part(&command, &value).fact));
        }
        Ok(value)
    }

    fn custody_assign(
        &self,
        request: &CustodyAssign,
        cwd: &Path,
    ) -> Result<Result<Value, OwnerRefusal>, OwnerUnavailable> {
        let mut argv = vec![
            self.factory.clone(),
            "development".into(),
            "custody".into(),
            "assign".into(),
            "--position".into(),
            request.position_ref.clone(),
            "--work".into(),
            request.work_ref.clone(),
            "--reason".into(),
            request.reason.clone(),
            "--origin-communique".into(),
            request.origin_communique_ref.clone(),
        ];
        for (flag, value) in [
            ("--run", &request.run_ref),
            ("--journey", &request.journey_ref),
            ("--workflow-unit", &request.workflow_unit_ref),
        ] {
            if let Some(value) = value {
                argv.push(flag.into());
                argv.push(value.clone());
            }
        }
        argv.push("--json".into());
        let command = argv.join(" ");
        // An assignment is a write, not a probe: run it unbounded by the probe
        // budget but still as a real child of the owner binary.
        let output = SystemRunner::new()
            .with_cwd(cwd)
            .run(&argv)
            .map_err(|error| unavailable(&command, error.to_string()))?;
        let value: Value = match serde_json::from_str(output.stdout.trim()) {
            Ok(value) => value,
            Err(_) => {
                return Err(unavailable(
                    &command,
                    format!(
                        "exit {} with no JSON answer{}",
                        output.status,
                        stderr_tail(&output.stderr)
                    ),
                ))
            }
        };
        if output.status == 0 {
            return Ok(Ok(value));
        }
        let refusal = three_part(&command, &value);
        if refusal.code.starts_with("factory.") {
            Ok(Err(refusal))
        } else {
            Err(unavailable(&command, refusal.fact))
        }
    }
}

/// The record inside a Central position entry, whichever shape the owner
/// answered (`{record, source}` or the bare record).
pub fn position_record(entry: &Value) -> &Value {
    entry.get("record").unwrap_or(entry)
}

/// The current tenure of an Actuation occupancy reading, when occupied.
pub fn current_tenure(reading: &Value) -> Option<&Value> {
    (reading.get("state").and_then(Value::as_str) == Some("occupied"))
        .then(|| reading.get("current"))
        .flatten()
        .filter(|tenure| !tenure.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_three_part_refusal_is_read_from_either_owner_shape() {
        let nested = json!({"ok": false, "error": {"code": "occupancy.superseded", "fact": "F", "consequence": "C", "action": "A"}});
        let refusal = three_part("actuation occupancy verify", &nested);
        assert_eq!(refusal.code, "occupancy.superseded");
        assert_eq!(
            (
                refusal.fact.as_str(),
                refusal.consequence.as_str(),
                refusal.action.as_str()
            ),
            ("F", "C", "A")
        );
        let bare = json!({"schema": "factory.refusal/v1", "code": "factory.custody.invalid_position", "fact": "bad", "consequence": "none", "action": "fix"});
        assert_eq!(
            three_part("factory", &bare).code,
            "factory.custody.invalid_position"
        );
    }

    #[test]
    fn only_an_occupied_reading_has_a_current_tenure() {
        assert!(current_tenure(&json!({"state": "vacant", "generations": []})).is_none());
        let occupied =
            json!({"state": "occupied", "current": {"generation_ref": "actuation:generation:1"}});
        assert_eq!(
            current_tenure(&occupied).unwrap()["generation_ref"],
            "actuation:generation:1"
        );
    }

    #[test]
    fn a_missing_owner_binary_is_unavailable_with_the_command_named() {
        let owners = ProcessOwners {
            ctrl: "/nonexistent/ctrl".into(),
            actuation: "/nonexistent/actuation".into(),
            factory: "/nonexistent/factory".into(),
            central_root: None,
        };
        let error = owners.occupancy_list().unwrap_err();
        assert!(error
            .command
            .starts_with("/nonexistent/actuation occupancy list"));
        assert!(!error.reason.is_empty());
        let error = owners.position_list(None).unwrap_err();
        assert!(error.command.contains("central.position.list"));
    }
}
