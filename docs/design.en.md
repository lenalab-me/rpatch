# rpatch design

rpatch is a CLI that edits existing UTF-8 files by exact match, many edits at once. It is meant to be called by agents from a shell. Four principles:

- Matching is exact. Whitespace and line breaks are never matched loosely.
- All or nothing. If any edit does not match, no file is written.
- On failure, show line numbers and the cause, so the next attempt can fix it.
- The input needs no escaping (the block format).

Usage: `rpatch --help`. The MCP server: [mcp.md](mcp.md) (Japanese). Japanese version of this document: [design.md](design.md).

## Flow

`apply()` in `lib.rs` is shared by the CLI and the MCP server. One call goes through these steps.

1. **Parse** (`input.rs`, `block.rs`)
   - If the input starts with `{` (after whitespace), it is JSON; otherwise it is the block format.
   - Both become a list of `Edit`, which is then validated as a whole (`input::validate`). An empty OLD, `old == new`, a BEFORE/AFTER text of two or more lines, nothing to insert, and `count=0` are input errors (exit 2).
2. **Load** (`load_files` in `lib.rs`)
   - Each file named by the edits is read once. Paths are resolved against `cwd` with `canonicalize`, so different spellings of the same file (`a.txt` and `./a.txt`, a symlink and its target) become one. A symlink's target is what gets edited.
   - A file that is not UTF-8 fails to load (exit 3). A leading BOM is removed and remembered; it takes no part in matching.
   - The permissions and the hard link count are remembered too.
3. **Match and apply** (`matcher.rs`, `apply_edits` in `lib.rs`)
   - Edits are applied in input order, each to the file's current content: a later edit to the same file sees the result of the earlier ones.
   - When an edit does not match, the remaining edits to that file are not checked (reported as "not checked"). Edits to other files are still checked.
   - If the failed edit would have matched the file as it was before the earlier edits in the batch, that is reported too (an earlier edit changed the text, a common slip).
4. **Write** (`write.rs`)
   - If any edit did not match, nothing is written. `--dry-run` writes nothing either.
   - First, the files to be written are checked for read-only ones (a rename would replace a read-only file anyway).
   - Each file is written to a temporary file in the same directory (`.rpatch-*`), synced with `fsync`, given the original permissions, and renamed over the original. A file that had a BOM gets it back.
   - If writing a file fails, the files already written are restored to their original content. If restoring also fails, the files left modified are reported.
5. **Output**
   - One line per file: `M <file>  edits=<n>  at_lines=<changed lines>`. Below it: the replaced line range for a range edit, and a warning for a file with hard links.
   - With `--diff`, a unified diff follows, with headers `a/<file>` and `b/<file>`. Absolute paths are shown as they are.

Because files are replaced by rename, hard links are broken (other links keep the old content); rpatch warns when that happens. Concurrent edits by other processes, and rollback after the process is killed midway, are out of scope.

## Modules

| File | Role |
|---|---|
| `main.rs` | Arguments, reading stdin, exit codes. Hands `rpatch mcp` to the MCP server |
| `lib.rs` | `apply()`: loading, applying edits, computing `at_lines`, building the output |
| `input.rs` | Telling the formats apart, parsing JSON, the `Edit` and `Target` types, validation |
| `block.rs` | Parsing the block format, and diagnostics for mistyped markers |
| `matcher.rs` | Finding where one edit applies in a file; the entry point for mismatch diagnostics |
| `diagnose.rs` | Explanations when text is not found or the count is wrong (similar lines and so on) |
| `write.rs` | All-or-nothing writing, and rollback on failure |
| `text.rs` | Line numbers, and how lines are shown in diagnostics |
| `mcp.rs` | The MCP server (only with the Cargo feature `mcp`) |
| `help.txt` | The text of `--help` |

## Kinds of edits

An `Edit` has a target (`Target`), the replacement (`new`), and the exact number of matches required (`count`). Edits from the block format have `lines = true`; `OLD part` also sets `part = true`.

| Written as | `Target` | Matching |
|---|---|---|
| `<<<<<<< OLD` | `Text` | Only at line starts; whole lines |
| `<<<<<<< OLD part` | `Text` (`part`) | Anywhere; without the final line break |
| `<<<<<<< FROM` / `TO` | `Range` | Only at line starts; the last line may be just the start of a line |
| `<<<<<<< BEFORE` / `AFTER` | `Insert` | Anywhere (part of one line); lines are inserted at line boundaries |
| JSON `old` | `Text` | Anywhere |
| JSON `from` / `to` | `Range` | Anywhere |

### Common rules

- Overlapping matches count (`x\nx` occurs twice in `x\nx\nx`). If matches overlap, the edit fails whatever the count. Without counting overlaps, an ambiguous text would silently replace its first match.
- The number of matches must equal `count` (default 1).

### Line breaks in the block format

- Each line of a block is taken to end with a line break. If the file uses CRLF, the block's line breaks are turned into CRLF before matching. JSON strings are not converted.
- When the file's last line has no line break, OLD matches that line without one, and the replacement adds no final line break. The same goes for AFTER inserting after that last line.

### OLD and OLD part

- OLD matches whole lines only. If the start of a long line were allowed to match, an edit meant to replace the whole line would silently leave the rest of it behind. So when the text is found only as part of a line, the edit fails and that line is shown.
- To change part of a line, write `OLD part` on purpose. Its text matches anywhere and excludes the final line break, like JSON `old`.

### FROM / TO

- FROM must match exactly once. TO is the first match after the last line of FROM.
- The text from FROM up to (not including) TO is replaced with NEW; TO is kept. If NEW ends with the TO text, the edit fails (TO would appear twice, a common slip).

### BEFORE / AFTER

- The anchor is one line (or part of one) and must match exactly `count` times. Two matches on the same line are an error.
- No existing line is rewritten; NEW's lines are inserted at the line boundary before or after the line containing the anchor. Several AFTER blocks on one anchor end up in reverse order, since each goes right below the anchor.

## Diagnostics

An edit that does not match gets a headline and details.

- **Not found**
  - In the block format, if the text is found only as part of a line, the message says which line and how (only its start, only its end, or in the middle), and points to `OLD part` or BEFORE/AFTER (`partial_line` in `matcher.rs`).
  - Otherwise the first that applies, in this order (`diagnose::not_found`): a CRLF difference, a whitespace difference (with the line), how far the text matches and where it diverges, the most similar line.
- **Wrong count**: the lines that matched (up to 10) and how to write the count.
- **Overlap**: the overlapping ranges (columns, when within a line) and a hint to add surrounding text.
- **Input errors**: for JSON syntax errors, the text around the error with a caret and hints about `\'` and raw line breaks. For the block format, hints about marker lengths and unclosed blocks, and for guessed formats (`--- old` and the like) an example of the real one.

Lines in diagnostics are cut at 200 characters, except the line shown when the text is found only as part of a line: that one is shown up to 4000 characters, so it can be copied as is.

## at_lines

For each file, the byte ranges changed by the edits (`changes`) are kept, and remapped when later edits shift the content (`splice`).

- Lines that the replaced text and the new text share at the start or end are not counted (they were written as context).
- A deletion shows the line now at the place of the deleted lines, or the last line if the end of the file was deleted.
- Ranges are turned into line numbers, sorted, and adjacent ones merged (`3,10-12`).

## Exit status

| Code | Meaning |
|---|---|
| 0 | Every edit matched (including `--dry-run`) |
| 1 | An edit did not match, or a file is missing |
| 2 | Invalid input (unparsable, empty OLD, `old == new`, …) or invalid arguments |
| 3 | A read or write failed (not UTF-8, read-only, temporary file or rename failed) |

## Out of scope

- Creating and deleting files (`cat > file <<'EOF'` and `rm` do that).
- Regular expressions (`sd` does that).
- Syntax checks before writing. Only 3 of 670 logged edits broke a file's syntax (0.45%), and the agent's own check caught each right away, so this was dropped.

## Tests

- `tests/cli.rs`: CLI integration tests. Success and failure for each kind of edit, the wording of diagnostics, and properties of writing (permissions, symlinks, BOM, hard links, read-only files).
- Unit tests in the modules (line numbers, listing matches including overlapping ones, and so on).
- `tests/mcp.rs`: the MCP server's handshake, and that it returns the same output as the CLI (only with `--features mcp`).

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass both without features and with `--features mcp`. CI (`.github/workflows/build.yml`) runs on manual dispatch only.
