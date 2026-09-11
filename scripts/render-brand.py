#!/usr/bin/env python3
"""Draw the Breksos brand: the mark, the Windows resource, and the library banner.

    python3 scripts/render-brand.py

Outputs, all of them derived from the constants in this file and nothing else:

    assets/icon.svg              the editable source
    assets/icon.png              1024x1024 RGBA
    src-tauri/icons/icon.ico     16/24/32/48/64/128/256, from the same pixels
    assets/banner.png            3440x512 (215:32)
    /tmp/banner-128.png          the banner at the size the library actually draws it

No Pillow, no rasteriser, no network: a PNG is a zlib stream with a header, and this
family builds on machines that have neither (`scripts/render-icon.py`, which this
replaces, made the same bet).

Two things this file is careful about.

**The mark is Lucide's `square-kanban`, unmodified.** `clappkit/docs/icons.md` §5 forbids
designing a logo by hand. The path data below is copied verbatim from lucide-static
1.42.0 (ISC, credited in THIRD_PARTY_NOTICES.md) and is drawn at Lucide's own
stroke-width of 2 — scaled, never redrawn.

**The banner's palette is the icon's palette.** Every colour it uses is either one of the
four brand constants or a straight mix of two of them, computed by `mix()` at render
time. There is no second palette to drift.

Rendering is signed-distance-field based: each shape reports its distance to the pixel
centre and coverage falls out of it, which gives clean antialiasing at one sample per
pixel. Shapes composite front-to-back within their own bounding box, so a 3440x512
canvas costs the area of what is actually drawn on it rather than the area of the canvas.
"""

import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_SVG = ROOT / "assets" / "icon.svg"
ICON_PNG = ROOT / "assets" / "icon.png"
ICON_ICO = ROOT / "src-tauri" / "icons" / "icon.ico"
BANNER_PNG = ROOT / "assets" / "banner.png"
BANNER_PREVIEW = Path("/tmp/banner-128.png")

# --- the palette --------------------------------------------------------------------
# Decided in docs/work-orders/m6-brand.md and not reopened here. Green because these are
# deals, dark and desaturated so it reads as an instrument rather than as money.
GROUND = (0x12, 0x3B, 0x33)   # deep ink-green: the tile, and the banner's ground
BONE = (0xF2, 0xEF, 0xE6)     # the glyph
ACCENT = (0x2E, 0x8B, 0x72)
LOST = (0xA6, 0x50, 0x3F)     # unused here; kept so the file states the whole palette
DUE = (0xC0, 0x8A, 0x2E)      # never the accent — see the work order

# --- the mark -----------------------------------------------------------------------
SIDE = 1024
TILE_RADIUS = 0.225           # Apple's ratio; the Dock is where the corner is seen
GLYPH_SPAN = 0.72             # the Lucide 24-unit box as a fraction of the tile
LUCIDE_STROKE = 2.0           # Lucide's own weight, at Lucide's own scale

# lucide-static 1.42.0, icons/square-kanban.svg, verbatim. viewBox 0 0 24 24.
LUCIDE_SQUARE_KANBAN = {
    "rect": (3.0, 3.0, 18.0, 18.0, 2.0),          # x, y, w, h, rx
    "bars": [(8.0, 7.0, 14.0), (12.0, 7.0, 11.0), (16.0, 7.0, 16.0)],  # x, y0, y1
}

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

# --- the banner ---------------------------------------------------------------------
# format.md fixes the design canvas at 860x128 and the shipped asset at 4x that. Every
# number below is in canvas units, so "nothing thinner than 6px at 3440" reads as
# "nothing thinner than 1.5 units here".
BANNER_SCALE = 4
CANVAS_W, CANVAS_H = 860, 128
MIN_STROKE = 6.0 / BANNER_SCALE   # 1.5 canvas units


def mix(a, b, t):
    """Linear blend of two palette entries. Every banner colour comes from here, which
    is what keeps the two assets one palette rather than two."""
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


# ======================================================================================
# signed distance fields
# ======================================================================================

def sd_round_rect(px, py, cx, cy, hw, hh, r):
    """Distance to a rounded rectangle centred at (cx, cy). Negative inside."""
    r = min(r, hw, hh)
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    return outside + inside - r


def sd_segment(px, py, ax, ay, bx, by):
    """Distance to the line segment ab — a round-capped stroke is this, thresholded."""
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    denom = vx * vx + vy * vy
    t = 0.0 if denom == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / denom))
    return math.hypot(wx - vx * t, wy - vy * t)


def sd_circle(px, py, cx, cy, r):
    return math.hypot(px - cx, py - cy) - r


def coverage(d, softness=0.5):
    """SDF distance to alpha. One sample per pixel: the field is exact, so the only
    approximation is that the edge is straight across the pixel — which it is, at these
    radii."""
    if d <= -softness:
        return 1.0
    if d >= softness:
        return 0.0
    return (softness - d) / (2.0 * softness)


class Canvas:
    """An opaque RGB raster with float channels, painted back to front.

    Shapes carry their own bounding box and only touch pixels inside it, so cost tracks
    the ink rather than the canvas — which is what makes a 3440x512 banner render in
    seconds of pure Python instead of minutes.
    """

    def __init__(self, w, h, fill=None, opaque=True):
        self.w, self.h = w, h
        base = fill if fill else (0, 0, 0)
        a = 255.0 if opaque else 0.0
        self.px = [[float(base[0]), float(base[1]), float(base[2]), a]
                   for _ in range(w * h)]

    def paint(self, sdf, colour, bbox, alpha=1.0, softness=0.5):
        x0 = max(0, int(math.floor(bbox[0])))
        y0 = max(0, int(math.floor(bbox[1])))
        x1 = min(self.w, int(math.ceil(bbox[2])) + 1)
        y1 = min(self.h, int(math.ceil(bbox[3])) + 1)
        cr, cg, cb = float(colour[0]), float(colour[1]), float(colour[2])
        for y in range(y0, y1):
            py = y + 0.5
            row = y * self.w
            for x in range(x0, x1):
                a = coverage(sdf(x + 0.5, py), softness) * alpha
                if a <= 0.0:
                    continue
                p = self.px[row + x]
                inv = 1.0 - a
                # Source-over on straight colour, with alpha accumulated separately so a
                # transparent-cornered tile composites correctly.
                p[0] = p[0] * inv + cr * a
                p[1] = p[1] * inv + cg * a
                p[2] = p[2] * inv + cb * a
                p[3] = p[3] * inv + 255.0 * a

    def rows(self):
        out = []
        for y in range(self.h):
            row = bytearray()
            base = y * self.w
            for x in range(self.w):
                p = self.px[base + x]
                row += bytes((
                    max(0, min(255, int(round(p[0])))),
                    max(0, min(255, int(round(p[1])))),
                    max(0, min(255, int(round(p[2])))),
                    max(0, min(255, int(round(p[3])))),
                ))
            out.append(bytes(row))
        return out


# ======================================================================================
# PNG / ICO
# ======================================================================================

def png(rows, w, h):
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def resample(rows, w, h, tw, th):
    """Area-average down to (tw, th), premultiplied so a transparent corner does not
    fringe. Bounds are computed per target pixel rather than from an integer factor:
    1024 does not divide by 24 or 48, and the .ico needs both."""
    xs = [round(i * w / tw) for i in range(tw + 1)]
    ys = [round(j * h / th) for j in range(th + 1)]
    out = []
    for j in range(th):
        row = bytearray()
        y0, y1 = ys[j], max(ys[j] + 1, ys[j + 1])
        for i in range(tw):
            x0, x1 = xs[i], max(xs[i] + 1, xs[i + 1])
            ar = ag = ab = aa = 0
            n = (y1 - y0) * (x1 - x0)
            for y in range(y0, y1):
                src = rows[y]
                for x in range(x0, x1):
                    k = x * 4
                    a = src[k + 3]
                    ar += src[k] * a
                    ag += src[k + 1] * a
                    ab += src[k + 2] * a
                    aa += a
            if aa == 0:
                row += b"\x00\x00\x00\x00"
            else:
                row += bytes((min(255, ar // aa), min(255, ag // aa),
                              min(255, ab // aa), aa // n))
        out.append(bytes(row))
    return out


def ico(images):
    """PNG-compressed entries — what every Windows since Vista reads, and what keeps the
    resource identical to assets/icon.png rather than a second drawing."""
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = len(head) + 16 * len(images)
    entries, blobs = b"", b""
    for size, blob in images:
        s = size if size < 256 else 0
        entries += struct.pack("<BBBBHHII", s, s, 0, 0, 1, 32, len(blob), offset)
        blobs += blob
        offset += len(blob)
    return head + entries + blobs


# ======================================================================================
# the mark
# ======================================================================================

def glyph_geometry(side):
    """Lucide's 24-unit box placed on the tile. Returns the unit size and origin, so the
    SVG and the raster cannot disagree about where the glyph sits."""
    span = GLYPH_SPAN * side
    unit = span / 24.0
    origin = (side - span) / 2.0
    return unit, origin


def draw_icon(side):
    c = Canvas(side, side, fill=GROUND, opaque=False)

    # The tile: full-bleed, only the corners transparent (icons.md §2).
    r = TILE_RADIUS * side
    half = side / 2.0
    c.paint(lambda x, y: sd_round_rect(x, y, half, half, half, half, r),
            GROUND, (0, 0, side, side))

    unit, org = glyph_geometry(side)
    sw = LUCIDE_STROKE * unit
    hs = sw / 2.0

    def u(v):
        return org + v * unit

    rx, ry, rw, rh, rr = LUCIDE_SQUARE_KANBAN["rect"]
    cx, cy = u(rx + rw / 2), u(ry + rh / 2)
    hw, hh = rw * unit / 2, rh * unit / 2

    # An outline is the |distance| of the fill, thresholded at half the stroke.
    c.paint(lambda x, y: abs(sd_round_rect(x, y, cx, cy, hw, hh, rr * unit)) - hs,
            BONE, (u(rx) - hs, u(ry) - hs, u(rx + rw) + hs, u(ry + rh) + hs))

    for bx, by0, by1 in LUCIDE_SQUARE_KANBAN["bars"]:
        ax, ay, bxx, byy = u(bx), u(by0), u(bx), u(by1)
        c.paint(lambda x, y, ax=ax, ay=ay, bxx=bxx, byy=byy:
                sd_segment(x, y, ax, ay, bxx, byy) - hs,
                BONE, (ax - hs, ay - hs, bxx + hs, byy + hs))
    return c


def write_icon_svg(side):
    """The editable source, emitted from the same constants the raster uses."""
    r = TILE_RADIUS * side
    unit, org = glyph_geometry(side)
    rx, ry, rw, rh, rr = LUCIDE_SQUARE_KANBAN["rect"]
    bars = "\n".join(
        f'    <path d="M{bx} {by0}v{by1 - by0}" />'
        for bx, by0, by1 in LUCIDE_SQUARE_KANBAN["bars"])
    ICON_SVG.write_text(f"""<svg xmlns="http://www.w3.org/2000/svg"
     width="{side}" height="{side}" viewBox="0 0 {side} {side}">
  <!-- Breksos CRM. Generated by scripts/render-brand.py - edit that, not this.

       The tile is full-bleed at Apple's {TILE_RADIUS} corner ratio (clappkit/docs/icons.md
       section 2). The glyph is Lucide `square-kanban` from lucide-static 1.42.0, ISC,
       used unmodified at its own stroke-width of {LUCIDE_STROKE:g}; see
       THIRD_PARTY_NOTICES.md. -->
  <rect width="{side}" height="{side}" rx="{r:g}" fill="#{GROUND[0]:02X}{GROUND[1]:02X}{GROUND[2]:02X}" />
  <g transform="translate({org:g} {org:g}) scale({unit:g})"
     fill="none" stroke="#{BONE[0]:02X}{BONE[1]:02X}{BONE[2]:02X}"
     stroke-width="{LUCIDE_STROKE:g}" stroke-linecap="round" stroke-linejoin="round">
    <rect width="{rw:g}" height="{rh:g}" x="{rx:g}" y="{ry:g}" rx="{rr:g}" />
{bars}
  </g>
</svg>
""")


# ======================================================================================
# the banner
# ======================================================================================
#
# The motif: three stage columns right of centre, and one deal card caught in the air
# between the first two - the moment this app exists for. Every card carries a disc for
# who moved it: bone for the person, accent for an agent. The flying card's disc is an
# agent's, which is the whole thesis in one detail.
#
# The left 40% (0..344 canvas units) carries ground and nothing else: the launcher lays a
# dark scrim there and prints "Breksos CRM" over it in white.

BOARD = None          # filled in by _derive(), all mixes of the icon's own colours
CARD = None
CARD_LIVE = None
CARD_BAR = None
SLOT = None
HALO = None
DISC_PERSON = None
DISC_AGENT = None


def _derive():
    """Every banner colour, as a mix of the four the icon already uses.

    The value structure is the whole design: a dark ground, columns one step up, cards
    two steps up, and then the discs jumping straight to full bone or full accent. At
    128px tall a low-contrast disc is a smudge, and the disc is the thesis - so it gets
    the largest value step on the strip, and the cards give way to make room for it.
    """
    global BOARD, CARD, CARD_LIVE, CARD_BAR, SLOT, HALO, DISC_PERSON, DISC_AGENT
    BOARD = mix(GROUND, BONE, 0.08)      # the columns: one flat step up from the ground
    CARD = mix(GROUND, BONE, 0.30)       # cards: dark, so what sits on them can be light
    CARD_BAR = mix(CARD, BONE, 0.34)     # the fields on a card, held well back
    CARD_LIVE = BONE                     # the one in flight is the brightest thing here
    SLOT = mix(GROUND, BONE, 0.22)       # the empty slot it left behind
    HALO = mix(GROUND, (0, 0, 0), 0.60)  # so the flying card clears the column under it
    DISC_PERSON = BONE                   # the person moved this one
    DISC_AGENT = mix(ACCENT, BONE, 0.12)  # an agent moved this one


COL_X = [372.0, 520.0, 668.0]   # three columns, 120 wide, 28 apart, all right of centre
COL_W = 120.0
COL_TOP, COL_BOT = 10.0, 118.0
CARD_H = 30.0
CARD_GAP = 6.0
CARD_R = 5.0
CARDS_TOP = 44.0                # below the column's header bar
DISC_R = 8.0                    # 64px across at 3440, and 16px where the shelf draws it


def _card(c, S, x, y, w, h, colour, bar, disc, rot=0.0):
    """A deal card: the attribution disc, and two field bars. The window's cards carry
    three fields and a disc; this is that card at the size a shelf draws it.

    `x, y, w, h` arrive in device pixels; every constant below is in canvas units and
    goes through `S`. Mixing the two is how the discs ended up a quarter of their size
    the first time this was drawn.
    """
    cx, cy = x + w / 2, y + h / 2
    ca, sa = math.cos(-rot), math.sin(-rot)

    def to_local(px, py):
        dx, dy = px - cx, py - cy
        return cx + dx * ca - dy * sa, cy + dx * sa + dy * ca

    pad = abs(w * math.sin(rot)) + h
    bbox = (cx - w, cy - pad, cx + w, cy + pad)

    c.paint(lambda px, py: sd_round_rect(*to_local(px, py), cx, cy,
                                         w / 2, h / 2, S(CARD_R)),
            colour, bbox)

    # The disc is the largest single element on the card, by design: it is the one thing
    # on this strip that no other CRM's banner could carry.
    dcx = x + S(7.0) + S(DISC_R)
    c.paint(lambda px, py: sd_circle(*to_local(px, py), dcx, cy, S(DISC_R)), disc, bbox)

    # Two field bars: the deal, and the company under it. 5 units is 20px at 3440.
    left = dcx + S(DISC_R + 7.0)
    for dy, right, bh in ((-5.5, x + w - S(9.0), 6.0), (6.0, x + w - S(46.0), 5.0)):
        c.paint(lambda px, py, dy=dy, right=right, bh=bh:
                sd_round_rect(*to_local(px, py), (left + right) / 2, cy + S(dy),
                              (right - left) / 2, S(bh) / 2, S(bh) / 2),
                bar, bbox)


def draw_banner(scale):
    _derive()
    W, H = CANVAS_W * scale, CANVAS_H * scale
    c = Canvas(W, H, fill=GROUND)

    def S(v):
        return v * scale

    # --- the three columns ------------------------------------------------------------
    header_w = [46, 40, 34]
    for i, cx0 in enumerate(COL_X):
        x0, x1 = S(cx0), S(cx0 + COL_W)
        y0, y1 = S(COL_TOP), S(COL_BOT)
        c.paint(lambda x, y, x0=x0, x1=x1, y0=y0, y1=y1:
                sd_round_rect(x, y, (x0 + x1) / 2, (y0 + y1) / 2,
                              (x1 - x0) / 2, (y1 - y0) / 2, S(6.0)),
                BOARD, (x0, y0, x1, y1))

        # Header: the stage name, and its count beside it. 7 units = 28px at 3440.
        hx0, hx1 = S(cx0 + 10), S(cx0 + 10 + header_w[i])
        hy = S(COL_TOP + 13)
        c.paint(lambda x, y, a=hx0, b=hx1, hy=hy:
                sd_round_rect(x, y, (a + b) / 2, hy, (b - a) / 2, S(3.5), S(3.5)),
                mix(BOARD, BONE, 0.62), (hx0, hy - S(5), hx1, hy + S(5)))
        ccx = S(cx0 + COL_W - 15)
        c.paint(lambda x, y, ccx=ccx, hy=hy: sd_circle(x, y, ccx, hy, S(4.5)),
                ACCENT, (ccx - S(6), hy - S(6), ccx + S(6), hy + S(6)))

    # --- the settled cards ------------------------------------------------------------
    # LEAD has lost one to the air, so it shows one card and the slot it left behind.
    # The discs alternate: both of you move this board, and the strip says so.
    # The top row of the first two columns is deliberately clear: the card that belongs
    # in it is in the air between them, and a motif whose subject is buried under other
    # objects is a motif nobody reads at 128px.
    plan = [
        [("slot", None), ("card", DISC_PERSON)],
        [("air", None), ("card", DISC_AGENT)],
        [("card", DISC_PERSON), ("card", DISC_AGENT)],
    ]
    for ci, slots in enumerate(plan):
        for si, (kind, disc) in enumerate(slots):
            y = CARDS_TOP + si * (CARD_H + CARD_GAP)
            x0, y0 = S(COL_X[ci] + 6), S(y)
            w, h = S(COL_W - 12), S(CARD_H)
            if kind == "air":
                continue
            if kind == "slot":
                # An outline, not a fill: the card that was here is in the air.
                cx, cy = x0 + w / 2, y0 + h / 2
                c.paint(lambda px, py, cx=cx, cy=cy, w=w, h=h:
                        abs(sd_round_rect(px, py, cx, cy, w / 2, h / 2, S(CARD_R)))
                        - S(2.0),
                        SLOT, (x0 - S(3), y0 - S(3), x0 + w + S(3), y0 + h + S(3)))
            else:
                _card(c, S, x0, y0, w, h, CARD, CARD_BAR, disc)

    # --- the card in flight -----------------------------------------------------------
    # It spans the gap: it has left LEAD and has not landed in QUALIFIED. An agent moved
    # it, so its disc is the accent - which is the entire thesis of this app, drawn at
    # the one size somebody scrolling a shelf will actually see.
    fw, fh = S(COL_W - 12), S(CARD_H)
    gap_mid = (COL_X[0] + COL_W + COL_X[1]) / 2      # the air between LEAD and QUALIFIED
    fly_x = S(gap_mid + 13 - (COL_W - 12) / 2)   # biased toward the column it is entering
    fly_y = S(CARDS_TOP - 8)                          # lifted clear of the row it left
    rot = math.radians(-4.5)

    # Motion, reading back into the column it left.
    trail_y = fly_y + fh / 2 + S(9)
    for dx, length, alpha in ((-6, 34, 0.9), (-48, 20, 0.55), (-76, 11, 0.3)):
        ax = fly_x + S(dx)
        c.paint(lambda x, y, ax=ax, length=length:
                sd_segment(x, y, ax, trail_y, ax - S(length), trail_y) - S(2.0),
                ACCENT, (ax - S(length) - S(3), trail_y - S(4), ax + S(3),
                         trail_y + S(4)), alpha=alpha)

    # A tight dark halo: the brightest object on the strip has to clear the column behind
    # it. This is the one thing here that is genuinely floating.
    c.paint(lambda px, py: sd_round_rect(px, py, fly_x + fw / 2, fly_y + fh / 2 + S(2),
                                         fw / 2 + S(5), fh / 2 + S(5), S(CARD_R + 5)),
            HALO, (fly_x - S(12), fly_y - S(12), fly_x + fw + S(12), fly_y + fh + S(14)),
            alpha=0.5, softness=S(2.5))

    _card(c, S, fly_x, fly_y, fw, fh, CARD_LIVE, mix(CARD_LIVE, GROUND, 0.32),
          DISC_AGENT, rot=rot)

    # The accent ring - the window draws this too, for 1.2s, in the moving agent's tint.
    cx, cy = fly_x + fw / 2, fly_y + fh / 2
    ca, sa = math.cos(-rot), math.sin(-rot)

    def to_local(px, py):
        dx, dy = px - cx, py - cy
        return cx + dx * ca - dy * sa, cy + dx * sa + dy * ca

    c.paint(lambda px, py: abs(sd_round_rect(*to_local(px, py), cx, cy,
                                             fw / 2 + S(4), fh / 2 + S(4),
                                             S(CARD_R + 4))) - S(2.2),
            ACCENT, (cx - fw, cy - fh - S(8), cx + fw, cy + fh + S(8)))
    return c


# ======================================================================================

def fill_of(rows, w, h):
    """The bbox of everything non-transparent, as a percentage of the canvas — the
    measurement icons.md asks for, without needing Pillow to take it."""
    x0, y0, x1, y1 = w, h, -1, -1
    for y in range(h):
        src = rows[y]
        for x in range(w):
            if src[x * 4 + 3]:
                if x < x0:
                    x0 = x
                if x > x1:
                    x1 = x
                if y < y0:
                    y0 = y
                if y > y1:
                    y1 = y
    if x1 < 0:
        return 0, 0
    return 100 * (x1 - x0 + 1) // w, 100 * (y1 - y0 + 1) // h


def left_band_is_clear(rows, w, h, fraction=0.40):
    """The launcher lays a dark scrim over the left 40% and prints the app name in white.
    Scan that band and report the rightmost column that is still bare ground, so the rule
    is checked rather than asserted."""
    limit = int(w * fraction)
    for x in range(limit - 1, -1, -1):
        for y in range(h):
            k = x * 4
            px = (rows[y][k], rows[y][k + 1], rows[y][k + 2])
            if px != GROUND:
                return False, x
    return True, limit


def main():
    ICON_PNG.parent.mkdir(parents=True, exist_ok=True)
    ICON_ICO.parent.mkdir(parents=True, exist_ok=True)

    write_icon_svg(SIDE)
    icon = draw_icon(SIDE).rows()
    ICON_PNG.write_bytes(png(icon, SIDE, SIDE))
    fw, fh = fill_of(icon, SIDE, SIDE)
    print(f"assets/icon.svg   source")
    print(f"assets/icon.png   {SIDE}x{SIDE}  fill {fw}% x {fh}%  "
          f"{ICON_PNG.stat().st_size} bytes")

    ICON_ICO.write_bytes(ico([(s, png(resample(icon, SIDE, SIDE, s, s), s, s))
                              for s in ICO_SIZES]))
    print(f"src-tauri/icons/icon.ico  {'/'.join(map(str, ICO_SIZES))}  "
          f"{ICON_ICO.stat().st_size} bytes")

    banner = draw_banner(BANNER_SCALE)
    bw, bh = banner.w, banner.h
    rows = banner.rows()
    BANNER_PNG.write_bytes(png(rows, bw, bh))
    print(f"assets/banner.png  {bw}x{bh}  ({bw}:{bh} = {bw // math.gcd(bw, bh)}:"
          f"{bh // math.gcd(bw, bh)})  {BANNER_PNG.stat().st_size} bytes")

    clear, edge = left_band_is_clear(rows, bw, bh)
    print(f"banner left 40% ({int(bw * 0.4)}px): "
          + ("clear ground" if clear else f"INK AT x={edge} - the app name sits here"))

    # The size the library actually draws it. Check the banner here, never at full size.
    small = resample(rows, bw, bh, 860, 128)
    BANNER_PREVIEW.write_bytes(png(small, 860, 128))
    print(f"{BANNER_PREVIEW}  860x128  the size the shelf draws")


if __name__ == "__main__":
    main()
