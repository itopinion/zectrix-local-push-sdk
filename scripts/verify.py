#!/usr/bin/env python3
"""Verify this source-only SDK snapshot without accessing a device or network."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
manifest = json.loads((ROOT / 'SDK_MANIFEST.json').read_text(encoding='utf-8'))
for name, record in manifest['files'].items():
    relative = Path(name)
    if relative.is_absolute() or '..' in relative.parts:
        raise SystemExit('Unsafe manifest path: ' + name)
    data = (ROOT / relative).read_bytes()
    if len(data) != record['size'] or hashlib.sha256(data).hexdigest() != record['sha256']:
        raise SystemExit('Checksum mismatch: ' + name)
for line in (ROOT / 'SHA256SUMS').read_text().splitlines():
    digest, name = line.split('  ', 1)
    relative = Path(name)
    if relative.is_absolute() or '..' in relative.parts:
        raise SystemExit('Unsafe checksum path: ' + name)
    if hashlib.sha256((ROOT / relative).read_bytes()).hexdigest() != digest:
        raise SystemExit('Checksum mismatch: ' + name)
for p in ROOT.rglob('*'):
    if any(part in ('.git', 'target', 'dist', '__pycache__') for part in p.relative_to(ROOT).parts):
        continue
    if p.is_file() and (p.suffix in ('.bin', '.exe', '.dmg') or p.name == 'zectrix.py'):
        raise SystemExit('Unexpected firmware/installer: ' + str(p.relative_to(ROOT)))
print('Verified source-only SDK: %d manifest files; SHA256SUMS valid' % len(manifest['files']))
