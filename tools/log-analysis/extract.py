import json, glob, os, sys, collections
root = os.path.expanduser('~/.claude/projects')
out = open(sys.argv[1] if len(sys.argv) > 1 else 'calls.jsonl', 'w')
n=0
for f in glob.glob(root+'/**/*.jsonl', recursive=True):
    uses = {}
    results = {}
    order = []
    proj = f.split('/projects/')[1].split('/')[0]
    for line in open(f, errors='replace'):
        try: d = json.loads(line)
        except: continue
        m = d.get('message')
        if not isinstance(m, dict): continue
        c = m.get('content')
        if not isinstance(c, list): continue
        for b in c:
            if not isinstance(b, dict): continue
            if b.get('type')=='tool_use':
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
        out.write(json.dumps({'proj':proj,'file':os.path.basename(f),'idx':i,'ts':ts,'side':side,'tool':name,'input':inp,'err':err,'res_len':len(res) if res else 0,'res':(res or '')[:1500]}, ensure_ascii=False)+'\n')
        n+=1
print(n)
