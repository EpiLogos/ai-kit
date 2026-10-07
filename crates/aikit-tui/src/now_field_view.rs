//! The active NOW field on the Workspace Worlds pane: the composed activity
//! reading the containing O:I surface supplies through the environment
//! boundary (`OI_NOW_FIELD`, the exact `central.now.field` document).
//!
//! This surface owns no activity truth. Central owns the NOW plane; Workcell
//! owns the material census; AIKit's Gateway owns conversation status. What
//! this module adds is the reading's presentation: one centre of activity per
//! Workcell root, its children with their lifecycle conditions, the material
//! and Gateway availability exactly as the owners reported them, and the
//! observation freshness of the whole reading.
//!
//! The unavailability law the rest of this crate keeps applies with full
//! force here: a census that could not be taken renders as "unavailable"
//! with its reason — never as an empty field, which would make a
//! disconnected machine look like it had nothing running.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::layout::Glyphs;

pub const NOW_FIELD_SCHEMA: &str = "central.now-field/v1";
/// The environment boundary: the containing O:I surface hands over the exact
/// `ctrl action run central.now.field --json` data document it read. Nothing
/// here invokes Central: this terminal stays standalone-capable, and a
/// missing supply is the ordinary standalone state, not an error.
pub const NOW_FIELD_ENV: &str = "OI_NOW_FIELD";

/// Why a child's row shows the condition it shows. Central's lifecycle is the
/// owner vocabulary; this maps it to the person's question "is this work
/// live, waiting, or done" without inventing states the register does not
/// carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildCondition {
    Live,
    Waiting,
    Returned,
    Closed,
}

impl ChildCondition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Waiting => "waiting",
            Self::Returned => "returned",
            Self::Closed => "closed",
        }
    }

    fn from_lifecycle(lifecycle: &str) -> Option<Self> {
        match lifecycle {
            "active" => Some(Self::Live),
            "quiescent" => Some(Self::Waiting),
            "archived" => Some(Self::Returned),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildNow {
    pub task_ref: String,
    pub purpose: String,
    pub condition: Option<ChildCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkcellField {
    pub workcell_ref: Option<String>,
    pub purpose: Option<String>,
    pub lifecycle: Option<String>,
    pub children: Vec<ChildNow>,
    /// The material block's own availability answer, verbatim in meaning:
    /// `Some(false)` is "the owner said unavailable" (reason carried), and
    /// `None` is "no material block was supplied" — different rows.
    pub material_available: Option<bool>,
    pub material_scope: Option<String>,
    pub material_reason: Option<String>,
    /// `(harness panes, total panes)` when the census was readable.
    pub census_panes: Option<(usize, usize)>,
    pub gateway_available: Option<bool>,
    pub gateway_states: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowFieldReading {
    pub generated_at_unix_seconds: u64,
    /// When this process read the supply, so the pane can show observation
    /// age without probing anything at draw time.
    pub read_at_unix_seconds: u64,
    pub declared_workcell_ref: Option<String>,
    pub time_policy_present: bool,
    pub day_present: bool,
    pub workcells: Vec<WorkcellField>,
}

impl NowFieldReading {
    /// Tolerant parse of the composed document. A non-matching schema yields
    /// `None` — the pane then simply does not render, exactly like the
    /// composed-World supply.
    pub fn parse(value: &serde_json::Value) -> Option<NowFieldReading> {
        if value["schema"].as_str()? != NOW_FIELD_SCHEMA {
            return None;
        }
        let workcells = value["workcells"]
            .as_array()?
            .iter()
            .map(|workcell| {
                let material = &workcell["material"];
                let census = &material["census"];
                let census_panes = census["reading"]["summary"]["panes"].as_u64().map(|total| {
                    (
                        census["reading"]["summary"]["harness"]
                            .as_u64()
                            .unwrap_or(0) as usize,
                        total as usize,
                    )
                });
                let gateway = &workcell["gateway"];
                WorkcellField {
                    workcell_ref: workcell["workcell_ref"].as_str().map(str::to_owned),
                    purpose: workcell["root"]["purpose"].as_str().map(str::to_owned),
                    lifecycle: workcell["root"]["lifecycle"].as_str().map(str::to_owned),
                    children: workcell["children"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .map(|row| ChildNow {
                                    task_ref: row["task_ref"].as_str().unwrap_or("?").to_owned(),
                                    purpose: row["purpose"].as_str().unwrap_or("").to_owned(),
                                    condition: row["lifecycle"]
                                        .as_str()
                                        .and_then(ChildCondition::from_lifecycle),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    material_available: material["available"].as_bool(),
                    material_scope: material["observation_scope"].as_str().map(str::to_owned),
                    material_reason: material
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    census_panes,
                    gateway_available: gateway["available"].as_bool(),
                    gateway_states: gateway["status"]["reading"]["data"]["status"]
                        ["connector_health"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|row| row["state"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect();
        Some(NowFieldReading {
            generated_at_unix_seconds: value["generated_at_unix_seconds"].as_u64()?,
            read_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            declared_workcell_ref: value["machine"]["declared_workcell_ref"]
                .as_str()
                .map(str::to_owned),
            time_policy_present: value["time"]["policy"].is_object(),
            day_present: value["time"]["day"].is_object(),
            workcells,
        })
    }

    /// The one construction-time read of the environment boundary.
    pub fn from_env() -> Option<NowFieldReading> {
        let raw = std::env::var(NOW_FIELD_ENV).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Self::parse(&value)
    }

    /// Observation age in seconds at draw time, from captured facts only.
    pub fn supply_age_seconds(&self, now: u64) -> u64 {
        now.saturating_sub(self.generated_at_unix_seconds)
    }
}

fn age_line(seconds: u64, glyphs: Glyphs) -> String {
    let word = match seconds {
        0..=90 => "moments".to_string(),
        91..=3_600 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3_600),
    };
    format!("supplied {} ago {}", word, glyphs.separator())
}

/// The Worlds-pane NOW field block. Every row is a fact a native owner
/// reported; an unavailable owner is a named row, never a gap.
pub fn now_field_lines(reading: &NowFieldReading, glyphs: Glyphs, now: u64) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![format!(
        "NOW field {sep} composed activity across this World's Workcells"
    )];
    lines.push(format!(
        "Declared machine Workcell  {}",
        reading
            .declared_workcell_ref
            .as_deref()
            .unwrap_or("none declared on this ground")
    ));
    lines.push(age_line(reading.supply_age_seconds(now), glyphs));
    lines.push(String::new());
    for workcell in &reading.workcells {
        let reference = workcell.workcell_ref.as_deref().unwrap_or("workcell:?");
        let lifecycle = workcell.lifecycle.as_deref().unwrap_or("?");
        lines.push(format!(
            "{reference} {sep} root NOW {}",
            match lifecycle {
                "active" => "active".to_string(),
                other => format!("lifecycle {other}"),
            }
        ));
        if let Some(purpose) = &workcell.purpose {
            let mut one_line = purpose.split_whitespace().collect::<Vec<_>>().join(" ");
            if one_line.len() > 96 {
                one_line.truncate(93);
                one_line.push_str("...");
            }
            lines.push(format!("  purpose  {one_line}"));
        }
        if workcell.children.is_empty() {
            lines.push("  children  none allocated under this root".to_string());
        } else {
            for child in &workcell.children {
                let condition = child
                    .condition
                    .as_ref()
                    .map(|condition| condition.label().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                lines.push(format!("  {} {sep} {condition}", child.task_ref));
            }
        }
        match (workcell.material_available, &workcell.material_reason) {
            (Some(true), _) => {
                let panes = match workcell.census_panes {
                    Some((harness, total)) => format!(" {sep} {harness} harness of {total} panes"),
                    None => String::new(),
                };
                lines.push(format!(
                    "  material  observed ({}){panes}",
                    workcell.material_scope.as_deref().unwrap_or("local")
                ));
            }
            (Some(false), Some(reason)) => {
                // Unreachable is not empty: the reason says what failed, and
                // the note says the work may still be live where hosted.
                let scope = workcell.material_scope.as_deref().unwrap_or("local");
                if scope == "remote" {
                    lines.push(format!("  material  not observed here {sep} {reason}"));
                } else {
                    lines.push(format!("  material  UNAVAILABLE {sep} {reason}"));
                }
            }
            (Some(false), None) => {
                lines.push("  material  UNAVAILABLE {sep} owner reported no reason".to_string());
            }
            (None, _) => {
                lines.push("  material  no reading supplied".to_string());
            }
        }
        match (workcell.gateway_available, &workcell.gateway_states) {
            (Some(true), states) if !states.is_empty() => {
                lines.push(format!("  gateway  {}", states.join(", ")));
            }
            (Some(true), _) => {
                lines.push("  gateway  reading available; no connectors reported".to_string())
            }
            (Some(false), _) => {
                lines.push(format!(
                    "  gateway  unavailable {} no status reading this cycle",
                    sep
                ));
            }
            (None, _) => lines.push("  gateway  no reading supplied".to_string()),
        }
        lines.push(String::new());
    }
    // Temporal honesty: an undeclared time policy or missing Day pointer is
    // the ground's real condition and is named, not papered over.
    lines.push(match (reading.time_policy_present, reading.day_present) {
        (true, true) => "Temporal  civil-time policy and today pointer recognised".to_string(),
        (true, false) => "Temporal  policy recognised; no today pointer on this ground".to_string(),
        (false, _) => {
            "Temporal  no recognised civil-time policy on this ground; Day reads stay absent"
                .to_string()
        }
    });
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn glyphs() -> Glyphs {
        Glyphs::ascii()
    }

    fn canned_field() -> serde_json::Value {
        json!({
            "schema": NOW_FIELD_SCHEMA,
            "scope_ref": "control:root",
            "generated_at_unix_seconds": 1_000,
            "machine": {"declared_workcell_ref": "workcell:omarchy"},
            "time": {"policy": null, "day": null, "note": "absent readings are reported as absent"},
            "workcells": [{
                "workcell_ref": "workcell:omarchy",
                "root": {"now_ref": "central:now:control:root:abc", "purpose": "the live material horizon", "lifecycle": "active", "workcell_ref": "workcell:omarchy"},
                "children": [
                    {"task_ref": "task:one", "purpose": "first", "lifecycle": "active", "live": true},
                    {"task_ref": "task:two", "purpose": "second", "lifecycle": "quiescent", "live": false}
                ],
                "children_count": 2,
                "material": {
                    "available": true,
                    "observation_scope": "local",
                    "native_workcell_ref": "workcell:local",
                    "census": {"available": true, "reading": {"summary": {"panes": 52, "harness": 7}}},
                    "instances": {"available": true, "reading": {"instances": []}}
                },
                "gateway": {
                    "available": true,
                    "status": {"available": true, "reading": {"data": {"status": {
                        "gateway_ref": "agency-gateway/omarchy",
                        "connector_health": [{"connector_ref": "gateway-connector/telegram/main", "state": "connected", "detail": "reachable", "provenance": ["x"]}]
                    }}}}
                }
            }],
            "missing_workcell_roots": [],
            "unjoined_local": null,
            "horizon": {"carried": [], "released": []},
            "bounds": {}
        })
    }

    #[test]
    fn parse_reads_the_composed_document() {
        let reading = NowFieldReading::parse(&canned_field()).expect("schema matches");
        assert_eq!(
            reading.declared_workcell_ref.as_deref(),
            Some("workcell:omarchy")
        );
        assert_eq!(reading.workcells.len(), 1);
        let workcell = &reading.workcells[0];
        assert_eq!(workcell.census_panes, Some((7, 52)));
        assert_eq!(workcell.gateway_states, vec!["connected".to_string()]);
        assert_eq!(workcell.children[0].condition, Some(ChildCondition::Live));
        assert_eq!(
            workcell.children[1].condition,
            Some(ChildCondition::Waiting)
        );
        assert!(!reading.time_policy_present);
        assert!(!reading.day_present);
    }

    #[test]
    fn a_non_matching_schema_is_not_a_field() {
        assert!(NowFieldReading::parse(&json!({"schema": "other/v1"})).is_none());
        assert!(NowFieldReading::parse(&json!({})).is_none());
    }

    #[test]
    fn conditions_map_the_owner_lifecycle_without_inventing_states() {
        assert_eq!(
            ChildCondition::from_lifecycle("active"),
            Some(ChildCondition::Live)
        );
        assert_eq!(
            ChildCondition::from_lifecycle("quiescent"),
            Some(ChildCondition::Waiting)
        );
        assert_eq!(
            ChildCondition::from_lifecycle("archived"),
            Some(ChildCondition::Returned)
        );
        assert_eq!(ChildCondition::from_lifecycle("mysterious"), None);
    }

    #[test]
    fn unavailable_material_is_a_named_row_never_an_empty_field() {
        let mut value = canned_field();
        value["workcells"][0]["material"] = json!({
            "available": false,
            "observation_scope": "local",
            "reason": "workcell places --json exited unsuccessfully: stub owner refused",
        });
        value["workcells"][0]["gateway"] = json!({"available": false});
        let reading = NowFieldReading::parse(&value).expect("schema matches");
        let lines = now_field_lines(&reading, glyphs(), 1_050);
        let text = lines.join("\n");
        assert!(
            text.contains("UNAVAILABLE"),
            "an unavailable census must be named: {text}"
        );
        assert!(text.contains("stub owner refused"));
        assert!(
            !text.contains("0 harness"),
            "absence of observation is not an empty field"
        );
        assert!(text.contains("no recognised civil-time policy"));
    }

    #[test]
    fn freshness_is_shown_from_the_supply_time_not_probed() {
        let reading = NowFieldReading::parse(&canned_field()).unwrap();
        let fresh = now_field_lines(&reading, glyphs(), 1_030);
        assert!(fresh.join("\n").contains("moments ago"));
        let stale = now_field_lines(&reading, glyphs(), 1_000 + 4_000);
        assert!(stale.join("\n").contains("1 h ago"));
    }

    #[test]
    fn remote_material_names_its_scope_instead_of_claiming_failure() {
        let mut value = canned_field();
        value["workcells"][0]["material"] = json!({
            "available": false,
            "observation_scope": "remote",
            "reason": "this cell observes its own material only",
        });
        let reading = NowFieldReading::parse(&value).unwrap();
        let lines = now_field_lines(&reading, glyphs(), 1_000);
        let text = lines.join("\n");
        assert!(text.contains("not observed here"));
        assert!(
            !text.contains("UNAVAILABLE"),
            "a remote Workcell is not this cell's failure"
        );
    }
}
