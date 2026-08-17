"""Offline contract tests for the Garmin helper's staged FIT handling."""
import importlib.util
import io
import contextlib
import json
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

    def test_fake_garmin_download_is_acknowledged_only_after_import(self):
        class FakeGarth:
            profile = {"displayName": "fixture", "fullName": "Fixture"}
            def login(self, *_args, **_kwargs): pass
            def dump(self, directory):
                target = pathlib.Path(directory)
                target.mkdir(parents=True, exist_ok=True)
                (target / "oauth1_token.json").write_text("{}")
                (target / "oauth2_token.json").write_text("{}")

        class FakeGarmin:
            ActivityDownloadFormat = types.SimpleNamespace(ORIGINAL="original")
            def __init__(self, *_args, **_kwargs): self.garth = FakeGarth()
            def get_activities(self, _offset, _limit): return [{"activityId": 42, "startTimeGMT": "2026-08-17T00:00:00Z"}]
            def download_activity(self, _identifier, dl_fmt):
                self.last_format = dl_fmt
                archive = io.BytesIO()
                with zipfile.ZipFile(archive, "w") as output:
                    output.writestr("inside.fit", fit_bytes())
                return archive.getvalue()

        original_garmin = HELPER.Garmin
        HELPER.Garmin = FakeGarmin
        try:
            with tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                args = types.SimpleNamespace(
                    output=root / "downloads", state=root / "journal.json", tokens=root / "tokens",
                    email="fixture@example.invalid", password="fixture", mfa=None, after=None, max_pages=1,
                    ids=[],
                )
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    HELPER.command_sync(args)
                result = json.loads(output.getvalue())
                self.assertEqual(result["status"], "ready")
                self.assertEqual(result["items"][0]["id"], "42")
                self.assertEqual((root / "downloads" / "42.fit").read_bytes(), fit_bytes())
                self.assertEqual(HELPER.load(args.state)["items"]["42"]["state"], "downloaded")
                args.ids = ["42"]
                with contextlib.redirect_stdout(io.StringIO()):
                    HELPER.command_ack(args)
                state = HELPER.load(args.state)
                self.assertEqual(state["items"]["42"]["state"], "imported")
                self.assertEqual(state["checkpoint"], "2026-08-17T00:00:00Z")
        finally:
            HELPER.Garmin = original_garmin


if __name__ == "__main__":
    unittest.main()
