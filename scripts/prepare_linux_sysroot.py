#!/usr/bin/env python3
"""Prepare the minimal Ubuntu sysroot needed to link Moeka from macOS."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import lzma
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

SUITE = "jammy"
PACKAGES = {
    "libxcb1-dev",
    "libxcb1",
    "libxcb-xkb-dev",
    "libxcb-xkb1",
    "libxkbcommon-dev",
    "libxkbcommon0",
    "libxkbcommon-x11-dev",
    "libxkbcommon-x11-0",
}
ARCHITECTURES = {
    "x86_64": ("amd64", "x86_64-linux-gnu", "https://archive.ubuntu.com/ubuntu"),
    "arm64": ("arm64", "aarch64-linux-gnu", "https://ports.ubuntu.com/ubuntu-ports"),
}


def download(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "Moeka Linux builder"})
    with urllib.request.urlopen(request) as response:
        return response.read()


def package_records(index: bytes) -> dict[str, dict[str, str]]:
    records = {}
    for paragraph in gzip.decompress(index).decode("utf-8").split("\n\n"):
        fields = {}
        for line in paragraph.splitlines():
            if ": " in line:
                key, value = line.split(": ", 1)
                fields[key] = value
        package = fields.get("Package")
        if package in PACKAGES:
            records[package] = fields
    missing = PACKAGES - records.keys()
    if missing:
        raise RuntimeError(f"Ubuntu package index is missing: {', '.join(sorted(missing))}")
    return records


def extract_deb(deb: Path, root: Path) -> None:
    members = subprocess.check_output(["ar", "t", deb], text=True).splitlines()
    data_member = next((member for member in members if member.startswith("data.tar.")), None)
    if data_member is None:
        raise RuntimeError(f"{deb.name} has no data archive")

    archive = subprocess.check_output(["ar", "p", deb, data_member])
    if data_member.endswith(".zst"):
        if shutil.which("zstd") is None:
            raise RuntimeError("zstd is required to extract Ubuntu packages")
        archive = subprocess.check_output(["zstd", "-q", "-d", "-c"], input=archive)
        mode = "r:"
    elif data_member.endswith(".xz"):
        archive = lzma.decompress(archive)
        mode = "r:"
    elif data_member.endswith(".gz"):
        archive = gzip.decompress(archive)
        mode = "r:"
    else:
        raise RuntimeError(f"unsupported deb data archive: {data_member}")

    with tarfile.open(fileobj=io.BytesIO(archive), mode=mode) as tar:
        tar.extractall(root)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("architecture", choices=ARCHITECTURES)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    apt_arch, triplet, mirror = ARCHITECTURES[args.architecture]
    root = args.output.resolve()
    stamp = root / ".moeka-sysroot.json"
    expected_stamp = {"suite": SUITE, "architecture": apt_arch, "packages": sorted(PACKAGES)}
    if stamp.exists() and json.loads(stamp.read_text()) == expected_stamp:
        print(root)
        return 0

    root.mkdir(parents=True, exist_ok=True)
    cache = root / ".debs"
    cache.mkdir(exist_ok=True)
    index_url = f"{mirror}/dists/{SUITE}/main/binary-{apt_arch}/Packages.gz"
    records = package_records(download(index_url))

    for package in sorted(PACKAGES):
        record = records[package]
        filename = record["Filename"]
        deb = cache / Path(filename).name
        if not deb.exists() or hashlib.sha256(deb.read_bytes()).hexdigest() != record["SHA256"]:
            payload = download(f"{mirror}/{filename}")
            if hashlib.sha256(payload).hexdigest() != record["SHA256"]:
                raise RuntimeError(f"checksum mismatch for {filename}")
            deb.write_bytes(payload)
        extract_deb(deb, root)

    library_dir = root / "usr" / "lib" / triplet
    for library in ("libxcb.so", "libxkbcommon.so", "libxkbcommon-x11.so"):
        if not (library_dir / library).exists():
            raise RuntimeError(f"sysroot did not provide {library_dir / library}")

    stamp.write_text(json.dumps(expected_stamp, indent=2) + "\n")
    print(root)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
