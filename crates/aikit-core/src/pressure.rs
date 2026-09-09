//! Context pressure (W1/CASE 04): what a turn is allowed to spend when the
//! window is filling up.
//!
//! The brackets are PROGRAMME §9's, with their reasons attached:
//!
//! ```text
//! FRESH     ≤20%   the window is empty enough that bounding it would only
//!                  hide material the session can still afford
//! MODERATE  ≤45%   past this point the cheap signals have already fired, so
//!                  ordinary payload competes with the work itself
//! DEPLETED  ≤70%   what remains is nearly all the session's own thinking;
//!                  ordinary payload is a guest and behaves like one
//! CRITICAL  >70%   only what must not be forgotten still arrives
//! ```
//!
//! Two laws hold across every bracket:
//!
//! * **Standing guidance is never bounded away.** A rule classified `standing`
//!   in its authored declaration is dedup-exempt by law
//!   ([`crate::domain::PressureClass`]) and pressure-exempt here for the same
//!   reason: a critical rule that quietly stops being asserted under pressure
//!   is worse than no rule, because nobody notices.
//! * **Withholding is disclosed.** A bounded block says how many lines it kept
//!   back and what pressure did it. Ordinary payload that vanished silently
//!   would look, to a reader, exactly like ordinary payload that never
//!   existed.
//!
//! Nothing here reads global state. The thresholds come from the composition's
//! tuning, and the reading comes from the event.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The default bracket edges, as fractions of the context window.
pub const DEFAULT_FRESH: f64 = 0.20;
pub const DEFAULT_MODERATE: f64 = 0.45;
pub const DEFAULT_DEPLETED: f64 = 0.70;

/// The default denominator for the prompt-count fallback.
///
/// No harness capability descriptor declares a context-consumption figure
/// today, so this is not a rarely-used fallback — it is the ordinary path. 60
/// prompt turns is a deliberately blunt stand-in for a full window: long
/// enough that an ordinary working session never reaches CRITICAL on turn
/// count alone, short enough that a genuinely long session does. It is a
/// composition tuning precisely because it is a guess; a harness that starts
/// reporting real consumption stops using it.
pub const DEFAULT_PROMPT_BUDGET: u32 = 60;

/// Which bracket a reading falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pressure {
    Fresh,
    Moderate,
    Depleted,
    Critical,
}

impl Pressure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Moderate => "moderate",
            Self::Depleted => "depleted",
            Self::Critical => "critical",
        }
    }

    /// What ordinary payload may spend at this pressure.
    pub const fn allowance(self) -> Allowance {
        match self {
            Self::Fresh => Allowance {
                blocks: usize::MAX,
                lines_per_block: usize::MAX,
            },
            Self::Moderate => Allowance {
                blocks: 3,
                lines_per_block: 8,
            },
            Self::Depleted => Allowance {
                blocks: 1,
                lines_per_block: 3,
            },
            Self::Critical => Allowance {
                blocks: 0,
                lines_per_block: 0,
            },
        }
    }
}

impl fmt::Display for Pressure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How much ordinary payload survives: how many blocks may carry ordinary
/// lines at all, and how many lines each of those may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Allowance {
    pub blocks: usize,
    pub lines_per_block: usize,
}

impl Allowance {
    /// The report's form: `usize::MAX` is "unbounded", not a number anyone
    /// should read as a limit.
    pub fn describe(&self) -> serde_json::Value {
        let render = |value: usize| {
            if value == usize::MAX {
                serde_json::Value::String("unbounded".to_owned())
            } else {
                serde_json::Value::from(value)
            }
        };
        serde_json::json!({
            "blocks": render(self.blocks),
            "lines_per_block": render(self.lines_per_block),
        })
    }
}

/// The bracket edges in effect, and the fallback denominator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureBrackets {
    pub fresh: f64,
    pub moderate: f64,
    pub depleted: f64,
    pub prompt_budget: u32,
}

impl Default for PressureBrackets {
    fn default() -> Self {
        Self {
            fresh: DEFAULT_FRESH,
            moderate: DEFAULT_MODERATE,
            depleted: DEFAULT_DEPLETED,
            prompt_budget: DEFAULT_PROMPT_BUDGET,
        }
    }
}

impl PressureBrackets {
    /// Read the edges from the composed capsule's config table.
    ///
    /// A value that is not a number, not in `(0, 1]`, or out of order is
    /// refused *as a whole set*: half-applied brackets would classify by a
    /// scheme nobody declared. The refusal is returned as a warning so the
    /// operator sees which value was rejected, rather than silently running on
    /// defaults that look like a choice.
    pub fn from_config(config: Option<&crate::profile::ConfigTable>) -> (Self, Vec<String>) {
        let defaults = Self::default();
        let Some(config) = config else {
            return (defaults, Vec::new());
        };
        let mut warnings = Vec::new();
        let mut tuned = defaults;

        // The closure borrows `warnings`; the block ends the borrow where the
        // reads end, which is what a scope is for.
        let read = |key: &str, current: f64, warnings: &mut Vec<String>| -> f64 {
            match config.get(key) {
                None => current,
                Some(value) => match value.as_float().or_else(|| value.as_integer().map(|v| v as f64)) {
                    Some(number) if number > 0.0 && number <= 1.0 => number,
                    Some(number) => {
                        warnings.push(format!(
                            "continuity/context-pressure: `{key} = {number}` is not a fraction in (0, 1]; \
                             keeping {current}"
                        ));
                        current
                    }
                    None => {
                        warnings.push(format!(
                            "continuity/context-pressure: `{key}` is not a number; keeping {current}"
                        ));
                        current
                    }
                },
            }
        };
        tuned.fresh = read("fresh", defaults.fresh, &mut warnings);
        tuned.moderate = read("moderate", defaults.moderate, &mut warnings);
        tuned.depleted = read("depleted", defaults.depleted, &mut warnings);

        if let Some(value) = config.get("prompt_budget").and_then(|v| v.as_integer()) {
            if value > 0 {
                tuned.prompt_budget = value as u32;
            } else {
                warnings.push(format!(
                    "continuity/context-pressure: `prompt_budget = {value}` must be positive; \
                     keeping {}",
                    defaults.prompt_budget
                ));
            }
        }

        if !(tuned.fresh < tuned.moderate && tuned.moderate < tuned.depleted) {
            warnings.push(format!(
                "continuity/context-pressure: declared brackets are out of order \
                 (fresh {}, moderate {}, depleted {}); keeping the defaults",
                tuned.fresh, tuned.moderate, tuned.depleted
            ));
            tuned.fresh = defaults.fresh;
            tuned.moderate = defaults.moderate;
            tuned.depleted = defaults.depleted;
        }
        (tuned, warnings)
    }

    /// Which bracket a consumed fraction falls in.
    pub fn classify(&self, consumed: f64) -> Pressure {
        if consumed <= self.fresh {
            Pressure::Fresh
        } else if consumed <= self.moderate {
            Pressure::Moderate
        } else if consumed <= self.depleted {
            Pressure::Depleted
        } else {
            Pressure::Critical
        }
    }

    /// The thresholds as the operator reads them — the composition's declared
    /// values, not the engine's defaults, whenever they differ.
    pub fn describe(&self) -> serde_json::Value {
        serde_json::json!({
            "fresh_max": self.fresh,
            "moderate_max": self.moderate,
            "depleted_max": self.depleted,
            "critical_above": self.depleted,
            "prompt_budget": self.prompt_budget,
        })
    }
}

/// Where the consumed fraction came from.
///
/// This is part of the answer, not metadata about it: "45% consumed" means
/// something different when a harness measured it than when it was inferred
/// from a turn count, and a report that hides the difference invites the
/// reader to trust a number nobody measured.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PressureSource {
    /// The harness reported consumption for this session.
    Reported { detail: String },
    /// No harness figure: inferred from how many prompt turns this session has
    /// already spent.
    PromptCount { prompts: u32, budget: u32 },
}

/// A pressure reading: the bracket, the fraction it came from, and where the
/// fraction came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    pub pressure: Pressure,
    pub consumed: f64,
    pub source: PressureSource,
}

impl Reading {
    pub fn from_prompt_count(brackets: &PressureBrackets, prompts: u32) -> Self {
        let budget = brackets.prompt_budget.max(1);
        let consumed = (f64::from(prompts) / f64::from(budget)).min(1.0);
        Self {
            pressure: brackets.classify(consumed),
            consumed,
            source: PressureSource::PromptCount { prompts, budget },
        }
    }

    pub fn from_reported(brackets: &PressureBrackets, consumed: f64, detail: String) -> Self {
        let consumed = consumed.clamp(0.0, 1.0);
        Self {
            pressure: brackets.classify(consumed),
            consumed,
            source: PressureSource::Reported { detail },
        }
    }

    pub fn describe(&self) -> serde_json::Value {
        serde_json::json!({
            "pressure": self.pressure.as_str(),
            "consumed": self.consumed,
            "source": self.source,
            "allowance": self.pressure.allowance().describe(),
        })
    }
}

/// One reaction's output, with its two kinds of content kept apart.
///
/// The split is not cosmetic: `standing` survives every bracket and `ordinary`
/// does not, so a block that flattened both into one string could only be
/// bounded by cutting standing guidance too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub header: String,
    pub standing: Vec<String>,
    pub ordinary: Vec<String>,
}

impl Block {
    /// A block whose whole body is ordinary payload.
    pub fn ordinary(header: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            header: header.into(),
            standing: Vec::new(),
            ordinary: lines,
        }
    }

    /// A block that pressure never bounds — an explicitly invoked protocol,
    /// or guidance classified `standing` where it was authored.
    pub fn standing(header: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            header: header.into(),
            standing: lines,
            ordinary: Vec::new(),
        }
    }

    /// Render this block whole, without any pressure bound. Used where no
    /// pressure capability is composed: the descope law says an uncomposed
    /// capability changes nothing at all, including this.
    pub fn render(&self) -> String {
        let mut lines = vec![self.header.clone()];
        lines.extend(self.standing.iter().cloned());
        lines.extend(self.ordinary.iter().cloned());
        lines.join("\n")
    }

    fn is_empty(&self) -> bool {
        self.header.is_empty() && self.standing.is_empty() && self.ordinary.is_empty()
    }
}

/// What pressure did to a turn's blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Bounded {
    /// The rendered blocks, in the order they were produced.
    pub blocks: Vec<String>,
    /// How many ordinary lines were withheld in total.
    pub withheld_lines: usize,
    /// How many blocks were suppressed outright — they carried ordinary
    /// payload only, and this bracket allowed none of it.
    pub withheld_blocks: usize,
    /// The one line that discloses suppressed blocks for the turn. A block
    /// that vanishes leaves no header behind to carry its own notice, so the
    /// turn carries it instead: withholding is never silent.
    pub notice: Option<String>,
}

/// Apply the bracket's allowance to a turn's blocks.
///
/// Standing content is emitted whole, always, in every bracket — including
/// CRITICAL, where it is the only thing that arrives. Ordinary content is
/// bounded, and every bound is disclosed: partially bounded blocks say so in
/// place, and blocks suppressed outright are counted in the turn's notice.
pub fn bound(blocks: &[Block], pressure: Pressure) -> Bounded {
    let allowance = pressure.allowance();
    let mut rendered = Vec::new();
    let mut withheld_lines = 0usize;
    let mut withheld_blocks = 0usize;
    let mut ordinary_blocks_used = 0usize;

    for block in blocks {
        if block.is_empty() {
            continue;
        }
        let may_carry_ordinary = ordinary_blocks_used < allowance.blocks;
        let kept: &[String] = if may_carry_ordinary {
            let keep = block.ordinary.len().min(allowance.lines_per_block);
            &block.ordinary[..keep]
        } else {
            &[]
        };
        let withheld = block.ordinary.len() - kept.len();
        withheld_lines += withheld;
        if !kept.is_empty() {
            ordinary_blocks_used += 1;
        }

        // A block that carried nothing but ordinary payload, and kept none of
        // it, does not arrive as a bare heading: it is suppressed, and the
        // turn's notice says how many went that way.
        if block.standing.is_empty() && kept.is_empty() && !block.ordinary.is_empty() {
            withheld_blocks += 1;
            continue;
        }

        let mut lines = vec![block.header.clone()];
        lines.extend(block.standing.iter().cloned());
        lines.extend(kept.iter().cloned());
        if withheld > 0 {
            lines.push(format!(
                "  [context pressure {pressure}] {withheld} further line(s) withheld; \
                 standing guidance is exempt and unaffected"
            ));
        }
        rendered.push(lines.join("\n"));
    }

    let notice = (withheld_blocks > 0).then(|| {
        format!(
            "[continuity/context-pressure] {pressure}: {withheld_blocks} block(s) of ordinary \
             context withheld this turn; standing guidance is exempt and was reasserted"
        )
    });

    Bounded {
        blocks: rendered,
        withheld_lines,
        withheld_blocks,
        notice,
    }
}
