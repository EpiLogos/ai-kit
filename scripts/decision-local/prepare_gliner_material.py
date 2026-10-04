"""Prepare pinned model material in an explicitly supplied Workcell target.

Run from the Workcell-owned environment with the pinned SDK's [local] extra
installed editable. Dependency installation, capacity policy, service lifetime
and provider election remain the existing owners' operations. This command
downloads only the selected snapshot, hashes it and emits the existing material
manifest; it neither installs dependencies nor starts a classifier.
"""
import argparse
import fcntl
import hashlib
import importlib.metadata
import os
from pathlib import Path
import shutil
import subprocess
import uuid

import gliner_decision as g


def write_claim(lease, claim):
    lease.seek(0)
    lease.truncate()
    lease.write(g.canonical(claim) + b"\n")
    lease.flush()


def reserve_target(target, recipe_digest, resume_operation_ref=None):
    """Reserve the named model directory before any download can write it."""
    model = target / "model"
    claim_path = target / "gliner-material-preparation.json"
    if resume_operation_ref is not None:
        lease = claim_path.open("r+b")
        try:
            try:
                fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise g.Refused("material operation is already reserved") from None
            claim = g.decode(lease.read(g.MAX_BYTES + 1))
            g.require(isinstance(claim, dict) and claim.get("operation_ref") == resume_operation_ref
                      and claim.get("recipe_sha256") == recipe_digest
                      and claim.get("target") == str(target)
                      and claim.get("phase") in ("preparing", "failed"), "matching prior material operation required")
            pid = claim.get("pid")
            g.require(type(pid) is int and pid > 0, "prior process attribution required")
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                pass
            else:
                raise g.Refused("prior material process is still present; no concurrent recovery")
            g.require(model.is_dir(), "prior owned material directory missing")
            claim.update(pid=os.getpid(), phase="preparing")
            write_claim(lease, claim)
            return model, lease, claim
        except BaseException:
            lease.close()
            raise
    g.require(not model.exists(), "unowned model directory already present; download refused")
    target.mkdir(parents=True, exist_ok=True)
    claim = {"operation_ref": "gliner-material:" + uuid.uuid4().hex, "pid": os.getpid(),
             "target": str(target), "recipe_sha256": recipe_digest, "phase": "preparing"}
    lease = claim_path.open("x+b")
    try:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        write_claim(lease, claim)
        # Exclusive directory creation also catches a competing writer between
        # the absence check and reservation. Existing material is never adopted.
        model.mkdir(exist_ok=False)
        return model, lease, claim
    except BaseException as error:
        claim.update(phase="failed", failure_type=type(error).__name__)
        write_claim(lease, claim)
        lease.close()
        raise


def recover_publication(target, lease, claim):
    """Retain an incomplete publication during explicitly locked recovery."""
    final = target / "decision-model-manifest.json"
    if not final.exists():
        return
    # Caller holds the exact prior operation's flock. Preserve the actual
    # interrupted bytes before clearing its publication path, never replace
    # them with a newly generated explanation.
    retained = target / ("failed-manifest-" + claim["operation_ref"].split(":")[-1]
                         + "-" + uuid.uuid4().hex + ".json")
    os.link(final, retained)
    claim["retained_publication"] = str(retained)
    write_claim(lease, claim)
    final.unlink()


def publish_manifest(target, lease, claim, manifest):
    staging = target / ("staged-manifest-" + claim["operation_ref"].split(":")[-1]
                        + "-" + uuid.uuid4().hex + ".json")
    claim["staging_manifest"] = str(staging)
    write_claim(lease, claim)
    with staging.open("xb") as stream:
        stream.write(g.canonical(manifest) + b"\n")
    g.verify_material(staging)
    # Hard-link publication is atomic and exclusive. It preserves a competing
    # publication instead of replacing it. Interrupted staging remains owned
    # evidence and cannot be mistaken for installed material.
    final = target / "decision-model-manifest.json"
    os.link(staging, final)
    staging.unlink()
    return final


def prepare(args):
    recipe_bytes = args.recipe.read_bytes()
    recipe_digest = hashlib.sha256(recipe_bytes).hexdigest()
    recipe = g.decode(recipe_bytes)
    g.require(isinstance(recipe, dict) and set(recipe) == {"recipe", "artifact_repo", "artifact_revision",
              "sdk_repo", "sdk_revision", "model_id", "license", "files"}
              and recipe["recipe"] == "gliner2.5-decide", "bounded GLiNER recipe required")
    for field in ("artifact_revision", "sdk_revision"):
        value = recipe[field]
        g.require(isinstance(value, str) and len(value) == 40
                  and all(c in "0123456789abcdef" for c in value), "exact " + field + " required")
    files = recipe["files"]
    g.require(isinstance(files, list) and bool(files)
              and all(isinstance(name, str) and bool(name) and not Path(name).is_absolute()
                      and ".." not in Path(name).parts for name in files)
              and len(files) == len(set(files)) and g.MODEL_FILES <= set(files),
              "complete relative artifact allow-list required")
    g.require(type(args.reserve_free_bytes) is int and args.reserve_free_bytes > 0
              and type(args.max_download_bytes) is int and args.max_download_bytes > 0,
              "explicit Workcell capacity bounds required")
    target = args.target.resolve()
    manifest_path = target / "decision-model-manifest.json"
    if manifest_path.exists() and args.resume_operation_ref is None:
        current, _, _ = g.verify_material(manifest_path)
        g.require(all(current[key] == recipe[key] for key in ("recipe", "artifact_repo", "artifact_revision",
                  "sdk_revision", "model_id", "license")), "target already owns different model material")
        return manifest_path
    sdk = args.sdk_directory.resolve(strict=True)
    revision = subprocess.check_output(["git", "-C", str(sdk), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(sdk), "status", "--porcelain", "--",
                                     "gliner2", "pyproject.toml"], text=True)
    g.require(revision == recipe["sdk_revision"] and not dirty, "pinned clean SDK checkout required")
    import gliner2
    g.require(Path(gliner2.__file__).resolve().is_relative_to(sdk / "gliner2"),
              "material environment must use the verified editable SDK")
    from huggingface_hub import HfApi, snapshot_download
    info = HfApi().model_info(recipe["artifact_repo"], revision=recipe["artifact_revision"], files_metadata=True)
    g.require(info.sha == recipe["artifact_revision"], "upstream snapshot differs from selected pin")
    sizes = {entry.rfilename: entry.size for entry in info.siblings}
    g.require(set(files) <= set(sizes) and all(type(sizes[name]) is int and sizes[name] > 0 for name in files),
              "upstream must disclose every selected artifact size")
    growth = sum(sizes[name] for name in files)
    g.require(growth <= args.max_download_bytes, "selected native model exceeds material budget")
    parent = target
    while not parent.exists():
        parent = parent.parent
    g.require(shutil.disk_usage(parent).free >= args.reserve_free_bytes + growth,
              "insufficient disk headroom for selected snapshot and Workcell reserve")
    g.require(g.sha256(args.recipe) == recipe_digest, "recipe changed during material admission")
    model, lease, claim = reserve_target(target, recipe_digest, args.resume_operation_ref)
    try:
        if args.resume_operation_ref is not None:
            recover_publication(target, lease, claim)
        snapshot = Path(snapshot_download(recipe["artifact_repo"], revision=recipe["artifact_revision"],
                        allow_patterns=files, local_dir=str(model))).resolve()
        artifacts = {}
        for name in files:
            artifact = snapshot / name
            g.require(artifact.stat().st_size == sizes[name], "downloaded artifact size differs: " + name)
            artifacts[name] = {"bytes": artifact.stat().st_size, "sha256": g.sha256(artifact)}
        manifest = {key: recipe[key] for key in ("recipe", "artifact_repo", "artifact_revision", "sdk_repo",
                    "sdk_revision", "model_id", "license")}
        manifest.update(schema="central.material-manifest/v1", snapshot=str(snapshot), sdk_directory=str(sdk),
                        artifacts=artifacts, weights_bytes=artifacts["model.safetensors"]["bytes"],
                        recipe_sha256=recipe_digest, material_operation_ref=claim["operation_ref"],
                        packages={name: importlib.metadata.version(name)
                                  for name in ("gliner2", "torch", "transformers", "huggingface-hub")})
        manifest_path = publish_manifest(target, lease, claim, manifest)
        claim["phase"] = "prepared"
        write_claim(lease, claim)
        return manifest_path
    except BaseException as error:
        claim.update(phase="failed", failure_type=type(error).__name__)
        write_claim(lease, claim)
        raise
    finally:
        lease.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--recipe", type=Path, default=Path(__file__).with_name("gliner-material.json"))
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--sdk-directory", type=Path, required=True)
    parser.add_argument("--reserve-free-bytes", type=int, required=True)
    parser.add_argument("--max-download-bytes", type=int, required=True)
    parser.add_argument("--resume-operation-ref", help="exact failed operation; refuses a present prior process")
    args = parser.parse_args()
    print(prepare(args))


if __name__ == "__main__":
    main()
