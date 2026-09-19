# Local speech captures — provenance

These are verbatim request/response captures from the fully local speech
stack running on this development machine (Apple M4, macOS). They are the
frozen truth the `aikit-adapters::local_speech` adapter instance is conformed
against. Capture date: **2026-09-19**.

Revision pin used by the adapter: `fixture:local-speech-captures/2026-09-19`.

## Services captured

- **STT** — whisper.cpp 1.8.2 `whisper-server` (Homebrew, MIT), model
  `ggml-large-v3-turbo-q5_0` (Whisper large-v3-turbo q5_0, 547 MB, Metal).
  `POST http://127.0.0.1:8080/inference`, multipart form
  (`file=<16 kHz mono WAV>`, `response_format=json`), reply `{"text": ...}`.
  Note the route delta: this build does **not** serve the OpenAI path
  `/v1/audio/transcriptions` (it 404s); the body is OpenAI-shaped, the path
  is `/inference`. The path override is a declared route fact, never hidden.
- **TTS** — Kokoro-82M v1.0 (Apache-2.0, 310 MB fp32 ONNX, onnxruntime CPU)
  behind a FastAPI OpenAI-shape wrapper.
  `POST http://127.0.0.1:8880/v1/audio/speech`, JSON body
  (`model`, `input`, `voice`, `response_format`, `speed`), reply is one
  complete WAV (24 kHz Int16 mono); only `response_format: "wav"` is
  supported. No streaming, no keys, stateless per request.

## Files

- `whisper-stt-response.json` — verbatim STT reply for the 7.02 s sample
  `sample-16k.wav` ("The local voice is online. ...").
- `kokoro-tts-request.json` — verbatim TTS request body for
  "Nara here — the local voice is online." (voice `af_heart` — a
  provider-private fact the adapter validates and drops).
- `roundtrip-stt-response.json` — verbatim STT reply for the TTS output fed
  back through the STT service: the two local services compose.

## Hosting and reproduction

The services are hosted outside this repository; run/start instructions and
latency measurements live in `/Users/admin/.local-speech/README.md`
(`start.sh` / `stop.sh`). The adapter's declared facts come from these
captures, never from a live probe; tests here run from the frozen files
only.
