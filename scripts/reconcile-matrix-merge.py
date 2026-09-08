#!/usr/bin/env python3
"""Resolve a merge conflict on the shared capability-matrix/account artifacts.

Every slice that touches runtime source reconciles product-ground, which
regenerates three files that every other in-flight branch also regenerates:

    ProjectCentral/user/capability-matrix.csv   (record store: code_refs, code_basis)
    ProjectCentral/user/capability-matrix.md    (rendering)
    ProjectCentral/user/aikit.html              (rendering / account page)

Because every slice touches them, every open PR conflicts on them as soon as
another PR merges. This script performs the resolution that used to be a
six-step manual dance:

    1. take main's version of the three artifacts (never trust a 3-way merge
       of generated content, and never rely on `git checkout --theirs`, which
       silently no-ops on a path git no longer considers unmerged)
    2. refuse loudly if any of them still carries conflict markers
    3. re-apply *this branch's own* capability-matrix.csv row edits, recovered
       from the diff between the merge base and the branch tip (not retyped)
    4. regenerate the renderings through the product-ground pyz's plan/apply
       flow (building target/debug/aikit first if it is missing)
    5. assert that the resulting plan's changed_records is exactly the set of
       records this branch itself changed -- if it names anything else,
       another PR's reconciliation has been clobbered and we refuse
    6. run the product-ground checker and require "errors": []

It never stages or commits anything. Review the result with `git diff` /
`git status`, then `git add` and commit yourself.

Usage:
    scripts/reconcile-matrix-merge.py --change-ref EpiLogos/ai-kit#221
    scripts/reconcile-matrix-merge.py --change-ref EpiLogos/ai-kit#221 --reviewed q0 q3

Run once with only --change-ref: if the plan requires reading expanded
sections you have not attested to, the script prints exactly which ones
(`required_review`) and stops without writing the final reconciliation.
Rerun with --reviewed <those sections> once you have actually read them.
"""
from __future__ import annotations

import argparse
import csv
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

CSV_PATH = "ProjectCentral/user/capability-matrix.csv"
MD_PATH = "ProjectCentral/user/capability-matrix.md"
HTML_PATH = "ProjectCentral/user/aikit.html"
MANIFEST_PATH = "ProjectCentral/user/capability-matrix.json"
FORCED_ARTIFACTS = (CSV_PATH, MD_PATH, HTML_PATH)

ACCOUNT = "aikit.html"
NAMESPACE = "aikit"
PRODUCT_INDEX = 2

CONFLICT_MARKER_RE = re.compile(r"^(?:<{7}(?: .*)?|={7}|>{7}(?: .*)?)$", re.M)


def fail(message: str) -> "SystemExit":
    return SystemExit(f"reconcile-matrix-merge: {message}")


def run(args: list[str], cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True)
    if check and result.returncode != 0:
        raise fail(f"command failed ({' '.join(args)}): {result.stderr.strip() or result.stdout.strip()}")
    return result


def git(root: Path, *args: str, check: bool = True) -> str:
    return run(["git", *args], cwd=root, check=check).stdout.strip()


def git_show(root: Path, ref: str, relpath: str) -> str:
    result = run(["git", "show", f"{ref}:{relpath}"], cwd=root, check=False)
    if result.returncode != 0:
        raise fail(f"cannot read {relpath} at {ref}: {result.stderr.strip()}")
    return result.stdout


def parse_csv(text: str) -> tuple[list[str], list[dict[str, str]]]:
    """Mirror capability_matrix.load's strict row parsing, without a manifest file."""
    reader = csv.reader(io.StringIO(text, newline=""), strict=True)
    header = next(reader, [])
    records = []
    for line, values in enumerate(reader, 2):
        if len(values) != len(header):
            raise fail(f"CSV line {line} has {len(values)} fields; expected {len(header)}")
        records.append(dict(zip(header, values)))
    return header, records


def by_id(records: list[dict[str, str]]) -> dict[str, dict[str, str]]:
    keyed: dict[str, dict[str, str]] = {}
    for record in records:
        key = record.get("id", "")
        if not key:
            raise fail("CSV row is missing an id")
        if key in keyed:
            raise fail(f"duplicate CSV row id: {key}")
        keyed[key] = record
    return keyed


def check_no_conflict_markers(paths: dict[str, Path]) -> None:
    offenders = []
    for name, path in paths.items():
        text = path.read_text(encoding="utf-8", errors="replace")
        if CONFLICT_MARKER_RE.search(text):
            offenders.append(name)
    if offenders:
        raise fail(
            "refusing to proceed: conflict markers remain after taking main's version of "
            + ", ".join(offenders)
            + ". Nothing was staged. Inspect and fix these files (or the commit on origin/main "
            "that produced them) before rerunning."
        )


def build_merged_csv(main_header: list[str], main_rows: dict[str, dict[str, str]],
                      branch_header: list[str], branch_rows: dict[str, dict[str, str]],
                      branch_changed_ids: set[str], main_row_order: list[str]) -> str:
    header = list(main_header) + [c for c in branch_header if c not in main_header]
    out_rows: list[dict[str, str]] = []
    for record_id in main_row_order:
        row = dict(main_rows[record_id])
        if record_id in branch_changed_ids:
            row = dict(branch_rows[record_id])
        for column in header:
            row.setdefault(column, "")
        out_rows.append(row)
    seen = set(main_row_order)
    for record_id in branch_rows:
        if record_id not in seen and record_id in branch_changed_ids:
            row = dict(branch_rows[record_id])
            for column in header:
                row.setdefault(column, "")
            out_rows.append(row)
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=header, lineterminator="\n")
    writer.writeheader()
    writer.writerows(out_rows)
    return buffer.getvalue()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--change-ref", required=True, help="Attributable ticket/PR/session reference, forwarded to reconcile apply --change-ref")
    parser.add_argument("--reviewed", nargs="*", default=None, metavar="SECTION",
                         help="Expanded sections (q0..q5) you have actually read. "
                              "Omit to have the script tell you what required_review demands and stop.")
    args = parser.parse_args()

    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))

    merge_head = git(root, "rev-parse", "-q", "--verify", "MERGE_HEAD", check=False)
    if not merge_head:
        raise fail(
            "no merge in progress (no MERGE_HEAD). Run `git fetch origin && git merge origin/main` "
            "first; this tool resolves the conflict that produces on the shared matrix/account files."
        )
    ours = git(root, "rev-parse", "HEAD")
    base = git(root, "merge-base", "HEAD", "MERGE_HEAD")
    print(f"==> Merge in progress: HEAD {ours[:12]} <- MERGE_HEAD {merge_head[:12]} (base {base[:12]})")

    UNMERGED_CODES = {"DD", "AU", "UD", "UA", "DU", "AA", "UU"}
    status = git(root, "status", "--porcelain=1")
    conflicted = {line[3:] for line in status.splitlines() if line[:2] in UNMERGED_CODES}
    for path in FORCED_ARTIFACTS:
        note = " (git shows it conflicted)" if path in conflicted else " (not flagged conflicted by git, forcing anyway)"
        print(f"    - {path}{note}")

    print("==> Taking main's version of the three shared artifacts")
    paths = {name: root / name for name in FORCED_ARTIFACTS}
    for name, path in paths.items():
        path.write_text(git_show(root, "MERGE_HEAD", name), encoding="utf-8")

    manifest_path = root / MANIFEST_PATH
    manifest_base = git_show(root, base, MANIFEST_PATH)
    manifest_branch = git_show(root, "HEAD", MANIFEST_PATH)
    if manifest_branch != manifest_base:
        print(f"    - {MANIFEST_PATH}: branch itself edits the manifest; leaving working-tree resolution as-is")
        if CONFLICT_MARKER_RE.search(manifest_path.read_text(encoding="utf-8", errors="replace")):
            raise fail(f"{MANIFEST_PATH} still contains conflict markers and this branch edits it structurally; resolve it by hand.")
    else:
        manifest_path.write_text(git_show(root, "MERGE_HEAD", MANIFEST_PATH), encoding="utf-8")
        print(f"    - {MANIFEST_PATH}: branch did not touch it; took main's version too")

    check_no_conflict_markers(paths)
    print("    OK: no conflict markers in any shared artifact")

    print("==> Recovering this branch's own capability-matrix.csv row edits (base -> HEAD)")
    base_header, base_records = parse_csv(git_show(root, base, CSV_PATH))
    head_header, head_records = parse_csv(git_show(root, "HEAD", CSV_PATH))
    main_header, main_records = parse_csv(git_show(root, "MERGE_HEAD", CSV_PATH))
    main_row_order = [r["id"] for r in main_records]
    base_rows, head_rows, main_rows = by_id(base_records), by_id(head_records), by_id(main_records)

    removed = set(base_rows) - set(head_rows)
    if removed:
        raise fail(f"this branch deletes capability-matrix.csv row(s) {sorted(removed)}; resolve that by hand, this tool only reapplies edits/additions.")

    modified = {rid for rid, row in head_rows.items() if rid in base_rows and row != base_rows[rid]}
    added = set(head_rows) - set(base_rows)
    branch_changed_ids = modified | added
    if not branch_changed_ids:
        print("    Branch has no capability-matrix.csv edits relative to the merge base; nothing to reapply.")
        print("    main's shared artifacts are now in place. Review with git diff/status, then commit.")
        return 0
    print(f"    Branch's own row edits: {sorted(branch_changed_ids)}")

    overlap = {
        rid for rid in branch_changed_ids
        if rid in main_rows and rid in base_rows
        and main_rows[rid] != base_rows[rid]
        and main_rows[rid] != head_rows[rid]
    }
    if overlap:
        raise fail(
            "genuine conflicting edits on the same capability-matrix.csv row(s) "
            f"{sorted(overlap)}: both this branch and origin/main changed them differently since "
            "the merge base. This tool only reapplies non-overlapping edits; resolve these rows by hand."
        )

    merged_csv = build_merged_csv(main_header, main_rows, head_header, head_rows, branch_changed_ids, main_row_order)
    if CONFLICT_MARKER_RE.search(merged_csv):
        raise fail("refusing to proceed: reconstructed capability-matrix.csv contains conflict-marker-shaped text; nothing was staged.")
    paths[CSV_PATH].write_text(merged_csv, encoding="utf-8")
    print("    Wrote reconstructed capability-matrix.csv (main's rows + this branch's own row edits)")

    print("==> Preparing the product-ground pyz and target/debug/aikit")
    pyz = root / ".github/product-ground.pyz"
    if not pyz.is_file():
        raise fail(f"missing {pyz}")
    workdir = Path(tempfile.mkdtemp(prefix="reconcile-matrix-merge-"))
    try:
        with zipfile.ZipFile(pyz) as archive:
            archive.extractall(workdir)
        reconcile_py = workdir / "reconcile_product_ground.py"

        binary = root / "target/debug/aikit"
        if not binary.is_file():
            print("    target/debug/aikit missing; building (cargo build --package aikit-cli --bin aikit)")
            build = subprocess.run(["cargo", "build", "--package", "aikit-cli", "--bin", "aikit"], cwd=root, text=True)
            if build.returncode != 0 or not binary.is_file():
                raise fail("cargo build --package aikit-cli --bin aikit failed; cannot run the checker's CLI discovery")
        else:
            print("    target/debug/aikit already present")

        print("==> Regenerating the renderings (reconcile_product_ground.py plan --direction csv-to-html)")
        plan_path = workdir / "plan.json"
        plan_result = run(["python3", str(reconcile_py), "plan", "--root", str(root), "--account", ACCOUNT,
                            "--namespace", NAMESPACE, "--direction", "csv-to-html", "--out", str(plan_path)], cwd=workdir)
        plan = json.loads(plan_path.read_text())
        actual_changed = set(plan["changed_records"])
        expected_changed = set(branch_changed_ids)

        print(f"    plan changed_records: {sorted(actual_changed)}")
        print("==> Asserting the changed-records invariant (plan must name exactly this branch's own edits)")
        extra = actual_changed - expected_changed
        missing = expected_changed - actual_changed
        if extra or missing:
            detail = []
            if extra:
                detail.append(f"plan reports record(s) this branch never touched: {sorted(extra)} -- another PR's reconciliation may have been clobbered")
            if missing:
                detail.append(f"plan does not report record(s) this branch touched: {sorted(missing)} -- the edit may be a no-op or was lost")
            raise fail("changed-records invariant violated: " + "; ".join(detail) + ". Nothing was staged.")
        print("    OK: plan.changed_records == this branch's own row edits, exactly")

        required_review = plan["required_review"]
        if args.reviewed is None:
            print("==> --reviewed not supplied. required_review from the plan:")
            for section in required_review:
                print(f"    - {section}")
            print("    Read those expanded sections and any linked capabilities, then rerun with:")
            print(f"        scripts/reconcile-matrix-merge.py --change-ref {args.change_ref!r} --reviewed " + " ".join(required_review))
            print("    Nothing was staged. main's artifacts + your reapplied CSV edits remain in the working tree, unapplied.")
            return 1

        reviewed = set(args.reviewed)
        if not set(required_review) <= reviewed:
            raise fail(f"--reviewed {sorted(reviewed)} does not cover required_review {required_review}; read those sections and pass them all.")

        print("==> Applying the reviewed reconciliation (reconcile_product_ground.py apply)")
        apply_result = subprocess.run(
            ["python3", str(reconcile_py), "apply", str(plan_path), "--reviewed", *args.reviewed, "--change-ref", args.change_ref],
            cwd=workdir, text=True, capture_output=True,
        )
        if apply_result.returncode != 0:
            raise fail(f"apply failed: {apply_result.stderr.strip() or apply_result.stdout.strip()}. Nothing further was staged.")
        receipt_path = Path(apply_result.stdout.strip())
        print(f"    Applied. Transaction receipt: {receipt_path}")

        print("==> Verifying the product-ground checker reports \"errors\": []")
        checker = subprocess.run(
            ["python3", str(pyz), "--root", str(root), "--account", ACCOUNT, "--namespace", NAMESPACE,
             "--product-index", str(PRODUCT_INDEX), "--reference-scope", "repository", "--base", base],
            cwd=root, text=True, capture_output=True,
        )
        report = json.loads(checker.stdout or "{}")
        if report.get("errors"):
            print(json.dumps(report, indent=2))
            print("==> Checker reported errors; restoring pre-apply bytes and refusing to leave a broken reconciliation")
            restore = subprocess.run(["python3", str(reconcile_py), "restore", str(receipt_path)], cwd=workdir, text=True, capture_output=True)
            if restore.returncode != 0:
                raise fail(f"checker failed AND restore failed ({restore.stderr.strip()}); working tree may be inconsistent, inspect by hand before doing anything else.")
            raise fail("product-ground checker reported errors after apply (restored to pre-apply state; see errors above). Nothing was staged.")
        print("    OK: \"errors\": []")
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    print("==> Done. Review with `git diff` / `git status`, then:")
    print(f"        git add {CSV_PATH} {MD_PATH} {HTML_PATH} {MANIFEST_PATH}")
    print("        git commit")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit as exc:
        if isinstance(exc.code, str):
            print(exc.code, file=sys.stderr)
            raise SystemExit(1)
        raise
