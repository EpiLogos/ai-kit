#!/usr/bin/env python3
"""Restore every non-credential path to origin/main before cloud validation."""
from pathlib import Path
import subprocess

ALLOWED = {
    ".github/workflows/credential-cloud-finalize.yml",
    "Cargo.lock",
    "Cargo.toml",
    "crates/aikit-core/src/credential.rs",
    "crates/aikit-core/src/lib.rs",
    "crates/aikit-adapters/Cargo.toml",
    "crates/aikit-adapters/src/credential_provider.rs",
    "crates/aikit-adapters/src/lib.rs",
    "crates/aikit-adapters/tests/macos_keychain.rs",
    "crates/aikit-store/src/credentials.rs",
    "crates/aikit-store/src/home.rs",
    "crates/aikit-store/src/lib.rs",
    "crates/aikit-tui/src/credential_surface.rs",
    "crates/aikit-tui/src/lib.rs",
    "crates/aikit-cli/Cargo.toml",
    "crates/aikit-cli/src/credential.rs",
    "crates/aikit-cli/src/lib.rs",
    "crates/aikit-cli/src/doctor.rs",
    "crates/aikit-cli/src/cli.rs",
    "crates/aikit-cli/src/main.rs",
}


def allowed(path: str) -> bool:
    return path in ALLOWED or path.startswith("scripts/credential_")

changed = subprocess.check_output(
    ["git", "diff", "--name-only", "origin/main...HEAD"], text=True
).splitlines()
for path in changed:
    if allowed(path):
        continue
    # All non-allowed paths existed on main; restore their exact live-base bytes.
    subprocess.run(["git", "checkout", "origin/main", "--", path], check=True)

# Make sure the cleanup itself never changes generated/new credential files.
remaining = subprocess.check_output(
    ["git", "diff", "--name-only", "origin/main"], text=True
).splitlines()
unexpected = [path for path in remaining if not allowed(path)]
if unexpected:
    raise SystemExit("unexpected non-credential diff remains: " + ", ".join(unexpected))

print(f"credential diff constrained to {len(remaining)} paths")
