# Choosing files

An agent names the file to edit with a `=== path` line, a file argument (`rpatch FILE`), or by mistake a bare path or a diff header `--- path` before the first block; all of these are read as the file line when the next line opens a block. JSON input names files in `"file"` and matches text anywhere.

## Sub-features

- `target-file-line` edits the files named by `=== path` lines, several per call.
- `target-argument` takes the file from `rpatch FILE` for blocks before any `=== ` line.
- `target-loose` reads a bare path or `--- path` before a block opener as `=== path`.
- `target-unused-arg` rejects a file argument that no block uses.
- `target-json` applies JSON edits (`old`, or `from`/`to`) with exact substrings.
- `target-group` applies the blocks after consecutive `=== ` lines to every file in the group.
- `target-home` reads `~/` in a file name as $HOME.

## How to get to it (user POV)

- `rpatch <<'EOF'` with `=== path` lines, `rpatch path <<'EOF'`, or JSON on stdin starting with `{`.

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **Bare path line.** `$S/scripts/drive.sh loose-file-line "$F"` with stdin `app.js` / `<<<<<<< OLD` / `const MAX_ITEMS = 20;` / `=======` / `const MAX_ITEMS = 30;` / `>>>>>>> NEW`. Exit code `0`, `M app.js  edits=1  at_lines=3`. Same with `--- app.js` as the first line.
- **Unused argument.** `$S/scripts/drive.sh unused-arg "$F" app.js` with stdin starting `=== notes.md` and a block for notes.md. Exit code `2`, stderr `the file argument \`app.js\` is not used`; `diff.txt` empty.
- **JSON.** `$S/scripts/drive.sh json "$F"` with stdin `{"edits": [{"file": "app.js", "old": "MAX_ITEMS = 20", "new": "MAX_ITEMS = 40"}]}`. Exit code `0`, `M app.js  edits=1  at_lines=3`.
- **Several files.** `=== app.js` block and `=== notes.md` block in one input. Exit code `0`, one `M` line per file, and `diff.txt` covers both.
- **Group.** `BEFORE_RUN='printf "x = 1\n" > a.txt; cp a.txt b.txt; cp a.txt c.txt' $S/scripts/drive.sh file-line-group "$F"` with stdin `=== a.txt` / `=== b.txt` / `=== c.txt` / `<<<<<<< OLD` / `x = 1` / `=======` / `x = 2` / `>>>>>>> NEW`. Exit code `0`, one `M` line per file, `changed: 3 file(s)`. Make b.txt differ (`printf other > b.txt` in BEFORE_RUN): exit code `1`, `diff.txt` empty.

## Gotchas

- A file argument with JSON input is an input error (exit `2`).
- A path that does not exist is exit `1` (`file not found`), even when written as a loose file line; the loose form never creates files.
