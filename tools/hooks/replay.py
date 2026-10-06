#!/usr/bin/env python3
"""Replays logged Bash commands through plugin/hooks/edit_guard.py and counts what each rule stops.

usage: python3 tools/hooks/replay.py CORPUS.jsonl [--samples N] [--rule RULE] [--old HOOK.py]

CORPUS has one JSON object per line with "command", "cwd", "ts" and "session" (write it outside
the repo: commands can hold secrets). "Tracked by git" is judged from each command's cwd on the
disk as it is now, two ways: "committed" counts a file only when git had it in a commit before
the command ran (a file the command was creating is new, as it was then); "in index" counts what
is in git's index today, an upper bound for files that were added but not yet committed then.
A file deleted since then counts in neither. The slowest command shows how long the hook takes.
--old compares the python-replace rule with an earlier hook that has python_code, REPLACE and
WRITE (default: ~/.claude/hooks/no-python-replace.py when it exists).
"""
import argparse
import collections
import importlib.util
import json
import os
import subprocess
import sys
import time
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def as_of(guard):
    """guard.Files that also requires the file to be in a commit older than `self.ts`."""

    class AsOf(guard.Files):
        ts = None
        added = {}

        def is_tracked(self, path):
            if not super().is_tracked(path):
                return False
            real = os.path.realpath(path)
            if real not in self.added:
                r = subprocess.run(["git", "-C", os.path.dirname(real), "log", "--diff-filter=A", "--format=%cI",
                                    "--", os.path.basename(real)], capture_output=True, text=True, timeout=10)
                dates = r.stdout.split()
                self.added[real] = dates[-1] if dates else None
            added = self.added[real]
            if not added or not self.ts:
                return False
            return datetime.fromisoformat(added) < datetime.fromisoformat(self.ts.replace("Z", "+00:00"))

    return AsOf()


def old_blocks(old, command):
    return "python" in command and any(old.REPLACE.search(c) and old.WRITE.search(c) for c in old.python_code(command))


def main():
    p = argparse.ArgumentParser()
    p.add_argument("corpus")
    p.add_argument("--samples", type=int, default=5)
    p.add_argument("--rule")
    p.add_argument("--old", default=os.path.expanduser("~/.claude/hooks/no-python-replace.py"))
    a = p.parse_args()

    guard = load(os.path.join(HERE, "..", "..", "plugin", "hooks", "edit_guard.py"), "edit_guard")
    old = load(a.old, "old_hook") if a.old and os.path.exists(a.old) else None
    files, index = as_of(guard), guard.Files()
    counts, in_index, samples = collections.Counter(), collections.Counter(), collections.defaultdict(list)
    agree = collections.Counter()
    errors = 0
    total = 0
    slowest = (0.0, "")
    with open(a.corpus) as fh:
        for line in fh:
            row = json.loads(line)
            command, cwd = row["command"], row.get("cwd")
            files.ts = row.get("ts")
            total += 1
            try:
                start = time.monotonic()
                verdict = guard.decide(command, cwd, files)
                slowest = max(slowest, (time.monotonic() - start, command))
                upper = guard.decide(command, cwd, index)
            except Exception as e:  # the hook lets these through; count them
                errors += 1
                samples["error"].append((row["session"], f"{type(e).__name__}: {e}", command))
                continue
            if upper:
                in_index[upper[0]] += 1
            rule = verdict[0] if verdict else None
            if rule:
                counts[rule] += 1
                samples[rule].append((row["session"], verdict[1][:160], command))
            if old:
                agree[(old_blocks(old, command), rule == "python-replace")] += 1

    rules = ["python-replace"] + [r for r, _ in guard.RULES]
    print(f"{total} commands          committed  in index")
    for rule in rules:
        print(f"  {rule:18} {counts[rule]:9}  {in_index[rule]:8}")
    print(f"  {'error':18} {errors}")
    print(f"slowest: {slowest[0] * 1000:.0f} ms  {slowest[1].splitlines()[0][:100] if slowest[1] else ''}")
    if old:
        print(f"python-replace vs the old hook: both {agree[(True, True)]}, only old {agree[(True, False)]}, "
              f"only new {agree[(False, True)]}")
    for rule in [a.rule] if a.rule else rules + ["error"]:
        for session, why, command in samples[rule][:a.samples]:
            first = "\n".join(command.splitlines()[:3])[:300]
            print(f"\n[{rule}] {session.split('/')[0][-30:]}  {why[:110]}\n  {first}")


if __name__ == "__main__":
    sys.exit(main())
