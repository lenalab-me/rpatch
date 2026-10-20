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
   - A leading BOM is removed (Windows PowerShell can add one to what it pipes). Then, if the input starts with `{` (after whitespace), it is JSON; otherwise it is the block format.
   - In the block format, the CLI's file argument (`rpatch FILE`) is the file for blocks before the first `=== <file>` line. Agents often wrote `rpatch FILE <<'EOF'` and failed (2026-10-03, 7 times in Claude Code). At most one file; not for JSON input. If a `=== ` line for another file comes before the first block, so that the argument is not used, the input is invalid (it is not silently ignored).
   - A file line written without its `=== `, as a bare path (`main.go`) or a diff header (`--- main.go`), is read as `=== main.go` when the very next line is a block opener. It has only one meaning, and a missing file still fails as file not found, so accepting it cannot break anything. It used to stop with a hint, but none of the 7 cases from 2026-10-04 to 10-06 came back to rpatch: they moved to other means, which write without checking, so stopping was the riskier choice.
   - Consecutive `=== ` lines (blank lines between are fine) make one group, and each block after them is applied to every file in it: exact matching per file, all or nothing. A `=== ` line with no block has no other meaning. Before, only the last file got the blocks and the others silently stayed as they were, with exit status 0 (2026-10-07, adding one line to CLAUDE.md, AGENTS.md and the Cursor rule at once).
   - `~/` in a file name is the home directory, in blocks and in JSON, where no shell expands it (the same day, `=== ~/.cursor/rules/global.mdc` failed as file not found).
   - Both become a list of `Edit`, which is then validated as a whole (`input::validate`). An empty OLD, `old == new`, an empty BEFORE/AFTER text, nothing to insert, and `count=0` are input errors (exit 2).
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
   - First, the files to be written are checked for read-only ones (a rename would replace a read-only file anyway). `--dry-run` checks this too, so it does not report success where the real run fails.
   - Likewise, each file is checked to be unchanged since it was read (byte for byte, BOM included); if one changed, nothing is written (exit status 3), so that a change by another agent, a formatter or an editor on the same tree is not silently dropped. Each file is checked again right before its rename, to keep the window small.
   - Each file is written to a temporary file in the same directory (`.rpatch-*`), synced with `fsync`, given the original permissions, and renamed over the original. A file that had a BOM gets it back.
   - If writing a file fails, the files already written are restored to their original content. If restoring also fails, the files left modified are reported.
5. **Output**
   - One line per file: `M <file>  edits=<n>  at_lines=<changed lines>`. Below it: the replaced line range for a range edit, and a warning for a file with hard links.
   - A file whose edits cancel out, so that its content equals the original, shows `= <file>  edits=<n>  unchanged` and is not written (`M` and at_lines would claim a change that is not there).
   - With `--diff`, a unified diff follows, with headers `a/<file>` and `b/<file>`. Absolute paths are shown as they are.

Because files are replaced by rename, hard links are broken (other links keep the old content); rpatch warns when that happens. A file restored after a failed write is restored by rename as well, so its hard links break too. Only the permission bits carry over, not extended attributes or ACLs. Changes after reading are caught as above, but a change in the tiny window between that check and the rename, and rollback after the process is killed midway, are out of scope (no lock is taken).

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
| `help.txt` | The text of `--help`: the summary (through the exit status paragraph), then the sections |
| `help.rs` | What `--help` prints: the summary (within 40 lines), `--help TOPIC` a section, `--help all` everything |

## Kinds of edits

An `Edit` has a target (`Target`), the replacement (`new`), and the exact number of matches required (`count`). Edits from the block format have `lines = true`; `OLD part` also sets `part = true`.

| Written as | `Target` | Matching |
|---|---|---|
| `<<<<<<< OLD` | `Text` | Only at line starts; whole lines |
| `<<<<<<< OLD part` | `Text` (`part`) | Anywhere; without the final line break |
| `<<<<<<< FROM` / `TO` | `Range` | Only at line starts; the last line may be just the start of a line |
| `<<<<<<< BEFORE` / `AFTER` | `Insert` | Anywhere (part of one or more lines); lines are inserted at line boundaries |
| JSON `old` | `Text` | Anywhere |
| JSON `from` / `to` | `Range` | Anywhere |

### Common rules

- Overlapping matches count (`x\nx` occurs twice in `x\nx\nx`). If matches overlap, the edit fails whatever the count. Without counting overlaps, an ambiguous text would silently replace its first match.
- The number of matches must equal `count` (default 1).

### Line breaks in the block format

- Each line of a block is taken to end with a line break. If the file uses CRLF, the block's line breaks are turned into CRLF before matching. JSON strings are not converted.
- A file that mixes LF and CRLF cannot be edited with blocks, and gets its own error (naming an LF line and a CRLF line): there is no single line ending to follow, its LF lines would never match, and inserted lines would get CRLF. JSON matches its texts as written.
- When the file's last line has no line break, OLD matches that line without one, and the replacement adds no final line break. The same goes for AFTER inserting after that last line.

### OLD and OLD part

- OLD matches whole lines only. If the start of a long line were allowed to match, an edit meant to replace the whole line would silently leave the rest of it behind. So when the text is found only as part of a line, the edit fails and that line is shown.
- To change part of a line, write `OLD part` on purpose. Its text matches anywhere and excludes the final line break, like JSON `old`.

### FROM / TO

- FROM must match exactly once. TO is the first match after the last line of FROM.
- The text from FROM up to (not including) TO is replaced with NEW; TO is kept. If NEW's last lines are the TO lines, the edit fails (TO would appear twice, a common slip). The comparison is by whole lines, so a TO of `}` with NEW ending in `    }` passes.
- No TO lines is the end of the file: FROM up to the end is replaced. Agents cut a file's tail with `head -n` and line numbers otherwise (2026-10-02).
- When the last line of FROM, TO or an anchor is blank, it matches a blank line, not the start of a line. An empty start matches any line, so a TO of one blank line used to mean "to the end of the file" and silently dropped the lines after it. Now it means "up to the next blank line".

### BEFORE / AFTER

- The anchor is one or more lines (or part of them) and must match exactly `count` times. Two matches on the same line are an error. BEFORE inserts before the line where the match starts, AFTER after the line where it ends. Anchors of two or more lines arrived in v0.10.0 (before, only one line was allowed, and Codex often wrote more and failed).
- No existing line is rewritten; NEW's lines are inserted at the line boundary before or after the line containing the anchor. Several AFTER blocks on one anchor end up in reverse order, since each goes right below the anchor.

## Diagnostics

An edit that does not match gets a headline and details.

- **Not found**
  - In the block format, if the text is found only as part of a line, the message says which line and how (only its start, only its end, or in the middle), and points to `OLD part` or BEFORE/AFTER (`partial_line` in `matcher.rs`). For OLD, it also shows what the lines would become with `OLD part` and the same OLD and NEW (`part_preview`): all the replaced lines, up to 8, not only the cut one, since for an edit that adds lines before the cut line, that line alone read unchanged (seen in a trial on 2026-10-06). Most of these failures are a long line written only up to where the change ends (39 of 52 from 2026-09-29 to 10-06); naming `OLD part` alone could lead an agent whose NEW holds the whole line to switch and keep the rest of the line twice, so the result is shown to choose from.
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
| 3 | A read or write failed (not UTF-8, read-only, changed on disk after reading, temporary file or rename failed) |

When 1 and 3 both happen, the status is 3. Failing to print the result after the files were written is 3 too (a reader that stopped early, as in `| head`, is not a failure).

## Out of scope

- Creating and deleting files (`cat > file <<'EOF'` and `rm` do that).
- Regular expressions (`sd` does that).
- Syntax checks before writing. Only 3 of 670 logged edits broke a file's syntax (0.45%), and the agent's own check caught each right away, so this was dropped.

## Tests

- `tests/cli.rs`: CLI integration tests. Success and failure for each kind of edit, the wording of diagnostics, and properties of writing (permissions, symlinks, BOM, hard links, read-only files).
- Unit tests in the modules (line numbers, listing matches including overlapping ones, and so on).
- `tests/mcp.rs`: the MCP server's handshake, and that it returns the same output as the CLI (only with `--features mcp`).

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass both without features and with `--features mcp`. CI (`.github/workflows/build.yml`) runs on manual dispatch only.
