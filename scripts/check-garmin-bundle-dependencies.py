#!/usr/bin/env python3
"""Verify the bundled Garmin dependency identity, license metadata, and payload size."""
from importlib import metadata
from pathlib import Path
import sys


EXPECTED_GARMIN_VERSION = "0.3.10"
MAX_GARMIN_PAYLOAD_BYTES = 32 * 1024 * 1024
REQUIRED_DISTRIBUTIONS = ("curl_cffi", "requests", "ua-generator")


def distribution_size(distribution):
    total = 0
    for file in distribution.files or ():
        path = distribution.locate_file(file)
        if path.is_file():
            total += path.stat().st_size
    return total


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check-garmin-bundle-dependencies.py /path/to/PicManager.app")
    app_path = Path(sys.argv[1]).resolve()
    if not (app_path / "Contents/Resources/python/bin/python3").is_file():
        raise SystemExit("Garmin dependency verification requires a bundled Python runtime")

    garmin = metadata.distribution("garminconnect")
    if garmin.version != EXPECTED_GARMIN_VERSION:
        raise SystemExit(f"expected garminconnect {EXPECTED_GARMIN_VERSION}, found {garmin.version}")
    license_name = (garmin.metadata.get("License") or "").strip()
    if license_name != "MIT":
        raise SystemExit(f"expected garminconnect MIT license metadata, found {license_name!r}")
    try:
        legacy = metadata.version("garth")
    except metadata.PackageNotFoundError:
        legacy = None
    if legacy is not None:
        raise SystemExit(f"obsolete garth {legacy} is present in the Garmin runtime")

    required = [metadata.distribution(name) for name in REQUIRED_DISTRIBUTIONS]
    payload_bytes = sum(distribution_size(distribution) for distribution in [garmin, *required])
    if payload_bytes > MAX_GARMIN_PAYLOAD_BYTES:
        raise SystemExit(
            f"Garmin dependency payload is {payload_bytes} bytes; limit is {MAX_GARMIN_PAYLOAD_BYTES} bytes"
        )
    required_versions = ", ".join(f"{item.metadata['Name']}={item.version}" for item in required)
    print(
        f"Validated garminconnect={garmin.version} license={license_name} "
        f"payload_bytes={payload_bytes} dependencies={required_versions}"
    )


if __name__ == "__main__":
    main()
