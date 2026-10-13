// Prints the Subsystem field of a Windows PE executable: 3 = CONSOLE, 2 = GUI, or "notpe".
// Used by check-cli.sh. A GUI-subsystem clapp binary makes every CLI call return instantly,
// empty, with exit 0 (clappkit/src/role.rs), so this is read from the file, not trusted.
import { readFileSync } from "node:fs";

export function subsystem(b) {
  if (b.length < 0x40 || b.readUInt16LE(0) !== 0x5a4d) return "notpe";
  const pe = b.readUInt32LE(0x3c);
  if (pe + 24 + 70 > b.length || b.toString("latin1", pe, pe + 4) !== "PE\0\0") return "notpe";
  // The optional header starts 24 bytes after "PE\0\0"; Subsystem sits 68 bytes into it for
  // both PE32 and PE32+.
  return String(b.readUInt16LE(pe + 24 + 68));
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  console.log(subsystem(readFileSync(process.argv[2])));
}
