---
name: verify-rpatch
description: Drive the rpatch CLI the way an agent does (a heredoc of blocks on stdin, in a scratch copy of real files) and capture proof of what it wrote, printed and exited with. Use to prove a change to rpatch works before calling it done, to reproduce a reported misbehavior, or to check a feature in features/ end to end. Covers the CLI; the MCP server and the log-analysis dashboard are secondary surfaces, noted below.
---

# Verify rpatch

rpatch is a short-lived CLI: no server to keep alive. Each drive builds once, copies a fixture to a fresh scratch dir, pipes one input to the binary there, and keeps the evidence. Drives never touch the repo's own files or the fixture.

## Launch

From the repo root:

```bash
cargo build                      # the binary under test: target/debug/rpatch
```

Ready when `target/debug/rpatch -V` prints the version in `Cargo.toml`. Verify the working tree's build, not the `rpatch` on PATH (`~/.cargo/bin/rpatch` is the installed release and may be older). For a release build, set `RPATCH_BIN=target/release/rpatch`. There is nothing to tear down after a build.

## Doctor

```bash
.agents/skills/verify-rpatch/scripts/doctor.sh
```

Read-only. Exit 0 and `OK` when the binary exists, its version matches `Cargo.toml`, and no file in `src/` is newer than it. On `FAIL: sources newer than the binary`, run `cargo build` before driving: a stale binary proves nothing about the change. It also prints the PATH rpatch, for information.

## Drive

```bash
.agents/skills/verify-rpatch/scripts/drive.sh <name> <fixture-dir> [rpatch args...] <<'EOF'
...rpatch input...
EOF
```

- `<fixture-dir>` is copied; `.agents/skills/verify-rpatch/fixtures/basic` ships with `app.js`, `notes.md` (one long paragraph line) and `windows.txt` (CRLF). Make another fixture dir for a case these do not cover.
- The input is passed exactly as an agent would: quote the heredoc delimiter (`<<'EOF'`). When the input itself contains a line `EOF`, use another delimiter.
- `BEFORE_RUN='chmod 444 app.js'` runs a shell snippet in the scratch copy right before rpatch, for states such as read-only files or symlinks.
- The script prints a summary (version, args, exit code, stdout, stderr, diff) and the evidence path. Its own exit status is 0 even when rpatch fails: read `exit code:` in the summary.

The block format is in `target/debug/rpatch --help` (the first 40 lines are what agents read). Feature recipes with exact inputs and expected results are in `features/`; start at `features/README.md`.

## Evidence

Each drive writes `target/verify-rpatch/<UTC time>-<name>/`: `input.txt`, `args.txt`, `stdout.txt`, `stderr.txt`, `exit_code`, `before/`, `after/`, `diff.txt`, `summary.txt`. `target/` is gitignored, so evidence never lands in a commit by accident.

Proof standards:

- Drive the real path: the built binary, stdin, a real file tree. Not `rpatch::apply` from a test, not a hand-written expectation.
- Capture the action and the result: the input, the exit code, the output, and the file tree before and after. A success message without `diff.txt` showing the change is not proof.
- A failure is proved by all three: the expected exit code, the expected message in `stderr.txt`, and an empty `diff.txt` (nothing written).
- `--dry-run` is checked by observing: `diff.txt` must be empty, and stdout must end with `(dry run: nothing written)`.
- Byte-level claims (CRLF kept, BOM kept, no final newline) need `od -c` or `cmp` on files in `after/`, not a look at the diff.

## Cleanup

`drive.sh` deletes its scratch dir on exit (also after a failure). It starts no processes. Evidence stays under `target/verify-rpatch/`; delete old runs by hand with `rm -r target/verify-rpatch/<run>` when no longer needed, never as part of a verification.

## Other surfaces

- **MCP server** (`cargo build --features mcp`, then `target/debug/rpatch mcp`): driven by `tests/mcp.rs` over stdio; it calls the same `rpatch::apply`, so CLI proofs cover its behavior except the transport and the tool description in `src/mcp.rs`.
- **Dashboard** (`python3 tools/log-analysis/dashboard.py`, http://127.0.0.1:8765/): reads real session logs, which can hold credentials. Do not drive it for verification on someone else's logs; if port 8765 is taken, it is probably the user's own instance: use `--port` with a free port and kill only the process you started.

## Isolation

Any number of drives can run at once: each has its own scratch dir and evidence dir (the evidence name has the second; give concurrent drives distinct names).
