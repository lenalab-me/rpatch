"""Print non-secret, per-run overrides for an isolated Codex experiment.

Does not change the user's config or credentials.
"""
import json
import os
from pathlib import Path
import tomllib

root = Path(os.environ.get('CODEX_HOME', Path.home() / '.codex'))
config = root / 'config.toml'
data = tomllib.loads(config.read_text()) if config.exists() else {}
for name in data.get('mcp_servers', {}):
    if '.' in name:
        raise ValueError('Codex CLI overrides cannot isolate a server name containing a dot')
    print('mcp_servers.' + name + '={enabled=false,command="rpatch-disabled"}')
for name in data.get('plugins', {}):
    if '.' in name:
        raise ValueError('Codex CLI overrides cannot isolate a plugin name containing a dot')
    print(f'plugins.{name}.enabled=false')
skills = set()
for directory in (root / 'skills', root / 'skills' / '.system', Path.home() / '.agents' / 'skills'):
    if directory.exists():
        skills.update(path.parent.resolve() for path in directory.glob('*/SKILL.md'))
items = ','.join('{path=' + json.dumps(str(path)) + ',enabled=false}' for path in sorted(skills))
print('skills.config=[' + items + ']')
