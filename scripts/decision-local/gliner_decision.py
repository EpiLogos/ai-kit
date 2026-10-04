"""Offline GLiNER discriminator behind AIKit's existing endpoint transport.

This route supports Noul and Choice questions used by the QL comparison.
Every question is a native softmax head in one packed inference. The
QL frame, semantic labels, thresholding and admission stay outside this owner.
The service runs in the foreground; Workcell owns its process and material.
"""
import argparse
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


MAX_BYTES = 1024 * 1024
PROTOCOL = "systemone-compatible/v1"
MODEL_FILES = {"config.json", "encoder_config/config.json", "model.safetensors", "tokenizer.json",
               "tokenizer_config.json", "special_tokens_map.json"}


class Refused(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Refused(message)


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"),
                      allow_nan=False).encode("utf-8")


def decode(data):
    require(len(data) <= MAX_BYTES, "request exceeds 1 MiB")

    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result

    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: require(False, "non-finite JSON: " + value))


def identifier(value):
    return (isinstance(value, str) and 0 < len(value.encode("utf-8")) <= 256
            and not any(ord(character) < 32 or 127 <= ord(character) < 160 for character in value))


def entry(value):
    return value is None or isinstance(value, (str, dict, list))


def plan_request(request, model_id, label_rendering="canonical"):
    require(label_rendering in ("canonical", "criteria"), "unsupported label rendering")
    require(isinstance(request, dict) and set(request) == {"model", "state", "questions"},
            "expected native model/state/questions request")
    require(identifier(request["model"]) and request["model"] == model_id,
            "requested model differs from the installed artifact")
    require(isinstance(request["state"], (str, dict, list)), "invalid shared native state")
    questions = request["questions"]
    require(isinstance(questions, dict) and 1 <= len(questions) <= 256
            and all(identifier(key) for key in questions), "expected 1..256 native questions")
    require(len(canonical(request)) <= MAX_BYTES, "encoded request exceeds 1 MiB")
    tasks, bindings = {}, {}
    for index, (key, question) in enumerate(questions.items()):
        require(isinstance(question, dict) and set(question) <= {"type", "instructions", "criteria"}
                and question.get("type") in ("noul", "choice"), "this route admits Noul and Choice questions")
        instructions = question.get("instructions")
        require(entry(instructions), "invalid question instructions")
        criteria = question.get("criteria")
        if question["type"] == "noul":
            require(criteria is None or (isinstance(criteria, dict)
                    and set(criteria) <= {"true", "false"} and all(entry(v) for v in criteria.values())),
                    "Noul criteria may describe only true and false")
            labels = ("true", "false")
        else:
            require(isinstance(criteria, dict) and 1 <= len(criteria) <= 255
                    and all(identifier(label) and entry(value) for label, value in criteria.items()),
                    "Choice requires 1..255 described native criteria")
            labels = tuple(criteria)
        descriptions = {}
        for label in labels:
            criterion = (criteria or {}).get(label)
            if label_rendering == "criteria" and criterion is not None:
                descriptions[label] = criterion if isinstance(criterion, str) else canonical(criterion).decode("utf-8")
            else:
                descriptions[label] = canonical({"question": instructions,
                    "outcome": label, "criteria": criterion}).decode("utf-8")
        # threshold zero returns ALL actual native softmax probabilities. It
        # avoids GLiNER's best-label fallback while retaining its own logits.
        task_id = "decision_" + str(index)
        bindings[task_id] = key
        tasks[task_id] = {"labels": descriptions, "multi_label": True,
                          "cls_threshold": 0.0, "class_act": "softmax", "native_type": question["type"]}
    state = request["state"]
    if label_rendering == "criteria":
        # Retain each instruction once in the shared input, with its exact
        # owner/provider question identity. Label criteria remain verbatim.
        state = {"state": state, "questions": [
            {"id": key, "instructions": question.get("instructions")}
            for key, question in questions.items()]}
    return canonical(state).decode("utf-8"), tasks, bindings


def native_answer(raw, question_ids, model_id, input_tokens, elapsed_ms, tasks=None):
    require(isinstance(raw, dict) and set(raw) == set(question_ids),
            "native classifier must cover exactly the packed questions")
    require(type(input_tokens) is int and input_tokens > 0, "actual encoder token usage required")
    require(math.isfinite(elapsed_ms) and elapsed_ms >= 0, "invalid actual inference latency")
    answers = {}
    for key in question_ids:
        pairs = raw[key]
        labels = tuple(tasks[key]["labels"]) if tasks is not None else ("true", "false")
        require(isinstance(pairs, (list, tuple)) and len(pairs) == len(labels),
                "every native softmax probability required")
        scores = {}
        for pair in pairs:
            require(isinstance(pair, (list, tuple)) and len(pair) == 2,
                    "invalid native label/probability pair")
            label, score = pair
            require(label in labels and label not in scores,
                    "duplicate or foreign native label")
            require(isinstance(score, (int, float)) and not isinstance(score, bool)
                    and math.isfinite(score) and 0 <= score <= 1, "invalid native probability")
            scores[label] = score
        require(abs(sum(scores.values()) - 1) <= 0.00001,
                "native softmax probabilities do not sum to one")
        # The task's protocol kind is separate from its label spellings: a
        # native Choice may legitimately name options true and false.
        kind = tasks[key].get("native_type", "noul") if tasks is not None else "noul"
        if kind == "noul":
            answers[key] = {"type": "noul", "noul": scores["true"]}
        else:
            chosen = max(labels, key=lambda label: scores[label])
            answers[key] = {"type": "choice", "choice": chosen,
                            "probabilities": scores, "confidence": scores[chosen]}
    # Encoder classification emits no generated tokens. Input usage is the
    # actual unpadded schema+state token sequence passed to the encoder.
    return {"model": model_id, "answers": answers,
            "usage": {"input_tokens": input_tokens, "output_tokens": 0}, "latency_ms": elapsed_ms}


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(MAX_BYTES), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_material(path):
    manifest = decode(path.read_bytes())
    require(isinstance(manifest, dict) and manifest.get("schema") == "central.material-manifest/v1"
            and manifest.get("recipe") == "gliner2.5-decide", "GLiNER material manifest required")
    require(identifier(manifest.get("model_id")), "installed model identity required")
    require(isinstance(manifest.get("artifact_repo"), str) and bool(manifest["artifact_repo"].strip())
            and isinstance(manifest.get("sdk_repo"), str) and bool(manifest["sdk_repo"].strip())
            and isinstance(manifest.get("license"), str) and bool(manifest["license"].strip()),
            "complete artifact/source/license provenance required")
    require(all(isinstance(manifest.get(key), str) and bool(manifest[key].strip())
                for key in ("snapshot", "sdk_directory")), "explicit material paths required")
    for key in ("artifact_revision", "sdk_revision"):
        value = manifest.get(key)
        require(isinstance(value, str) and len(value) == 40
                and all(c in "0123456789abcdef" for c in value), "exact " + key + " required")
    snapshot = Path(manifest["snapshot"]).resolve(strict=True)
    sdk = Path(manifest["sdk_directory"]).resolve(strict=True)
    artifacts = manifest.get("artifacts")
    require(isinstance(artifacts, dict) and bool(artifacts), "installed artifact digests required")
    for name, basis in artifacts.items():
        relative = PurePosixPath(name)
        require(not relative.is_absolute() and ".." not in relative.parts,
                "artifact paths must be relative")
        require(isinstance(basis, dict) and set(basis) == {"bytes", "sha256"}, "artifact basis shape")
        require(type(basis["bytes"]) is int and basis["bytes"] > 0
                and isinstance(basis["sha256"], str) and len(basis["sha256"]) == 64
                and all(c in "0123456789abcdef" for c in basis["sha256"]), "exact artifact size/digest required")
        artifact = snapshot / relative
        require(artifact.is_file() and artifact.stat().st_size == basis["bytes"]
                and sha256(artifact) == basis["sha256"], "installed artifact changed: " + name)
    require(MODEL_FILES <= set(artifacts), "incomplete native model material")
    actual = {p.relative_to(snapshot).as_posix() for p in snapshot.rglob("*") if p.is_file()
              and ".cache" not in p.relative_to(snapshot).parts}
    require(actual == set(artifacts), "unattributed native model material")
    revision = subprocess.check_output(["git", "-C", str(sdk), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(sdk), "status", "--porcelain", "--",
                                     "gliner2", "pyproject.toml"], text=True)
    require(revision == manifest["sdk_revision"] and not dirty, "installed SDK source differs from pin")
    return manifest, snapshot, sdk


class Discriminator:
    def __init__(self, manifest_path, device, max_encoder_tokens, label_rendering="canonical", cpu_threads=2):
        require(label_rendering in ("canonical", "criteria"), "unsupported label rendering")
        require(type(max_encoder_tokens) is int and 1 <= max_encoder_tokens <= 32768,
                "explicit bounded encoder token ceiling required")
        started = time.monotonic()
        manifest_digest = sha256(manifest_path)
        manifest, snapshot, sdk = verify_material(manifest_path)
        # Serving never downloads missing artifacts. The material recipe is a
        # separate operation executed by the Workcell material owner.
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        import torch
        require(type(cpu_threads) is int and 1 <= cpu_threads <= 8,
                "bounded CPU thread count required")
        if device == "cpu":
            torch.set_num_threads(cpu_threads)
        import gliner2
        require(Path(gliner2.__file__).resolve().is_relative_to(sdk / "gliner2"),
                "imported SDK differs from the verified source checkout")
        from gliner2 import GLiNER2
        self.model = GLiNER2.from_pretrained(str(snapshot), local_files_only=True).to(device)
        self.model.eval()
        self.model.processor.change_mode(is_training=False)
        require(sha256(manifest_path) == manifest_digest, "material manifest changed during model load")
        self.torch = torch
        self.model_id = manifest["model_id"]
        self.max_encoder_tokens = max_encoder_tokens
        self.label_rendering = label_rendering
        self.gate = threading.Lock()
        self.card = {"id": self.model_id, "protocol": PROTOCOL, "questions": ["noul", "choice"],
                     "artifact": manifest["artifact_repo"], "artifact_revision": manifest["artifact_revision"],
                     "sdk_revision": manifest["sdk_revision"], "manifest_sha256": manifest_digest,
                     "adapter_sha256": sha256(Path(__file__)), "device": str(next(self.model.parameters()).device),
                     "dtype": str(next(self.model.parameters()).dtype), "license": manifest["license"],
                     "cpu_threads": torch.get_num_threads(),
                     "calibration": "native softmax; no QL calibration",
                     "label_rendering": label_rendering,
                     "max_encoder_tokens": max_encoder_tokens, "truncation": "refused",
                     "usage_basis": "actual unpadded encoder tokens; zero generated tokens",
                     "load_seconds": time.monotonic() - started,
                     "packages": {name: importlib.metadata.version(name)
                                  for name in ("gliner2", "torch", "transformers")}}

    def decide(self, request):
        text, tasks, bindings = plan_request(request, self.model_id, self.label_rendering)
        require(self.gate.acquire(blocking=False), "inference already active; retry outside body entry")
        started = time.monotonic()
        try:
            schema = self.model.create_schema()
            for key, config in tasks.items():
                schema.classification(key, **{name: value for name, value in config.items()
                                             if name != "native_type"})
            schemas, metadata = self.model._build_schema_dicts_and_metadata([schema])
            # Pinned SDK's native processor: no word truncation and no dummy
            # fallback. This is the exact batch whose usage is reported.
            batch = self.model.processor.collate_fn_inference(
                [(text, schemas[0])], max_len=None, error_policy="raise",
                architecture=self.model.architecture, build_targets=False,
                on_capacity_exceeded="raise")
            require(len(batch) == 1 and len(batch.original_lengths) == 1, "native batch shape")
            tokens = int(batch.attention_mask.sum().item())
            require(tokens == batch.original_lengths[0] and 0 < tokens <= self.max_encoder_tokens,
                    "encoded schema/state exceeds explicit token ceiling; nothing truncated")
            parameter = next(self.model.parameters())
            batch = batch.to(parameter.device, parameter.dtype if parameter.dtype != self.torch.float32 else None)
            with self.torch.inference_mode():
                results = self.model._extract_from_batch(batch, 0.5, metadata, True, False)
            require(isinstance(results, list) and len(results) == 1, "native result batch shape")
            answer = native_answer(results[0], list(tasks), self.model_id, tokens,
                                   (time.monotonic() - started) * 1000, tasks)
            answer["answers"] = {bindings[key]: value for key, value in answer["answers"].items()}
            return answer
        finally:
            self.gate.release()


def handler_for(discriminator):
    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(5)

        def respond(self, status, value):
            body = canonical(value)
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == "/v1/models":
                self.respond(200, {"models": [discriminator.card]})
            else:
                self.respond(404, {"error": "unknown native decision resource"})

        def do_POST(self):
            if self.path != "/v1/systemone":
                self.respond(404, {"error": "unknown native decision resource"})
                return
            try:
                lengths = self.headers.get_all("Content-Length", [])
                require(len(lengths) == 1 and lengths[0].isascii() and lengths[0].isdigit(),
                        "one explicit content length required")
                require(not self.headers.get("Transfer-Encoding"), "chunked requests are not admitted")
                length = int(lengths[0])
                require(0 < length <= MAX_BYTES, "request exceeds bounded native envelope")
                data = self.rfile.read(length)
                require(len(data) == length, "incomplete native request")
                request = decode(data)
                self.respond(200, discriminator.decide(request))
            except (Refused, ValueError, TypeError, KeyError) as error:
                self.respond(422, {"error": str(error)})
            except Exception as error:
                # Failed inference never receives a success-shaped probability.
                self.respond(503, {"error": "native inference failed: " + type(error).__name__})

        def log_message(self, format, *args):
            # Request material is never written into serving logs.
            sys.stderr.write("gliner decision: " + (format % args) + "\n")
    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--device", choices=("cpu", "mps", "cuda"), required=True)
    parser.add_argument("--max-encoder-tokens", type=int, required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--label-rendering", choices=("canonical", "criteria"), default="canonical")
    parser.add_argument("--cpu-threads", type=int, default=2)
    args = parser.parse_args()
    require(1024 <= args.port <= 65535, "unprivileged loopback port required")
    discriminator = Discriminator(args.manifest, args.device, args.max_encoder_tokens, args.label_rendering, args.cpu_threads)
    with ThreadingHTTPServer(("127.0.0.1", args.port), handler_for(discriminator)) as server:
        server.daemon_threads = True
        server.serve_forever()


if __name__ == "__main__":
    main()
