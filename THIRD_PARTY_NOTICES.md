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

The window's faces are **bundled**, never linked: this app is local-first and must render
identically with no network, and a `<link>` to a font CDN silently falls back to system
fonts offline. The files land under `src/assets/fonts/` with the window in M3, and their
licences are recorded here when they do.

| Face | Role | Licence |
|---|---|---|
| Geist | UI | SIL Open Font License 1.1 |
| Geist Mono | numerals, ids, money, code | SIL Open Font License 1.1 |
