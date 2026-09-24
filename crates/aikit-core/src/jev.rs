//! The provider's general state + typed-question protocol. This is an I/O-free
//! contract, not a document-only mode, a chat model, or another Agent registry.
//! Question IDs are attribution; the provider does not use them for inference.
//! https://docs.typesafe.ai/api (checked 2026-09-22).
use crate::{AikitError, Result};
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use std::{collections::BTreeMap, fmt};

pub const JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const JEV_PROTOCOL: &str = "typesafe.systemone/v1";
pub const JEV_ACTION_REF: &str = "action/model/decide";
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const TOLERANCE: f64 = 0.00001;

fn invalid(message: &str) -> AikitError {
    AikitError::new("jev.invalid_request", message)
}
fn malformed(message: &str) -> AikitError {
    AikitError::new("jev.invalid_answer", message)
}
fn entry(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Object(_) | Value::Array(_) | Value::Null
    )
}
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    Noul {
        #[serde(default)]
        instructions: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<BTreeMap<String, Value>>,
    },
    Choice {
        #[serde(default)]
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    Score {
        #[serde(default)]
        instructions: Value,
        criteria: Vec<Value>,
    },
}
impl Question {
    pub fn validate(&self) -> Result<()> {
        let instructions = match self {
            Self::Noul {
                instructions,
                criteria,
            } => {
                if let Some(criteria) = criteria {
                    if criteria.keys().any(|key| key != "true" && key != "false")
                        || !criteria.values().all(entry)
                    {
                        return Err(invalid(
                            "Noul criteria may describe only the true and false outcomes",
                        ));
                    }
                }
                instructions
            }
            Self::Choice {
                instructions,
                criteria,
            } => {
                if !(1..=255).contains(&criteria.len())
                    || !criteria.keys().all(|key| identifier(key))
                    || !criteria.values().all(entry)
                {
                    return Err(invalid(
                        "Choice requires 1–255 named options with structured entries",
                    ));
                }
                instructions
            }
            Self::Score {
                instructions,
                criteria,
            } => {
                if !(2..=10).contains(&criteria.len()) || !criteria.iter().all(entry) {
                    return Err(invalid(
                        "Score requires 2–10 ordered structured rubric levels",
                    ));
                }
                instructions
            }
        };
        if !entry(instructions) {
            return Err(invalid(
                "Instructions must be a string, object, array or null",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevRequest {
    pub model: String,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}
impl JevRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = unique_json(bytes, MAX_REQUEST_BYTES)
            .map_err(|_| invalid("Request must be bounded JSON with no duplicate object keys"))?;
        let request: Self = serde_json::from_value(value)
            .map_err(|_| invalid("Request does not satisfy the typed state/question contract"))?;
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<()> {
        if !identifier(&self.model) || !self.model.starts_with("jev-") {
            return Err(invalid("An explicit Jev model selector is required"));
        }
        if !matches!(
            self.state,
            Value::String(_) | Value::Object(_) | Value::Array(_)
        ) {
            return Err(invalid("Shared state must be a string, object or array"));
        }
        if !(1..=256).contains(&self.questions.len())
            || !self.questions.keys().all(|id| identifier(id))
        {
            return Err(invalid(
                "Provide 1–256 uniquely named, bounded typed questions; nothing is truncated",
            ));
        }
        for question in self.questions.values() {
            question.validate()?;
        }
        if serde_json::to_vec(self)
            .map_err(|_| invalid("Cannot encode request"))?
            .len()
            > MAX_REQUEST_BYTES
        {
            return Err(invalid("Encoded request exceeds the 1 MiB native limit"));
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "blake3:{}",
            blake3::hash(&serde_json::to_vec(self).map_err(|_| invalid("Cannot encode request"))?)
                .to_hex()
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        // Structured rubric entries retain their native form. Do not flatten a
        // matrix's axis meanings into a second independently worded registry.
        legend: BTreeMap<String, Value>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevResponse {
    /// The provider's returned concrete version, never copied from the request.
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    /// Required on success. Absence is not zero usage.
    pub usage: TokenUsage,
}
fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn distribution<'a>(
    values: &BTreeMap<String, f64>,
    keys: impl Iterator<Item = &'a String>,
) -> Result<()> {
    if !values.keys().eq(keys)
        || !values.values().all(|value| probability(*value))
        || (values.values().sum::<f64>() - 1.0).abs() > TOLERANCE
    {
        return Err(malformed(
            "Probability distribution must cover the exact options and sum to one",
        ));
    }
    Ok(())
}
impl JevResponse {
    pub fn parse_for(bytes: &[u8], request: &JevRequest) -> Result<Self> {
        let value = unique_json(bytes, MAX_RESPONSE_BYTES).map_err(|_| {
            malformed("Response must be bounded JSON with no duplicate object keys")
        })?;
        let response: Self = serde_json::from_value(value)
            .map_err(|_| malformed("Response is missing a required answer, version, usage field, or has the wrong type"))?;
        response.validate_for(request)?;
        Ok(response)
    }
    pub fn validate_for(&self, request: &JevRequest) -> Result<()> {
        request.validate()?;
        let Some(version) = self.model.strip_prefix("jev-") else {
            return Err(malformed("Provider did not return a concrete Jev version"));
        };
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() != 3
            || !parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(malformed("Provider returned a model alias or malformed version instead of the evaluated version"));
        }
        if !self.answers.keys().eq(request.questions.keys()) {
            return Err(malformed("Every requested question needs exactly one answer; missing or surplus answers refuse the determination"));
        }
        for (id, question) in &request.questions {
            match (question, &self.answers[id]) {
                (Question::Noul { .. }, Answer::Noul { noul }) if probability(*noul) => {}
                (
                    Question::Choice { criteria, .. },
                    Answer::Choice {
                        choice,
                        probabilities,
                        confidence,
                    },
                ) => {
                    distribution(probabilities, criteria.keys())?;
                    let chosen = probabilities
                        .get(choice)
                        .ok_or_else(|| malformed("Chosen option was not in the question"))?;
                    if !probability(*confidence)
                        || probabilities
                            .values()
                            .any(|value| value > &(chosen + TOLERANCE))
                    {
                        return Err(malformed(
                            "Choice must be a highest-probability option with finite confidence",
                        ));
                    }
                }
                (
                    Question::Score { criteria, .. },
                    Answer::Score {
                        score,
                        legend,
                        probabilities,
                        confidence,
                    },
                ) => {
                    let expected: BTreeMap<String, Value> = criteria
                        .iter()
                        .enumerate()
                        .map(|(i, entry)| (i.to_string(), entry.clone()))
                        .collect();
                    distribution(probabilities, expected.keys())?;
                    if legend != &expected
                        || !probability(*confidence)
                        || !score.is_finite()
                        || !(0.0..=(criteria.len() - 1) as f64).contains(score)
                    {
                        return Err(malformed("Score legend must retain the exact ordered rubric and a bounded score/confidence"));
                    }
                    let weighted = criteria
                        .iter()
                        .enumerate()
                        .map(|(i, _)| i as f64 * probabilities[&i.to_string()])
                        .sum::<f64>();
                    if (score - weighted).abs() > TOLERANCE * criteria.len() as f64 {
                        return Err(malformed(
                            "Score is not the expected value of its distribution",
                        ));
                    }
                }
                _ => return Err(malformed(
                    "Answer kind must match its question and probabilities must be finite in [0,1]",
                )),
            }
        }
        Ok(())
    }
}

/// A per-invocation bound is explicit, finite and reserved before each attempt.
/// A missing usage report retains the reservation as *unknown*, never as free.
/// Rates are operator-selected policy with a concrete version/source basis;
/// this is not an assertion about a vendor invoice or an invented token count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevLimits {
    pub timeout_ms: u64,
    pub max_attempts: u32,
    pub max_total_reserved_microusd: u64,
    pub tariff: JevTariff,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevTariff {
    pub model_version: String,
    pub source: String,
    pub max_input_tokens_per_attempt: u64,
    pub max_output_tokens_per_attempt: u64,
    pub input_microusd_per_million_tokens: u64,
    pub output_microusd_per_million_tokens: u64,
}
impl JevTariff {
    pub fn cost_microusd(&self, usage: &TokenUsage) -> Result<u64> {
        let input = (usage.input_tokens as u128) * (self.input_microusd_per_million_tokens as u128);
        let output =
            (usage.output_tokens as u128) * (self.output_microusd_per_million_tokens as u128);
        let cost = input
            .checked_add(output)
            .ok_or_else(|| invalid("Tariff arithmetic overflow"))?
            .div_ceil(1_000_000);
        cost.try_into()
            .map_err(|_| invalid("Tariff arithmetic overflow"))
    }
    pub fn reservation(&self) -> Result<u64> {
        self.cost_microusd(&TokenUsage {
            input_tokens: self.max_input_tokens_per_attempt,
            output_tokens: self.max_output_tokens_per_attempt,
        })
    }
}
/// Conservative input-token estimate for a Jev request: serialized bytes over
/// 2.5 (measured ~2.65 on matrix/spine state), so it errs toward refusing.
pub fn estimated_input_tokens(request: &JevRequest) -> u64 {
    let bytes = serde_json::to_vec(request)
        .map(|b| b.len())
        .unwrap_or(usize::MAX) as u64;
    bytes.saturating_mul(10) / 25
}

impl JevLimits {
    pub fn validate(&self, request: &JevRequest) -> Result<()> {
        request.validate()?;
        if !(1..=180_000).contains(&self.timeout_ms)
            || !(1..=8).contains(&self.max_attempts)
            || self.tariff.max_input_tokens_per_attempt == 0
            || self.tariff.max_output_tokens_per_attempt == 0
            || self.tariff.source.trim().is_empty()
            || self.tariff.source.len() > 4096
            || !identifier(&self.tariff.model_version)
            || self.tariff.model_version == "jev-latest"
            || self.tariff.model_version == "jev-preview"
            || (request.model != self.tariff.model_version
                && request.model != "jev-latest"
                && request.model != "jev-preview")
        {
            return Err(invalid(
                "Time, attempts and concrete model-specific token/tariff bounds must be explicit",
            ));
        }
        // The provider refuses an over-ceiling request with an opaque HTTP 400
        // (observed 2026-09-24: 31,342 input tokens accepted, ~34k refused).
        // Estimate conservatively (~2.5 bytes per token; measured ~2.65) and
        // refuse locally, naming both numbers, before anything is sent.
        let estimated = estimated_input_tokens(request);
        if estimated > self.tariff.max_input_tokens_per_attempt {
            return Err(AikitError::new(
                "jev.request_over_input_ceiling",
                format!(
                    "The request is ~{estimated} input tokens, over the declared ceiling of {}; \
                     select fewer rows or ask fewer questions",
                    self.tariff.max_input_tokens_per_attempt
                ),
            ));
        }
        let reservation = self.tariff.reservation()?;
        if self.max_total_reserved_microusd == 0 || reservation > self.max_total_reserved_microusd {
            return Err(AikitError::new(
                "jev.budget_exhausted",
                "The invocation cannot reserve one bounded attempt",
            ));
        }
        Ok(())
    }
}

/// serde_json's ordinary Value parser silently keeps the last duplicate key.
/// Decision protocols must not permit two contradictory answers or criteria to
/// become successful by parser choice. This preserves JSON values without that
/// ambiguity; the normal serde recursion limit and caller byte limit still apply.
pub fn unique_json(bytes: &[u8], max_bytes: usize) -> Result<Value> {
    if bytes.len() > max_bytes {
        return Err(invalid("JSON exceeds the declared byte bound"));
    }
    serde_json::from_slice::<UniqueValue>(bytes)
        .map(|value| value.0)
        .map_err(|_| invalid("Malformed JSON or duplicate object keys"))
}
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(value.into())))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(value.into())))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                Number::from_f64(value)
                    .map(|value| UniqueValue(Value::Number(value)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value.into())))
            }
            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value)))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, UniqueValue(value))) =
                    map.next_entry::<String, UniqueValue>()?
                {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
