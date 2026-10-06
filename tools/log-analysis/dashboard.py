"""session.py の集計をブラウザで見る。ページを開くたびに集計し直し、10 分ごとに自動で再読み込みする。

使い方: python3 tools/log-analysis/dashboard.py [--port 8765] [--hours 24]
セッションの中身（認証情報を含むことがある）を出すので、127.0.0.1 でだけ待ち受ける。
"""
import argparse, collections, html, os
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, urlparse

from session import COLUMNS, DEFAULT_EXCLUDE, ROOT, analyze, recent

REFRESH_SECS = 600
HOME = os.path.expanduser('~').replace('/', '-') + '-'
e = html.escape

CSS = """
:root { --bg:#fbfbfa; --fg:#1d1d1b; --muted:#6b6b66; --line:#e4e3de; --card:#fff;
  --bad:#b4372c; --warn:#a2650a; --good:#2f7a4a; --accent:#3a5fa8; --code:#f3f2ee; }
@media (prefers-color-scheme: dark) { :root { --bg:#161615; --fg:#e8e7e3; --muted:#9a9993;
  --line:#2e2e2b; --card:#1e1e1c; --bad:#ec7a6e; --warn:#e0a54a; --good:#6cc28c; --accent:#8fb0f0; --code:#262624; } }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--fg);
  font:14px/1.55 -apple-system, BlinkMacSystemFont, "Hiragino Sans", sans-serif; }
main { max-width:1100px; margin:0 auto; padding:24px 16px 64px; }
header { display:flex; flex-wrap:wrap; align-items:baseline; gap:8px 16px; margin-bottom:20px; }
h1 { font-size:20px; margin:0; } h2 { font-size:15px; margin:32px 0 10px; }
.muted { color:var(--muted); } a { color:var(--accent); text-decoration:none; } a:hover { text-decoration:underline; }
.tiles { display:grid; grid-template-columns:repeat(auto-fit, minmax(150px, 1fr)); gap:10px; }
.tile { background:var(--card); border:1px solid var(--line); border-radius:8px; padding:12px 14px; }
.tile b { display:block; font-size:24px; font-variant-numeric:tabular-nums; }
.wrap { overflow-x:auto; border:1px solid var(--line); border-radius:8px; background:var(--card); }
table { border-collapse:collapse; width:100%; }
th, td { padding:6px 10px; border-bottom:1px solid var(--line); text-align:left; white-space:nowrap; }
th { font-weight:600; color:var(--muted); font-size:12px; }
td.n, th.n { text-align:right; font-variant-numeric:tabular-nums; }
td.zero { color:var(--line); } tr:last-child td { border-bottom:0; }
td.prompt { max-width:320px; overflow:hidden; text-overflow:ellipsis; color:var(--muted); }
.bad { color:var(--bad); font-weight:600; } .warn { color:var(--warn); } .good { color:var(--good); }
.fail { background:var(--card); border:1px solid var(--line); border-radius:8px; padding:10px 14px; margin-bottom:8px; }
.fail code { display:block; white-space:pre-wrap; word-break:break-all; font-size:12.5px; }
.tag { font-size:11px; padding:1px 6px; border-radius:4px; border:1px solid currentColor; margin-left:6px; }
details { margin-top:6px; } summary { cursor:pointer; color:var(--muted); font-size:12px; }
pre { background:var(--code); padding:10px; border-radius:6px; overflow-x:auto; font-size:12px; max-height:420px; }
form { display:inline; } select { font:inherit; }
"""


def page(title, body):
    now = datetime.now().strftime('%H:%M:%S')
    return f"""<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="{REFRESH_SECS}">
<title>{e(title)}</title><style>{CSS}</style></head><body><main>
<header><h1>{e(title)}</h1><span class="muted">{now} に集計・{REFRESH_SECS // 60} 分ごとに自動更新</span></header>
{body}</main></body></html>"""


def project(rel):
    return rel.split('/')[0].removeprefix(HOME).removeprefix('project-')


def num(v, cls=''):
    if not v:
        return '<td class="n zero">0</td>'
    return f'<td class="n {cls}">{v}</td>'


def counts_row(label, c):
    return (f'<tr><td>{label}</td>' + num(c['rpatch']) + num(c['失敗'], 'bad')
            + ''.join(num(c[k], 'warn' if k == 'py置換' else '') for k in COLUMNS[2:]) + '</tr>')


def failure_html(f, with_name=True, link=None):
    tag = f'<span class="tag muted">{e(f.get("kind", ""))}</span>'
    tag += '<span class="tag warn">終了コード 0</span>' if f['masked'] else ''
    fix = (f'<span class="good">→ {f["fix_secs"]:.0f} 秒後に成功</span>' if f['fix_secs'] is not None
           else '<span class="bad">→ 同じファイルへの成功なし</span>')
    where = f' · {e(f["name"])}' if with_name else ''
    if link:
        where += f' · <a href="{link}">{e(project(link.split("=", 1)[1]))}</a>'
    lines = ''.join(f'<code>{e(l)}</code>' for l in [f['error']] + f['details'])
    return f"""<div class="fail"><div><b>{f['ts']:%m-%d %H:%M:%S}</b>{where}{tag} {fix}</div>{lines}
<details><summary>入力と出力</summary><pre>{e(f['input'])}</pre><pre>{e(f['output'][:6000])}</pre></details></div>"""


def breakdown_html(fails):
    """失敗の種類ごとの件数、rpatch で立ち直った割合と時間、何で立ち直ったか。"""
    groups = collections.defaultdict(list)
    for f in fails:
        groups[f['kind']].append(f)
    rows = ''
    for kind, fs in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        secs = sorted(f['fix_secs'] for f in fs if f['fix_secs'] is not None)
        median = f'{secs[len(secs) // 2]:.0f} 秒' if secs else '–'
        vias = collections.Counter(f['via'] or '（なし）' for f in fs).most_common(4)
        via = '、'.join(f'{e(v)} {n}' for v, n in vias)
        rows += (f'<tr><td>{e(kind)}</td><td class="n">{len(fs)}</td>'
                 f'<td class="n">{len(secs)}/{len(fs)}</td><td class="n">{median}</td>'
                 f'<td class="muted">{via}</td></tr>')
    return ('<h2>失敗の内訳</h2><div class="wrap"><table><tr><th>種類</th><th class="n">件数</th>'
            '<th class="n">rpatch で成功</th><th class="n">中央値</th><th>そのあと何で書き換えたか</th></tr>'
            + rows + '</table></div>')


def blocked_html(blocked):
    """フックが止めた書き込みを規則ごとに。止めたあと何で書いたかも。"""
    groups = collections.defaultdict(list)
    for b in blocked:
        groups[b['rule']].append(b)
    rows = ''
    for rule, bs in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        vias = collections.Counter(b['via'] or '（なし）' for b in bs).most_common(4)
        rows += (f'<tr><td>{e(rule)}</td><td class="n">{len(bs)}</td>'
                 f'<td class="muted">{"、".join(f"{e(v)} {n}" for v, n in vias)}</td></tr>')
    return ('<h2>フックが止めた書き込み</h2><div class="wrap"><table><tr><th>規則</th><th class="n">件数</th>'
            '<th>そのあと何で書いたか</th></tr>' + rows + '</table></div>')


def overview(hours, show_all):
    sessions = recent(hours=hours, exclude=() if show_all else DEFAULT_EXCLUDE)
    edits = lambda s: s['total']['rpatch'] + s['total']['py置換'] + s['total']['Edit'] + s['total']['sd']
    # ファイルの更新時刻は再開などでも変わるので、最後の呼び出しの時刻で絞って並べる
    since = datetime.now().astimezone().timestamp() - hours * 3600
    active = sorted((s for s in sessions if edits(s) and s['end'] and s['end'].timestamp() >= since),
                    key=lambda s: s['end'], reverse=True)
    total = {c: sum(s['total'][c] for s in active) for c in COLUMNS}
    masked = sum(s['masked'] for s in active)
    rate = f'{total["失敗"] / total["rpatch"]:.0%}' if total['rpatch'] else '–'
    choose = ''.join(f'<option value="{h}"{" selected" if h == hours else ""}>{label}</option>'
                     for h, label in [(3, '3 時間'), (24, '24 時間'), (72, '3 日'), (168, '7 日')])
    body = f"""<p><form>期間 <select name="hours" onchange="this.form.submit()">{choose}</select>
<label><input type="checkbox" name="all" value="1"{" checked" if show_all else ""} onchange="this.form.submit()">
prod-rpatch・pi-agent も含める</label></form>
<span class="muted">編集のあったセッション {len(active)} 件（全 {len(sessions)} 件）</span></p>
<div class="tiles">
<div class="tile"><span class="muted">rpatch の編集</span><b>{total['rpatch']}</b></div>
<div class="tile"><span class="muted">rpatch の失敗</span><b class="bad">{total['失敗']}</b><span class="muted">{rate}</span></div>
<div class="tile"><span class="muted">終了コードが隠れた失敗</span><b class="warn">{masked}</b></div>
<div class="tile"><span class="muted">python での置換</span><b class="warn">{total['py置換']}</b></div>
<div class="tile"><span class="muted">フックが止めた</span><b>{total['阻止']}</b></div>
<div class="tile"><span class="muted">Edit ツール</span><b>{total['Edit']}</b></div>
<div class="tile"><span class="muted">sd</span><b>{total['sd']}</b></div>
</div>
<h2>セッション</h2><div class="wrap"><table><tr><th>最終</th><th>プロジェクト</th>
{''.join(f'<th class="n">{c}</th>' for c in COLUMNS)}<th>最初の依頼</th></tr>"""
    for s in active:
        link = f'/s?path={quote(s["rel"])}'
        when = f'{s["end"]:%m-%d %H:%M}' if s['end'] else ''
        cells = (num(s['total']['rpatch']) + num(s['total']['失敗'], 'bad')
                 + ''.join(num(s['total'][k], 'warn' if k == 'py置換' else '') for k in COLUMNS[2:]))
        body += (f'<tr><td>{when}</td><td><a href="{link}">{e(project(s["rel"]))}</a>'
                 f' <span class="muted">{e(s["rel"].split("/")[1][:8])}</span></td>{cells}'
                 f'<td class="prompt">{e(s["prompt"][:80])}</td></tr>')
    body += '</table></div>'
    fails = sorted(((f, s) for s in active for f in s['failures']), key=lambda x: x[0]['ts'], reverse=True)
    if fails:
        body += breakdown_html([f for f, _ in fails])
    blocked = [b for s in active for b in s['blocked']]
    if blocked:
        body += blocked_html(blocked)
    body += f'<h2>最近の rpatch の失敗（新しい順、{min(len(fails), 30)} / {len(fails)} 件）</h2>'
    body += ''.join(failure_html(f, link=f'/s?path={quote(s["rel"])}') for f, s in fails[:30])
    if not fails:
        body += '<p class="muted">なし</p>'
    return page('rpatch の使われ方', body)


def detail(rel):
    path = os.path.realpath(os.path.join(ROOT, rel))
    if not path.startswith(os.path.realpath(ROOT) + os.sep) or not path.endswith('.jsonl') or not os.path.isfile(path):
        return None
    s = analyze(path)
    span = f'{s["start"]:%Y-%m-%d %H:%M}〜{s["end"]:%H:%M}' if s['start'] else ''
    body = f"""<p><a href="/">← 一覧</a></p><p><b>{e(project(s['rel']))}</b> <span class="muted">{e(s['rel'])}<br>{span}</span></p>
<p class="muted">{e(s['prompt'][:400])}</p>
<h2>編集の仕方</h2><div class="wrap"><table><tr><th></th>{''.join(f'<th class="n">{c}</th>' for c in COLUMNS)}</tr>"""
    body += ''.join(counts_row(e(name), c) for name, c in s['rows'] if any(c.values()))
    body += counts_row('<b>合計</b>', s['total']) + '</table></div>'
    body += f'<h2>rpatch の失敗 {len(s["failures"])} 件（終了コードが隠れたもの {s["masked"]} 件）</h2>'
    body += ''.join(failure_html(f) for f in s['failures']) or '<p class="muted">なし</p>'
    return page(project(s['rel']), body)


def parse_hours(q, default):
    """?hours= を数として読む。数でなければ既定値、負なら 0。"""
    try:
        h = float(q.get('hours', [default])[0])
    except ValueError:
        return default
    return max(0, h) if h == h and h != float('inf') else default  # nan と inf は既定値


class Handler(BaseHTTPRequestHandler):
    hours = 24
    port = 8765

    def do_GET(self):
        # DNS rebinding で別のサイトからセッションの中身を読まれないよう、Host を確かめる
        host = (self.headers.get('Host') or '').lower()
        if host not in ('127.0.0.1', 'localhost', f'127.0.0.1:{self.port}', f'localhost:{self.port}'):
            self.send_error(403)
            return
        url = urlparse(self.path)
        q = parse_qs(url.query)
        if url.path == '/':
            hours = parse_hours(q, self.hours)
            out = overview(hours, q.get('all') == ['1'])
        elif url.path == '/s':
            out = detail(q.get('path', [''])[0])
        else:
            out = None
        if out is None:
            self.send_error(404)
            return
        data = out.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--port', type=int, default=8765)
    ap.add_argument('--hours', type=int, default=24, help='一覧に出す期間の既定値')
    a = ap.parse_args()
    Handler.hours = a.hours
    Handler.port = a.port
    server = ThreadingHTTPServer(('127.0.0.1', a.port), Handler)
    print(f'http://127.0.0.1:{a.port}/', flush=True)
    server.serve_forever()


if __name__ == '__main__':
    main()
