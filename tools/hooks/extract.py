#!/usr/bin/env python3
"""Writes the Bash commands of recent Claude Code sessions as JSONL for tools/hooks/replay.py.

usage: python3 tools/hooks/extract.py DAYS OUT.jsonl

One line per command: session, cwd, ts, command, the first 400 characters of the result.
The output holds session contents, which can include credentials: write it outside the repo
(e.g. /tmp), chmod 600, and delete it when done. prod-rpatch and pi-agent sessions are left out.
"""
import glob
import json
import os
import sys
import time

ROOT = os.path.expanduser('~/.claude/projects')
EXCLUDE = ('prod-rpatch', 'pi-agent')
days = float(sys.argv[1]); out = sys.argv[2]
now = time.time(); n = 0
with open(out, 'w') as w:
    for f in glob.glob(ROOT + '/*/*.jsonl') + glob.glob(ROOT + '/*/*/subagents/*.jsonl'):
        if any(x in f for x in EXCLUDE) or now - os.path.getmtime(f) > days * 86400:
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
