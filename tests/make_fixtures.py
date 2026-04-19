#!/usr/bin/env python3
"""Create all JPEG test fixtures.

Usage: python3 tests/make_fixtures.py

TIFF inline rule: if count × type_size ≤ 4, value is stored inline in the
value/offset field (left-justified, padded with zeros in little-endian TIFF).
ASCII refs like "N\0" (2 bytes) must therefore be stored inline, not as offsets.
"""
import struct, os, io
from PIL import Image, ImageDraw

FIXTURES = os.path.join(os.path.dirname(__file__), "fixtures")
os.makedirs(FIXTURES, exist_ok=True)


# ── TIFF binary helpers (little-endian) ───────────────────────────────────────

def le(fmt, *args):
    return struct.pack("<" + fmt, *args)


def ifd_entry(tag: int, typ: int, count: int, value: int) -> bytes:
    """12-byte IFD entry. Pass inline value or file offset as `value`."""
    return le("HHII", tag, typ, count, value)


def tiff_header(ifd0_offset: int = 8) -> bytes:
    return b"II" + le("H", 42) + le("I", ifd0_offset)


def rational(num: int, den: int) -> bytes:
    return le("II", num, den)


def ascii_inline(s: str) -> int:
    """Encode ≤4-char ASCII string as inline 32-bit little-endian value."""
    padded = s.encode() + b"\x00" * (4 - len(s))
    return struct.unpack("<I", padded[:4])[0]


def jpeg_with_exif(tiff_data: bytes) -> bytes:
    payload = b"Exif\x00\x00" + tiff_data
    app1_len = len(payload) + 2
    return b"\xff\xd8" + b"\xff\xe1" + struct.pack(">H", app1_len) + payload + b"\xff\xd9"


# ── with_exif.jpg ─────────────────────────────────────────────────────────────
#
# IFD0 (5 entries): Make, Model, DateTime, ExifIFD, GPSIFD
# ExifIFD (1 entry): DateTimeOriginal
# GPSIFD (4 entries, sorted by tag):
#   0x0001 GPSLatitudeRef  "N\0" inline
#   0x0002 GPSLatitude     3 rationals → 37°46'29.64" ≈ 37.7749
#   0x0003 GPSLongitudeRef "W\0" inline
#   0x0004 GPSLongitude    3 rationals → 122°25'9.72" ≈ 122.4194

def make_with_exif_tiff() -> bytes:
    make_str  = b"Apple\x00"               # 6 bytes
    model_str = b"iPhone 15 Pro\x00"       # 14 bytes
    dt_str    = b"2024:06:15 10:30:00\x00" # 20 bytes
    dt_orig   = dt_str                     # reused
    lat_rats  = rational(37,1) + rational(46,1) + rational(2964,100)   # 24 bytes
    lon_rats  = rational(122,1) + rational(25,1) + rational(972,100)   # 24 bytes

    # Layout:
    #   0-7:   TIFF header (IFD0 at 8)
    #   8-73:  IFD0 (2 + 5×12 + 4 = 66 bytes)
    #  74-91:  ExifIFD (2 + 1×12 + 4 = 18 bytes)
    #  92-145: GPSIFD (2 + 4×12 + 4 = 54 bytes)
    #  146:    make_str (6) → 152
    #  152:    model_str (14) → 166
    #  166:    dt_str (20) → 186
    #  186:    dt_orig (20) → 206
    #  206:    lat_rats (24) → 230
    #  230:    lon_rats (24) → 254
    exif_off = 74
    gps_off  = 92
    make_off  = 146
    model_off = 152
    dt_off    = 166
    dto_off   = 186
    latr_off  = 206
    lonr_off  = 230

    ifd0 = (
        le("H", 5)
        + ifd_entry(0x010f, 2, len(make_str),  make_off)
        + ifd_entry(0x0110, 2, len(model_str), model_off)
        + ifd_entry(0x0132, 2, 20,             dt_off)
        + ifd_entry(0x8769, 4, 1,              exif_off)
        + ifd_entry(0x8825, 4, 1,              gps_off)
        + le("I", 0)
    )
    exif_ifd = (
        le("H", 1)
        + ifd_entry(0x9003, 2, 20, dto_off)
        + le("I", 0)
    )
    gps_ifd = (
        le("H", 4)
        # Entries MUST be sorted ascending by tag:
        + ifd_entry(0x0001, 2, 2, ascii_inline("N\x00"))  # inline "N\0"
        + ifd_entry(0x0002, 5, 3, latr_off)               # 3 rationals
        + ifd_entry(0x0003, 2, 2, ascii_inline("W\x00"))  # inline "W\0"
        + ifd_entry(0x0004, 5, 3, lonr_off)               # 3 rationals
        + le("I", 0)
    )
    return (tiff_header() + ifd0 + exif_ifd + gps_ifd
            + make_str + model_str + dt_str + dt_orig + lat_rats + lon_rats)


def make_with_exif_jpeg() -> bytes:
    img = Image.new("RGB", (100, 100), (200, 120, 50))
    draw = ImageDraw.Draw(img)
    for y in range(0, 100, 10):
        for x in range(0, 100, 10):
            if (x // 10 + y // 10) % 2 == 0:
                draw.rectangle([x, y, x + 9, y + 9], fill=(50, 120, 200))
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=85)
    jpeg_data = buf.getvalue()

    tiff = make_with_exif_tiff()
    exif_payload = b"Exif\x00\x00" + tiff
    app1_len = len(exif_payload) + 2
    app1 = b"\xff\xe1" + struct.pack(">H", app1_len) + exif_payload
    return jpeg_data[:2] + app1 + jpeg_data[2:]


# ── no_exif.jpg ───────────────────────────────────────────────────────────────

def make_no_exif() -> bytes:
    img = Image.new("RGB", (60, 60), (180, 180, 180))
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=85)
    return buf.getvalue()


# ── different.jpg ─────────────────────────────────────────────────────────────

def make_different() -> bytes:
    img = Image.new("RGB", (80, 80), (20, 200, 80))
    draw = ImageDraw.Draw(img)
    draw.ellipse([10, 10, 70, 70], fill=(200, 20, 80))
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=85)
    return buf.getvalue()


# ── digitized_only.jpg ────────────────────────────────────────────────────────
#
# IFD0: ExifIFD pointer
# ExifIFD: DateTimeDigitized (0x9004) = "2024:07:20 09:15:00\0"

def make_digitized_only() -> bytes:
    dt = b"2024:07:20 09:15:00\x00"  # 20 bytes
    # Layout:
    #  0-7:   tiff header (IFD0 at 8)
    #  8-25:  IFD0 (2 + 1×12 + 4 = 18 bytes)
    # 26-43:  ExifIFD (2 + 1×12 + 4 = 18 bytes)
    # 44-63:  dt string
    exif_off = 26
    dt_off   = 44
    ifd0 = le("H", 1) + ifd_entry(0x8769, 4, 1, exif_off) + le("I", 0)
    exif_ifd = le("H", 1) + ifd_entry(0x9004, 2, 20, dt_off) + le("I", 0)
    return jpeg_with_exif(tiff_header() + ifd0 + exif_ifd + dt)


# ── gps_time_only.jpg ─────────────────────────────────────────────────────────
#
# IFD0: GPSIFD pointer
# GPSIFD (sorted): GPSTimeStamp (0x0007), GPSDateStamp (0x001d)
# GPSDateStamp = "2024:08:10\0" (11 bytes)
# GPSTimeStamp = 3 rationals: 14/1, 30/1, 0/1 → 14:30:00

def make_gps_time_only() -> bytes:
    date_str  = b"2024:08:10\x00"                              # 11 bytes
    time_rats = rational(14, 1) + rational(30, 1) + rational(0, 1)  # 24 bytes
    # Layout:
    #  0-7:   tiff header (IFD0 at 8)
    #  8-25:  IFD0 (18 bytes)
    # 26-55:  GPSIFD (2 + 2×12 + 4 = 30 bytes)
    # 56-66:  date_str (11 bytes)
    # 67-90:  time_rats (24 bytes)
    gps_off       = 26
    date_str_off  = 56
    time_rats_off = 67

    ifd0 = le("H", 1) + ifd_entry(0x8825, 4, 1, gps_off) + le("I", 0)
    gps_ifd = (
        le("H", 2)
        + ifd_entry(0x0007, 5, 3, time_rats_off)   # GPSTimeStamp (tag 7 < 29)
        + ifd_entry(0x001d, 2, 11, date_str_off)   # GPSDateStamp (tag 29)
        + le("I", 0)
    )
    return jpeg_with_exif(tiff_header() + ifd0 + gps_ifd + date_str + time_rats)


# ── datetime_only.jpg ─────────────────────────────────────────────────────────
#
# IFD0: DateTime (0x0132) = "2024:09:05 08:00:00\0"

def make_datetime_only() -> bytes:
    dt = b"2024:09:05 08:00:00\x00"  # 20 bytes
    dt_off = 26
    ifd0 = le("H", 1) + ifd_entry(0x0132, 2, 20, dt_off) + le("I", 0)
    return jpeg_with_exif(tiff_header() + ifd0 + dt)


# ── Write helpers ─────────────────────────────────────────────────────────────

def write(name: str, data: bytes) -> None:
    path = os.path.join(FIXTURES, name)
    with open(path, "wb") as f:
        f.write(data)
    print(f"wrote {path} ({len(data):,} bytes)")


if __name__ == "__main__":
    with_exif_data = make_with_exif_jpeg()
    write("with_exif.jpg", with_exif_data)

    img_full = Image.open(io.BytesIO(with_exif_data))
    w, h = img_full.size
    img_small = img_full.resize((w // 2, h // 2), Image.LANCZOS)
    buf = io.BytesIO()
    img_small.save(buf, "JPEG", quality=75)
    write("with_exif_small.jpg", buf.getvalue())

    write("no_exif.jpg",       make_no_exif())
    write("different.jpg",     make_different())
    write("digitized_only.jpg", make_digitized_only())
    write("gps_time_only.jpg",  make_gps_time_only())
    write("datetime_only.jpg",  make_datetime_only())

    print("done")
