# Insert lines

An agent adds lines before or after the line containing an anchor, which may be any part of one or more lines. No existing line is rewritten.

## Sub-features

- `insert-after` inserts after the line where the anchor ends.
- `insert-before` inserts before the line where the anchor starts.
- `insert-count` inserts at every match with `count=N`.
- `insert-blank-anchor` requires a blank last anchor line to match a real blank line.

## How to get to it (user POV)

- A block `<<<<<<< AFTER` (or `BEFORE`) / anchor / `=======` / lines to insert / `>>>>>>> NEW`.

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **After.** `$S/scripts/drive.sh insert-after "$F" notes.md` with stdin `<<<<<<< AFTER` / `- add a filter` / `=======` / `- add sorting` / `>>>>>>> NEW`. Exit code `0`, `M notes.md  edits=1  at_lines=9`; `diff.txt` adds one line after `- add a filter` and changes nothing else.
- **Part of a line.** Anchor `parses it as JSON` (middle of `notes.md` line 3) with `<<<<<<< BEFORE`. The new line lands before line 3; line 3 is unchanged in `diff.txt`.
- **Blank anchor line missing.** Anchor `export function loadItems(path) {` plus an empty line, on `app.js` (no blank line follows it). Exit code `1`, stderr `found without the blank line at its end`; `diff.txt` empty.

## Gotchas

- Two matches on the same line are an error even with a matching count.
- Several AFTER blocks on one anchor stack in reverse order; verify the final order in `after/`.
