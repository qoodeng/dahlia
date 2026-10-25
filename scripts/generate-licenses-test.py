#!/usr/bin/env python3
import importlib.util
import sys
import unittest
from pathlib import Path
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("licenses", Path(__file__).with_name("generate-licenses.py"))
licenses = importlib.util.module_from_spec(spec)
spec.loader.exec_module(licenses)


class LicenseTests(unittest.TestCase):
    def inventory(self, name="example", source="registry"):
        return {"licenses": [{"name": "MIT", "text": "permission terms", "source_path": None,
                              "used_by": [{"crate": {"name": name, "version": "1.0.0", "source": source,
                                           "authors": ["Original Author"], "repository": "https://example.invalid/source"}}]}]}

    def test_missing_attribution_is_not_silently_certified(self):
        text, record = licenses.render(self.inventory(), [], b"lock", "target")
        self.assertEqual(record["unresolved_attribution"], ["example@1.0.0"])
        self.assertIn(b"Original Author", text)
        self.assertIn(b"permission terms", text)
        self.assertEqual(record["notices_sha256"], licenses.digest(text))

    def test_recovered_original_notice_is_preserved_and_checked(self):
        original = "Copyright Original Author\nPermission granted."
        sources = [{"repo": "upstream/example", "crates": ["example@1.0.0"], "files": [
            {"url": "https://example.invalid/blob", "text": original,
             "sha256": licenses.digest(original.encode())}]}]
        text, record = licenses.render(self.inventory(), sources, b"lock", "target")
        self.assertEqual(record["unresolved_attribution"], [])
        self.assertIn(original.encode(), text)
        sources[0]["files"][0]["text"] = "altered"
        with self.assertRaisesRegex(ValueError, "checksum differs"):
            licenses.render(self.inventory(), sources, b"lock", "target")

    def test_complete_original_package_preserves_notice_provenance(self):
        archive = {"path": "third-party-sources/example-1.0.0.crate", "sha256": "a" * 64,
                   "url": "https://crates.io/api/v1/crates/example/1.0.0/download"}
        sources = [{"repo": "upstream/example", "crates": ["example@1.0.0"], "files": [], "archive": archive}]
        text, record = licenses.render(self.inventory(), sources, b"lock", "target")
        self.assertEqual(record["unresolved_attribution"], [])
        self.assertEqual(record["source_archives"], [archive])
        self.assertIn(archive["path"].encode(), text)

    def test_separate_markraft_license_covers_only_local_pinned_crates(self):
        _, local = licenses.render(self.inventory("markraft-core", None), [], b"lock", "target")
        self.assertEqual(local["unresolved_attribution"], [])
        _, registry = licenses.render(self.inventory("markraft-core"), [], b"lock", "target")
        self.assertEqual(registry["unresolved_attribution"], ["markraft-core@1.0.0"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
