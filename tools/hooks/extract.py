#!/usr/bin/env python3
"""Writes the Bash commands of recent Claude Code sessions as JSONL for tools/hooks/replay.py.

usage: python3 tools/hooks/extract.py DAYS OUT.jsonl [--exclude NAME]...

One line per command: session, cwd, ts, command, the first 400 characters of the result.
The output holds session contents, which can include credentials: write it outside the repo
(e.g. /tmp), chmod 600, and delete it when done. --exclude leaves out the projects whose names
contain NAME (any number of times).
"""
import argparse
import glob
import json
import os
import time

ROOT = os.path.expanduser('~/.claude/projects')
ap = argparse.ArgumentParser(usage=__doc__.splitlines()[2].removeprefix('usage: '))
ap.add_argument('days', type=float)
ap.add_argument('out')
ap.add_argument('--exclude', action='append', default=[], metavar='NAME')
a = ap.parse_args()
days, out, exclude = a.days, a.out, a.exclude
now = time.time(); n = 0
with open(out, 'w') as w:
    for f in glob.glob(ROOT + '/*/*.jsonl') + glob.glob(ROOT + '/*/*/subagents/*.jsonl'):
        if any(x in os.path.relpath(f, ROOT).split(os.sep)[0] for x in exclude) or now - os.path.getmtime(f) > days * 86400:
            continue
        uses, results = [], {}
        with open(f, errors='replace') as fh:
            for line in fh:
                try:
                    d = json.loads(line)
                except ValueError:
                    continue
                m = d.get('message')
                if not isinstance(m, dict) or not isinstance(m.get('content'), list):
                    continue
                for b in m['content']:
                    if not isinstance(b, dict):
                        continue
                    if b.get('type') == 'tool_use' and b.get('name') == 'Bash' and 'id' in b:
                        uses.append((b['id'], (b.get('input') or {}).get('command') or '', d.get('cwd'), d.get('timestamp')))
                    elif b.get('type') == 'tool_result':
                        rc = b.get('content')
                        if isinstance(rc, list):
                            rc = '\n'.join(x.get('text', '') for x in rc if isinstance(x, dict))
                        results[b.get('tool_use_id')] = rc if isinstance(rc, str) else str(rc)
        for i, cmd, cwd, ts in uses:
            w.write(json.dumps({'session': os.path.relpath(f, ROOT), 'cwd': cwd, 'ts': ts, 'command': cmd,
                                'result': (results.get(i) or '')[:400]}, ensure_ascii=False) + '\n')
            n += 1
print(n, 'commands')
