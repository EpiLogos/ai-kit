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

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model_modality::{
    InteractionCapability, ModelModality, ModelModalityContract, TransformCapability, TransportKind,
};
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
    }
}

/// The seed's speech entries (`model:gpt-realtime`, `model:gpt-4o-transcribe`,
/// `model:gpt-4o-mini-tts`) are how speech is visible as a class of model in
/// every listing. Swapping in a better model never touches the generic
/// modality contract: a same-provider replacement is one new entry here plus
/// the adapter's recorded session fixture — zero core changes; a new provider
/// wire is one adapter instance following the `openai_realtime` pattern,
/// whose surfaces this catalogue already knows how to join by
/// (provider, provider-native id).
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
        })?;
    }
    Ok(catalogue)
}

// ---------------------------------------------------------------------------
// Catalogue modality disclosure
// ---------------------------------------------------------------------------

/// The declared modality class facts one adapter-declared surface contributes
/// to a catalogued Model. This is the read a listing needs to show speech as
/// a class of model: what the surface hears and says, what it converts,
/// which interaction forms it offers, and the transport it is reached over —
/// exactly the facts the surface's [`ModelModalityContract`] declares, with
/// the credential condition presence-resolved (ref/presence only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueModalityClass {
    pub input_modalities: BTreeSet<ModelModality>,
    pub output_modalities: BTreeSet<ModelModality>,
    /// Declared positive transform supports; an absent capability is the
    /// surface's explicit unsupported fact, never a flattening.
    pub transforms: BTreeSet<TransformCapability>,
    /// Declared interaction capabilities.
    pub interaction: BTreeSet<InteractionCapability>,
    pub transport: TransportKind,
    /// Interactive speech in both directions on this surface.
    pub speech_capable: bool,
    pub provider: ProviderRef,
    pub provider_native_surface: String,
    /// The surface's credential condition, presence-resolved against the
    /// bound refs the reader supplied. Presence and refs only — a secret has
    /// no representation here.
    pub credential: CredentialCondition,
    /// Where the declared facts came from (adapter fixture revisions,
    /// documentation pins), verbatim from the declaring surface.
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Honest availability of a catalogued option at the catalogue plane. The
/// classes keep three facts apart that a listing must never collapse: the
/// option exists (catalogued), the option is real but a named credential is
/// absent (credential-gated), the option is real but reduced (degraded). A
/// surface that declares itself not offered stays its own state rather than
/// being flattened into any of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CatalogueAvailability {
    /// Catalogued identity with nothing gating it at this plane: either no
    /// modality surface declared for it (a plain text model) or the joined
    /// surface's facts hold with its credential bound. This is never a claim
    /// that a route was observed — route observation stays the compose join.
    Catalogued,
    /// A declared surface joins this Model and its credential is not bound
    /// on this machine. The option is visible and unusable until the named
    /// credential is bound; `missing` is the declaring surface's own hint
    /// for which credential that is.
    CredentialGated {
        missing: String,
    },
    /// The declared surface is offered in a reduced form; the reason is the
    /// declarer's own words.
    Degraded {
        reason: String,
    },
    /// The declared surface states itself not offered.
    Unavailable {
        reason: String,
    },
}

impl CatalogueAvailability {
    /// How restrictive a state is, for combining several joined surfaces
    /// into one entry-level answer: the most restrictive surface wins, so a
    /// listing never reads more usable than its least usable surface.
    fn restrictiveness(&self) -> u8 {
        match self {
            Self::Unavailable { .. } => 0,
            Self::CredentialGated { .. } => 1,
            Self::Degraded { .. } => 2,
            Self::Catalogued => 3,
        }
    }
}

/// One catalogued Model's disclosure: identity plus every declared modality
/// class joined to its routes, plus the entry-level availability answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueModelDisclosure {
    pub model: ResourceRef,
    pub name: String,
    pub description: String,
    pub source: SourceRef,
    /// One entry per distinct adapter-declared surface joined through this
    /// entry's routes. Empty when no surface declares this Model — which is
    /// itself the honest "nothing is known here" fact.
    pub modality_classes: Vec<CatalogueModalityClass>,
    pub availability: CatalogueAvailability,
}

impl CatalogueModelDisclosure {
    /// Whether any joined surface declares speech in either direction. This
    /// is the class membership test: speech is visible as a class of model
    /// because membership is derived from declared facts, never from a
    /// consumer knowing model names.
    pub fn carries_speech(&self) -> bool {
        self.modality_classes.iter().any(|class| {
            class.input_modalities.contains(&ModelModality::Speech)
                || class.output_modalities.contains(&ModelModality::Speech)
        })
    }
}

/// The speech class over a set of disclosures: the catalogued Models a
/// listing should show under "speech", in catalogue order.
pub fn speech_class_models(disclosures: &[CatalogueModelDisclosure]) -> Vec<ResourceRef> {
    disclosures
        .iter()
        .filter(|disclosure| disclosure.carries_speech())
        .map(|disclosure| disclosure.model.clone())
        .collect()
}

/// Join the resolved catalogue against adapter-declared surfaces and the
/// machine's bound credential refs. Pure: catalogue entries and declared
/// surfaces are the recorded facts, `bound_credential_refs` is the reader's
/// presence evidence (the credential binding store's non-revoked refs); no
/// re-resolution, no network, no secret material.
///
/// The join key is (route provider, provider-native id) on both sides — the
/// catalogue's own claim key — so nothing here branches on a model or
/// provider name. A declared surface no entry claims appears nowhere: it is
/// not a Model, and inventing an entry for it would mint identity.
pub fn disclose_catalogue_modalities(
    catalogue: &ModelCatalogue,
    declared_surfaces: &[ModelModalityContract],
    bound_credential_refs: &BTreeSet<String>,
) -> Vec<CatalogueModelDisclosure> {
    catalogue
        .entries()
        .map(|entry| {
            let mut classes: Vec<CatalogueModalityClass> = Vec::new();
            let mut availability = CatalogueAvailability::Catalogued;
            for route in &entry.routes {
                for surface in declared_surfaces {
                    if &surface.provider != &route.provider || !route.claims(&surface.provider_native_surface)
                    {
                        continue;
                    }
                    let already_joined = classes.iter().any(|class| {
                        class.provider == surface.provider
                            && class.provider_native_surface == surface.provider_native_surface
                    });
                    if already_joined {
                        continue;
                    }
                    classes.push(class_from_surface(surface, bound_credential_refs));
                    let surface_state = availability_from_surface(surface, bound_credential_refs);
                    if surface_state.restrictiveness() < availability.restrictiveness() {
                        availability = surface_state;
                    }
                }
            }
            CatalogueModelDisclosure {
                model: entry.model.clone(),
                name: entry.name.clone(),
                description: entry.description.clone(),
                source: entry.source.clone(),
                modality_classes: classes,
                availability,
            }
        })
        .collect()
}

fn class_from_surface(
    surface: &ModelModalityContract,
    bound_credential_refs: &BTreeSet<String>,
) -> CatalogueModalityClass {
    CatalogueModalityClass {
        input_modalities: surface.input_modalities.clone(),
        output_modalities: surface.output_modalities.clone(),
        transforms: surface.transforms.keys().copied().collect(),
        interaction: surface.interaction.iter().copied().collect(),
        transport: surface.transport,
        speech_capable: surface.is_speech_capable(),
        provider: surface.provider.clone(),
        provider_native_surface: surface.provider_native_surface.clone(),
        credential: crate::credential_world::resolve_credential_presence(
            &surface.credential,
            &surface.provider,
            bound_credential_refs,
        ),
        provenance: surface.provenance.clone(),
    }
}

/// One surface's availability answer. The credential gap outranks a
/// degradation deliberately: the question the listing answers is "what
/// stands between me and using this", and while the credential is absent
/// that is the gap — the reduction becomes the visible fact as soon as the
/// credential is bound.
fn availability_from_surface(
    surface: &ModelModalityContract,
    bound_credential_refs: &BTreeSet<String>,
) -> CatalogueAvailability {
    let credential = crate::credential_world::resolve_credential_presence(
        &surface.credential,
        &surface.provider,
        bound_credential_refs,
    );
    if let CredentialCondition::Required { hint } = &credential {
        return CatalogueAvailability::CredentialGated {
            missing: hint.clone(),
        };
    }
    match &surface.availability {
        crate::model_modality::SurfaceAvailability::Available => CatalogueAvailability::Catalogued,
        crate::model_modality::SurfaceAvailability::Degraded { reason } => {
            CatalogueAvailability::Degraded {
                reason: reason.clone(),
            }
        }
        crate::model_modality::SurfaceAvailability::Unavailable { reason } => {
            CatalogueAvailability::Unavailable {
                reason: reason.clone(),
            }
        }
    }
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
            })
            .unwrap();
        let found = catalogue
            .get(&ResourceRef::parse("model:renamed-before").unwrap())
            .unwrap();
        assert_eq!(found.model.as_str(), "model:renamed-now");
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

    // -- catalogue modality disclosure -------------------------------------

    use crate::model_modality::{
        ConnectionSemantics, DeclaredSupport, InteractionCapability, ReconnectSupport,
        SurfaceAvailability,
    };

    /// A declared speech-to-speech surface, shaped like an adapter instance
    /// would declare it, joined by the seed's own (provider, native id) key.
    fn declared_surface(
        provider: &str,
        native: &str,
        inputs: &[ModelModality],
        outputs: &[ModelModality],
        transforms: &[TransformCapability],
        credential: CredentialCondition,
    ) -> ModelModalityContract {
        let mut contract = ModelModalityContract::new(
            ProviderRef::parse(provider).unwrap(),
            native,
        );
        contract.input_modalities = inputs.iter().copied().collect();
        contract.output_modalities = outputs.iter().copied().collect();
        for transform in transforms {
            contract
                .transforms
                .insert(*transform, DeclaredSupport::Supported);
        }
        contract.interaction = BTreeSet::from([
            InteractionCapability::StreamingInput,
            InteractionCapability::StreamingOutput,
        ]);
        contract.transport = crate::model_modality::TransportKind::WebSocket;
        contract.connection = ConnectionSemantics::Connected {
            reconnect: ReconnectSupport::ReconnectWithoutSession,
        };
        contract.credential = credential;
        contract
            .provenance
            .push("fixture:test/declared-surface".into());
        contract
    }

    fn realtime_seed_surface(credential: CredentialCondition) -> ModelModalityContract {
        declared_surface(
            "provider:openai",
            "gpt-realtime",
            &[ModelModality::Audio, ModelModality::Speech, ModelModality::Text],
            &[ModelModality::Audio, ModelModality::Speech, ModelModality::Text],
            &[TransformCapability::SpeechToSpeech],
            credential,
        )
    }

    fn openai_bound() -> BTreeSet<String> {
        BTreeSet::from(["credential:openai".to_string()])
    }

    fn disclosure_for(
        surfaces: &[ModelModalityContract],
        bound: &BTreeSet<String>,
        model: &str,
    ) -> CatalogueModelDisclosure {
        let catalogue = ModelCatalogue::first_party_seed();
        disclose_catalogue_modalities(&catalogue, surfaces, bound)
            .into_iter()
            .find(|disclosure| disclosure.model.as_str() == model)
            .unwrap_or_else(|| panic!("the seed catalogues {model}"))
    }

    #[test]
    fn a_keyless_speech_model_is_a_visible_option_with_its_gap_named() {
        let surface = realtime_seed_surface(CredentialCondition::Required {
            hint: "openai realtime credential".into(),
        });
        let disclosure = disclosure_for(&[surface], &BTreeSet::new(), "model:gpt-realtime");

        // The class is visible: modalities, transforms, interaction, transport.
        assert_eq!(disclosure.modality_classes.len(), 1);
        let class = &disclosure.modality_classes[0];
        assert!(class.speech_capable);
        assert!(class.input_modalities.contains(&ModelModality::Speech));
        assert!(class.output_modalities.contains(&ModelModality::Speech));
        assert!(class.transforms.contains(&TransformCapability::SpeechToSpeech));
        assert!(class
            .interaction
            .contains(&InteractionCapability::StreamingInput));
        assert_eq!(class.provider_native_surface, "gpt-realtime");

        // The gap is named, from the declaring surface's own hint.
        match &disclosure.availability {
            CatalogueAvailability::CredentialGated { missing } => {
                assert_eq!(missing, "openai realtime credential");
            }
            other => panic!("a keyless surface must read credential-gated, got {other:?}"),
        }
        assert!(matches!(
            class.credential,
            CredentialCondition::Required { .. }
        ));
        // And membership in the speech class is derived, not declared.
        assert!(disclosure.carries_speech());
    }

    #[test]
    fn a_bound_credential_resolves_the_same_option_to_catalogued() {
        let surface = realtime_seed_surface(CredentialCondition::Required {
            hint: "openai realtime credential".into(),
        });
        let disclosure = disclosure_for(&[surface], &openai_bound(), "model:gpt-realtime");
        assert_eq!(disclosure.availability, CatalogueAvailability::Catalogued);
        match &disclosure.modality_classes[0].credential {
            CredentialCondition::Satisfied { binding_ref, .. } => {
                assert_eq!(binding_ref, "credential:openai");
            }
            other => panic!("a bound credential must read satisfied, got {other:?}"),
        }
    }

    #[test]
    fn an_entry_with_no_joined_surface_stays_catalogued_with_nothing_claimed() {
        let disclosure = disclosure_for(&[], &openai_bound(), "model:llama3.2");
        assert!(disclosure.modality_classes.is_empty());
        assert_eq!(disclosure.availability, CatalogueAvailability::Catalogued);
        assert!(!disclosure.carries_speech());
    }

    #[test]
    fn degraded_and_unavailable_surfaces_keep_their_own_words() {
        let mut degraded = realtime_seed_surface(CredentialCondition::NotRequired);
        degraded.availability = SurfaceAvailability::Degraded {
            reason: "region failover active".into(),
        };
        let disclosure = disclosure_for(
            std::slice::from_ref(&degraded),
            &openai_bound(),
            "model:gpt-realtime",
        );
        assert_eq!(
            disclosure.availability,
            CatalogueAvailability::Degraded {
                reason: "region failover active".into()
            }
        );

        let mut gone = realtime_seed_surface(CredentialCondition::NotRequired);
        gone.availability = SurfaceAvailability::Unavailable {
            reason: "provider decommitted the surface".into(),
        };
        let disclosure = disclosure_for(
            std::slice::from_ref(&gone),
            &openai_bound(),
            "model:gpt-realtime",
        );
        assert_eq!(
            disclosure.availability,
            CatalogueAvailability::Unavailable {
                reason: "provider decommitted the surface".into()
            }
        );
    }

    #[test]
    fn a_missing_credential_outranks_a_degradation_while_it_is_missing() {
        let mut degraded = realtime_seed_surface(CredentialCondition::Required {
            hint: "openai realtime credential".into(),
        });
        degraded.availability = SurfaceAvailability::Degraded {
            reason: "region failover active".into(),
        };
        let disclosure = disclosure_for(
            std::slice::from_ref(&degraded),
            &BTreeSet::new(),
            "model:gpt-realtime",
        );
        // The gap is what stands between the caller and the surface; the
        // reduction becomes the visible fact once the credential is bound.
        assert!(matches!(
            disclosure.availability,
            CatalogueAvailability::CredentialGated { .. }
        ));
        let disclosure = disclosure_for(std::slice::from_ref(&degraded), &openai_bound(), "model:gpt-realtime");
        assert_eq!(
            disclosure.availability,
            CatalogueAvailability::Degraded {
                reason: "region failover active".into()
            }
        );
    }

    #[test]
    fn the_speech_class_is_derived_membership_across_the_whole_listing() {
        let surfaces = vec![
            realtime_seed_surface(CredentialCondition::Required {
                hint: "openai realtime credential".into(),
            }),
            declared_surface(
                "provider:openai",
                "gpt-4o-transcribe",
                &[ModelModality::Audio, ModelModality::Speech],
                &[ModelModality::Text],
                &[TransformCapability::SpeechToText],
                CredentialCondition::Required {
                    hint: "openai transcription credential".into(),
                },
            ),
            declared_surface(
                "provider:openai",
                "gpt-4o-mini-tts",
                &[ModelModality::Text],
                &[ModelModality::Audio, ModelModality::Speech],
                &[TransformCapability::TextToSpeech],
                CredentialCondition::Required {
                    hint: "openai speech credential".into(),
                },
            ),
        ];
        let catalogue = ModelCatalogue::first_party_seed();
        let disclosures = disclose_catalogue_modalities(&catalogue, &surfaces, &BTreeSet::new());
        let speech = speech_class_models(&disclosures);
        assert_eq!(
            speech
                .iter()
                .map(|model| model.as_str())
                .collect::<Vec<_>>(),
            vec![
                "model:gpt-4o-mini-tts",
                "model:gpt-4o-transcribe",
                "model:gpt-realtime",
            ],
            "exactly the seed's speech models, in catalogue order"
        );
        // Every entry stayed in the listing — nothing silently dropped.
        assert_eq!(disclosures.len(), catalogue.len());
    }

    #[test]
    fn a_surface_no_entry_claims_appears_nowhere() {
        let orphan = declared_surface(
            "provider:unheard-of",
            "phantom-model",
            &[ModelModality::Speech],
            &[ModelModality::Speech],
            &[TransformCapability::SpeechToSpeech],
            CredentialCondition::NotRequired,
        );
        let catalogue = ModelCatalogue::first_party_seed();
        let disclosures = disclose_catalogue_modalities(&catalogue, &[orphan], &BTreeSet::new());
        assert_eq!(disclosures.len(), catalogue.len());
        assert!(speech_class_models(&disclosures).is_empty());
        assert!(disclosures
            .iter()
            .all(|disclosure| disclosure.modality_classes.is_empty()));
    }
}
