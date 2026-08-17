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
    def test_authentication_errors_are_classified_without_provider_text(self):
        class UnauthorizedError(Exception):
            status_code = 401

        class ProviderContractError(AttributeError):
            pass

        class ProxyFailure(ConnectionError):
            pass

        self.assertEqual(HELPER.authentication_error_status(UnauthorizedError("account=private@example.invalid"), "credential_login"), "invalid_credentials")
        self.assertEqual(HELPER.authentication_error_status(UnauthorizedError("code=private"), "mfa_login"), "invalid_mfa")
        self.assertEqual(HELPER.authentication_error_status(ProviderContractError("missing private response"), "credential_login"), "sso_contract_error")
        self.assertEqual(HELPER.authentication_error_status(ProxyFailure("proxy password=secret"), "credential_login"), "network_error")
        self.assertEqual(HELPER.safe_exception_class(UnauthorizedError("private@example.invalid")), "UnauthorizedError")

    def test_mfa_and_dependency_messages_are_stable(self):
        self.assertEqual(HELPER.authentication_message("invalid_mfa"), "Garmin Connect rejected the one-time verification code")
        self.assertEqual(HELPER.authentication_message("network_error"), "Garmin Connect could not be reached; check network or proxy settings")

    def test_hash_locked_pypi_wheels_match_the_fake_provider_contract(self):
        requirements = pathlib.Path(__file__).with_name("requirements-garmin.txt").read_text()
        self.assertIn("garminconnect==0.3.10 --hash=sha256:a3fed44465df36981a6858f23e56417c6e7ac778550464440a5967007378ec88", requirements)
        self.assertIn("curl-cffi==0.16.0", requirements)
        self.assertNotIn("git+", requirements)
        self.assertNotIn("garth", requirements)

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

    def test_fake_garmin_mfa_token_restore_and_stale_token_refresh(self):
        events = []

        class FakeClient:
            def dump(self, directory):
                target = pathlib.Path(directory)
                target.mkdir(parents=True, exist_ok=True)
                (target / "garminconnect.json").write_text('{"fixture":"private-token"}')

        class FakeGarmin:
            stale_tokens = False

            def __init__(self, _email, _password, is_cn, prompt_mfa=None, return_on_mfa=False):
                self.is_cn = is_cn
                self.prompt_mfa = prompt_mfa
                self.return_on_mfa = return_on_mfa
                self.client = FakeClient()

            def login(self, tokenstore=None):
                if tokenstore is not None:
                    events.append("token_restore")
                    if type(self).stale_tokens:
                        raise ConnectionError("token=private-token")
                    return (None, None)
                events.append("credential_login")
                if self.return_on_mfa:
                    return ("needs_mfa", None)
                self.prompt_mfa()
                return (None, None)

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
                first = io.StringIO()
                with contextlib.redirect_stdout(first):
                    HELPER.command_auth(args)
                self.assertEqual(json.loads(first.getvalue())["status"], "mfa_required")
                self.assertFalse(HELPER.tokens_exist(args.tokens))

                args.mfa = "123456"
                second = io.StringIO()
                with contextlib.redirect_stdout(second):
                    HELPER.command_auth(args)
                self.assertEqual(json.loads(second.getvalue())["status"], "authenticated")
                self.assertTrue(HELPER.tokens_exist(args.tokens))
                self.assertEqual(HELPER.token_file(args.tokens).stat().st_mode & 0o777, 0o600)
                self.assertNotIn("private-token", second.getvalue())

                args.mfa = None
                third = io.StringIO()
                with contextlib.redirect_stdout(third):
                    HELPER.command_auth(args)
                self.assertEqual(json.loads(third.getvalue())["status"], "authenticated")
                self.assertEqual(events.count("token_restore"), 1)

                FakeGarmin.stale_tokens = True
                fourth = io.StringIO()
                with contextlib.redirect_stdout(fourth), contextlib.redirect_stderr(io.StringIO()):
                    HELPER.command_auth(args)
                self.assertEqual(json.loads(fourth.getvalue())["status"], "mfa_required")
                self.assertEqual(events.count("token_restore"), 2)
        finally:
            HELPER.Garmin = original_garmin

    def test_fake_garmin_download_is_acknowledged_only_after_import(self):
        class FakeClient:
            def dump(self, directory):
                target = pathlib.Path(directory)
                target.mkdir(parents=True, exist_ok=True)
                (target / "garminconnect.json").write_text("{}")

        class FakeGarmin:
            ActivityDownloadFormat = types.SimpleNamespace(ORIGINAL="original")
            def __init__(self, *_args, **_kwargs): self.client = FakeClient()
            def login(self, *_args, **_kwargs): return (None, None)
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
