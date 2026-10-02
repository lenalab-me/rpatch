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

from metrics import JSON_FILE, PY, REPLACE, RPATCH, RPATCH_DRY_RUN, RPATCH_HEADER, SD, WRITE

ROOT = os.path.expanduser('~/.claude/projects')
DEFAULT_EXCLUDE = ('prod-rpatch', 'pi-agent')
HEREDOC_WRITE = re.compile(r'(^|[;&|(\n])\s*(cat|tee)\s+(-a\s+)?>{0,2}\s*\S+\s*<<|cat\s*>>?\s*\S+\s*<<')
NODE = re.compile(r'(^|[;&|(\n])\s*node\s')
# rpatch 自身のエラー行（ほかのコマンドの "error:" と区別するため、rpatch の書式に限る）
RPATCH_ERROR = re.compile(r'^(rpatch: (?!nothing written).*|error: edits\[\d+\].*)$', re.M)
COLUMNS = ['rpatch', '失敗', 'help', 'sd', 'cat>', 'Edit', 'Write', 'py置換', 'py他', 'node']


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
            meta = json.load(open(f[:-len('.jsonl')] + '.meta.json'))
            name = meta.get('description') or name
        except (OSError, ValueError):
            pass
        out.append((f, name))
    return out


def load(path):
    """ツール呼び出しを順に。結果（出力・is_error）を対応づける。"""
    uses, results, first_prompt = [], {}, None
    for line in open(path, errors='replace'):
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
    if RPATCH.search(cmd):
        files = RPATCH_HEADER.findall(cmd) + JSON_FILE.findall(cmd)
        return ('rpatch', {os.path.basename(f) for f in files}) if files else ('help', None)
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
    return 'nothing written' in u['res'] or bool(RPATCH_ERROR.search(u['res']))


def pick(query):
    for f in sessions():
        rel = os.path.relpath(f, ROOT)
        if query:
            if query in rel or os.path.abspath(query) == f:
                return f
        elif not any(x in rel for x in DEFAULT_EXCLUDE):
            return f
    raise SystemExit(f'セッションが見つかりません: {query or "(直近)"}')


def list_sessions(n):
    for f in sessions()[:n]:
        count = sum(sum(1 for u in load(t)[0] if classify(u)[0] == 'rpatch') for t, _ in transcripts(f))
        when = datetime.fromtimestamp(os.path.getmtime(f)).strftime('%m-%d %H:%M')
        print(f'{when}  rpatch {count:4}  {os.path.relpath(f, ROOT)}')


def report(path, verbose):
    rows, failures, span = [], [], []
    first_prompt = None
    for t, name in transcripts(path):
        uses, prompt = load(t)
        first_prompt = first_prompt or prompt
        counts = dict.fromkeys(COLUMNS, 0)
        last_rpatch = None
        for i, u in enumerate(uses):
            span.append(u['ts'])
            kind, files = classify(u)
            if not kind:
                continue
            counts[kind] += 1
            if kind != 'rpatch':
                continue
            cmd = u['input'].get('command') or ''
            if failed(u) and not RPATCH_DRY_RUN.search(cmd):
                counts['失敗'] += 1
                # 同じファイルへの次の rpatch が成功するまで
                fix = next((v for v in uses[i + 1:] if classify(v)[0] == 'rpatch' and not failed(v)
                            and (classify(v)[1] & files)), None)
                failures.append((u, name, fix))
        rows.append((name, counts))

    span = [s for s in span if s]
    start, end = (local(min(span)), local(max(span))) if span else (None, None)
    print(f'セッション: {os.path.relpath(path, ROOT)}')
    if start:
        print(f'期間: {start:%Y-%m-%d %H:%M}〜{end:%H:%M}')
    if first_prompt:
        print('最初の依頼: ' + ' '.join(first_prompt.split())[:100])
    print()

    print('編集の仕方（呼び出しの数。rpatch は編集した回数、help はそれ以外の rpatch）')
    names = [n[:24] for n, _ in rows] + ['合計']
    w = max(width(n) for n in names)
    print(pad('', w) + ''.join(pad(c, max(width(c), 4) + 2, right=True) for c in COLUMNS))
    total = dict.fromkeys(COLUMNS, 0)
    for (name, counts), label in zip(rows, names):
        for c in COLUMNS:
            total[c] += counts[c]
        if any(counts.values()):
            print(pad(label, w) + ''.join(pad(str(counts[c]), max(width(c), 4) + 2, right=True) for c in COLUMNS))
    print(pad('合計', w) + ''.join(pad(str(total[c]), max(width(c), 4) + 2, right=True) for c in COLUMNS))
    print()

    failures.sort(key=lambda f: f[0]['ts'] or '')
    masked = sum(1 for u, _, _ in failures if not u['err'])
    print(f'rpatch の失敗 {len(failures)} 件（後続のコマンドで終了コードが 0 になったもの {masked} 件）')
    for u, name, fix in failures:
        errors = RPATCH_ERROR.findall(u['res']) or ['(エラー行なし)']
        status = '終了コード 0' if not u['err'] else '失敗'
        print(f'  {local(u["ts"]):%H:%M:%S}  {name[:24]}  [{status}]')
        print(f'    {errors[0][:140]}')
        # エラー行に続く字下げの行（hint や、どこで食い違ったか）を 2 行まで
        lines = u['res'].splitlines()
        at = lines.index(errors[0]) + 1 if errors[0] in lines else len(lines)
        for line in [l for l in lines[at:at + 2] if l.startswith('  ')]:
            print(f'    {line.strip()[:140]}')
        if fix:
            secs = (local(fix['ts']) - local(u['ts'])).total_seconds()
            print(f'    → {secs:.0f} 秒後の rpatch で成功')
        else:
            print('    → 同じファイルへの rpatch の成功なし')
        if verbose:
            print('    --- 入力')
            print('\n'.join('    | ' + line for line in (u['input'].get('command') or '').splitlines()))
            print('    --- 出力')
            print('\n'.join('    | ' + line for line in u['res'].splitlines()[:30]))


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
