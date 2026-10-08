#!/usr/bin/env python3
"""A network-free stand-in for the two things the Kev recipe runs.

`python -m kev.serve --run R --host H --port P`   a SystemOne-compatible server
`python -c SNAPSHOT a_repo a_rev b_repo b_rev`    the pinned-artifact fetch

Both are driven through the same argv the real recipe uses, so the lifecycle
code under test runs its real command lines. The server answers `GET /v1/models`
with a Kev-shaped card and `POST /v1/systemone` with strictly valid answers.
"""
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def snapshot(args):
    adapter_repo, adapter_rev, base_repo, base_rev = args[:4]
    root = os.environ["FAKE_HF_ROOT"]
    adapter = os.path.join(root, "adapter", adapter_rev)
    base = os.path.join(root, "base", base_rev)
    os.makedirs(adapter, exist_ok=True)
    os.makedirs(base, exist_ok=True)
    for name in ["adapter_config.json", "adapter_model.safetensors", "head.pt",
                 "provenance.json", "tokenizer.json", "tokenizer_config.json"]:
        with open(os.path.join(adapter, name), "w") as f:
            f.write(f"{adapter_repo}@{adapter_rev}:{name}")
    with open(os.path.join(base, "model.safetensors"), "w") as f:
        f.write(f"{base_repo}@{base_rev}:weights")
    print("downloading (fake)", file=sys.stderr)
    print(json.dumps({"adapter_dir": adapter, "base_dir": base}))


def answers(request):
    out = {}
    for qid, q in request["questions"].items():
        kind = q["type"]
        if kind == "noul":
            out[qid] = {"type": "noul", "noul": 0.75}
        elif kind == "choice":
            keys = sorted(q["criteria"].keys())
            probabilities = {k: 1.0 / len(keys) for k in keys}
            out[qid] = {"type": "choice", "choice": keys[0],
                        "probabilities": probabilities, "confidence": 1.0 / len(keys)}
        else:
            criteria = q["criteria"]
            n = len(criteria)
            probabilities = {str(i): 1.0 / n for i in range(n)}
            legend = {str(i): criteria[i] for i in range(n)}
            score = sum(i * (1.0 / n) for i in range(n))
            out[qid] = {"type": "score", "score": score, "legend": legend,
                        "probabilities": probabilities, "confidence": 1.0 / n}
    return out


def serve(args):
    def flag(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    run = flag("--run")
    host = flag("--host", "127.0.0.1")
    port = int(flag("--port"))
    base = os.environ.get("FAKE_KEV_BASE", "Qwen/Qwen3.5-0.8B-Base")
    # Evidence the lifecycle did not leak another owner's authority.
    here = os.path.dirname(os.getcwd())
    with open(os.path.join(here, "fake-env.json"), "w") as f:
        json.dump(sorted(os.environ.keys()), f)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def send(self, body):
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path == "/v1/models":
                self.send({"models": [{"name": "kev-latest", "run": run, "base": base,
                                       "backend": "fake", "dtype": "float32"}]})
            else:
                self.send_response(404)
                self.end_headers()

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            request = json.loads(self.rfile.read(length) or b"{}")
            if self.path != "/v1/systemone":
                self.send_response(404)
                self.end_headers()
                return
            self.send({"model": request["model"], "answers": answers(request),
                       "usage": {"input_tokens": 11, "output_tokens": 3}})

    ThreadingHTTPServer((host, port), Handler).serve_forever()


if __name__ == "__main__":
    argv = sys.argv[1:]
    if argv and argv[0] == "-c":
        snapshot(argv[2:])
    elif argv and argv[0] == "-m":
        serve(argv[2:])
    else:
        sys.exit("fake_kev: unsupported invocation")
