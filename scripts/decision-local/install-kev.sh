#!/usr/bin/env sh
# Install the selected local decision model (Kev-0.8B) behind a Workcell-owned
# serving process. Installs ONLY the selected model — never a larger one
# "while we're here" — and records every pinned artifact identity in a
# material manifest the operator keeps beside the serving checkout.
#
# Selection basis (evaluated 2026-09-27, Apple M4 / 16 GB / 10 cores):
#   Kev-4B bf16 needs ~8 GB resident weights + ~1 GB MLX cache and its own
#   model card targets 32 GB Macs; Kev-0.8B needs ~1.7 GB resident and is the
#   candidate that fits this machine's honest headroom. The endpoint transport
#   is model-agnostic, so a larger self-hosted model can be admitted later by
#   re-electing it in the provider configuration.
#
# Pins (recorded in the manifest, enforced by re-verification):
#   upstream   jaredpalmer/kev @ 5920c5fe4ca8e0970ed4209ac2c9b8e18bea5109
#   adapter    jaredpalmer/kev-0.8b (LoRA r16 + pointer head + tokenizer)
#   base       Qwen/Qwen3.5-0.8B-Base @ dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68
#   runtime    uv-managed Python 3.13, mlx-lm >=0.31.3,<0.32 (Apple Metal)
#   precision  backbone as stored (bf16) on MLX; fp32 pointer head
#   license    Apache-2.0 (code, adapter and base)
set -eu
KEV_UPSTREAM_PIN="5920c5fe4ca8e0970ed4209ac2c9b8e18bea5109"
ADAPTER_REPO="jaredpalmer/kev-0.8b"
BASE_REPO="Qwen/Qwen3.5-0.8B-Base"
BASE_REVISION="dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68"
TARGET="${1:?usage: install-kev.sh <target-dir>}"

command -v git >/dev/null || { echo "git is required" >&2; exit 1; }
command -v uv >/dev/null || { echo "uv is required (https://docs.astral.sh/uv/)" >&2; exit 1; }

mkdir -p "$TARGET"
cd "$TARGET"

if [ ! -d kev/.git ]; then
  git clone https://github.com/jaredpalmer/kev.git kev
fi
cd kev
git fetch origin "$KEV_UPSTREAM_PIN" 2>/dev/null || git fetch origin main
git checkout --detach "$KEV_UPSTREAM_PIN"
echo "upstream: $(git rev-parse HEAD)"

uv sync --extra serve

# Pre-download the pinned artifacts into the local HF cache so the serving
# process can start and answer with outbound network disabled.
uv run --extra serve python - <<PY
from huggingface_hub import snapshot_download
adapter = snapshot_download("$ADAPTER_REPO")
base = snapshot_download("$BASE_REPO", revision="$BASE_REVISION")
print("adapter:", adapter)
print("base:", base)
PY

# Material manifest: what is installed, from where, under which license.
uv run --extra serve python - "$TARGET" <<PY
import hashlib, json, pathlib, sys
from huggingface_hub import hf_hub_download
target = pathlib.Path(sys.argv[1])
def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()
adapter_files = ["adapter_config.json", "adapter_model.safetensors", "head.pt",
                 "provenance.json", "tokenizer.json", "tokenizer_config.json"]
manifest = {
    "schema": "central.material-manifest/v1",
    "recipe": "kev-0.8b",
    "upstream_pin": "$KEV_UPSTREAM_PIN",
    "adapter_repo": "$ADAPTER_REPO",
    "base_repo": "$BASE_REPO",
    "base_revision": "$BASE_REVISION",
    "license": "Apache-2.0",
    "artifacts": {},
}
from huggingface_hub import snapshot_download
adapter_dir = pathlib.Path(snapshot_download("$ADAPTER_REPO"))
for name in adapter_files:
    p = adapter_dir / name
    if p.exists():
        manifest["artifacts"][f"{name}"] = {"sha256": sha256(p), "bytes": p.stat().st_size}
base_dir = pathlib.Path(snapshot_download("$BASE_REPO", revision="$BASE_REVISION"))
for p in sorted(base_dir.glob("*.safetensors")):
    manifest["artifacts"][f"base/{p.name}"] = {"sha256": sha256(p), "bytes": p.stat().st_size}
weights_bytes = sum(v["bytes"] for k, v in manifest["artifacts"].items() if k.startswith("base/"))
manifest["weights_bytes"] = weights_bytes
out = target / "decision-model-manifest.json"
out.write_text(json.dumps(manifest, indent=2, sort_keys=True))
print("manifest:", out)
PY
echo "done. Now declare the serving process through Workcell's declared-services path."
