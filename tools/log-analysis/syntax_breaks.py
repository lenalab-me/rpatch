"""編集のあと、同じ呼び出しか直後の数回で構文エラーが出た回数を数える（v1 の判断材料）。

使い方: python3 syntax_breaks.py calls.jsonl [--exclude-proj rpatch] [--samples N]
数は候補で、curl の応答の JSON エラーなど編集と関係ないものも拾う。--samples で中身を見て分ける。
"""
import argparse, collections, json, re, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from metrics import edit_event  # noqa: E402

SYNTAX = re.compile(
    r'SyntaxError|Unexpected token|Unexpected end of input|Unexpected end of JSON|IndentationError'
    r'|unexpected closing delimiter|unclosed delimiter|mismatched closing delimiter'
    r'|JSONDecodeError|is not valid JSON|error TS1\d{3}|Unterminated string')
RUNS = re.compile(r'\b(node|npm|npx|pnpm|yarn|bun|deno|python3?|pytest|cargo|tsc|jq|make)\b')
READS = re.compile(r'^\s*(rg|grep|cat|sed -n|head|tail|less|bat)\b')
WINDOW = 5


def syntax_error(c):
    if c['tool'] != 'Bash':
        return None
    cmd = (c.get('input') or {}).get('command') or ''
    if not RUNS.search(cmd) or READS.search(cmd):
        return None
    m = SYNTAX.search(c.get('res') or '')
    return m.group(0) if m else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('calls')
    ap.add_argument('--exclude-proj', action='append', default=[])
    ap.add_argument('--samples', type=int, default=0)
    a = ap.parse_args()

    sessions = collections.defaultdict(list)
    for line in open(a.calls):
        c = json.loads(line)
        if any(x in c['proj'] for x in a.exclude_proj):
            continue
        sessions[(c['proj'], c['file'])].append(c)

    edits = collections.Counter()
    broke = collections.Counter()
    samples = []
    any_syntax = 0
    for calls in sessions.values():
        calls.sort(key=lambda c: c['idx'])
        any_syntax += sum(1 for c in calls if syntax_error(c))
        for i, c in enumerate(calls):
            e = edit_event(c)
            if not e:
                continue
            kind, files, failed = e
            exts = {f.rsplit('.', 1)[-1] for f in files if '.' in f}
            key = kind
            edits[key] += 1
            hit = syntax_error(c) and (c, syntax_error(c))
            if not hit:
                for d in calls[i + 1:i + 1 + WINDOW]:
                    if edit_event(d):
                        break
                    if syntax_error(d):
                        hit = (d, syntax_error(d))
                        break
            if hit:
                broke[key] += 1
                samples.append((kind, sorted(exts), c, hit[0], hit[1]))
    print('all calls with a syntax error in a run:', any_syntax)
    for k in sorted(edits):
        print(f'{k:8} edits={edits[k]:5}  followed_by_syntax_error={broke[k]:4}  ({broke[k] / edits[k]:.1%})')
    total_e, total_b = sum(edits.values()), sum(broke.values())
    print(f'{"total":8} edits={total_e:5}  followed_by_syntax_error={total_b:4}  ({total_b / total_e if total_e else 0:.1%})')
    print('by extension:', collections.Counter(x for s in samples for x in s[1] or ['?']).most_common())
    for kind, exts, c, d, sig in samples[:a.samples]:
        cmd = (d.get('input') or {}).get('command') or ''
        res = d.get('res') or ''
        k = res.find(sig)
        print('---', c['proj'], c['ts'], kind, exts)
        print('  check:', cmd[:160].replace('\n', ' '))
        print('  error:', res[max(0, k - 150):k + 150].replace('\n', ' | '))


if __name__ == '__main__':
    main()
