---
name: rpatch
description: Edit existing files from Bash with rpatch, by exact match and all or nothing. Use whenever you change an existing file outside the Edit tool, even one spot, a part of a line, or many places across many files at once; and when a hook blocked `sed -i`, a python replace, or `cat >`/`echo >`/`printf >`/`tee` over a tracked file.
---

# rpatch

`rpatch` applies blocks read from stdin to existing UTF-8 files. Every edit must match exactly, or nothing is written and the error shows line numbers and the likely cause. Requires the `rpatch` binary on PATH (`cargo install --locked --path .` in the rpatch repo).

## Use

- To change a file, use `rpatch`, even for one spot. `sd` is for replacing in a pipe or with a regex.
- Read the format once: `rpatch --help | head -40`.
- Quote the heredoc delimiter, and chain follow-up commands on the opening line: `rpatch <<'EOF' && git diff --stat`. A command after the closing `EOF` runs even when the edit failed, and hides the failure.
- Do not replace with a python heredoc.

```
rpatch path/to/file <<'EOF' && git diff --stat
<<<<<<< OLD
exact lines to replace
=======
replacement lines
>>>>>>> NEW
EOF
```

`<<<<<<< OLD part` covers part of a line; `FROM`/`TO` replaces a range; `BEFORE`/`AFTER` inserts lines. `=== other/file` lines switch files within one input.

## When the hook blocks a command

The plugin's PreToolUse hook (`hooks/edit_guard.py`) stops writes that cannot be checked, and names what to use instead:

- python that replaces text and writes a file, and `sed -i` / `perl -i` on a file git tracks: use rpatch.
- `cat`/`echo`/`printf` `>` or `tee` without `-a` over a file git tracks: use rpatch for a part. To rewrite the whole file on purpose, read it first and use the Write tool, or write `>|` (`cat >| file <<'EOF'`).

New files, files git does not track, temp dirs and appends (`>>`) are never blocked. `RPATCH_GUARD=off` in the environment Claude Code starts with turns the hook off.
