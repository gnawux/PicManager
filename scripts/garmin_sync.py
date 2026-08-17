#!/usr/bin/env python3
"""Garmin Connect China helper with JSON-only, resumable local state.

The Rust service owns catalogue import and only acknowledges an activity after a FIT
file has been parsed successfully.  This helper owns remote authentication, token
refresh, download staging and the corresponding write-ahead journal.
"""
import argparse
import hashlib
import io
import json
import os
import pathlib
import socket
import sys
import tempfile
import zipfile

try:
    from garminconnect import Garmin
    DEPENDENCY_IMPORT_ERROR = None
except ImportError as error:  # reported as safe JSON after argparse has initialized
    Garmin = None
    DEPENDENCY_IMPORT_ERROR = error

try:
    from garminconnect import (
        GarminConnectAuthenticationError,
        GarminConnectConnectionError,
        GarminConnectTooManyRequestsError,
    )
except ImportError:
    # Keep the helper importable for the isolated fake-provider test suite. The
    # real 0.3.10 bundle exports all three classes.
    GarminConnectAuthenticationError = ()
    GarminConnectConnectionError = ()
    GarminConnectTooManyRequestsError = ()


def emit(status, **fields):
    print(json.dumps({"status": status, **fields}, sort_keys=True))


def safe_exception_class(error):
    """Return only a bounded exception class name, never provider response text."""
    return "".join(char for char in type(error).__name__ if char.isalnum() or char == "_")[:80] or "UnknownError"


def safe_http_status(error):
    response = getattr(error, "response", None)
    status = getattr(response, "status_code", None) or getattr(error, "status_code", None)
    return status if isinstance(status, int) and 100 <= status <= 599 else None


def authentication_error_status(error, phase):
    """Map provider failures to stable codes without inspecting provider text."""
    http_status = safe_http_status(error)
    if isinstance(error, GarminConnectTooManyRequestsError) or http_status == 429:
        return "rate_limited"
    if isinstance(error, (socket.timeout, TimeoutError, ConnectionError, OSError)):
        return "network_error"
    if isinstance(error, GarminConnectConnectionError) or (http_status is not None and http_status >= 500):
        return "network_error"
    if isinstance(error, GarminConnectAuthenticationError) or http_status in (401, 403):
        return "invalid_mfa" if phase == "mfa_login" else "invalid_credentials"
    if isinstance(error, (AssertionError, AttributeError, KeyError, TypeError, ImportError, ModuleNotFoundError)):
        return "sso_contract_error"
    return "sso_contract_error"


def log_authentication_failure(error, phase, status):
    """Diagnostics deliberately exclude email, response body, URL, and exception message."""
    http_status = safe_http_status(error)
    fields = [f"phase={phase}", f"status={status}", f"class={safe_exception_class(error)}"]
    if http_status is not None:
        fields.append(f"http_status={http_status}")
    print("garmin_auth_failure " + " ".join(fields), file=sys.stderr)


def authentication_message(status):
    return {
        "invalid_credentials": "Garmin Connect rejected the saved account or password",
        "invalid_mfa": "Garmin Connect rejected the one-time verification code",
        "mfa_required": "Garmin Connect requires a one-time verification code",
        "network_error": "Garmin Connect could not be reached; check network or proxy settings",
        "rate_limited": "Garmin Connect is temporarily rate limiting sign-in attempts",
        "sso_contract_error": "The Garmin sign-in protocol changed or this bundled client is incompatible",
        "dependency_error": "The bundled Garmin client dependency is unavailable",
        "token_store_error": "The local Garmin token store could not be updated",
    }.get(status, "Garmin Connect could not complete authentication")


def emit_failure(status, error, phase, message=None):
    """Emit a literal, redacted helper diagnostic contract.

    Provider exception text can contain account data, response bodies, signed URLs,
    or token fragments. Only the stable status, phase, exception class and bounded
    HTTP status cross this process boundary.
    """
    log_authentication_failure(error, phase, status)
    emit(
        status,
        error_code=status,
        message=message or authentication_message(status),
        phase=phase,
        exception_class=safe_exception_class(error),
        http_status=safe_http_status(error),
    )


def load(path):
    try:
        state = json.loads(path.read_text())
    except FileNotFoundError:
        state = {"version": 2, "items": {}, "checkpoint": None}
    if state.get("version") != 2:
        state = {"version": 2, "items": {}, "checkpoint": None}
    state.setdefault("items", {})
    state.setdefault("checkpoint", None)
    return state


def save(path, state):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", dir=path.parent, delete=False) as file:
        json.dump(state, file, sort_keys=True)
        file.flush()
        os.fsync(file.fileno())
        name = file.name
    os.replace(name, path)
    os.chmod(path, 0o600)


def valid_fit(data):
    return len(data) >= 12 and data[8:12] == b".FIT"


def original_fit(data):
    if valid_fit(data):
        return data
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        candidates = [name for name in archive.namelist() if name.lower().endswith(".fit") and not name.endswith("/")]
        if len(candidates) != 1:
            raise ValueError("original_archive_requires_exactly_one_fit")
        fit = archive.read(candidates[0])
    if not valid_fit(fit):
        raise ValueError("original_archive_contains_invalid_fit")
    return fit


def write_atomic(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("wb", dir=path.parent, delete=False) as file:
        file.write(data)
        file.flush()
        os.fsync(file.fileno())
        name = file.name
    os.replace(name, path)


# garminconnect 0.3.10 treats a directory token store as this exact filename.
# Keep this explicit: the Rust status endpoint uses the same provider contract.
TOKEN_STORE_FILENAME = "garmin_tokens.json"


def token_file(token_dir):
    return token_dir / TOKEN_STORE_FILENAME


def tokens_exist(token_dir):
    return token_file(token_dir).is_file()


def dump_tokens(api, token_dir):
    """Persist the 0.3.10 token store with private permissions only."""
    token_dir.mkdir(parents=True, exist_ok=True)
    os.chmod(token_dir, 0o700)
    api.client.dump(str(token_dir))
    # The provider owns the atomic write. Enforce owner-only permissions again
    # for deterministic behavior with fake providers and restrictive umasks.
    for token in token_dir.glob("*.json"):
        os.chmod(token, 0o600)


def authenticate(args):
    if DEPENDENCY_IMPORT_ERROR is not None or Garmin is None:
        emit_failure("dependency_error", DEPENDENCY_IMPORT_ERROR, "dependency_import")
        return None
    password = args.password or os.environ.get("PICMANAGER_GARMIN_PASSWORD")
    if not password:
        emit("not_configured", message="Garmin credentials are not available to the local service")
        return None

    # Restore valid OAuth tokens first. A rejected or malformed token is never
    # returned as an authentication result: the helper makes a new credential
    # attempt below, which can independently request MFA.
    if tokens_exist(args.tokens):
        try:
            api = Garmin(args.email, password, is_cn=True)
            api.login(str(args.tokens))
            dump_tokens(api, args.tokens)
            return api
        except Exception as error:
            # A stale token falls through to a credential refresh. Logging keeps
            # the phase/class/status available without exposing the token or body.
            status = authentication_error_status(error, "token_restore")
            log_authentication_failure(error, "token_restore", status)

    try:
        if args.mfa:
            # 0.3.10 keeps the challenge inside the client object. The helper is
            # intentionally short-lived, so a submitted code starts a clean SSO
            # exchange with the code supplied through the prompt callback.
            api = Garmin(args.email, password, is_cn=True, prompt_mfa=lambda: args.mfa)
            api.login()
        else:
            # The first probe returns a structured MFA state instead of asserting
            # against the old China success-page title.
            api = Garmin(args.email, password, is_cn=True, return_on_mfa=True)
            mfa_status, _ = api.login()
            if mfa_status in {"needs_mfa", "mfa_required"}:
                emit("mfa_required", error_code="mfa_required", message=authentication_message("mfa_required"), phase="credential_login")
                return None
            if mfa_status is not None:
                emit("sso_contract_error", error_code="sso_contract_error", message=authentication_message("sso_contract_error"), phase="credential_login", exception_class="UnexpectedMfaStatus")
                return None
        dump_tokens(api, args.tokens)
        return api
    except Exception as error:
        phase = "mfa_login" if args.mfa else "credential_login"
        status = authentication_error_status(error, phase)
        emit_failure(status, error, phase)
        return None


def command_auth(args):
    api = authenticate(args)
    if api is not None:
        emit("authenticated")


def activity_start(activity):
    return activity.get("startTimeGMT") or activity.get("startTimeLocal") or ""


def command_sync(args):
    api = authenticate(args)
    if api is None:
        return
    state = load(args.state)
    cutoff = state.get("checkpoint") or args.after
    downloaded = 0
    offset = 0
    pages = 0
    while pages < args.max_pages:
        try:
            activities = api.get_activities(offset, 100)
        except Exception as error:
            status = authentication_error_status(error, "activity_list")
            emit_failure(status, error, "activity_list")
            return
        if not activities:
            break
        pages += 1
        older_than_cutoff = False
        for activity in sorted(activities, key=activity_start):
            identifier = str(activity["activityId"])
            start = activity_start(activity)
            if cutoff and start and start <= cutoff:
                older_than_cutoff = True
                continue
            item = state["items"].get(identifier)
            if item and item.get("state") in {"downloaded", "imported"}:
                continue
            try:
                original = api.download_activity(activity["activityId"], dl_fmt=api.ActivityDownloadFormat.ORIGINAL)
                fit = original_fit(original)
            except Exception as error:
                if isinstance(error, ValueError):
                    emit_failure("download_failed", error, "download", "Garmin returned an invalid activity download")
                else:
                    status = authentication_error_status(error, "download")
                    emit_failure(status, error, "download")
                return
            target = args.output / f"{identifier}.fit"
            write_atomic(target, fit)
            state["items"][identifier] = {
                "state": "downloaded",
                "start_time": start,
                "sha256": hashlib.sha256(fit).hexdigest(),
            }
            save(args.state, state)
            downloaded += 1
        if len(activities) < 100 or older_than_cutoff:
            break
        offset += len(activities)
    pending = [
        {"id": identifier, "file": f"{identifier}.fit", "start_time": item.get("start_time")}
        for identifier, item in state["items"].items()
        if item.get("state") == "downloaded"
    ]
    pending.sort(key=lambda item: item.get("start_time") or "")
    try:
        dump_tokens(api, args.tokens)
    except Exception as error:
        emit_failure("token_store_error", error, "token_persist")
        return
    emit("ready", downloaded=downloaded, items=pending)


def command_ack(args):
    state = load(args.state)
    for identifier in args.ids:
        item = state["items"].get(str(identifier))
        if item and item.get("state") == "downloaded":
            item["state"] = "imported"
            if item.get("start_time"):
                state["checkpoint"] = item["start_time"]
    save(args.state, state)
    emit("acknowledged", checkpoint=state.get("checkpoint"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["auth", "sync", "ack"])
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--state", type=pathlib.Path, required=True)
    parser.add_argument("--tokens", type=pathlib.Path, required=True)
    parser.add_argument("--email", required=False, default="")
    parser.add_argument("--password")
    parser.add_argument("--mfa")
    parser.add_argument("--mfa-stdin", action="store_true")
    parser.add_argument("--after")
    parser.add_argument("--max-pages", type=int, default=20)
    parser.add_argument("--ids", nargs="*", default=[])
    args = parser.parse_args()
    if args.mfa_stdin:
        # MFA must not appear in the child command line or diagnostic output.
        args.mfa = sys.stdin.readline().strip()
    if args.command == "auth":
        command_auth(args)
    elif args.command == "sync":
        command_sync(args)
    else:
        command_ack(args)


if __name__ == "__main__":
    main()
