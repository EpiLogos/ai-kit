---
register: episteme
---

# Model modality, interaction and transport surfaces

Status: canonical home for the generic modality seam after #317. It extends the Model field (see `15-MODEL-ROSTER-CAPABILITY-FIT.md`) with the facts a resolved body carries about what it can hear, say and do. It is **not** a voice ontology, a consumer's dialogue model, or a second capability registry.

## What the seam is

`aikit.model-modality/v1` (`aikit-core::model_modality`) is the contract one **model surface** declares about itself:

- **input/output modalities** — `text`, `audio` (raw audio in or out), `speech` (interactive voice);
- **transform capabilities** — `speech-to-text`, `text-to-speech`, `speech-to-speech`, `audio-understanding`, `multimodal-text-audio`;
- **interaction forms** — `request-response`, `streaming-input`, `streaming-output`, `full-duplex-realtime`, `structured-events`, `tool-requests`, `timestamps`, `partial-transcripts`, `final-transcripts`, `vad-turn-detection`, `barge-in`;
- **transport facts** — `in-process | cli | http | websocket | webrtc | sip | provider-native`, connection semantics (`stateless` or `connected` with a `reconnect` answer), credential scope (`bearer` or `ephemeral-surface-token`), availability, material constraints, and provider/provider-native-surface/revision provenance.

The contract rides on the existing `ModelSurfaceReading` seam (`model_surface.modality`, serde-optional, so older payloads load unchanged). A provider adapter fills it in; consumers — an agency constitution, a desktop client — read it.

## Four answers, never two

Every query (`input_support`, `output_support`, `transform_support`, `interaction_support`) answers with an explicit state:

| State | Meaning |
| --- | --- |
| `supported` | Declared and usable. |
| `degraded` | Usable with the provider's own stated reduction. |
| `unsupported` | Proven-absent: the surface's declaration (or the adapter's refusal) withholds it, and the reason names the surface. |
| `unknown` | Unproven: no modality contract was declared, or an opaque stage leaves the claim unprovable. |

Proven, degraded, proven-absent and unproven stay four different facts. Absence from a declared set is the explicit unsupported statement — the vocabulary is never silently flattened to what one provider happens to offer.

## Two body shapes, one resolver

Speech bodies resolve through the ordinary `HarnessComposition` resolver — no second resolution path exists:

- **Native realtime speech-to-speech**: one model-adapter component on a conversation surface; `composition.model` names the body's Model; `disclose_model_runtime` attaches the runtime relation with its modality contract.
- **Cascade (STT → text harness → TTS)**: three model-bearing components wired through ordinary Contract requirements and providers; `composition.model` stays unset; `disclose_staged_model_runtime` keeps one full model/provider/engine/materialisation relation **per stage** and derives the body-level view strictly.

A body-level interaction capability is carried only where every fully-declared stage carries it. One stage's explicit absence refutes the body claim and names the stage; an opaque stage (no modality contract — a plain text harness is legitimate) turns body-level claims into `unknown` rather than letting a resolution overclaim. Body input modalities are the first declared stage's inputs; body outputs are the last declared stage's outputs. Every derived answer carries a stage-named `basis`.

## Identity law

Component, provider, engine, materialisation and surface changes change facts and fingerprints. They never change Project, Agent, Agency, Harness or AgentSession identity — the same law the composition body already enforces, now proven for provider replacement inside a speech body. A model's structured tool request is an interaction capability: a channel on which proposals arrive, adjudicated by the caller through its own authority path. It cannot project itself onto a native Action surface (`RuntimeSurfaceReading.non_action_refs`, never `action_refs`).

## Provider adapters

`aikit-adapters::openai_realtime` is the first realtime adapter instance, deliberately an adapter: a frozen session fixture (`crates/aikit-adapters/tests/fixtures/openai-realtime/session.json`, pinned by `OPENAI_REALTIME_ADAPTER_REVISION`) is translated into the generic contract, and provider-private session configuration — voices, audio formats, tool definitions, instructions — is validated and dropped, so none of it can leak into generic types, logs or history. Unrecognised provider vocabulary is refused, never coerced. Declared STT/TTS surfaces (`transcription_surface`, `speech_synthesis_surface`) cover the cascade endpoints. The seed catalogue carries the matching Models (`model:gpt-realtime`, `model:gpt-4o-transcribe`, `model:gpt-4o-mini-tts`); route observation stays Actuation's join, and credential conditions stay ref/presence only.

## Honest degradation

- a required-but-unbound credential surfaces as a `modality-credential` unavailability in the read model;
- a degraded surface keeps its own reason and degrades every capability it carries;
- a roster demand for `speech` fails loudly against a text-only candidate (`modality:speech` gate) using the contract's flat tags (`modality_tags()`, `capability_tags()`).

## Explanations

`explain_model_modality` and `explain_staged_model_runtime` emit `ExplainEvidence` through the existing Explain/History seam: per-stage model relations with provider-native provenance, declared modalities per stage, the derived body basis, and every honest absence. Provider-native spellings travel in provenance only; canonical refs carry identity.

## Explicitly not built here

WebRTC and SIP transports (the vocabulary carries them; no adapter speaks them yet). Non-OpenAI realtime adapters. Partial-transcript and timestamp claims for any surface whose fixture does not prove them. No consumer semantics: Nara, Actuation agencies and desktop clients are readers of this seam, never its owners.
