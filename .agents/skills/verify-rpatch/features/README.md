# rpatch verification map

This directory is the maintained source for verifying what rpatch does for its users: agents that edit files by piping blocks to `rpatch` from Bash. Read this index, then use the matching feature file as the recipe.

## Baseline preconditions

- `cargo build` has run and `.agents/skills/verify-rpatch/scripts/doctor.sh` prints `OK`.
- Every drive goes through `scripts/drive.sh` on a fixture dir; `fixtures/basic` holds `app.js`, `notes.md` (line 3 is one long paragraph) and `windows.txt` (CRLF).
- Never point a drive at the repo's own files, the user's projects, or the installed `~/.cargo/bin/rpatch`.

## Driving conventions

- Paths below are relative to the repo root; `S=.agents/skills/verify-rpatch`.
- Quote every heredoc delimiter (`<<'EOF'`), as agents are told to.
- Pass the input literally. One drive per behavior, with a distinct `<name>`.
- Read `exit code:` from the summary; `drive.sh` itself exits 0 when rpatch fails.

## Proof and skip reporting

- Proof is the evidence dir: `input.txt`, `exit_code`, `stdout.txt`, `stderr.txt`, and `diff.txt` (or `before/` vs `after/` for byte checks).
- A failure proof needs the exit code, the message in `stderr.txt`, and an empty `diff.txt`.
- Name the feature ID and the evidence dir with every claim.
- If a recipe cannot run (build broken, fixture missing), report the command and the unmet precondition; do not report it as verified through another feature.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the behavior, then exactly four H2 sections: `Sub-features`, `How to get to it (user POV)`, `Driving it with drive.sh`, `Gotchas`.

## Features

- [replace.md](replace.md): replace whole lines (`OLD`, `count=N`) or part of a line (`OLD part`), following the file's line endings.
- [range.md](range.md): replace a range between `FROM` and `TO` lines, including a blank TO and no TO.
- [insert.md](insert.md): insert lines `BEFORE` or `AFTER` an anchor without rewriting any line.
- [failure.md](failure.md): all-or-nothing on a mismatch, exit codes, and the diagnostics that tell the agent how to fix the input.
- [write-safety.md](write-safety.md): `--dry-run`, read-only files, files changed after reading, BOM, mixed line endings.
- [targeting.md](targeting.md): choosing files with `=== path`, a file argument, a bare path line, `--- path`, and JSON input.
