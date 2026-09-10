#!/usr/bin/env python3
"""Check maintained Markdown's local file links; archived audit records are excluded."""
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
paths = sorted(set(ROOT.glob('*.md')) | set((ROOT / 'docs').glob('*.md')) |
               set((ROOT / 'crates').glob('*/*.md')) | set((ROOT / 'crates').glob('*/fixtures/*.md')) |
               set((ROOT / 'scripts').glob('*.md')) | {ROOT / 'benches/README.md'})
errors = []
for path in paths:
    source = re.sub(r'```.*?```', '', path.read_text(), flags=re.S)
    for match in re.finditer(r'\[[^\]]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)', source):
        target = match.group(1).strip('<>')
        url = urlsplit(target)
        if url.scheme or url.netloc or not url.path:
            continue
        destination = path.parent / unquote(url.path)
        if not destination.exists():
            errors.append(f'{path.relative_to(ROOT)}: missing {target}')
if errors:
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)
print(f'Local file links passed in {len(paths)} maintained Markdown files (anchors and remote URLs not checked).')
