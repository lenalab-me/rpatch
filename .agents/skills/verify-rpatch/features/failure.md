# All or nothing, and diagnostics

When any edit in a call does not match, rpatch writes nothing to any file, exits non-zero, and explains on stderr where the text was or was not found and how to fix the input, so the agent can succeed on the next try.

## Sub-features

- `fail-atomic` writes no file when one edit of several fails.
- `fail-partial` refuses OLD written as only part of a line, and shows what `OLD part` would give.
- `fail-not-found` names the closest text (whitespace, CRLF, divergence point).
- `fail-input` rejects malformed input with exit `2` and the real format.
- `fail-exit-codes` uses 0 ok, 1 no match or missing file, 2 invalid input, 3 I/O.

## How to get to it (user POV)

- Any `rpatch` call whose input does not match the file, or is malformed.

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **Atomic + partial line.** `$S/scripts/drive.sh fail-partial "$F"` with stdin: `=== app.js` and a valid OLD block (`const MAX_ITEMS = 20;` → `const MAX_ITEMS = 30;`), then `=== notes.md` with `<<<<<<< OLD` / `The loader reads the whole file at once,` / `=======` / `The loader reads the file at once,` / `>>>>>>> NEW`. Exit code `1`; stderr has `"old" not found`, `OLD line 1 is only the start of line 3`, the preview line starting `       3 | The loader reads the file at once, parses it as JSON`, and ends with `rpatch: nothing written`; `diff.txt` is empty, so the valid app.js edit was not written either.
- **Not found.** OLD `const MAX_ITEMS=20;` (spaces left out). Exit code `1`, stderr `"old" matches the file up to a point, then differs at line 3:` with the OLD line and file line 3 one above the other; `diff.txt` empty.
- **Malformed input.** stdin `--- app.js` / `+++ app.js` / `-const MAX_ITEMS = 20;` (a diff). Exit code `2`, stderr `this is not a diff or patch format` and the block format example.
- **Missing file.** `=== nope.js` with any block. Exit code `1`, stderr `nope.js: file not found` and `cwd:`.

## Gotchas

- Exit status 3 wins over 1 when both happen in one call.
- A failure printed through `| tail` or followed by a command after the closing `EOF` can look like exit 0 in a shell; `drive.sh` records rpatch's own exit code.
