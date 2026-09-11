// Two namespaces that must never be confused.
//
// **`id` is for machines. `handle` is for humans and agents.** The core keeps both on every
// record and the snapshot carries both side by side.
//
//   id      a ULID. Stored, referenced, keyed on. Never displayed, never printed, never put
//           in a command the person or their agent is told to type.
//   handle  `acme`, `acme-2`. Typable, stable across renames. Everything a person reads.
//
// The test is: could the reader type this string back into a verb? If not, it is the wrong
// field. Round one of QA shipped `crm task 01K4Z8QH3M7XC9VBN2RTFA6EDS "call back"` because
// `Row` had only an `id`, the two were the same TypeScript type, and nothing objected.
//
// The brands below make that a compile error rather than a convention. They are erased at
// runtime — a `Handle` *is* a string — but `commands.ts` takes `Handle` and only `Handle`,
// so an id cannot reach a printed command without somebody deliberately casting it.
//
// This lives apart from `bridge.ts` because `bridge.ts` re-exports `@clappkit` through a
// Vite alias, which only a bundler can resolve. Everything here is plain TypeScript, so
// `src/commands.test.ts` can import it under bare `node --test`. `bridge.ts` re-exports it,
// so components still have one seam to import from.

declare const HANDLE_BRAND: unique symbol;
declare const ID_BRAND: unique symbol;

/** A ULID. Opaque, and never rendered. */
export type Id = string & { readonly [ID_BRAND]: true };

/** What somebody types. The only record reference that may appear in visible text. */
export type Handle = string & { readonly [HANDLE_BRAND]: true };

/** Brand a string that genuinely came from the snapshot's `id` field. */
export function asId(s: string): Id {
  return s as Id;
}

/** Brand a string that genuinely came from the snapshot's `handle` field. */
export function asHandle(s: string): Handle {
  return s as Handle;
}

/**
 * 26 Crockford base32 characters — the shape `model.rs`'s `Ulid` prints.
 *
 * Crockford omits `I`, `L`, `O` and `U` so a handwritten id cannot be misread, which is
 * also what keeps this from matching ordinary words. Used by the guard test, and by the
 * preview harness to shout if one of these ever reaches the screen.
 */
export function looksLikeId(s: string): boolean {
  return /^[0-9ABCDEFGHJKMNPQRSTVWXYZ]{26}$/.test(s);
}
