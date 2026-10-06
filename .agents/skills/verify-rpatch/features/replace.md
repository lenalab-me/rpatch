# Replace lines

An agent replaces exact text in a file: whole lines with `<<<<<<< OLD`, the same lines everywhere with `count=N`, or part of a line with `<<<<<<< OLD part`. The file keeps its line endings (LF or CRLF) and everything not matched.

## Sub-features

- `replace-old` replaces whole lines matched exactly once.
- `replace-count` replaces N matches with `count=N` and fails on any other number.
- `replace-part` replaces part of a line, leaving the rest of the line.
- `replace-crlf` edits a CRLF file and keeps CRLF on every line.

## How to get to it (user POV)

- `rpatch FILE <<'EOF'` followed by `<<<<<<< OLD` / `=======` / `>>>>>>> NEW` blocks.
- `rpatch <<'EOF'` with a `=== FILE` line before the blocks (several files in one call).

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **Whole lines.** `$S/scripts/drive.sh replace-old "$F" app.js` with stdin `<<<<<<< OLD` / `const MAX_ITEMS = 20;` / `=======` / `const MAX_ITEMS = 50;` / `>>>>>>> NEW`. Exit code `0`, stdout `M app.js  edits=1  at_lines=3`, and `diff.txt` changes only line 3.
- **Count.** Add a second `const MAX_ITEMS = 20;` line to a copy of the fixture, then drive `<<<<<<< OLD count=2` with the same block. Exit code `0`, `at_lines` lists both lines. With `count=3`: exit code `1`, stderr `expected 3 matches, found 2` and the hint `open the block with \`<<<<<<< OLD count=2\` if that is intended`; `diff.txt` empty.
- **Part of a line.** `$S/scripts/drive.sh replace-part "$F"` with stdin `=== notes.md` / `<<<<<<< OLD part` / `keeps only the first items` / `=======` / `keeps only the first MAX_ITEMS items` / `>>>>>>> NEW`. Exit code `0`, `M notes.md  edits=1  at_lines=3`; `diff.txt` shows line 3 with the rest of the sentence intact.
- **CRLF.** In the same input add `=== windows.txt` / `<<<<<<< OLD` / `second line` / `=======` / `second line, edited` / `>>>>>>> NEW`. `od -c <evidence>/after/windows.txt` shows `\r\n` after every line, the edited one included.

## Gotchas

- Plain `OLD` refuses a line written only partly (the usual long-line slip); that is `failure.md` `fail-partial`, not a replace bug.
- `OLD` and `NEW` identical is an input error (exit `2`), so a no-op cannot be "verified" as a replace.
- Edits that cancel out (a→Z then Z→a) print `= FILE  edits=2  unchanged` and write nothing.
