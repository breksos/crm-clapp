#!/usr/bin/env python3
"""WCAG contrast audit of the window's tokens, read straight from src/styles.css.

    python3 scripts/contrast.py            # prints every pairing, exits 1 if a required one fails

Reads the light block (bare `:root`) and the explicit dark block (`:root[data-theme="dark"]`),
so a token changed in the stylesheet is the token measured here. Text pairings need 4.5:1;
graphical ones (focus ring, stage stripe) need 3:1. Pairings marked `info` are reported but
never fail the run — they are known, documented, and judged elsewhere.
"""
import re, sys, pathlib

CSS = pathlib.Path(__file__).resolve().parent.parent / "src" / "styles.css"
TINTS = ["#45548C", "#267369", "#784D82", "#996138", "#4D6178"]  # clappkit AGENT_TINTS

def block(css, opener):
    i = css.index(opener)
    j = css.index("{", i) + 1
    depth, k = 1, j
    while depth:
        depth += {"{": 1, "}": -1}.get(css[k], 0)
        k += 1
    return css[j : k - 1]

def tokens(body):
    return dict(re.findall(r"--([\w-]+):\s*(#[0-9a-fA-F]{6})", body))

def lin(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

def lum(h):
    h = h.lstrip("#")
    r, g, b = (int(h[i : i + 2], 16) / 255 for i in (0, 2, 4))
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)

def ratio(a, b):
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)

css = CSS.read_text()
light = tokens(block(css, ":root {"))
dark = {**light, **tokens(block(css, ':root[data-theme="dark"]'))}

# (foreground, background, needed, kind, note)
TEXT = 4.5
GRAPHIC = 3.0
def pairs(t):
    out = []
    for fg in ("ink", "ink-2", "ink-3"):
        for bg in ("ground", "surface", "surface-2"):
            out.append((fg, bg, TEXT, "req", "text"))
    for fg in ("accent", "won", "lost", "due"):
        for bg in ("surface", "surface-2"):
            out.append((fg, bg, TEXT, "req", "semantic text"))
    out.append(("accent", "accent-weak", TEXT, "req", "active chip"))
    out.append(("ink-2", "surface-2", TEXT, "req", "human disc initial"))
    out.append(("focus", "surface", GRAPHIC, "req", "focus ring"))
    out.append(("focus", "ground", GRAPHIC, "req", "focus ring"))
    return out

fails = 0
for name, t in (("light", light), ("dark", dark)):
    print(f"\n== {name}")
    for fg, bg, need, kind, note in pairs(t):
        r = ratio(t[fg], t[bg])
        ok = r >= need
        fails += (not ok) and kind == "req"
        print(f"  {'ok  ' if ok else 'FAIL'} {fg:>9} on {bg:<10} {r:5.2f}  (need {need})  {note}")
    print("  -- agent initials (white on clappkit tint)")
    for tint in TINTS:
        r = ratio("#ffffff", tint)
        ok = r >= TEXT
        fails += not ok
        print(f"  {'ok  ' if ok else 'FAIL'} {tint} {r:5.2f}  (need {TEXT})")
    print("  -- info: agent tint as a shape against the surface (initial carries the identity)")
    for tint in TINTS:
        print(f"  info {tint} on surface {ratio(tint, t['surface']):5.2f}")
    for k in sorted(k for k in t if k.startswith("stage-") and not k.endswith(("-weak",))):
        r = ratio(t[k], t["surface"])
        r2 = ratio(t[k], t["surface-2"])
        need = GRAPHIC
        ok = min(r, r2) >= need
        fails += not ok
        print(f"  {'ok  ' if ok else 'FAIL'} {k:>16} stripe: {r:5.2f} on surface, {r2:5.2f} on surface-2  (need {need})")

print(f"\n{fails} required pairing(s) failing")
sys.exit(1 if fails else 0)
