//! The canonical AIKit Model catalogue.
//!
//! A catalogue entry is what makes a Model exist as an AIKit citizen: a stable
//! `ModelRef` of the form `model:<stable-id>`, plus the provider-native ids
//! that are *known ways of naming it*. Identity is stable across provider
//! renames precisely because it does not come from a provider.
//!
//! Detection never writes here. If detection could mint catalogue entries then
//! Model identity would be a function of whatever happens to be installed on
//! this machine today, which is what stable identity forbids. Discovery that
//! no entry claims stays an unmatched offer (see [`crate::model_route`]) until
//! an owner or a Provider Source admits or maps it.
//!
//! The catalogue is carried by the resource system: every entry projects to a
//! `ResourceKind::Model` `ResourceRecord`. This module creates no second
//! registry — it is the authored half of the catalogue -> availability join.
//!
//! # The owner's model book
//!
//! An owner entry may carry an [`OwnerModelBook`]: the owner's authored
//! judgement about one Model — its class facets, its quirks (testable
//! conditional claims), what work it is good for, a preference signal, and, if
//! the owner decides so, an exclusion. The law of the book:
//!
//! * every record carries its own source and date; the loader refuses a record
//!   that cannot say where it came from;
//! * authored judgements ride explanations and never become observational task
//!   fitness, availability or trust — the roster keeps them in
//!   `ModelRosterCandidate::authored_preference` and in the ranking
//!   explanation's components, never in `task_fitness` / `observed_fitness`;
//! * an exclusion is the only authored authorisation surface: it is what makes
//!   a candidate fail the `authorised` / `policy-allowed` gates. There is no
//!   separate permission system;
//! * no secret ever belongs in a book record — keys are none of the book's
//!   business. Model identity stays the canonical `ModelRef`; provider-native
//!   ids mentioned in a quirk's conditions are route metadata, never promoted.
//!
//! The file format is the catalogue's own owner overlay
//! (`<home>/model-catalogue/*.json`, documented in
//! `aikit-store::model_catalogue`): an entry gains a `"book"` object.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::{
    CredentialCondition, ModelRouteKind, ProviderRef, ResourceDescriptor, ResourceKind,
    ResourceRecord, ResourceRef, ResourceSource, SourceAuthority, SourceRef, SourceState,
};
use crate::{AikitError, Result};

pub const MODEL_CATALOGUE_VERSION: &str = "aikit.model-catalogue/v1";

/// The canonical prefix. `model:<stable-id>` — one colon, then a stable id.
pub const MODEL_REF_PREFIX: &str = "model:";

/// The stale in-tree form, retained only so it can be recognised and migrated.
const STALE_MODEL_REF_PREFIX: &str = "model/";

/// The first-party catalogue's own source identity.
pub const FIRST_PARTY_CATALOGUE_SOURCE: &str = "source/aikit-model-catalogue";

/// Parse a canonical ModelRef. `model/...` is refused here: it is the stale
/// form, and silently accepting it would let two spellings of one Model
/// coexist. Use [`migrate_model_ref`] at an ingestion boundary.
pub fn canonical_model_ref(raw: impl AsRef<str>) -> Result<ResourceRef> {
    let raw = raw.as_ref();
    let Some(stable) = raw.strip_prefix(MODEL_REF_PREFIX) else {
        return Err(AikitError::new(
            "model_catalogue.non_canonical_ref",
            format!("`{raw}` is not a canonical ModelRef (expected `model:<stable-id>`)"),
        ));
    };
    if stable.trim().is_empty() {
        return Err(AikitError::new(
            "model_catalogue.non_canonical_ref",
            format!("`{raw}` carries no stable id after `{MODEL_REF_PREFIX}`"),
        ));
    }
    ResourceRef::parse(raw)
}

/// Accept either spelling at an ingestion boundary and return the canonical
/// ref, plus a disclosure when a stale `model/...` form was migrated. The
/// migration is always disclosed: a silently rewritten identity is a lie about
/// what the source said.
pub fn migrate_model_ref(raw: impl AsRef<str>) -> Result<(ResourceRef, Option<String>)> {
    let raw = raw.as_ref();
    if let Some(stable) = raw.strip_prefix(STALE_MODEL_REF_PREFIX) {
        let canonical = canonical_model_ref(format!("{MODEL_REF_PREFIX}{stable}"))?;
        return Ok((
            canonical.clone(),
            Some(format!(
                "`{raw}` uses the stale `model/` form; migrated to canonical `{canonical}`"
            )),
        ));
    }
    Ok((canonical_model_ref(raw)?, None))
}

/// One declared way of reaching a catalogued Model. Declared is not available:
/// this says "if this provider is there, these are the names it uses", and the
/// join against live detection decides whether any of it is actually true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredRoute {
    pub provider: ProviderRef,
    pub kind: ModelRouteKind,
    /// The provider-native ids this provider is known to use for this Model.
    /// Several are normal: providers rename, tag and version their own names.
    pub provider_native_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub credential: CredentialCondition,
}

impl DeclaredRoute {
    pub fn claims(&self, native_id: &str) -> bool {
        self.provider_native_ids
            .iter()
            .any(|declared| declared == native_id)
    }
}

/// One catalogued Model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogueEntry {
    pub model: ResourceRef,
    pub name: String,
    pub description: String,
    /// Refs this entry absorbed. A provider rename moves the provider-native
    /// id, not the ModelRef; this is for the rarer case of an AIKit-side
    /// identity correction, where the old ref must still resolve.
    #[serde(default)]
    pub superseded_refs: BTreeSet<String>,
    #[serde(default)]
    pub routes: Vec<DeclaredRoute>,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    /// The owner's authored model book, when this entry carries one. Only an
    /// owner entry carries it: the seed and Provider Sources publish identity,
    /// never judgement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book: Option<OwnerModelBook>,
}

/// The owner's authored judgement about one Model — the model book record.
///
/// Plain structured fields, no secret material, canonical `ModelRef` identity.
/// Every record names where it came from and when it was authored; a record
/// that cannot say so is refused at load, loudly, naming file and field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerModelBook {
    /// What authored this record (an owner ref, a session ref — a source, not
    /// a key or a prompt).
    pub source: String,
    /// When it was authored (an ISO date is enough).
    pub authored_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Class facets: independent classifications, never one quality tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ModelClassFacets>,
    /// Testable conditional claims. Conflicting qualified observations are
    /// kept side by side, never overwritten into a universal warning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quirks: Vec<ModelQuirk>,
    /// The kinds of work the owner reaches for this model. Free tagged
    /// strings; extensible, not a closed taxonomy. These are authored
    /// affinities and never become observational task fitness.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub use_for: Vec<String>,
    /// The owner's preference signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preference: Option<AuthoredPreference>,
    /// An authored exclusion: the one thing that makes the model fail the
    /// roster's `authorised` / `policy-allowed` gates. Absent means allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion: Option<AuthoredExclusion>,
}

/// Class facets. Each facet is an independent classification; unknown or
/// undisclosed internals stay unknown by simply not being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelClassFacets {
    /// Family/architecture lineage, e.g. "Claude 5 family".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Generation within the family, e.g. "2026-03 snapshot".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    /// Reasoning/interaction regime, e.g. "deliberate, long-horizon".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// text / vision / audio / other input-output modalities.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub modalities: BTreeSet<String>,
    /// Additional expandable facets (parameter scale where genuinely
    /// disclosed, operational efficiency conditions, and kin), each carrying
    /// its value only where it is genuinely known.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub facets: BTreeMap<String, String>,
}

/// One quirk: a testable conditional claim, kept as plain structured fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelQuirk {
    /// The claim, stated conditionally ("under long tool loops, ...").
    pub claim: String,
    /// Route/body/context/task conditions the claim applies under.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    /// What was seen that supports it (refs/summaries, never secrets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// A concrete case that did *not* show the effect, if one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterexample: Option<String>,
    /// What works around it, with its cost if that matters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workaround: Option<String>,
    /// Current standing of the claim.
    pub standing: QuirkStanding,
    /// When the behaviour was last observed (ISO date).
    pub observed_at: String,
    /// Where the observation came from.
    pub source: String,
    /// What would retest, supersede or retire this claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retest: Option<String>,
}

/// The standing of a quirk claim. Freshness lives in `observed_at`; this says
/// what kind of claim it presently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuirkStanding {
    /// Seen here or reported with named evidence.
    Observed,
    /// Believed but not yet evidenced; a question to test, not a fact.
    Hypothesised,
    /// A newer observation has qualified this one; kept, not deleted.
    Superseded,
    /// Retested and not reproduced; kept for the record.
    Retired,
}

/// The owner's preference signal. Higher rank is more preferred. It is
/// visible in roster explanations and breaks exact ranking ties; it never
/// becomes task fitness and never gates eligibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredPreference {
    pub rank: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The owner's authored exclusion. This is the only authorisation surface in
/// the roster: a book with an exclusion makes the model ineligible everywhere,
/// with the reason carried on the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredExclusion {
    /// Why the owner excluded it. Refused when empty: an exclusion without a
    /// stated reason is not a record, it is a mood.
    pub reason: String,
    /// When the exclusion was authored (ISO date).
    pub since: String,
}

impl OwnerModelBook {
    /// Validate the record. The error message names the offending field so the
    /// loader can refuse a bad record loudly, with the file named beside it.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, why: &str| {
            Err(AikitError::new(
                "model_book.invalid_record",
                format!("`{field}`: {why}"),
            ))
        };
        if self.source.trim().is_empty() {
            return bad("source", "every book record names where it came from");
        }
        if self.authored_at.trim().is_empty() {
            return bad("authored_at", "every book record carries its date");
        }
        if let Some(class) = &self.class {
            class.validate()?;
        }
        for (index, quirk) in self.quirks.iter().enumerate() {
            quirk.validate(&format!("quirks[{index}]"))?;
        }
        if let Some(tag) = self.use_for.iter().find(|tag| tag.trim().is_empty()) {
            return bad(
                "use_for",
                format!("empty tag {tag:?} is not a work type").as_str(),
            );
        }
        if let Some(preference) = &self.preference {
            if preference
                .note
                .as_deref()
                .is_some_and(|n| n.trim().is_empty())
            {
                return bad("preference.note", "an empty note is not a note");
            }
        }
        if let Some(exclusion) = &self.exclusion {
            if exclusion.reason.trim().is_empty() {
                return bad(
                    "exclusion.reason",
                    "an exclusion without a reason is a mood, not a record",
                );
            }
            if exclusion.since.trim().is_empty() {
                return bad(
                    "exclusion.since",
                    "an exclusion carries the date it was authored",
                );
            }
        }
        Ok(())
    }

    /// True when the book excludes the model — the roster's authorisation
    /// surface.
    pub fn excluded(&self) -> bool {
        self.exclusion.is_some()
    }
}

impl ModelClassFacets {
    fn validate(&self) -> Result<()> {
        let bad = |field: &str| {
            Err(AikitError::new(
                "model_book.invalid_record",
                format!("`{field}`: an empty facet says nothing; leave it out instead"),
            ))
        };
        if self.family.as_deref().is_some_and(|v| v.trim().is_empty()) {
            return bad("class.family");
        }
        if self
            .generation
            .as_deref()
            .is_some_and(|v| v.trim().is_empty())
        {
            return bad("class.generation");
        }
        if self
            .reasoning
            .as_deref()
            .is_some_and(|v| v.trim().is_empty())
        {
            return bad("class.reasoning");
        }
        if self.modalities.iter().any(|m| m.trim().is_empty()) {
            return bad("class.modalities");
        }
        if self
            .facets
            .iter()
            .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
        {
            return bad("class.facets");
        }
        Ok(())
    }
}

impl ModelQuirk {
    fn validate(&self, path: &str) -> Result<()> {
        let bad = |field: &str, why: &str| {
            Err(AikitError::new(
                "model_book.invalid_record",
                format!("`{path}.{field}`: {why}"),
            ))
        };
        if self.claim.trim().is_empty() {
            return bad("claim", "a quirk is its claim");
        }
        if self.observed_at.trim().is_empty() {
            return bad("observed_at", "a quirk carries when it was observed");
        }
        if self.source.trim().is_empty() {
            return bad("source", "a quirk names where the observation came from");
        }
        if let Some(conditions) = self.conditions.iter().find(|c| c.trim().is_empty()) {
            return bad(
                "conditions",
                format!("empty condition {conditions:?}").as_str(),
            );
        }
        Ok(())
    }
}

impl ModelCatalogueEntry {
    /// The entry as a resource-system citizen. Routes are not attached here:
    /// a catalogue record asserts identity only, and availability is added by
    /// the join against live detection evidence.
    pub fn resource_record(&self) -> ResourceRecord {
        let mut descriptor = ResourceDescriptor::new(
            self.model.clone(),
            ResourceKind::Model,
            self.name.clone(),
            self.description.clone(),
        );
        descriptor.sources.push(ResourceSource {
            source: self.source.clone(),
            authority: Some(SourceAuthority::Authored),
            revision: None,
            locator: None,
            // The catalogue entry itself is present; that is a statement about
            // the catalogue, never about whether the Model can be reached.
            state: SourceState::Available,
        });
        if let Some(freshness) = &self.freshness {
            descriptor
                .annotations
                .insert("catalogue_freshness".into(), freshness.clone());
        }
        if !self.superseded_refs.is_empty() {
            descriptor.annotations.insert(
                "superseded_refs".into(),
                self.superseded_refs
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        ResourceRecord::new(descriptor)
    }
}

/// The catalogue as a whole. Entries are keyed by canonical ModelRef; a later
/// insert for the same ref replaces the earlier one, so owner-authored entries
/// layered over the first-party seed win without either being edited.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogue {
    entries: BTreeMap<String, ModelCatalogueEntry>,
}

impl ModelCatalogue {
    pub fn insert(&mut self, entry: ModelCatalogueEntry) -> Result<()> {
        canonical_model_ref(entry.model.as_str())?;
        self.entries.insert(entry.model.to_string(), entry);
        Ok(())
    }

    pub fn entries(&self) -> impl Iterator<Item = &ModelCatalogueEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, model: &ResourceRef) -> Option<&ModelCatalogueEntry> {
        self.entries.get(model.as_str()).or_else(|| {
            self.entries
                .values()
                .find(|entry| entry.superseded_refs.contains(model.as_str()))
        })
    }

    /// Which catalogued Model, if any, claims this provider-native id. This is
    /// the whole join key: a provider-native id is route metadata, and the
    /// catalogue is the only thing that can say which identity it belongs to.
    pub fn claiming(
        &self,
        provider: &ProviderRef,
        native_id: &str,
    ) -> Option<(&ModelCatalogueEntry, &DeclaredRoute)> {
        self.entries.values().find_map(|entry| {
            entry
                .routes
                .iter()
                .find(|route| &route.provider == provider && route.claims(native_id))
                .map(|route| (entry, route))
        })
    }

    /// Merge another catalogue over this one, later entries winning.
    pub fn extend(&mut self, other: ModelCatalogue) {
        self.entries.extend(other.entries);
    }

    /// The first-party seed: the Models AIKit itself knows about, with the
    /// provider-native names those providers are known to use. It is authored
    /// ground carrying a freshness note, not a live provider feed — a provider
    /// that renames a model needs an entry update, and until then the rename
    /// shows up honestly as an unmatched offer rather than a silent identity
    /// change.
    pub fn first_party_seed() -> Self {
        let mut catalogue = Self::default();
        for entry in seed_entries() {
            catalogue.entries.insert(entry.model.to_string(), entry);
        }
        catalogue
    }
}

fn seed_source() -> SourceRef {
    SourceRef::parse(FIRST_PARTY_CATALOGUE_SOURCE).expect("first-party catalogue source ref")
}

const SEED_FRESHNESS: &str =
    "first-party catalogue seed, authored 2026-09-09; provider-native names are point-in-time \
     and an owner catalogue entry supersedes this one";

fn hosted(provider: &str, ids: &[&str]) -> DeclaredRoute {
    DeclaredRoute {
        provider: ProviderRef::parse(provider).expect("seed provider ref"),
        kind: ModelRouteKind::ProviderNative,
        provider_native_ids: ids.iter().map(|id| (*id).to_string()).collect(),
        endpoint: None,
        credential: CredentialCondition::Required {
            hint: format!("{provider} inference credential"),
        },
    }
}

fn local(ids: &[&str]) -> DeclaredRoute {
    DeclaredRoute {
        provider: ProviderRef::parse("provider:ollama").expect("seed provider ref"),
        kind: ModelRouteKind::LocalServing,
        provider_native_ids: ids.iter().map(|id| (*id).to_string()).collect(),
        endpoint: Some("http://127.0.0.1:11434".into()),
        credential: CredentialCondition::NotRequired,
    }
}

fn entry(
    model: &str,
    name: &str,
    description: &str,
    routes: Vec<DeclaredRoute>,
) -> ModelCatalogueEntry {
    ModelCatalogueEntry {
        model: canonical_model_ref(model).expect("seed model ref"),
        name: name.to_string(),
        description: description.to_string(),
        superseded_refs: BTreeSet::new(),
        routes,
        source: seed_source(),
        freshness: Some(SEED_FRESHNESS.to_string()),
        book: None,
    }
}

fn seed_entries() -> Vec<ModelCatalogueEntry> {
    vec![
        entry(
            "model:claude-opus-5",
            "Claude Opus 5",
            "Anthropic frontier model",
            vec![hosted("provider:anthropic", &["claude-opus-5"])],
        ),
        entry(
            "model:claude-sonnet-5",
            "Claude Sonnet 5",
            "Anthropic balanced model",
            vec![hosted("provider:anthropic", &["claude-sonnet-5"])],
        ),
        entry(
            "model:claude-haiku-4.5",
            "Claude Haiku 4.5",
            "Anthropic fast model",
            vec![hosted("provider:anthropic", &["claude-haiku-4-5-20251001"])],
        ),
        entry(
            "model:gpt-5.4",
            "GPT-5.4",
            "OpenAI frontier model",
            vec![hosted("provider:openai", &["gpt-5.4"])],
        ),
        entry(
            "model:gpt-realtime",
            "GPT Realtime",
            "OpenAI realtime speech-to-speech model (speech/audio in, speech/audio/text out)",
            vec![hosted("provider:openai", &["gpt-realtime"])],
        ),
        entry(
            "model:gpt-4o-transcribe",
            "GPT-4o Transcribe",
            "OpenAI speech-to-text model",
            vec![hosted("provider:openai", &["gpt-4o-transcribe"])],
        ),
        entry(
            "model:gpt-4o-mini-tts",
            "GPT-4o mini TTS",
            "OpenAI text-to-speech model",
            vec![hosted("provider:openai", &["gpt-4o-mini-tts"])],
        ),
        entry(
            "model:deepseek-chat",
            "DeepSeek Chat",
            "DeepSeek hosted chat model",
            vec![hosted("provider:deepseek", &["deepseek-chat"])],
        ),
        entry(
            "model:smollm2-135m",
            "SmolLM2 135M",
            "Small local model, locally served",
            vec![local(&["smollm2:135m"])],
        ),
        entry(
            "model:llama3.2",
            "Llama 3.2",
            "Meta Llama 3.2, locally served",
            vec![local(&[
                "llama3.2:latest",
                "llama3.2",
                "llama3.2:3b",
                "llama3.2:1b",
            ])],
        ),
        entry(
            "model:qwen2.5-coder",
            "Qwen2.5 Coder",
            "Qwen2.5 Coder, locally served",
            vec![local(&[
                "qwen2.5-coder:latest",
                "qwen2.5-coder",
                "qwen2.5-coder:7b",
                "qwen2.5-coder:1.5b",
            ])],
        ),
    ]
}

// ---------------------------------------------------------------------------
// Provider Sources
// ---------------------------------------------------------------------------

/// The schema this product already defines for one model as a provider or
/// router publishes it. Provider evidence only: never fitness, preference,
/// authorisation, or availability for a concrete account.
pub const PROVIDER_CATALOG_OBSERVATION_SCHEMA: &str = "aikit.provider-catalog-observation/v1";

/// The source identity stamped on catalogue entries a Provider Source supplied.
pub const PROVIDER_CATALOG_SOURCE: &str = "source/aikit-provider-catalog";

/// One published model, as read from a provider's or router's own listing.
///
/// `model_ref` is the canonical identity this listing was resolved to;
/// `listed_variant` is how the listing spells it and `provider_native_id` is
/// how the underlying provider spells it. Both stay opaque route metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderCatalogObservation {
    pub schema_version: String,
    pub observation_kind: String,
    pub source: String,
    pub observed_at: String,
    /// The provider that actually serves the model behind the listing.
    pub provider_ref: ProviderRef,
    /// The router or provider whose listing this came from.
    pub listed_by: ProviderRef,
    pub model_ref: ResourceRef,
    /// The listing's own id for it (e.g. a router id).
    pub listed_variant: String,
    /// The serving provider's own id for it, split out at ingestion.
    pub provider_native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_variant: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub supported_parameters: Vec<String>,
    /// USD per 1M tokens, converted from the listing's own figures.
    #[serde(default)]
    pub pricing_usd_per_1m: BTreeMap<String, f64>,
    pub freshness: String,
}

/// A cached Provider Source reading, as persisted between runs. Observed
/// material, never authored ground: it carries when it was read and from
/// where, so a stale catalogue can be recognised as stale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderCatalogDocument {
    pub schema_version: String,
    pub listed_by: ProviderRef,
    pub source: String,
    pub observed_at: String,
    pub observations: Vec<ProviderCatalogObservation>,
}

impl ProviderCatalogDocument {
    pub fn new(
        listed_by: ProviderRef,
        source: impl Into<String>,
        observed_at: impl Into<String>,
        observations: Vec<ProviderCatalogObservation>,
    ) -> Self {
        Self {
            schema_version: PROVIDER_CATALOG_OBSERVATION_SCHEMA.to_string(),
            listed_by,
            source: source.into(),
            observed_at: observed_at.into(),
            observations,
        }
    }
}

/// Fold provider observations into catalogue entries.
///
/// Several listings routinely collapse onto one `ModelRef` — variant suffixes,
/// and the same model published by more than one vendor. That collapsing is
/// the point: one identity, several declared routes. An owner-authored entry
/// for the same ModelRef supersedes all of it.
pub fn catalogue_from_observations(
    observations: &[ProviderCatalogObservation],
) -> Result<ModelCatalogue> {
    struct Group {
        name: String,
        listed: BTreeMap<String, Vec<String>>,
        native: BTreeMap<String, Vec<String>>,
    }
    let source = SourceRef::parse(PROVIDER_CATALOG_SOURCE)?;
    let mut grouped: BTreeMap<String, Group> = BTreeMap::new();
    for observation in observations {
        let group = grouped
            .entry(observation.model_ref.to_string())
            .or_insert_with(|| Group {
                name: observation.name.clone(),
                listed: BTreeMap::new(),
                native: BTreeMap::new(),
            });
        let listed = group
            .listed
            .entry(observation.listed_by.to_string())
            .or_default();
        if !listed.contains(&observation.listed_variant) {
            listed.push(observation.listed_variant.clone());
        }
        let native = group
            .native
            .entry(observation.provider_ref.to_string())
            .or_default();
        if !native.contains(&observation.provider_native_id) {
            native.push(observation.provider_native_id.clone());
        }
    }

    let mut catalogue = ModelCatalogue::default();
    for (model, group) in grouped {
        let model_ref = canonical_model_ref(&model)?;
        let mut routes = Vec::new();
        for (router, ids) in group.listed {
            routes.push(DeclaredRoute {
                provider: ProviderRef::parse(&router)?,
                kind: ModelRouteKind::RouterRoute,
                provider_native_ids: ids,
                endpoint: None,
                credential: CredentialCondition::Required {
                    hint: format!("{router} API key"),
                },
            });
        }
        for (vendor, ids) in group.native {
            routes.push(DeclaredRoute {
                provider: ProviderRef::parse(&vendor)?,
                kind: ModelRouteKind::ProviderNative,
                provider_native_ids: ids,
                endpoint: None,
                credential: CredentialCondition::Required {
                    hint: format!("{vendor} inference credential"),
                },
            });
        }
        catalogue.insert(ModelCatalogueEntry {
            model: model_ref,
            name: group.name,
            description: "published by a Provider Source listing".to_string(),
            superseded_refs: BTreeSet::new(),
            routes,
            source: source.clone(),
            freshness: Some(
                "point-in-time provider listing; refresh before treating as current catalogue truth"
                    .to_string(),
            ),
            book: None,
        })?;
    }
    Ok(catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_grammar_accepts_model_colon_and_refuses_the_stale_slash_form() {
        assert_eq!(
            canonical_model_ref("model:deepseek-v4-flash")
                .unwrap()
                .as_str(),
            "model:deepseek-v4-flash"
        );
        let error = canonical_model_ref("model/deepseek-v4-flash").unwrap_err();
        assert_eq!(error.code(), "model_catalogue.non_canonical_ref");
        assert!(canonical_model_ref("model:").is_err());
    }

    #[test]
    fn the_stale_form_migrates_and_the_migration_is_disclosed() {
        let (canonical, note) = migrate_model_ref("model/deepseek-v3").unwrap();
        assert_eq!(canonical.as_str(), "model:deepseek-v3");
        assert!(note.unwrap().contains("stale"));
        let (canonical, note) = migrate_model_ref("model:deepseek-v3").unwrap();
        assert_eq!(canonical.as_str(), "model:deepseek-v3");
        assert_eq!(note, None);
    }

    #[test]
    fn a_provider_native_id_resolves_to_the_catalogued_identity_not_to_itself() {
        let catalogue = ModelCatalogue::first_party_seed();
        let ollama = ProviderRef::parse("provider:ollama").unwrap();
        let (entry, route) = catalogue.claiming(&ollama, "llama3.2:latest").unwrap();
        assert_eq!(entry.model.as_str(), "model:llama3.2");
        assert_eq!(route.kind, ModelRouteKind::LocalServing);
        // The provider-native id is never the identity.
        assert_ne!(entry.model.as_str(), "llama3.2:latest");
    }

    #[test]
    fn an_unknown_provider_native_id_is_claimed_by_nobody() {
        let catalogue = ModelCatalogue::first_party_seed();
        let ollama = ProviderRef::parse("provider:ollama").unwrap();
        assert!(catalogue
            .claiming(&ollama, "some-model-nobody-catalogued:7b")
            .is_none());
    }

    #[test]
    fn the_same_native_id_under_a_different_provider_is_not_a_match() {
        let catalogue = ModelCatalogue::first_party_seed();
        let elsewhere = ProviderRef::parse("provider:somewhere-else").unwrap();
        assert!(catalogue.claiming(&elsewhere, "llama3.2:latest").is_none());
    }

    #[test]
    fn owner_entries_layer_over_the_seed_without_creating_a_second_identity() {
        let mut catalogue = ModelCatalogue::first_party_seed();
        let seeded = catalogue.len();
        let mut owner = ModelCatalogue::default();
        owner
            .insert(ModelCatalogueEntry {
                model: canonical_model_ref("model:llama3.2").unwrap(),
                name: "Llama 3.2 (owner)".into(),
                description: "owner-pinned".into(),
                superseded_refs: BTreeSet::new(),
                routes: vec![local(&["llama3.2:8b"])],
                source: SourceRef::parse("source/owner").unwrap(),
                freshness: None,
                book: None,
            })
            .unwrap();
        catalogue.extend(owner);
        assert_eq!(catalogue.len(), seeded, "layering must not fork identity");
        let ollama = ProviderRef::parse("provider:ollama").unwrap();
        assert!(catalogue.claiming(&ollama, "llama3.2:8b").is_some());
        assert!(catalogue.claiming(&ollama, "llama3.2:latest").is_none());
    }

    #[test]
    fn a_catalogue_entry_projects_to_a_model_resource_record() {
        let catalogue = ModelCatalogue::first_party_seed();
        let entry = catalogue
            .get(&canonical_model_ref("model:gpt-5.4").unwrap())
            .unwrap();
        let record = entry.resource_record();
        assert_eq!(record.descriptor.kind, ResourceKind::Model);
        assert_eq!(record.descriptor.id.as_str(), "model:gpt-5.4");
        assert_eq!(
            record.descriptor.sources[0].authority,
            Some(SourceAuthority::Authored)
        );
        // A catalogue record asserts identity, never a live route.
        assert!(record.providers.is_empty());
    }

    #[test]
    fn a_superseded_ref_still_resolves_to_the_current_identity() {
        let mut catalogue = ModelCatalogue::default();
        catalogue
            .insert(ModelCatalogueEntry {
                model: canonical_model_ref("model:renamed-now").unwrap(),
                name: "Renamed".into(),
                description: "identity correction".into(),
                superseded_refs: BTreeSet::from(["model:renamed-before".to_string()]),
                routes: Vec::new(),
                source: SourceRef::parse("source/owner").unwrap(),
                freshness: None,
                book: None,
            })
            .unwrap();
        let found = catalogue
            .get(&ResourceRef::parse("model:renamed-before").unwrap())
            .unwrap();
        assert_eq!(found.model.as_str(), "model:renamed-now");
    }

    fn book() -> OwnerModelBook {
        OwnerModelBook {
            source: "owner/model-book".into(),
            authored_at: "2026-09-19".into(),
            note: Some("the owner's own reading of this model".into()),
            class: Some(ModelClassFacets {
                family: Some("Claude 5 family".into()),
                generation: None,
                reasoning: Some("deliberate, long-horizon".into()),
                modalities: BTreeSet::from(["text".into(), "vision".into()]),
                facets: BTreeMap::new(),
            }),
            quirks: vec![ModelQuirk {
                claim: "under very long tool loops it drops the oldest constraint".into(),
                conditions: vec!["40+ tool calls in one session".into()],
                evidence: Some("session journal, 2026-09-12".into()),
                counterexample: None,
                workaround: Some("restate the constraint every 20 calls (cheap)".into()),
                standing: QuirkStanding::Observed,
                observed_at: "2026-09-12".into(),
                source: "owner session journal".into(),
                retest: Some("re-run the 40-call loop after the next provider snapshot".into()),
            }],
            use_for: vec!["implementation".into(), "review".into()],
            preference: Some(AuthoredPreference {
                rank: 5,
                note: Some("first pick for hard refactors".into()),
            }),
            exclusion: None,
        }
    }

    #[test]
    fn an_owner_book_round_trips_on_the_entry_and_the_seed_has_none() {
        let mut entry = entry("model:claude-opus-5", "Claude Opus 5", "d", vec![]);
        assert!(
            entry.book.is_none(),
            "the seed publishes identity, never judgement"
        );
        entry.book = Some(book());
        let text = serde_json::to_string(&entry).unwrap();
        let back: ModelCatalogueEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(back.book, Some(book()));
        assert!(!back.book.as_ref().unwrap().excluded());
    }

    #[test]
    fn a_book_record_without_source_or_date_is_refused() {
        let mut bad = book();
        bad.source = "  ".into();
        let error = bad.validate().unwrap_err();
        assert_eq!(error.code(), "model_book.invalid_record");
        assert!(error.message().contains("source"), "{}", error);
    }

    #[test]
    fn a_quirk_without_a_claim_is_refused_and_names_the_field() {
        let mut bad = book();
        bad.quirks[0].claim = "".into();
        let error = bad.validate().unwrap_err();
        assert!(error.message().contains("quirks[0].claim"), "{}", error);
    }

    #[test]
    fn an_exclusion_without_a_reason_is_a_mood_not_a_record() {
        let mut bad = book();
        bad.exclusion = Some(AuthoredExclusion {
            reason: " ".into(),
            since: "2026-09-19".into(),
        });
        let error = bad.validate().unwrap_err();
        assert!(error.message().contains("exclusion.reason"), "{}", error);
    }

    #[test]
    fn the_seed_is_canonical_throughout() {
        for entry in ModelCatalogue::first_party_seed().entries() {
            canonical_model_ref(entry.model.as_str()).unwrap();
            assert!(
                !entry.routes.is_empty(),
                "{} declares no route",
                entry.model
            );
            for route in &entry.routes {
                assert!(!route.provider_native_ids.is_empty());
                for native in &route.provider_native_ids {
                    assert!(
                        !native.starts_with("model:"),
                        "a provider-native id must never be spelled as a ModelRef"
                    );
                }
            }
        }
    }

    #[test]
    fn the_seed_carries_the_speech_surface_models_the_realtime_adapter_serves() {
        let catalogue = ModelCatalogue::first_party_seed();
        let openai = ProviderRef::parse("provider:openai").unwrap();
        for (model_ref, native_id) in [
            ("model:gpt-realtime", "gpt-realtime"),
            ("model:gpt-4o-transcribe", "gpt-4o-transcribe"),
            ("model:gpt-4o-mini-tts", "gpt-4o-mini-tts"),
        ] {
            let (entry, route) = catalogue
                .claiming(&openai, native_id)
                .unwrap_or_else(|| panic!("seed must declare {native_id}"));
            assert_eq!(entry.model.as_str(), model_ref);
            assert_eq!(route.kind, ModelRouteKind::ProviderNative);
            assert!(route.credential.requires_credential());
        }
    }
}
