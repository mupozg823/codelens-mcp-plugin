#!/usr/bin/env python3
"""Generate a machine-readable release manifest from checksummed artifacts."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


ASSET_PATTERNS = [
    (
        re.compile(r"^codelens-mcp-airgap-(?P<target>.+)\.tar\.gz$"),
        "airgap_bundle",
    ),
    (
        re.compile(r"^codelens-mcp-(?P<target>.+)\.tar\.gz$"),
        "archive",
    ),
    (
        re.compile(r"^codelens-mcp-(?P<target>.+)\.zip$"),
        "archive",
    ),
    (
        re.compile(r"^codelens-mcp-(?P<target>.+)\.cdx\.json$"),
        "sbom",
    ),
]
SCHEMA_VERSION = "codelens-release-manifest-v1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate release-manifest.json from checksums-sha256.txt"
    )
    parser.add_argument("--bundle-dir", default=".", help="Directory containing release assets")
    parser.add_argument(
        "--checksums",
        default=None,
        help="Path to checksums-sha256.txt (default: <bundle-dir>/checksums-sha256.txt)",
    )
    parser.add_argument("--repo", required=True, help="GitHub repository owner/name")
    parser.add_argument("--tag", required=True, help="Git tag, e.g. v1.9.18")
    parser.add_argument("--version", required=True, help="Release version, e.g. 1.9.18")
    parser.add_argument(
        "--image",
        default=None,
        help="OCI image repository, e.g. ghcr.io/org/repo",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Output path (default: <bundle-dir>/release-manifest.json)",
    )
    return parser.parse_args()


def parse_checksums(path: Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    for raw_line in path.read_text().splitlines():
        line = raw_line.strip()
        if not line:
            continue
        parts = line.split(maxsplit=1)
        if len(parts) != 2:
            raise SystemExit(f"invalid checksum line in {path}: {raw_line!r}")
        checksum, name = parts
        if name in entries:
            raise SystemExit(f"duplicate artifact in {path}: {name}")
        entries[name] = checksum
    if not entries:
        raise SystemExit(f"checksums file is empty: {path}")
    return entries


def classify_asset(name: str) -> tuple[str, str] | None:
    for pattern, kind in ASSET_PATTERNS:
        match = pattern.match(name)
        if match:
            return kind, match.group("target")
    return None


def minor_series(version: str) -> str | None:
    parts = version.split(".")
    if len(parts) < 2 or not parts[0] or not parts[1]:
        return None
    return f"{parts[0]}.{parts[1]}"


def main() -> None:
    args = parse_args()
    bundle_dir = Path(args.bundle_dir).resolve()
    checksums_path = (
        Path(args.checksums).resolve()
        if args.checksums
        else (bundle_dir / "checksums-sha256.txt")
    )
    output_path = (
        Path(args.output).resolve()
        if args.output
        else (bundle_dir / "release-manifest.json")
    )

    checksum_entries = parse_checksums(checksums_path)
    assets = []
    for name, checksum in sorted(checksum_entries.items()):
        if name == output_path.name:
            continue
        classified = classify_asset(name)
        if classified is None:
            continue
        kind, target = classified
        assets.append(
            {
                "name": name,
                "kind": kind,
                "target": target,
                "sha256": checksum,
                "download_url": f"https://github.com/{args.repo}/releases/download/{args.tag}/{name}",
            }
        )

    if not assets:
        raise SystemExit(
            f"no release assets matched supported patterns in {checksums_path}"
        )

    manifest: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "repository": args.repo,
        "tag": args.tag,
        "version": args.version,
        "checksums_file": checksums_path.name,
        "assets": assets,
    }

    if args.image:
        tags = [args.version]
        minor_tag = minor_series(args.version)
        if minor_tag:
            tags.append(minor_tag)
        manifest["oci_images"] = [
            {
                "repository": args.image,
                "tags": tags,
            }
        ]

    output_path.write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
