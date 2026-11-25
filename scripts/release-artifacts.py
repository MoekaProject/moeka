#!/usr/bin/env python3
"""Record and validate immutable, same-run release artifacts without network I/O."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil


GROUPS = {
    (platform, arch): [
        f"Moeka-{platform}-{arch}.{suffix}"
        for suffix in (("dmg", "zip") if platform == "macos" else ("zip",))
    ]
    for platform in ("macos", "windows", "linux")
    for arch in ("arm64", "x86_64")
}


def identity(version, sha, run):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("Expected a version without the v prefix")
    if not re.fullmatch(r"[0-9a-f]{40}", sha) or not re.fullmatch(r"[1-9][0-9]*", run):
        raise ValueError("Invalid source SHA or workflow run ID")
    return {"version": version, "source_sha": sha, "run_id": run}


def verify_version(root, version):
    def field(section, name):
        matches = re.findall(rf'^{name}\s*=\s*"([^"]*)"\s*(?:#.*)?$', section, re.MULTILINE)
        if len(matches) != 1:
            raise ValueError(f"Expected one {name} in the Cargo package section")
        return matches[0]

    manifest = (root / "Cargo.toml").read_text()
    sections = re.findall(r"^\[package\]\s*\n(.*?)(?=^\[|\Z)", manifest, re.MULTILINE | re.DOTALL)
    packages = [
        section for section in (root / "Cargo.lock").read_text().split("[[package]]")[1:]
        if field(section, "name") == "moeka" and not re.search(r"^source\s*=", section, re.MULTILINE)
    ]
    if len(sections) != 1 or field(sections[0], "version") != version or len(packages) != 1 or field(packages[0], "version") != version:
        raise ValueError("Version must match Cargo.toml and the root Cargo.lock package")


def digest(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
        raise ValueError(f"Missing, empty or symlinked artifact: {path}")
    with path.open("rb") as stream:
        hasher = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
        sha = hasher.hexdigest()
    return {"name": path.name, "size": path.stat().st_size, "sha256": sha}


def record(directory, platform, arch, expected):
    names = GROUPS[(platform, arch)]
    data = {
        **expected,
        "platform": platform,
        "arch": arch,
        "assets": [digest(directory / name) for name in names],
    }
    with (directory / "manifest.json").open("x") as stream:
        json.dump(data, stream, indent=2)
        stream.write("\n")


def collect(source, destination, expected):
    seen = set()
    verified = []
    for manifest in sorted(source.rglob("manifest.json")):
        if manifest.is_symlink() or not manifest.resolve().is_relative_to(source.resolve()):
            raise ValueError(f"Unsafe manifest: {manifest}")
        data = json.loads(manifest.read_text())
        if any(data.get(key) != value for key, value in expected.items()):
            raise ValueError(f"Artifact belongs to a different version, commit or run: {manifest}")
        group = (data.get("platform"), data.get("arch"))
        if group not in GROUPS or group in seen:
            raise ValueError(f"Unexpected or duplicate platform: {group}")
        seen.add(group)
        assets = data.get("assets", [])
        if [asset.get("name") for asset in assets] != GROUPS[group]:
            raise ValueError(f"Unexpected artifact names: {manifest}")
        for asset in assets:
            path = manifest.parent / asset["name"]
            if digest(path) != asset:
                raise ValueError(f"Artifact size or digest mismatch: {path}")
            verified.append((path, asset))
    if seen != set(GROUPS):
        raise ValueError(f"Missing platform groups: {sorted(set(GROUPS) - seen)}")
    # Validate everything before writing any output or publishing anything.
    destination.mkdir()
    for path, _ in verified:
        shutil.copyfile(path, destination / path.name)
    with (destination / "release-manifest.json").open("x") as stream:
        json.dump({**expected, "assets": [asset for _, asset in verified]}, stream, indent=2)
        stream.write("\n")


def verify(directory, expected):
    manifest = directory / "release-manifest.json"
    if manifest.is_symlink():
        raise ValueError("Release manifest cannot be a symlink")
    data = json.loads(manifest.read_text())
    if any(data.get(key) != value for key, value in expected.items()):
        raise ValueError("Release provenance mismatch")
    assets = data.get("assets", [])
    names = [asset.get("name") for asset in assets]
    expected_names = {name for group in GROUPS.values() for name in group}
    if len(names) != 8 or set(names) != expected_names:
        raise ValueError("Expected exactly eight platform attachments")
    for asset in assets:
        if digest(directory / asset["name"]) != asset:
            raise ValueError(f"Release artifact changed: {asset['name']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("version", "record", "collect", "verify"))
    parser.add_argument("--version", required=True)
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    parser.add_argument("--source", type=Path)
    parser.add_argument("--platform", choices=("macos", "windows", "linux"))
    parser.add_argument("--arch", choices=("arm64", "x86_64"))
    args = parser.parse_args()
    expected = identity(
        args.version,
        os.environ.get("MOEKA_RELEASE_SOURCE_SHA") or os.environ["GITHUB_SHA"],
        os.environ["GITHUB_RUN_ID"],
    )
    verify_version(Path.cwd(), args.version)
    if args.operation == "record":
        record(args.directory, args.platform, args.arch, expected)
    elif args.operation == "collect":
        if args.source is None:
            parser.error("collect requires --source")
        collect(args.source, args.directory, expected)
    elif args.operation == "verify":
        verify(args.directory, expected)
    print(f"Validated {args.operation}: {args.version} at {expected['source_sha']}")


if __name__ == "__main__":
    main()
