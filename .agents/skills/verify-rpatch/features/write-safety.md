# Write safety

rpatch writes each file through a temp file and rename, keeps permissions and a UTF-8 BOM, and refuses writes that would lose something: read-only files, files changed on disk after it read them, and files mixing LF and CRLF. `--dry-run` checks everything a real run checks and writes nothing.

## Sub-features

- `safety-dry-run` reports the edits and writes nothing.
- `safety-read-only` refuses a read-only file (also on dry run), exit `3`.
- `safety-changed` refuses a file changed after reading, exit `3`.
- `safety-mixed-eol` refuses block edits on a file mixing LF and CRLF.
- `safety-bom-perms` keeps a BOM and the permission bits.

## How to get to it (user POV)

- `rpatch -n` / `--dry-run`, and any write to a file in one of these states.

## Driving it with drive.sh

Preconditions:

- doctor prints `OK`; `F=$S/fixtures/basic`.

- **Dry run.** `$S/scripts/drive.sh dry-run "$F" -n app.js` with the replace-old block. Exit code `0`; stdout `M app.js  edits=1  at_lines=3` then `(dry run: nothing written)`; `diff.txt` empty.
- **Read-only.** `BEFORE_RUN='chmod 444 app.js' $S/scripts/drive.sh read-only "$F" app.js` with the same block. Exit code `3`, stderr `rpatch: app.js: file is read-only; nothing written`; `diff.txt` empty. Repeat with `-n`: also exit `3`.
- **Mixed line endings.** Make a fixture file with `printf 'a\nb\r\nc\n' > mixed.txt`, then drive `<<<<<<< OLD` / `a` / `=======` / `A` / `>>>>>>> NEW` on it. Exit code `1`, stderr `the file mixes LF and CRLF line endings` and `line 1 ends with LF, line 2 with CRLF`; `diff.txt` empty.
- **BOM and permissions.** A fixture file starting with `\xef\xbb\xbf` and `chmod 755`. After an edit, `od -c after/<file> | head -1` still starts with `357 273 277`, and `ls -l after/<file>` shows `rwxr-xr-x`.
- **Changed after reading.** Not drivable from the CLI (rpatch reads and writes within one call). Proof is the unit test: `cargo test --lib write::tests` passes `a_file_changed_after_reading_is_not_overwritten`.

## Gotchas

- `drive.sh` copies with `cp -Rp`, so `before/` and `after/` keep modes; compare modes there, not in the fixture.
- Hard links break on write (rpatch warns); a fixture with hard links needs `BEFORE_RUN='ln app.js app2.js'`, since `cp -R` does not keep them.
