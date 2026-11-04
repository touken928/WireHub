"""Keep generated release notes tied to the selected package version."""

import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "release_notes", Path(__file__).resolve().parents[1] / "scripts/release-notes.py"
)
NOTES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NOTES)


class ReleaseNotesTests(unittest.TestCase):
    def test_changes_and_downloads_use_selected_version(self):
        document = (
            "## 1.3.0 release\n\nNew QR import.\n\n## 1.2.0 release\n\nOld notes.\n"
        )
        result = NOTES.render("1.3.0", "touken928/WireHub", document)
        self.assertIn("New QR import.", result)
        self.assertNotIn("Old notes.", result)
        self.assertIn("/blob/v1.3.0/docs/operations.md", result)
        self.assertIn("wirehub-v1.3.0-linux-arm64", result)
        self.assertIn("ghcr.io/touken928/wirehub:1.3.0", result)

    def test_missing_version_fails_before_publication(self):
        with self.assertRaisesRegex(ValueError, "Missing release notes for 1.3.0"):
            NOTES.render("1.3.0", "touken928/WireHub", "## 1.2.0 release\nOld notes.\n")


if __name__ == "__main__":
    unittest.main()
