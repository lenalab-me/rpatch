# Replace a range

An agent replaces everything from some lines (`FROM`) up to, but not including, later lines (`TO`) without copying the lines in between. The last line of FROM and TO may be just the start of a line; a blank TO line matches the next blank line; no TO lines means to the end of the file. TO is kept.

## Sub-features

- `range-to` replaces from FROM up to the first TO after it.
- `range-blank-to` stops at the next blank line when TO is one blank line.
- `range-eof` replaces to the end of the file when there are no TO lines.
- `range-twice` refuses NEW that ends with the TO lines (TO would appear twice).

## How to get to it (user POV)

- A block `<<<<<<< FROM` / lines / `<<<<<<< TO` / lines (or none) / `=======` / new lines / `>>>>>>> NEW`, in `rpatch FILE <<'EOF'` or after `=== FILE`.

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **Blank TO.** `$S/scripts/drive.sh range-blank-to "$F" app.js` with stdin `<<<<<<< FROM` / `export function loadItems(path) {` / `<<<<<<< TO` / (one empty line) / `=======` / the new 4-line function / `>>>>>>> NEW`. Exit code `0`; stdout has `edits[0] replaced lines 5-8`; `diff.txt` changes only the function, and `formatItem` below the blank line is untouched.
- **To the end.** Same FROM, `<<<<<<< TO` followed directly by `=======`. Exit code `0`; `after/app.js` ends with the NEW lines; the stdout note names the range to the last line.
- **TO repeated.** FROM `export function loadItems(path) {`, TO `}`, NEW ending in a line `}` at column 0. Exit code `1`, stderr `"new" ends with the "to" text, which is kept, so it would appear twice`; `diff.txt` empty. NEW ending in an indented `  }` passes (lines are compared whole).

## Gotchas

- TO is the first match after FROM; with nested braces a TO of `}` can stop early. Check the `replaced lines` note, not just the exit code.
- A FROM block written like OLD (`=======` before `<<<<<<< TO`) is an input error (exit `2`) with a hint to use OLD.
