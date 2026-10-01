"""extract.py の出力から、rpatch の導入効果を測る数値を出す。

使い方: python3 metrics.py calls.jsonl [--since 2026-09-24] [--until 2026-10-02] [--exclude-proj rpatch]
ts は UTC の ISO 文字列なので、--since/--until は前方一致の文字列比較で絞る。
"""
import argparse, json, re, collections, statistics
from datetime import datetime

PY = re.compile(r'\bpython3?\b')
REPLACE = re.compile(r'\.replace\(|\bre\.sub\(')
WRITE = re.compile(r"""open\([^)]*['"]w['"]|write_text\(|\.write\(""")
GUARD = re.compile(r'\bassert\b|\braise\b|\.count\(')
PY_FAIL = re.compile(r'Traceback|AssertionError')
# コマンドの先頭で呼んでいるものだけ（`cd .../rpatch` や `for n in rpatch ...` は数えない）
RPATCH = re.compile(r'(^|[;&|(\n])\s*(\S*/)?rpatch(\s|$)')
# dry run は、失敗するケースをわざと試すのにも使われるので、本番の失敗と分けて数える
RPATCH_DRY_RUN = re.compile(r'\brpatch\b[^\n]*\s(-n|--dry-run)\b')
EDIT_MISMATCH = re.compile(r'String to replace not found|Found \d+ matches')

# --- やり直しと確認（rework） ---
# 編集した呼び出しから、触ったファイル名（basename）を拾う
FILE_IN_QUOTES = re.compile(r"""['"]([^'"\s]+\.[A-Za-z0-9]{1,6})['"]""")
RPATCH_HEADER = re.compile(r'^=== (.+?)\s*$', re.M)
JSON_FILE = re.compile(r'"file"\s*:\s*"([^"]+)"')
SD = re.compile(r'(^|[;&|(\n])\s*sd\s')
SD_FILE = re.compile(r'(\S+\.[A-Za-z0-9]{1,6})(?=\s|$)')
# 編集の直後の「確かめ」: ファイル名を含む読み出し、または diff や構文チェック
CHECK_CMD = re.compile(r'\b(sed -n|cat|head|tail|rg|grep|wc|nl|bat)\b')
ALWAYS_CHECK = re.compile(r'\bgit diff\b|node --check|new Function')
# 同じコマンドの中での確かめ（python で書いたあとに読み出して print する、など）
PRINT_CHECK = re.compile(r'print\(.*(open|read|s\[|count)')
CHECK_WINDOW = 2


def base(path):
    return (path or '').rstrip('/').rsplit('/', 1)[-1]


def edit_event(c):
    """編集する呼び出しなら (種類, 触ったファイル名の集合, 失敗したか)。それ以外は None。"""
    tool, inp, res = c['tool'], c.get('input') or {}, c.get('res') or ''
    if tool in ('Edit', 'MultiEdit'):
        return 'edit', {base(inp.get('file_path'))}, bool(c.get('err'))
    if tool == 'mcp__rpatch__rpatch':
        if inp.get('dry_run'):
            return None
        text = inp.get('input') or ''
        files = RPATCH_HEADER.findall(text) + JSON_FILE.findall(text)
        return 'rpatch', {base(f) for f in files}, bool(c.get('err'))
    if tool != 'Bash':
        return None
    cmd = inp.get('command') or ''
    if RPATCH.search(cmd):
        if RPATCH_DRY_RUN.search(cmd):
            return None
        files = RPATCH_HEADER.findall(cmd) + JSON_FILE.findall(cmd)
        if not files:
            return None  # rpatch -V や --help など、編集しない呼び出し
        return 'rpatch', {base(f) for f in files}, 'nothing written' in res
    if PY.search(cmd) and REPLACE.search(cmd) and WRITE.search(cmd):
        files = FILE_IN_QUOTES.findall(cmd)
        return 'python', {base(f) for f in files}, bool(c.get('err') or PY_FAIL.search(res))
    if SD.search(cmd):
        return 'sd', {base(f) for f in SD_FILE.findall(cmd)}, bool(c.get('err'))
    return None


def is_check(c, files):
    tool, inp = c['tool'], c.get('input') or {}
    if tool == 'Read':
        return base(inp.get('file_path')) in files
    if tool != 'Bash':
        return False
    cmd = inp.get('command') or ''
    if ALWAYS_CHECK.search(cmd):
        return True
    # ヒアドキュメントや > はファイルを書く側なので数えない
    if '<<' in cmd or re.search(r'>\s*\S', cmd):
        return False
    return bool(CHECK_CMD.search(cmd)) and any(f and f in cmd for f in files)


def seconds(a, b):
    t = lambda c: datetime.fromisoformat(c['ts'].replace('Z', '+00:00'))
    return (t(b) - t(a)).total_seconds()


def rework(calls, stats):
    """失敗した編集から、同じファイルへの編集が成功するまでの呼び出し数と秒数。
    成功した編集のあと CHECK_WINDOW 回以内に、同じファイルを確かめたか。"""
    events = [edit_event(c) for c in calls]
    idx = [i for i, e in enumerate(events) if e]
    for k, i in enumerate(idx):
        kind, files, failed = events[i]
        s = stats[kind]
        s['events'] += 1
        s_len = calls[i].get('res_len') or 0
        if failed:
            s['failures'] += 1
            s['failure_chars'].append(s_len)
            for j in idx[k + 1:]:
                _, files2, failed2 = events[j]
                if not failed2 and (files & files2 or not files or not files2):
                    s['recovered'] += 1
                    s['recovery_calls'].append(j - i)
                    s['recovery_secs'].append(seconds(calls[i], calls[j]))
                    break
        else:
            s['ok'] += 1
            cmd = (calls[i].get('input') or {}).get('command') or ''
            if ALWAYS_CHECK.search(cmd) or PRINT_CHECK.search(cmd):
                s['checked_inline'] += 1
            if kind == 'rpatch' and re.search(r'\s--diff\b', cmd):
                s['diff_flag'] += 1
            stop = idx[k + 1] if k + 1 < len(idx) else len(calls)
            window = calls[i + 1:min(i + 1 + CHECK_WINDOW, stop)]
            s['result_chars'] += s_len
            checks = [c for c in window if is_check(c, files)]
            check_len = sum(c.get('res_len') or 0 for c in checks)
            if checks:
                s['checked_after'] += 1
                s['check_chars'] += check_len
            # 1 回の編集とその確かめで、コンテキストに載った文字数
            s['context_lens'].append(s_len + check_len)


def summarize(stats):
    out = {}
    for kind, s in sorted(stats.items()):
        med = lambda xs: round(statistics.median(xs), 1) if xs else None
        out[kind] = {
            'events': s['events'],
            'failures': s['failures'],
            'recovered': s['recovered'],
            'recovery_calls_median': med(s['recovery_calls']),
            'recovery_secs_median': med(s['recovery_secs']),
            'ok': s['ok'],
            # 同じ呼び出しの中での確かめ / rpatch の --diff / 直後の別の呼び出しでの確かめ
            'checked_inline': s['checked_inline'],
            'diff_flag': s['diff_flag'],
            'checked_after': s['checked_after'],
            'check_ratio': round(s['checked_after'] / s['ok'], 3) if s['ok'] else None,
            # コンテキストに載った文字数（成功した編集 1 回あたり）: 編集した呼び出しそのものの
            # 出力（rpatch の --diff や python の print を含む）と、直後の確かめの出力
            'result_chars_per_ok': round(s['result_chars'] / s['ok']) if s['ok'] else None,
            'check_chars_per_ok': round(s['check_chars'] / s['ok']) if s['ok'] else None,
            'context_chars_per_ok': round((s['result_chars'] + s['check_chars']) / s['ok']) if s['ok'] else None,
            'failure_chars_median': med(s['failure_chars']),
            # 同じ呼び出しでつないだコマンド（node --check、git diff など）の出力も含む。
            # python は置換と確かめを 1 回の呼び出しにまとめることが多いので、合計で比べる
            'context_chars_median': med(s['context_lens']),
        }
    return out



def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('calls')
    ap.add_argument('--since', default='')
    ap.add_argument('--until', default='9999')
    ap.add_argument('--exclude-proj', action='append', default=[])
    ap.add_argument('--only-proj', action='append', default=[])
    a = ap.parse_args()

    sessions = collections.defaultdict(list)
    for line in open(a.calls):
        c = json.loads(line)
        ts = c.get('ts') or ''
        if not (a.since <= ts < a.until):
            continue
        if any(x in c['proj'] for x in a.exclude_proj):
            continue
        if a.only_proj and not any(x in c['proj'] for x in a.only_proj):
            continue
        sessions[(c['proj'], c['file'])].append(c)

    n = collections.Counter()
    rework_stats = collections.defaultdict(lambda: collections.defaultdict(int) | {
        'recovery_calls': [], 'recovery_secs': [], 'failure_chars': [],
        'context_lens': []})
    for calls in sessions.values():
        calls.sort(key=lambda c: c['idx'])
        rework(calls, rework_stats)
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
                if RPATCH.search(cmd) and not (RPATCH_HEADER.search(cmd) or JSON_FILE.search(cmd)):
                    # --help や -V など、編集しない呼び出し
                    n['rpatch_discovery'] += 1
                elif RPATCH.search(cmd):
                    n['rpatch'] += 1
                    dry = bool(RPATCH_DRY_RUN.search(cmd))
                    if dry:
                        n['rpatch_dry_run'] += 1
                    if 'nothing written' in res:
                        n['rpatch_failed'] += 1
                        if not dry:
                            n['rpatch_failed_real'] += 1
            if tool == 'mcp__rpatch__rpatch':
                n['rpatch'] += 1
                n['rpatch_mcp'] += 1
                dry = bool(inp.get('dry_run'))
                if dry:
                    n['rpatch_dry_run'] += 1
                if c.get('err'):
                    n['rpatch_failed'] += 1
                    if not dry:
                        n['rpatch_failed_real'] += 1
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
        'since': a.since, 'until': a.until, 'exclude_proj': a.exclude_proj, 'only_proj': a.only_proj,
        'sessions': len(sessions),
        **{k: n[k] for k in [
            'tool_calls', 'python_replace', 'python_replace_unguarded', 'python_replace_failed',
            'edit', 'edit_same_file_consecutive', 'edit_failed',
            'rpatch', 'rpatch_mcp', 'rpatch_dry_run', 'rpatch_failed', 'rpatch_failed_real',
            'rpatch_discovery',
        ]},
        'replace_failed_total': n['python_replace_failed'] + n['edit_failed'] + n['rpatch_failed'],
        'share_python_replace': round(n['python_replace'] / edits, 3) if edits else None,
        'python_replace_per_1k_calls': round(1000 * n['python_replace'] / n['tool_calls'], 1) if n['tool_calls'] else None,
        'rework': summarize(rework_stats),
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
