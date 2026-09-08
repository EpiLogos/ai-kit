//! KnowledgeDomain (W1): the domain-activation model.
//!
//! A domain is a profile-addressed declaration: when the active composition
//! arms domain activation (`hook/continuity/domain-activation`), a prompt
//! that matches a domain's deterministic triggers makes its guidance
//! operative — within the horizon span the domain declares, never outside
//! it. Everything is declared data with provenance; the engine invents
//! nothing.
//!
//! Two laws live here as code, not convention:
//!
//! * **Dedup:** unchanged rendered guidance is not re-injected. The key is
//!   the content hash of the sorted ordinary rendered lines (plus the
//!   domain id), so any change re-arms injection.
//! * **Standing exemption:** a rule classified `standing` is exempt from
//!   dedup *by that explicit classification*, never by accident — and the
//!   classification travels in the explanation.
//!
//! `retrieval` is a typed [`ResolveExpression`] — no local query DSL is
//! authored anywhere (PROGRAMME §5). Parsing it validates the expression;
//! reaction-time execution through the one resolver is the §5 binding the
//! resolver integration carries.

use serde::{Deserialize, Serialize};

use crate::resource::ResolveExpression;

pub const DOMAIN_SCHEMA: &str = "aikit.knowledge-domain/v1";

/// The disclosure (@0–@5) horizons a domain may address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonRange {
    /// Inclusive lower horizon (@-coordinate, 0–5).
    pub min: u8,
    /// Inclusive upper horizon (@-coordinate, 0–5).
    pub max: u8,
}

impl HorizonRange {
    pub fn admits(&self, horizon: u8) -> bool {
        self.min <= horizon && horizon <= self.max
    }

    pub fn render(&self) -> String {
        format!("@{}–@{}", self.min, self.max)
    }
}

/// Why a rule exists and where it came from — provenance is part of the
/// rule, not a side note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainRule {
    pub rule: String,
    #[serde(default)]
    pub rationale: String,
    pub provenance: String,
    /// `ordinary` (dedup applies) or `standing` (pressure-classified;
    /// dedup-exempt by this explicit classification).
    #[serde(rename = "classification", default = "default_classification")]
    pub pressure_class: PressureClass,
}

fn default_classification() -> PressureClass {
    PressureClass::Ordinary
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PressureClass {
    Ordinary,
    Standing,
}

impl PressureClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            PressureClass::Ordinary => "ordinary",
            PressureClass::Standing => "standing",
        }
    }
}

/// A declared knowledge domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeDomain {
    pub schema: String,
    /// The domain ref, e.g. `domain/release`.
    pub id: String,
    pub title: String,
    pub revision: String,
    /// Provenance: where this declaration is authored.
    pub source: String,
    #[serde(default)]
    pub horizon_range: Option<HorizonRange>,
    /// Deterministic, case-insensitive prompt triggers.
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<DomainRule>,
    /// Declared file-addressing patterns (W1/CASE 05): when the composition
    /// also arms `hook/continuity/file-context`, this domain's guidance
    /// becomes operative before an operation on a file matching any pattern.
    /// Same grammar as skill overlays (`glob_matches`: `*` within a segment,
    /// `**` across segments); patterns are project-root-relative data, like
    /// everything else here. A domain without patterns stays
    /// prompt-addressed only.
    #[serde(default)]
    pub path_patterns: Vec<String>,
    /// The typed retrieval expression this domain resolves when operative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<ResolveExpression>,
}

impl KnowledgeDomain {
    /// Parse and validate one domain declaration (TOML).
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        let domain: KnowledgeDomain =
            toml::from_str(text).map_err(|error| format!("invalid domain manifest: {error}"))?;
        domain.validate()?;
        Ok(domain)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DOMAIN_SCHEMA {
            return Err(format!(
                "schema must be `{DOMAIN_SCHEMA}`, got `{}`",
                self.schema
            ));
        }
        if self.id.trim().is_empty() {
            return Err("domain id is required".into());
        }
        if let Some(range) = self.horizon_range {
            if range.min > 5 || range.max > 5 || range.min > range.max {
                return Err(format!(
                    "horizon_range {} is not a valid @0–@5 span",
                    range.render()
                ));
            }
        }
        if self.triggers.is_empty() && self.path_patterns.is_empty() {
            return Err(
                "a domain declares at least one trigger or path pattern".into(),
            );
        }
        for trigger in &self.triggers {
            if trigger.trim().is_empty() {
                return Err("triggers must be non-empty".into());
            }
        }
        for pattern in &self.path_patterns {
            if pattern.trim().is_empty() {
                return Err(format!(
                    "domain {} declares an empty path pattern",
                    self.id
                ));
            }
        }
        for rule in &self.guidance {
            if rule.rule.trim().is_empty() {
                return Err(format!("domain {} has an empty guidance rule", self.id));
            }
            if rule.provenance.trim().is_empty() {
                return Err(format!(
                    "domain {} rule `{}` carries no provenance",
                    self.id, rule.rule
                ));
            }
        }
        Ok(())
    }
}

/// One matched domain: the trigger that fired and the explanation of why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainActivation<'a> {
    pub domain: &'a KnowledgeDomain,
    /// The trigger that matched, as declared.
    pub trigger: String,
}

/// Deterministic activation: case-insensitive trigger containment over the
/// prompt. The cheap explicit signal that opens deeper graph queries; no
/// fuzz, no model, no ambient state.
pub fn activate<'a>(domains: &'a [KnowledgeDomain], prompt: &str) -> Vec<DomainActivation<'a>> {
    let needle = prompt.to_lowercase();
    let mut activations = Vec::new();
    for domain in domains {
        if let Some(trigger) = domain
            .triggers
            .iter()
            .find(|trigger| needle.contains(&trigger.to_lowercase()))
        {
            activations.push(DomainActivation {
                domain,
                trigger: trigger.clone(),
            });
        }
    }
    activations
}

/// The rendered guidance lines for an activation, split by classification.
/// Standing rules render with their exemption visible; ordinary lines are
/// the dedup-tracked payload.
pub fn render_rules(activation: &DomainActivation<'_>) -> (Vec<String>, Vec<String>) {
    render_guidance_lines(activation.domain)
}

/// The rendered guidance lines for a domain's rules, independent of why the
/// domain became operative (prompt trigger or file addressing).
pub fn render_guidance_lines(domain: &KnowledgeDomain) -> (Vec<String>, Vec<String>) {
    let mut ordinary = Vec::new();
    let mut standing = Vec::new();
    for rule in &domain.guidance {
        let line = match rule.pressure_class {
            PressureClass::Ordinary => format!("- {} [ordinary]", rule.rule),
            PressureClass::Standing => format!(
                "- {} [standing — dedup-exempt by classification] (rationale: {})",
                rule.rule,
                if rule.rationale.is_empty() {
                    "none declared"
                } else {
                    &rule.rationale
                }
            ),
        };
        if rule.pressure_class == PressureClass::Standing {
            standing.push(line);
        } else {
            ordinary.push(line);
        }
    }
    (ordinary, standing)
}

/// The dedup key for a domain's ordinary rendered payload: FNV-1a over the
/// sorted lines prefixed by the domain id (the same derivation Central's
/// stores use for CAS keys).
pub fn dedup_hash(domain_id: &str, ordinary_lines: &[String]) -> String {
    let mut sorted: Vec<&String> = ordinary_lines.iter().collect();
    sorted.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for byte in domain_id.bytes() {
        feed(byte);
    }
    feed(0x1f);
    for line in sorted {
        for byte in line.bytes() {
            feed(byte);
        }
        feed(0x0a);
    }
    format!("{hash:016x}")
}

/// The explanation header: why this domain matched — trigger, horizon,
/// source — and whether the ordinary payload was deduped.
pub fn render_header(
    activation: &DomainActivation<'_>,
    deduped: bool,
    has_standing: bool,
) -> String {
    let domain = activation.domain;
    let horizon = domain
        .horizon_range
        .as_ref()
        .map(HorizonRange::render)
        .unwrap_or_else(|| "unbounded".into());
    format!(
        "[continuity/domain-activation] domain {} activated — trigger: {:?}; horizon: {horizon}; \
         source: {} revision {};{}{}",
        domain.id,
        activation.trigger,
        domain.source,
        domain.revision,
        if deduped { " ordinary payload deduped (unchanged rendered content);" } else { "" },
        if has_standing {
            " standing rules reasserted (dedup-exempt by classification)"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_domain() -> KnowledgeDomain {
        KnowledgeDomain {
            schema: DOMAIN_SCHEMA.into(),
            id: "domain/release".into(),
            title: "Release discipline".into(),
            revision: "r1".into(),
            source: "central:source:project:demo:.aikit/domains/release.toml".into(),
            horizon_range: Some(HorizonRange { min: 3, max: 5 }),
            triggers: vec!["release".into()],
            path_patterns: vec![],
            guidance: vec![
                DomainRule {
                    rule: "Run the verification suite before tagging".into(),
                    rationale: "tags are the immutable boundary".into(),
                    provenance: "central:source:project:demo:ProjectCentral/user/release.md".into(),
                    pressure_class: PressureClass::Ordinary,
                },
                DomainRule {
                    rule: "A red gate is a stop, never a note".into(),
                    rationale: "pressure declines with a red gate in view".into(),
                    provenance: "central:source:project:demo:ProjectCentral/user/release.md".into(),
                    pressure_class: PressureClass::Standing,
                },
            ],
            retrieval: Some(ResolveExpression::subject("wiki:space:release")),
        }
    }

    #[test]
    fn activation_is_deterministic_case_insensitive_trigger_matching() {
        let domains = [release_domain()];
        let hit = activate(&domains, "Please PREPARE THIS RELEASE for tagging");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].trigger, "release");
        assert_eq!(hit[0].domain.id, "domain/release");
        assert!(activate(&domains, "water the garden").is_empty());
    }

    #[test]
    fn the_explanation_names_trigger_horizon_source_and_classification() {
        let domains = [release_domain()];
        let activation = &activate(&domains, "prepare this release")[0];
        let (ordinary, standing) = render_rules(activation);
        assert_eq!(ordinary.len(), 1);
        assert_eq!(standing.len(), 1);
        assert!(standing[0].contains("[standing — dedup-exempt by classification]"));
        let header = render_header(activation, false, true);
        assert!(header.contains("trigger: \"release\""), "{header}");
        assert!(header.contains("horizon: @3–@5"), "{header}");
        assert!(header.contains("source: central:source:project:demo"), "{header}");
        assert!(header.contains("standing rules reasserted"), "{header}");
    }

    #[test]
    fn dedup_keys_on_rendered_content_and_changes_when_content_changes() {
        let domains = [release_domain()];
        let activation = &activate(&domains, "prepare this release")[0];
        let (ordinary, _) = render_rules(activation);
        let first = dedup_hash("domain/release", &ordinary);
        let again = dedup_hash("domain/release", &ordinary);
        assert_eq!(first, again, "unchanged content is the same dedup key");
        let changed = vec![ordinary[0].replace("tagging", "cutting")];
        assert_ne!(first, dedup_hash("domain/release", &changed));
    }

    #[test]
    fn retrieval_is_typed_data_not_a_text_dsl() {
        let domains = [release_domain()];
        let retrieval = domains[0].retrieval.as_ref().unwrap();
        assert_eq!(retrieval.render(), "wiki:space:release");
    }

    #[test]
    fn toml_declarations_parse_and_invalid_ones_refuse() {
        let text = r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/release"
title = "Release discipline"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/release.toml"
triggers = ["release"]

[horizon_range]
min = 3
max = 5

[[guidance]]
rule = "Run the verification suite before tagging"
rationale = "tags are the immutable boundary"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
classification = "ordinary"

[[guidance]]
rule = "A red gate is a stop, never a note"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
classification = "standing"
"#;
        let domain = KnowledgeDomain::from_toml_str(text).expect("valid declaration parses");
        assert_eq!(domain.id, "domain/release");
        assert_eq!(domain.guidance.len(), 2);

        let no_provenance = text.replace(
            "provenance = \"central:source:project:demo:ProjectCentral/user/release.md\"\nclassification = \"standing\"",
            "provenance = \"\"\nclassification = \"standing\"",
        );
        assert!(KnowledgeDomain::from_toml_str(&no_provenance).is_err());

        let bad_horizon = text.replace("max = 5", "max = 9");
        assert!(KnowledgeDomain::from_toml_str(&bad_horizon).is_err());

        let wrong_schema = text.replace(DOMAIN_SCHEMA, "aikit.knowledge-domain/v2");
        assert!(KnowledgeDomain::from_toml_str(&wrong_schema).is_err());
    }
}
