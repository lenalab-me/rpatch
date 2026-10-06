"""1 つのセッション（サブエージェントを含む）で、rpatch がどう使われたかを見る。

使い方:
  python3 tools/log-analysis/session.py              # 直近のセッション（prod-rpatch と pi-agent を除く）
  python3 tools/log-analysis/session.py network      # プロジェクト名かセッション ID の一部で選ぶ
  python3 tools/log-analysis/session.py --list       # 直近のセッションと rpatch の呼び出し数
  python3 tools/log-analysis/session.py network -v   # 失敗した呼び出しの入力と出力も出す

数え方は metrics.py と同じ正規表現を使う。全セッションの集計は extract.py と metrics.py で行う。
"""
import argparse, glob, json, os, re, unicodedata
from datetime import datetime

from metrics import HOOK_BLOCKED, PY, REPLACE, RPATCH_FAILED, SD, WRITE, rpatch_call

ROOT = os.path.expanduser('~/.claude/projects')
DEFAULT_EXCLUDE = ('prod-rpatch', 'pi-agent')
HEREDOC_WRITE = re.compile(r'(^|[;&|(\n])\s*(cat|tee)\s+(-a\s+)?>{0,2}\s*\S+\s*<<|cat\s*>>?\s*\S+\s*<<')
NODE = re.compile(r'(^|[;&|(\n])\s*node\s')
# rpatch 自身のエラー行（連ねた cargo などの "error:" と区別するため、rpatch の書式に限る）
RPATCH_ERROR = re.compile(r'^(rpatch: (?!nothing written).*|error: edits\[.*)$', re.M)
# 阻止: フック（edit_guard.py。それより前は no-python-replace.py）が止めた呼び出し。何も書いていない
COLUMNS = ['rpatch', '失敗', 'help', 'sd', 'cat>', 'Edit', 'Write', 'py置換', '阻止', 'py他', 'node']
# 止めた規則。edit_guard.py は理由の末尾に付ける。付いていないのは python の置換を止めた初代のフック
HOOK_RULE = re.compile(r'\[edit-guard: ([\w-]+)\]')
# 失敗の種類。上から順に、出力に最初に当たったもの
FAILURE_KINDS = [
    (r'mixes LF and CRLF', '改行の混在'),
    (r'is only the start of line', '行の一部: 後半が欠けている'),
    (r'is only the end of line', '行の一部: 先頭（字下げ）が欠けている'),
    (r'is in the middle of line', '行の一部: 行の途中'),
    (r'found without the blank line', '最後の空行がない'),
    (r'found with different whitespace', '空白・改行の違い'),
    (r'matches the file up to a point', '途中まで一致して食い違う'),
    (r'closest line to the first line', '似た行しかない'),
    (r'not found after "from"', 'TO が見つからない'),
    (r'" not found', '見つからない（その他）'),
    (r'would appear twice', 'NEW の末尾が TO と重複'),
    (r'expected \d+ match|overlap|more than once on the same line', '一致の数が合わない'),
    (r'file not found', 'ファイルがない'),
    (r'^rpatch: line \d+:', '入力の書式'),
    (r'unknown argument|file argument|at most one file', '引数'),
    (r'read-only|write failed|cannot read', '読み書き'),
]
BLOCK_KIND = re.compile(r'^<{7,} (OLD part|OLD|FROM|BEFORE|AFTER)', re.M)


def width(s):
    return sum(2 if unicodedata.east_asian_width(ch) in 'WF' else 1 for ch in s)


def pad(s, n, right=False):
    fill = ' ' * max(0, n - width(s))
    return fill + s if right else s + fill


def local(ts):
    return datetime.fromisoformat(ts.replace('Z', '+00:00')).astimezone()


def sessions():
    """トップレベルのセッションファイルを、更新が新しい順に。"""
    files = glob.glob(ROOT + '/*/*.jsonl')
    return sorted(files, key=os.path.getmtime, reverse=True)


def transcripts(path):
    """本体とサブエージェントのファイル、それぞれの表示名。"""
    out = [(path, '本体')]
    sub = os.path.join(path[:-len('.jsonl')], 'subagents')
    for f in sorted(glob.glob(sub + '/*.jsonl'), key=os.path.getmtime):
        name = os.path.basename(f)[:-len('.jsonl')]
        try:
            with open(f[:-len('.jsonl')] + '.meta.json') as fh:
                meta = json.load(fh)
            name = meta.get('description') or name
        except (OSError, ValueError):
            pass
        out.append((f, name))
    return out


def load(path):
    """ツール呼び出しを順に。結果（出力・is_error）を対応づける。"""
    uses, results, first_prompt = [], {}, None
    with open(path, errors='replace') as fh:
        lines = fh.readlines()
    for line in lines:
        try:
            d = json.loads(line)
        except ValueError:
            continue
        m = d.get('message')
        if not isinstance(m, dict):
            continue
        c = m.get('content')
        if first_prompt is None and d.get('type') == 'user' and isinstance(c, str):
            first_prompt = c
        if not isinstance(c, list):
            continue
        for b in c:
            if not isinstance(b, dict):
                continue
            if b.get('type') == 'tool_use':
                if 'id' not in b:
                    continue
                uses.append({'id': b['id'], 'tool': b.get('name'), 'input': b.get('input') or {},
                             'ts': d.get('timestamp')})
            elif b.get('type') == 'tool_result':
                rc = b.get('content')
                if isinstance(rc, list):
                    rc = '\n'.join(x.get('text', '') for x in rc if isinstance(x, dict))
                results[b.get('tool_use_id')] = (bool(b.get('is_error')), rc if isinstance(rc, str) else str(rc))
    for u in uses:
        u['err'], u['res'] = results.get(u['id'], (False, ''))
    return uses, first_prompt


def classify(u):
    """編集の仕方の列名と、rpatch なら編集したファイルの集合。"""
    tool, cmd = u['tool'], u['input'].get('command') or ''
    if tool in ('Edit', 'MultiEdit'):
        return 'Edit', None
    if tool == 'Write':
        return 'Write', None
    if tool != 'Bash':
        return None, None
    if HOOK_BLOCKED.search(u['res']):
        return '阻止', None
    call = rpatch_call(cmd)
    if call:
        return ('rpatch', {os.path.basename(f) for f in call[2]}) if call[1] else ('help', None)
    if PY.search(cmd):
        return ('py置換' if REPLACE.search(cmd) and WRITE.search(cmd) else 'py他'), None
    if SD.search(cmd):
        return 'sd', None
    if HEREDOC_WRITE.search(cmd):
        return 'cat>', None
    if NODE.search(cmd):
        return 'node', None
    return None, None


def failed(u):
    return bool(RPATCH_FAILED.search(u['res']) or RPATCH_ERROR.search(u['res']))


def pick(query):
    for f in sessions():
        rel = os.path.relpath(f, ROOT)
        if query:
            if query in rel or os.path.abspath(query) == f:
                return f
        elif not any(x in rel for x in DEFAULT_EXCLUDE):
            return f
    raise SystemExit(f'セッションが見つかりません: {query or "(直近)"}')


_cache = {}


def analyze(path):
    """1 つのセッションの集計。ファイルの更新時刻が変わらなければ前回の結果を返す。"""
    parts = transcripts(path)
    key = (path, tuple(os.path.getmtime(t) for t, _ in parts))
    if _cache.get(path, (None,))[0] == key:
        return _cache[path][1]
    rows, failures, blocked, span = [], [], [], []
    first_prompt = None
    for t, name in parts:
        uses, prompt = load(t)
        first_prompt = first_prompt or prompt
        counts = dict.fromkeys(COLUMNS, 0)
        for i, u in enumerate(uses):
            span.append(u['ts'])
            kind, files = classify(u)
            if not kind:
                continue
            counts[kind] += 1
            if kind == '阻止':
                m = HOOK_RULE.search(u['res'])
                blocked.append({'ts': local(u['ts']), 'name': name, 'rule': m.group(1) if m else 'python-replace',
                                'via': next_edit(uses[i + 1:]), 'input': u['input'].get('command') or ''})
            if kind != 'rpatch':
                continue
            cmd = u['input'].get('command') or ''
            if failed(u) and not rpatch_call(cmd)[0]:
                counts['失敗'] += 1
                # 同じファイルへの次の rpatch が成功するまで
                fix = next((v for v in uses[i + 1:] if classify(v)[0] == 'rpatch' and not failed(v)
                            and (classify(v)[1] & files)), None)
                failures.append(failure(u, name, fix, recovered_by(uses[i + 1:], files)))
        rows.append((name, counts))
    failures.sort(key=lambda f: f['ts'])
    span = [s for s in span if s]
    result = {
        'path': path,
        'rel': os.path.relpath(path, ROOT),
        'start': local(min(span)) if span else None,
        'end': local(max(span)) if span else None,
        'prompt': ' '.join((first_prompt or '').split()),
        'rows': rows,
        'total': {c: sum(r[c] for _, r in rows) for c in COLUMNS},
        'failures': failures,
        'blocked': blocked,
        'masked': sum(1 for f in failures if f['masked']),
    }
    _cache[path] = (key, result)
    return result


def failure_kind(res):
    return next((k for p, k in FAILURE_KINDS if re.search(p, res, re.M)), 'その他')


def recovered_by(after, files):
    """失敗のあと、同じファイルを何で書き換えたか。rpatch ならブロックの種類（OLD+OLD part など）。
    sd・python・cat> はファイルを特定できないので、次に出てきたものを数える。"""
    for v in after:
        kind, fv = classify(v)
        if kind == 'rpatch' and fv and files and (fv & files):
            if failed(v):
                continue
            used = sorted(set(BLOCK_KIND.findall(v['input'].get('command') or '')))
            return 'rpatch ' + '+'.join(used) if used else 'rpatch'
        if kind in ('Edit', 'Write') and os.path.basename(v['input'].get('file_path') or '') in (files or ()):
            return kind
        if kind in ('sd', 'py置換', 'cat>'):
            return kind
    return None


def next_edit(after):
    """フックに止められたあと、次に書き込んだ方法。ほかに止められた呼び出しは飛ばす。ファイルは
    特定しない（止めた理由の書式が規則ごとに違うため）。"""
    for v in after:
        kind, _ = classify(v)
        if kind == 'rpatch' and not failed(v):
            used = sorted(set(BLOCK_KIND.findall(v['input'].get('command') or '')))
            return 'rpatch ' + '+'.join(used) if used else 'rpatch'
        if kind in ('Edit', 'Write', 'sd', 'py置換', 'cat>'):
            return kind
    return None


def failure(u, name, fix, via=None):
    errors = RPATCH_ERROR.findall(u['res']) or ['(エラー行なし)']
    # エラー行に続く字下げの行（hint や、どこで食い違ったか）を 2 行まで
    lines = u['res'].splitlines()
    at = lines.index(errors[0]) + 1 if errors[0] in lines else len(lines)
    details = [l.strip() for l in lines[at:at + 2] if l.startswith('  ')]
    return {
        'ts': local(u['ts']),
        'name': name,
        'masked': not u['err'],
        'error': errors[0],
        'details': details,
        'fix_secs': (local(fix['ts']) - local(u['ts'])).total_seconds() if fix else None,
        'kind': failure_kind(u['res']),
        'via': via,
        'input': u['input'].get('command') or '',
        'output': u['res'],
    }


def recent(hours=None, limit=None, exclude=DEFAULT_EXCLUDE):
    """更新が新しい順のセッションの集計。"""
    now = datetime.now().timestamp()
    out = []
    for f in sessions():
        if hours is not None and now - os.path.getmtime(f) > hours * 3600:
            break
        if any(x in f for x in exclude):
            continue
        out.append(analyze(f))
        if limit and len(out) >= limit:
            break
    return out


def list_sessions(n):
    for s in recent(limit=n, exclude=()):
        when = s['end'].strftime('%m-%d %H:%M') if s['end'] else '--'
        print(f'{when}  rpatch {s["total"]["rpatch"]:4}  失敗 {s["total"]["失敗"]:3}  {s["rel"]}')


def report(path, verbose):
    s = analyze(path)
    print(f'セッション: {s["rel"]}')
    if s['start']:
        print(f'期間: {s["start"]:%Y-%m-%d %H:%M}〜{s["end"]:%H:%M}')
    if s['prompt']:
        print('最初の依頼: ' + s['prompt'][:100])
    print()

    print('編集の仕方（呼び出しの数。rpatch は編集した回数、help はそれ以外の rpatch）')
    names = [n[:24] for n, _ in s['rows']] + ['合計']
    w = max(width(n) for n in names)
    cell = lambda v, c: pad(str(v), max(width(c), 4) + 2, right=True)
    print(pad('', w) + ''.join(cell(c, c) for c in COLUMNS))
    for (_, counts), label in zip(s['rows'], names):
        if any(counts.values()):
            print(pad(label, w) + ''.join(cell(counts[c], c) for c in COLUMNS))
    print(pad('合計', w) + ''.join(cell(s['total'][c], c) for c in COLUMNS))
    print()

    print(f'rpatch の失敗 {len(s["failures"])} 件（後続のコマンドで終了コードが 0 になったもの {s["masked"]} 件）')
    for f in s['failures']:
        status = '終了コード 0' if f['masked'] else '失敗'
        print(f'  {f["ts"]:%H:%M:%S}  {f["name"][:24]}  [{status}]')
        for line in [f['error']] + f['details']:
            print(f'    {line[:140]}')
        if f['fix_secs'] is not None:
            print(f'    → {f["fix_secs"]:.0f} 秒後の rpatch で成功')
        else:
            print('    → 同じファイルへの rpatch の成功なし')
        if verbose:
            print('    --- 入力')
            print('\n'.join('    | ' + line for line in f['input'].splitlines()))
            print('    --- 出力')
            print('\n'.join('    | ' + line for line in f['output'].splitlines()[:30]))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('query', nargs='?', help='プロジェクト名・セッション ID の一部、または .jsonl のパス')
    ap.add_argument('--list', nargs='?', const=10, type=int, metavar='N', help='直近 N 件のセッションを並べる')
    ap.add_argument('-v', '--verbose', action='store_true', help='失敗した呼び出しの入力と出力も出す')
    a = ap.parse_args()
    if a.list:
        list_sessions(a.list)
    else:
        report(pick(a.query), a.verbose)


if __name__ == '__main__':
    main()
