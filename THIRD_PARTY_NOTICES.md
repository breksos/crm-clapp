# Third-party notices

Breksos CRM ships artwork and typefaces it did not draw. This file is the credit those
licences require, and it is the reason `clappkit/docs/icons.md` §5 forbids approximating
a mark by hand: a real glyph under a real licence is both better drawn and honestly
attributable.

---

## Lucide

**What we use.** The `square-kanban` glyph, taken verbatim from `lucide-static` **1.42.0**
(`icons/square-kanban.svg`) and used unmodified at Lucide's own stroke-width of 2. It is
the app mark in `assets/icon.svg` / `assets/icon.png` / `src-tauri/icons/icon.ico`, and the
window draws its UI icons from the same set at 16px / 1.5 stroke.

The path data lives in `scripts/render-brand.py`, which regenerates all three image
assets from it. Nothing here is hand-traced.

`square-kanban` is **not** one of the Lucide icons derived from Feather, so the ISC
licence below is the only one that applies to it. The Feather/MIT notice is reproduced
after it because other icons in the window's set may fall under it.

**Homepage.** <https://lucide.dev> · **Source.** <https://github.com/lucide-icons/lucide>

```
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

Some Lucide icons are derived from **Feather** (Copyright (c) 2013-present Cole Bemis)
and additionally carry the MIT licence:

```
The MIT License (MIT)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Typefaces

**Geist** and **Geist Mono**, © 2023 Vercel, made in collaboration with basement.studio,
under the **SIL Open Font License 1.1**. The full licence text ships beside the files at
[`src/assets/fonts/Geist-OFL.txt`](src/assets/fonts/Geist-OFL.txt).

The files are **bundled, never linked**: this app is local-first and must render
identically with no network, and a `<link>` to a font CDN silently falls back to system
fonts offline — it would also mean widening Tauri's CSP to allow a remote origin. The
latin subset is taken from `@fontsource/geist-sans` and `@fontsource/geist-mono` 5.3.0,
140 KB for the five faces the window actually uses.

| File | Face | Role |
|---|---|---|
| `geist-sans-latin-400-normal.woff2` | Geist 400 | body, table cells, card titles |
| `geist-sans-latin-500-normal.woff2` | Geist 500 | names, active nav, emphasis |
| `geist-sans-latin-600-normal.woff2` | Geist 600 | micro-labels, column headers, record title |
| `geist-mono-latin-400-normal.woff2` | Geist Mono 400 | money, counts, dates, ids, CLI verbs |
| `geist-mono-latin-500-normal.woff2` | Geist Mono 500 | emphasised figures |

Geist was chosen rather than defaulted. It is drawn for interfaces and holds together at
12–13px, which is where a table this dense actually lives — checked by rendering one and
looking at it, not from the spec. Geist Mono is a true companion rather than an unrelated
monospace, so a money column beside a name column does not look bolted on.

**Homepage.** <https://vercel.com/font>
