import json, glob, os, sys, collections
root = os.path.expanduser('~/.claude/projects')
# rpatch の `rpatch: nothing written` は長い診断の最後に出るので、先頭と末尾を残す
HEAD, TAIL = 1200, 300
def clip(res):
    if len(res) <= HEAD + TAIL: return res
    return res[:HEAD] + '\n...(中略)...\n' + res[-TAIL:]
USAGE = 'usage: python3 extract.py [OUT]   (OUT: the JSONL to write, default calls.jsonl)'
if len(sys.argv) > 1 and sys.argv[1] in ('-h', '--help'):
    print(USAGE)
    sys.exit(0)
# `extract.py --help` once wrote every session's calls to a file named `--help`, which got committed
if len(sys.argv) > 1 and sys.argv[1].startswith('-'):
    sys.exit(f'extract.py: OUT must be a file name, not an option: {sys.argv[1]}\n{USAGE}')
n=0
with open(sys.argv[1] if len(sys.argv) > 1 else 'calls.jsonl', 'w') as out:
    for f in glob.glob(root+'/**/*.jsonl', recursive=True):
        uses = {}
        results = {}
        order = []
        # ログのルートからの相対パスの先頭がプロジェクト名（ホームのパスに /projects/ があっても崩れない）
        proj = os.path.relpath(f, root).split(os.sep)[0]
        with open(f, errors='replace') as fh:
            for line in fh:
                try: d = json.loads(line)
                except ValueError: continue
                m = d.get('message')
                if not isinstance(m, dict): continue
                c = m.get('content')
                if not isinstance(c, list): continue
                for b in c:
                    if not isinstance(b, dict): continue
                    if b.get('type')=='tool_use':
                        if 'id' not in b: continue
                        uses[b['id']] = (b.get('name'), b.get('input'), d.get('timestamp'), d.get('isSidechain'))
                        order.append(b['id'])
                    elif b.get('type')=='tool_result':
                        rc = b.get('content')
                        if isinstance(rc, list):
                            rc = '\n'.join(x.get('text','') for x in rc if isinstance(x, dict))
                        results[b.get('tool_use_id')] = (bool(b.get('is_error')), rc if isinstance(rc,str) else str(rc))
        for i, uid in enumerate(order):
            name, inp, ts, side = uses[uid]
            err, res = results.get(uid, (None, None))
            out.write(json.dumps({'proj':proj,'file':os.path.basename(f),'idx':i,'ts':ts,'side':side,'tool':name,'input':inp,'err':err,'res_len':len(res) if res else 0,'res':clip(res or '')}, ensure_ascii=False)+'\n')
            n+=1
print(n)
