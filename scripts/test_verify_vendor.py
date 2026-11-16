"""Synthetic snapshot tests; never mutate vendor/."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from verify_vendor import verify


class SnapshotIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="loj-vendor-test-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        (self.root / "assets").mkdir()
        self.content = b"\xef\xbb\xbfline\r\nnext\r\x00\xff"
        (self.root / "assets/input.bin").write_bytes(self.content)
        (self.root / "COMMIT").write_text("0" * 40 + "\n", encoding="utf-8")
        self.manifest = {"assets/input.bin": hashlib.sha256(self.content).hexdigest()}
        self.write_manifest(self.manifest)

    def write_manifest(self, data):
        (self.root / "SHA256SUMS.json").write_text(json.dumps(data), encoding="utf-8")

    def test_exact_raw_bytes_pass(self):
        self.assertEqual(verify(self.root), [])

    def test_modified_bytes_fail(self):
        (self.root / "assets/input.bin").write_bytes(self.content.replace(b"\r\n", b"\n"))
        self.assertIn("SHA-256 mismatch: assets/input.bin", verify(self.root))

    def test_missing_and_unlisted_files_fail(self):
        (self.root / "assets/input.bin").unlink()
        (self.root / "extra.bin").write_bytes(b"extra")
        errors = verify(self.root)
        self.assertIn("missing asset: assets/input.bin", errors)
        self.assertIn("unlisted asset: extra.bin", errors)

    def test_missing_metadata_fails(self):
        (self.root / "COMMIT").unlink()
        self.assertTrue(verify(self.root))

    def test_missing_manifest_and_snapshot_fail(self):
        (self.root / "SHA256SUMS.json").unlink()
        self.assertTrue(verify(self.root))
        self.assertTrue(verify(self.root / "absent"))

    def test_invalid_manifests_fail(self):
        for data in [[], {}, {"../outside": "0" * 64}, {"/absolute": "0" * 64},
                     {"a\\b": "0" * 64}, {"assets/input.bin": "bad"},
                     {"COMMIT": "0" * 64}, {"assets/input.bin": 1}]:
            with self.subTest(data=data):
                self.write_manifest(data)
                self.assertTrue(verify(self.root))
        for text in ['{"a":"x","a":"y"}', '{']:
            (self.root / "SHA256SUMS.json").write_text(text, encoding="utf-8")
            self.assertTrue(verify(self.root))

    def test_symlink_is_rejected(self):
        link = self.root / "alias"
        try:
            link.symlink_to(self.root / "assets", target_is_directory=True)
        except OSError as error:
            self.skipTest(f"symlink creation unavailable: {error}")
        self.assertTrue(any("symlink" in error for error in verify(self.root)))
        link.unlink()
        asset = self.root / "assets/input.bin"
        asset.unlink()
        asset.symlink_to(self.root / "COMMIT")
        self.assertTrue(any("symlink" in error for error in verify(self.root)))


if __name__ == "__main__":
    unittest.main()
