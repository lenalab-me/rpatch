"""extract.py の出力から、rpatch の導入効果を測る数値を出す。

使い方: python3 metrics.py calls.jsonl [--since 2026-09-24] [--until 2026-10-02] [--exclude-proj rpatch]
ts は UTC の ISO 文字列なので、--since/--until は前方一致の文字列比較で絞る。
"""
import argparse, json, re, collections

PY = re.compile(r'\bpython3?\b')
REPLACE = re.compile(r'\.replace\(|\bre\.sub\(')
WRITE = re.compile(r"""open\([^)]*['"]w['"]|write_text\(|\.write\(""")
GUARD = re.compile(r'\bassert\b|\braise\b|\.count\(')
PY_FAIL = re.compile(r'Traceback|AssertionError')
# コマンドの先頭で呼んでいるものだけ（`cd .../rpatch` や `for n in rpatch ...` は数えない）
RPATCH = re.compile(r'(^|[;&|(\n])\s*(\S*/)?rpatch(\s|$)')
EDIT_MISMATCH = re.compile(r'String to replace not found|Found \d+ matches')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('calls')
    ap.add_argument('--since', default='')
    ap.add_argument('--until', default='9999')
    ap.add_argument('--exclude-proj', action='append', default=[])
    a = ap.parse_args()

    sessions = collections.defaultdict(list)
    for line in open(a.calls):
        c = json.loads(line)
        ts = c.get('ts') or ''
        if not (a.since <= ts < a.until):
            continue
        if any(x in c['proj'] for x in a.exclude_proj):
            continue
        sessions[(c['proj'], c['file'])].append(c)

    n = collections.Counter()
    for calls in sessions.values():
        calls.sort(key=lambda c: c['idx'])
        prev_edit_file = None
        for c in calls:
            n['tool_calls'] += 1
            tool, inp, res = c['tool'], c.get('input') or {}, c.get('res') or ''
            if tool == 'Bash':
                cmd = inp.get('command') or ''
                if PY.search(cmd) and REPLACE.search(cmd) and WRITE.search(cmd):
                    n['python_replace'] += 1
                    if not GUARD.search(cmd):
                        n['python_replace_unguarded'] += 1
                    if c.get('err') or PY_FAIL.search(res):
                        n['python_replace_failed'] += 1
                if RPATCH.search(cmd):
                    n['rpatch'] += 1
                    if 'nothing written' in res:
                        n['rpatch_failed'] += 1
            if tool in ('Edit', 'MultiEdit'):
                n['edit'] += 1
                f = inp.get('file_path')
                if f and f == prev_edit_file:
                    n['edit_same_file_consecutive'] += 1
                prev_edit_file = f
                if c.get('err') and EDIT_MISMATCH.search(res):
                    n['edit_failed'] += 1
            elif tool not in ('Read', 'Grep', 'Glob'):
                # 読むだけの呼び出しを挟んでも「連続編集」とみなす
                prev_edit_file = None

    edits = n['python_replace'] + n['edit'] + n['rpatch']
    out = {
        'since': a.since, 'until': a.until, 'exclude_proj': a.exclude_proj,
        'sessions': len(sessions),
        **{k: n[k] for k in [
            'tool_calls', 'python_replace', 'python_replace_unguarded', 'python_replace_failed',
            'edit', 'edit_same_file_consecutive', 'edit_failed', 'rpatch', 'rpatch_failed',
        ]},
        'replace_failed_total': n['python_replace_failed'] + n['edit_failed'] + n['rpatch_failed'],
        'share_python_replace': round(n['python_replace'] / edits, 3) if edits else None,
        'python_replace_per_1k_calls': round(1000 * n['python_replace'] / n['tool_calls'], 1) if n['tool_calls'] else None,
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))


main()
