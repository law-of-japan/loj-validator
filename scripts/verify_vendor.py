#!/usr/bin/env python3
"""Verify vendored specification bytes and exact manifest membership (Python 3.9+)."""

import hashlib
import json
import os
from pathlib import Path
import re
import sys


METADATA = {"COMMIT", "SHA256SUMS.json"}
SNAPSHOTS = ("loj-spec", "loj-spec-v1alpha1")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate manifest key: {key!r}")
        result[key] = value
    return result


def raise_walk_error(error):
    raise error


def verify(root):
    """Return all detected errors; never normalize or modify snapshot bytes."""
    errors = []
    try:
        if root.is_symlink() or not root.is_dir():
            return ["snapshot root is not a regular directory"]
        for name in METADATA:
            path = root / name
            if path.is_symlink() or not path.is_file():
                return [f"missing or non-regular metadata: {name}"]
        manifest = json.loads(
            (root / "SHA256SUMS.json").read_text(encoding="utf-8"),
            object_pairs_hook=unique_object,
        )
        if not isinstance(manifest, dict) or not manifest:
            return ["manifest must be a nonempty object"]
        for name, digest in manifest.items():
            if (
                not name
                or "\\" in name
                or ":" in name
                or any(part in ("", ".", "..") for part in name.split("/"))
                or name in METADATA
            ):
                errors.append(f"invalid manifest path: {name!r}")
            if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
                errors.append(f"invalid SHA-256: {name!r}")
        if errors:
            return errors

        actual = set()
        for directory, dirs, files in os.walk(root, onerror=raise_walk_error, followlinks=False):
            for name in dirs:
                path = Path(directory) / name
                if path.is_symlink():
                    errors.append(f"symlink is not a snapshot asset: {path.relative_to(root).as_posix()}")
            for name in files:
                path = Path(directory) / name
                relative = path.relative_to(root).as_posix()
                if path.is_symlink():
                    errors.append(f"symlink is not a snapshot asset: {relative}")
                elif not path.is_file():
                    errors.append(f"non-regular asset: {relative}")
                elif relative not in METADATA:
                    actual.add(relative)
        expected = set(manifest)
        errors.extend(f"missing asset: {name}" for name in sorted(expected - actual))
        errors.extend(f"unlisted asset: {name}" for name in sorted(actual - expected))
        for name in sorted(expected & actual):
            digest = hashlib.sha256()
            with (root / name).open("rb") as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
            if digest.hexdigest() != manifest[name]:
                errors.append(f"SHA-256 mismatch: {name}")
    except (OSError, ValueError) as error:
        errors.append(f"cannot verify snapshot: {error}")
    return errors


def main():
    vendor = Path(__file__).resolve().parents[1] / "vendor"
    failed = False
    for name in SNAPSHOTS:
        errors = verify(vendor / name)
        if errors:
            failed = True
            for error in errors:
                print(f"{name}: {error}", file=sys.stderr)
        else:
            print(f"{name}: all listed bytes and file membership match")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
