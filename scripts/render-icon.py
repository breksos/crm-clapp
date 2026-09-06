#!/usr/bin/env python3
"""Draw the app mark, and derive the Windows .ico from the same pixels.

**This is a placeholder — M6 owns the real brand.** It is here because two things
refuse to exist without it: `clatch validate` fails on a declared `icon` that is not on
disk, and `tauri-build` fails without an `.ico` in `bundle.icon` even with bundling off.
So M0 ships a mark that is obviously provisional rather than a build that cannot run.

It is a *script*, not a hand-drawn PNG, because `clappkit/docs/icons.md` asks for an
editable source beside the mark: the icon is regenerated, never hand-traced. M6 replaces
the drawing below and re-runs this; nothing else in the repo changes.

    python3 scripts/render-icon.py

No Pillow, no rasteriser, no network — a PNG is a zlib stream with a header, and this
family builds on machines that have neither.
"""

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_PNG = ROOT / "assets" / "icon.png"
ICON_ICO = ROOT / "src-tauri" / "icons" / "icon.ico"

SIDE = 1024
# Apple's own ratio, and the Dock is where the corner is actually seen (icons.md §2).
RADIUS = 0.225
# Provisional slate + the one accent. M6 picks the real palette.
GROUND = (0x1C, 0x21, 0x2B)
BARS = [(0x3D, 0x5A, 0x80), (0x5C, 0x88, 0xB8), (0x8E, 0xC5, 0xE8)]
# The Windows sizes: the OS picks a different one per context, and a scaled-down 256
# looks it (playbook §9).
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
SAMPLES = 4  # supersampling per axis, so the corners are not stairsteps


def rounded_tile(side):
    """RGBA pixels for a full-bleed rounded tile carrying three ascending bars — a
    pipeline, which is what this app is. Coverage is supersampled, so the only
    anti-aliasing is the corner arc and the bar edges."""
    r = RADIUS * side
    # Three bars, ascending, centred as a group: the pipeline advancing.
    bar_w = side * 0.13
    gap = side * 0.075
    group_w = 3 * bar_w + 2 * gap
    x0 = (side - group_w) / 2
    base = side * 0.755
    heights = [side * 0.20, side * 0.32, side * 0.44]

    def inside_tile(x, y):
        # Only the corners are transparent (icons.md §2).
        cx = min(max(x, r), side - r)
        cy = min(max(y, r), side - r)
        dx, dy = x - cx, y - cy
        return dx * dx + dy * dy <= r * r

    def bar_at(x, y):
        for i, h in enumerate(heights):
            bx = x0 + i * (bar_w + gap)
            if bx <= x <= bx + bar_w and base - h <= y <= base:
                return i
        return None

    rows = []
    step = 1.0 / SAMPLES
    off = step / 2.0
    for py in range(side):
        row = bytearray()
        for px in range(side):
            cover = 0
            hits = [0, 0, 0]
            for sy in range(SAMPLES):
                y = py + off + sy * step
                for sx in range(SAMPLES):
                    x = px + off + sx * step
                    if not inside_tile(x, y):
                        continue
                    cover += 1
                    b = bar_at(x, y)
                    if b is not None:
                        hits[b] += 1
            total = SAMPLES * SAMPLES
            if cover == 0:
                row += b"\x00\x00\x00\x00"
                continue
            painted = sum(hits)
            if painted:
                # Weighted blend of ground and whichever bars this pixel straddles.
                acc = [0.0, 0.0, 0.0]
                for i, n in enumerate(hits):
                    for c in range(3):
                        acc[c] += BARS[i][c] * n
                for c in range(3):
                    acc[c] += GROUND[c] * (cover - painted)
                rgb = tuple(int(round(v / cover)) for v in acc)
            else:
                rgb = GROUND
            a = int(round(255 * cover / total))
            row += bytes((rgb[0], rgb[1], rgb[2], a))
        rows.append(bytes(row))
    return rows


def png(rows, side):
    """Encode RGBA scanlines as a PNG. Filter 0 on every row: the image is flat colour,
    so a smarter filter would buy bytes we are not short of."""
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", side, side, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def downsample(rows, side, target):
    """Box-filter to `target`, straight-alpha weighted so the corner does not fringe."""
    f = side // target
    out = []
    for ty in range(target):
        row = bytearray()
        for tx in range(target):
            ar = ag = ab = aa = 0
            for y in range(ty * f, (ty + 1) * f):
                src = rows[y]
                for x in range(tx * f, (tx + 1) * f):
                    i = x * 4
                    a = src[i + 3]
                    ar += src[i] * a
                    ag += src[i + 1] * a
                    ab += src[i + 2] * a
                    aa += a
            if aa == 0:
                row += b"\x00\x00\x00\x00"
            else:
                n = f * f
                row += bytes(
                    (
                        min(255, ar // aa),
                        min(255, ag // aa),
                        min(255, ab // aa),
                        aa // n,
                    )
                )
        out.append(bytes(row))
    return out


def ico(images):
    """An .ico of PNG-compressed entries — what every Windows since Vista reads, and
    what keeps the mark identical to `assets/icon.png` rather than a second drawing."""
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = len(head) + 16 * len(images)
    entries, blobs = b"", b""
    for size, blob in images:
        entries += struct.pack(
            "<BBBBHHII",
            size if size < 256 else 0,
            size if size < 256 else 0,
            0,
            0,
            1,
            32,
            len(blob),
            offset,
        )
        blobs += blob
        offset += len(blob)
    return head + entries + blobs


def main():
    rows = rounded_tile(SIDE)
    ICON_PNG.parent.mkdir(parents=True, exist_ok=True)
    ICON_PNG.write_bytes(png(rows, SIDE))

    images = []
    for s in ICO_SIZES:
        images.append((s, png(downsample(rows, SIDE, s), s)))
    ICON_ICO.parent.mkdir(parents=True, exist_ok=True)
    ICON_ICO.write_bytes(ico(images))

    print(f"{ICON_PNG.relative_to(ROOT)}  {SIDE}x{SIDE}  {ICON_PNG.stat().st_size} bytes")
    print(
        f"{ICON_ICO.relative_to(ROOT)}  {'/'.join(str(s) for s in ICO_SIZES)}  "
        f"{ICON_ICO.stat().st_size} bytes"
    )


if __name__ == "__main__":
    main()
