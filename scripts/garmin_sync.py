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


class MfaRequired(Exception):
    pass


def emit(status, **fields):
    print(json.dumps({"status": status, **fields}, sort_keys=True))


def safe_exception_class(error):
    """Return only a bounded exception class name, never provider response text."""
    return "".join(char for char in type(error).__name__ if char.isalnum() or char == "_")[:80] or "UnknownError"


def safe_http_status(error):
    response = getattr(error, "response", None)
    status = getattr(response, "status_code", None) or getattr(error, "status_code", None)
    return status if isinstance(status, int) and 100 <= status <= 599 else None


def authentication_error_status(error):
    """Map provider failures to stable, user-safe codes without leaking exception text."""
    name = safe_exception_class(error).lower()
    text = str(error).lower()
    http_status = safe_http_status(error)
    if "mfa" in name or "verification" in name or "mfa" in text:
        return "mfa_required"
    if "token" in name and any(word in name for word in ("expired", "invalid", "stale")):
        return "stale_token"
    if http_status in (401, 403) or "credential" in name or "authentication" in name or "unauthorized" in name:
        return "invalid_credentials"
    if http_status == 429 or (http_status is not None and http_status >= 500):
        return "network_error"
    if isinstance(error, (socket.timeout, TimeoutError, ConnectionError, OSError)):
        return "network_error"
    if any(word in name or word in text for word in ("proxy", "connect", "timeout", "dns", "ssl")):
        return "network_error"
    if isinstance(error, (AttributeError, KeyError, TypeError, ImportError, ModuleNotFoundError)):
        return "sso_contract_error"
    return "authentication_failed"


def log_authentication_failure(error, phase):
    """Diagnostics deliberately exclude email, response body, URL, and exception message."""
    status = authentication_error_status(error)
    http_status = safe_http_status(error)
    fields = [f"phase={phase}", f"status={status}", f"class={safe_exception_class(error)}"]
    if http_status is not None:
        fields.append(f"http_status={http_status}")
    print("garmin_auth_failure " + " ".join(fields), file=sys.stderr)


def authentication_message(status):
    return {
        "invalid_credentials": "Garmin Connect rejected the saved account or password",
        "mfa_required": "Garmin Connect requires a one-time verification code",
        "network_error": "Garmin Connect could not be reached; check network or proxy settings",
        "stale_token": "The saved Garmin sign-in token has expired; verify the account again",
        "sso_contract_error": "The Garmin sign-in protocol changed or this bundled client is incompatible",
        "dependency_error": "The bundled Garmin client dependency is unavailable",
    }.get(status, "Garmin Connect could not complete authentication")


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


def tokens_exist(token_dir):
    return (token_dir / "oauth1_token.json").is_file() and (token_dir / "oauth2_token.json").is_file()


def authenticate(args):
    if DEPENDENCY_IMPORT_ERROR is not None or Garmin is None:
        log_authentication_failure(DEPENDENCY_IMPORT_ERROR, "dependency_import")
        emit("dependency_error", error_code="dependency_error", message=authentication_message("dependency_error"), diagnostic=safe_exception_class(DEPENDENCY_IMPORT_ERROR))
        return None
    password = args.password or os.environ.get("PICMANAGER_GARMIN_PASSWORD")
    if not password:
        emit("not_configured", message="Garmin credentials are not available to the local service")
        return None
    api = Garmin(args.email, password, is_cn=True)
    if tokens_exist(args.tokens):
        try:
            api.login(tokenstore=str(args.tokens))
            return api
        except Exception as error:
            # A stale token is not a usable authentication result. Preserve no secrets
            # and let the normal sign-in path obtain a fresh token below.
            log_authentication_failure(error, "token_refresh")

    def prompt_mfa():
        if args.mfa:
            return args.mfa
        raise MfaRequired()

    try:
        api.garth.login(args.email, password, prompt_mfa=prompt_mfa)
        api.display_name = api.garth.profile["displayName"]
        api.full_name = api.garth.profile.get("fullName")
        api.garth.dump(str(args.tokens))
        for token in args.tokens.glob("*.json"):
            os.chmod(token, 0o600)
        return api
    except MfaRequired:
        emit("mfa_required", error_code="mfa_required", message=authentication_message("mfa_required"))
        return None
    except Exception as error:
        status = authentication_error_status(error)
        log_authentication_failure(error, "password_login")
        emit(status, error_code=status, message=authentication_message(status), diagnostic=safe_exception_class(error), http_status=safe_http_status(error))
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
        activities = api.get_activities(offset, 100)
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
            except Exception:
                emit("download_failed", message="Garmin returned an invalid activity download")
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
    api.garth.dump(str(args.tokens))
    for token in args.tokens.glob("*.json"):
        os.chmod(token, 0o600)
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
    parser.add_argument("--after")
    parser.add_argument("--max-pages", type=int, default=20)
    parser.add_argument("--ids", nargs="*", default=[])
    args = parser.parse_args()
    if args.command == "auth":
        command_auth(args)
    elif args.command == "sync":
        command_sync(args)
    else:
        command_ack(args)


if __name__ == "__main__":
    main()
