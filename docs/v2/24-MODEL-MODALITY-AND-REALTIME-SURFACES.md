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

Component, provider, engine, materialisation and surface changes change facts and fingerprints. They never change Project, Agent, Agency, Harness or AgentSession identity — the same law the composition body already enforces, now proven for provider replacement inside a speech body. `diff_modality_contracts` produces the exact capability change across a replacement (`ModalityContractDelta`: gained/lost interaction, transforms, input/output modalities, availability movement, with basis lines). A model's structured tool request is an interaction capability: a channel on which proposals arrive, adjudicated by the caller through its own authority path. It cannot project itself onto a native Action surface (`RuntimeSurfaceReading.non_action_refs`, never `action_refs`).

## Provider adapters

`aikit-adapters::openai_realtime` is the first realtime adapter instance, deliberately an adapter: a frozen session fixture (`crates/aikit-adapters/tests/fixtures/openai-realtime/session.json`, pinned by `OPENAI_REALTIME_ADAPTER_REVISION`) is translated into the generic contract, and provider-private session configuration — voices, audio formats, tool definitions, instructions — is validated and dropped, so none of it can leak into generic types, logs or history. Unrecognised provider vocabulary is refused, never coerced. Declared STT/TTS surfaces (`transcription_surface`, `speech_synthesis_surface`) cover the cascade endpoints. The seed catalogue carries the matching Models (`model:gpt-realtime`, `model:gpt-4o-transcribe`, `model:gpt-4o-mini-tts`); route observation stays Actuation's join, and credential conditions stay ref/presence only.

`aikit-adapters::local_speech` is the second provider wire and the first non-cloud one: the same adapter pattern applied to a fully local STT/TTS pair (see the local speech stack section below). Its declared surfaces join the seed's local entries by the ordinary (provider, provider-native id) key, and both are credential `NotRequired`. Every adapter instance's inventory aggregates at `aikit_adapters::declared_model_surfaces()` — the one seam a new adapter instance joins by.

The frozen recording is session configuration only, and the adapter declares exactly what it proves: it proves `final-transcripts` (a transcription model is configured), but it carries no partial-transcript or timestamp vocabulary, so both stay undeclared — a future recording proves `partial-transcripts` when the recorded document itself carries the partial-delivery vocabulary (an input-audio-transcription delta/partial event type or an equivalent session configuration field), and proves `timestamps` when it carries timestamp-bearing transcription configuration or events. The local captures are held to the same law: they prove one committed transcript per request and one complete WAV per request, so no streaming or full-duplex form is declared, and the reason why travels as material constraint notes rather than by silence alone.

## Honest degradation

- a required-but-unbound credential surfaces as a `modality-credential` unavailability in the read model;
- a degraded surface keeps its own reason and degrades every capability it carries;
- a roster demand for `speech` fails loudly against a text-only candidate (`modality:speech` gate) using the contract's flat tags (`modality_tags()`, `capability_tags()`).

## Explanations

`explain_model_modality` and `explain_staged_model_runtime` emit `ExplainEvidence` through the existing Explain/History seam: per-stage model relations with provider-native provenance, declared modalities per stage, the derived body basis, and every honest absence. Provider-native spellings travel in provenance only; canonical refs carry identity.

## Reading the facts back: the CLI disclosure

`aikit model-modality show --document <read-model.json>` is the machine user's read over a resolved body: it takes a `aikit.model-runtime/v1` or `aikit.model-stage-runtime/v1` read model (document-in), and returns a `aikit.model-modality-disclosure/v1` document (document-out) carrying the resolved identity, the surface and per-stage facts, the four-state answer for **every** vocabulary member (absence is a rendered unsupported/unknown answer with its reason, never a missing key), the stage-named basis for staged bodies, and the explanation evidence. It re-resolves nothing and touches no network.

## Seeing the class before any key exists: the catalogue listing

`aikit model-catalogue show` is where speech is visible as a **class of model**, before any provider credential exists. The listing joins the resolved catalogue against every adapter instance's declared surfaces (`aikit_adapters::declared_model_surfaces`, each stating its credential condition) and this machine's non-revoked credential binding records, and per model discloses:

- the declared modality class facts — input/output modalities, transforms, interaction forms, transport, speech capability — whenever a declared surface joins the entry's routes (a plain text model carries none, and nothing is claimed for it);
- the credential condition presence-resolved (ref/presence only — a binding record has no secret field, so no secret can enter a listing);
- honest availability: `catalogued` (the option exists; no gating fact at this plane), `credential-gated` with the absent credential **named** from the declaring surface's own hint, `degraded` with the declarer's reason, or `unavailable` with the declarer's reason;
- each declared route with its endpoint — so a non-standard path (the local STT service's `/inference`) is route-standing fact in the listing, never hidden;
- a top-level `classes.speech` index — membership derived from declared facts (`carries_speech`), never from a consumer knowing model names.

Declared is still not observed: route observation stays the `compose` join, and a listing entry never claims a route was seen. A missing credential deliberately outranks a degradation while it is missing — the listing answers what stands between the caller and the surface. The visible contrast this produces on a keyless machine is the point: the cloud speech entries read `credential-gated` with the key named, while the local speech entries read `catalogued` with credential `not-required` — the same class, two honest states.

## Swapping models is a data-and-adapter act, never a core change

The generic contract (`aikit.model-modality/v1`) does not know any model or provider name. Swapping in a better model is therefore:

- **same provider, better model**: one new catalogue seed entry (or owner entry) plus the adapter's recorded session fixture (or one new declared-surface function following `transcription_surface`/`speech_synthesis_surface`). Zero core changes; the listing, the disclosure, and the class index pick it up by the ordinary (provider, provider-native id) join.
- **new provider wire**: one adapter instance following the `openai_realtime` pattern — recorded fixture or declared surfaces in, generic `ModelModalityContract` out, provider-private vocabulary validated and dropped, `declared_surfaces()` the inventory the catalogue joins against. The contract, the read models and the catalogue join are untouched.

No consumer may branch on model or provider names: the audit rule is that names live only in the adapter instances and the catalogue data, and every consumer question is answered through the vocabulary or the join keys.

## The local speech stack: the swap path's worked example

The second provider wire is fully local and needs no key. What is hosted, and how, is recorded outside this repository: `/Users/admin/.local-speech/README.md` hosts and runs the two services (`start.sh` / `stop.sh`), holds the models and their licences, and carries the latency measurements. In brief: whisper.cpp's `whisper-server` (large-v3-turbo q5_0, Metal) for speech-to-text and a Kokoro-82M ONNX wrapper for text-to-speech, both stateless request/response HTTP on this machine.

Endpoint shapes, as the adapter declares them:

- **STT** — `POST http://127.0.0.1:8080/inference`, multipart form (`file=<16 kHz mono WAV>`, `response_format=json`), reply `{"text": ...}`. The body is OpenAI-shaped; the path is not: this whisper.cpp build 404s on `/v1/audio/transcriptions` and serves `/inference`. The override is carried openly — as the catalogue route's endpoint, and in the surface's constraint notes — never hidden behind an OpenAI-compatible pretence.
- **TTS** — `POST http://127.0.0.1:8880/v1/audio/speech`, OpenAI-compatible body (`model`, `input`, `voice`, `response_format`, `speed`), reply is one complete WAV (24 kHz Int16 mono); only `response_format: "wav"` is served. `voice` and `speed` are provider-private request configuration: validated and dropped by the adapter.

The seed entries `model:local-whisper-large-v3-turbo` (provider `local-whisper-cpp`) and `model:kokoro-82m` (provider `local-kokoro`) are catalogued with `LocalServing` routes and credential `NotRequired`; the adapter instance `aikit-adapters::local_speech` declares both surfaces from verbatim frozen captures (`tests/fixtures/local-speech/`, pinned `fixture:local-speech-captures/2026-09-19`, with provenance in that directory). A fully local cascade body — local whisper STT stage, local text harness stage, local kokoro TTS stage — resolves through `disclose_staged_model_runtime` with one full relation per stage, local materialisation, `speech_capable: true`, and the strict body-level answers: `request-response` supported (every stage carries it), `full-duplex-realtime` and `streaming-output` unsupported with reasons (these servers return one complete response per request and do not stream). Each surface also resolves as a single body with its honest half-capability: the STT body listens and cannot speak; the TTS body speaks and cannot listen.

**Bring-your-own is the same path.** The local stack is not a special case; it is the worked example of the swap rule above. A user replacing it with any OpenAI-compatible STT/TTS service changes data and an adapter instance only: new catalogue entries (or owner entries) with the new provider and endpoint, plus a declared-surface function per service following `parse_transcription_capture`/`parse_synthesis_capture` — for a service speaking the standard `/v1/audio/transcriptions` path, only the endpoint constant differs. A service with a genuinely different wire gets a new adapter instance that parses its recorded documents into the same generic contract. In every case the contract, the read models, the staged composition and the catalogue join are untouched, and the four-state answers move honestly with the new declarer.

## Consumers reduce the raw relation

A consumer building an Actuation constitution from AIKit resolution must **reduce** the raw model relation before submission: AIKit's shape owners here are `ModelRuntimeRelation` and `ModelStageRelation` (the `aikit.model-runtime/v1` and `aikit.model-stage-runtime/v1` documents in `aikit-core::model_runtime`), and the raw relation includes the modality contract's `credential` field, which Actuation's admission secret-scan refuses. The Actuation-admitted shapes are its own speech-constitution admission grammar's `model_relation` and `access_profile`; mapping the AIKit relation into those admitted shapes — dropping what admission refuses — is the consumer's reduction duty, not a leniency expected of admission. AIKit owns the raw shapes and their honesty; Actuation owns the admitted shapes and their refusal; neither owns the other's.

## Explicitly not built here

WebRTC and SIP transports (the vocabulary carries them; no adapter speaks them yet). Non-OpenAI *realtime* adapters (the local speech wire is request/response, not realtime). Partial-transcript and timestamp claims for any surface whose fixture does not prove them. No consumer semantics: Nara, Actuation agencies and desktop clients are readers of this seam, never its owners.
