"""Offline contract tests for the Garmin helper's staged FIT handling."""
import importlib.util
import io
import pathlib
import sys
import tempfile
import types
import unittest
import zipfile


sys.modules.setdefault("garminconnect", types.SimpleNamespace(Garmin=object))
MODULE = pathlib.Path(__file__).with_name("garmin_sync.py")
SPEC = importlib.util.spec_from_file_location("garmin_sync", MODULE)
HELPER = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(HELPER)


def fit_bytes():
    return b"\x0e\x10\x00\x00\x00\x00\x00\x00.FIT" + b"fixture"


class GarminHelperTests(unittest.TestCase):
    def test_extracts_the_single_fit_from_an_original_archive(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("activity.fit", fit_bytes())
        self.assertEqual(HELPER.original_fit(archive.getvalue()), fit_bytes())

    def test_refuses_archives_without_exactly_one_valid_fit(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("first.fit", fit_bytes())
            output.writestr("second.fit", fit_bytes())
        with self.assertRaisesRegex(ValueError, "exactly_one_fit"):
            HELPER.original_fit(archive.getvalue())

    def test_journal_write_is_atomic_and_private(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "journal.json"
            HELPER.save(path, {"version": 2, "items": {}, "checkpoint": None})
            self.assertEqual(HELPER.load(path)["version"], 2)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
