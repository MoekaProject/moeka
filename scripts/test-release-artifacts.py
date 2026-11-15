#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "release_artifacts", Path(__file__).with_name("release-artifacts.py")
)
artifacts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(artifacts)


class ReleaseArtifactsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.inputs = self.root / "incoming"
        self.expected = artifacts.identity("9.8.7", "a" * 40, "123")
        for (platform, arch), names in artifacts.GROUPS.items():
            directory = self.inputs / f"{platform}-{arch}"
            directory.mkdir(parents=True)
            for name in names:
                (directory / name).write_bytes(name.encode())
            artifacts.record(directory, platform, arch, self.expected)

    def test_exact_eight_assets_keep_identity_and_detect_later_changes(self):
        output = self.root / "dist"
        artifacts.collect(self.inputs, output, self.expected)
        artifacts.verify(output, self.expected)
        self.assertEqual(8, len(list(output.glob("*.zip"))) + len(list(output.glob("*.dmg"))))
        (output / "Moeka-macos-arm64.dmg").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "changed"):
            artifacts.verify(output, self.expected)

    def test_rejects_other_runs_commits_versions_and_missing_architectures_before_copy(self):
        for key, value in [("run_id", "124"), ("source_sha", "b" * 40), ("version", "9.8.8")]:
            with self.subTest(key=key):
                with self.assertRaisesRegex(ValueError, "different version"):
                    artifacts.collect(self.inputs, self.root / "dist", {**self.expected, key: value})
                self.assertFalse((self.root / "dist").exists())
        (self.inputs / "windows-arm64" / "manifest.json").unlink()
        with self.assertRaisesRegex(ValueError, "Missing platform"):
            artifacts.collect(self.inputs, self.root / "dist", self.expected)

    def test_rejects_bad_digest_duplicate_group_and_filename_traversal(self):
        manifest = self.inputs / "macos-arm64" / "manifest.json"
        original = json.loads(manifest.read_text())
        for mutate in (
            lambda data: data["assets"][0].update(sha256="0" * 64),
            lambda data: data.update(platform="windows"),
            lambda data: data["assets"][0].update(name="../outside.zip"),
        ):
            data = json.loads(json.dumps(original))
            mutate(data)
            manifest.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                artifacts.collect(self.inputs, self.root / "dist", self.expected)
            self.assertFalse((self.root / "dist").exists())
        manifest.write_text(json.dumps(original))
        duplicate = self.inputs / "duplicate"
        duplicate.mkdir()
        (duplicate / "manifest.json").write_text(json.dumps(original))
        for asset in original["assets"]:
            (duplicate / asset["name"]).write_bytes((manifest.parent / asset["name"]).read_bytes())
        with self.assertRaisesRegex(ValueError, "duplicate"):
            artifacts.collect(self.inputs, self.root / "dist", self.expected)

    def test_rejects_symlinked_artifact(self):
        target = self.inputs / "linux-arm64" / "Moeka-linux-arm64.zip"
        original = target.read_bytes()
        target.unlink()
        outside = self.root / "outside.zip"
        outside.write_bytes(original)
        target.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, "symlinked"):
            artifacts.collect(self.inputs, self.root / "dist", self.expected)


if __name__ == "__main__":
    unittest.main()
