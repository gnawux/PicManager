#!/usr/bin/env python3
"""Create JPEG test fixtures with specific EXIF fields in APP1 (not XMP).

Usage: python3 tests/make_fixtures.py
"""
import struct, os

FIXTURES = os.path.join(os.path.dirname(__file__), "fixtures")


def pack_le(fmt, *args):
    return struct.pack("<" + fmt, *args)


def ifd_entry(tag, type_, count, value_or_offset):
    """12-byte little-endian IFD entry."""
    return pack_le("HHII", tag, type_, count, value_or_offset)


def jpeg_with_exif(tiff_data: bytes) -> bytes:
    """Wrap raw TIFF data in a minimal JPEG file."""
    app1_payload = b"Exif\x00\x00" + tiff_data
    app1_len = len(app1_payload) + 2          # length field includes itself
    app1 = b"\xff\xe1" + struct.pack(">H", app1_len) + app1_payload
    return b"\xff\xd8" + app1 + b"\xff\xd9"  # SOI + APP1 + EOI


def make_digitized_only() -> bytes:
    """JPEG with only DateTimeDigitized (0x9004) in ExifIFD.

    Layout (offsets from TIFF header start):
      0-7:   TIFF header (II, 0x002A, IFD0 at 8)
      8-25:  IFD0 (1 entry: ExifIFD pointer -> 26)
      26-43: ExifIFD (1 entry: DateTimeDigitized -> 44)
      44-63: ASCII "2024:07:20 09:15:00\0"
    """
    dt = b"2024:07:20 09:15:00\x00"  # 20 bytes
    assert len(dt) == 20

    exif_ifd_offset = 26
    data_offset = 44

    tiff_header = b"II" + pack_le("H", 42) + pack_le("I", 8)

    ifd0 = (
        pack_le("H", 1)
        + ifd_entry(0x8769, 4, 1, exif_ifd_offset)  # ExifIFD pointer
        + pack_le("I", 0)                            # next IFD = none
    )

    exif_ifd = (
        pack_le("H", 1)
        + ifd_entry(0x9004, 2, 20, data_offset)  # DateTimeDigitized
        + pack_le("I", 0)
    )

    return jpeg_with_exif(tiff_header + ifd0 + exif_ifd + dt)


def make_datetime_only() -> bytes:
    """JPEG with only DateTime (0x0132) in IFD0.

    Layout:
      0-7:   TIFF header
      8-25:  IFD0 (1 entry: DateTime -> 26)
      26-45: ASCII "2024:09:05 08:00:00\0"
    """
    dt = b"2024:09:05 08:00:00\x00"  # 20 bytes
    assert len(dt) == 20

    data_offset = 26

    tiff_header = b"II" + pack_le("H", 42) + pack_le("I", 8)

    ifd0 = (
        pack_le("H", 1)
        + ifd_entry(0x0132, 2, 20, data_offset)  # DateTime
        + pack_le("I", 0)
    )

    return jpeg_with_exif(tiff_header + ifd0 + dt)


def write(name, data):
    path = os.path.join(FIXTURES, name)
    with open(path, "wb") as f:
        f.write(data)
    print(f"wrote {path} ({len(data)} bytes)")


if __name__ == "__main__":
    write("digitized_only.jpg", make_digitized_only())
    write("datetime_only.jpg", make_datetime_only())
    print("done")
