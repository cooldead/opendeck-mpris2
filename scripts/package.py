#!/usr/bin/env python3
"""Build a native OpenDeck plugin archive without modifying installed plugins."""
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = subprocess.check_output(['rustc', '-vV'], text=True).split('host: ')[1].splitlines()[0]
if TARGET not in ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'):
    raise SystemExit(f'Unsupported target: {TARGET}')
subprocess.run(['cargo', 'build', '--release', '--locked'], cwd=ROOT, check=True)
manifest = json.loads((ROOT / 'assets/manifest.json').read_text())
name = 'com.cooldead.mpris2.sdPlugin'
folder = ROOT / 'dist' / TARGET / name
folder.mkdir(parents=True, exist_ok=True)
shutil.copytree(ROOT / 'assets', folder, dirs_exist_ok=True)
# Each archive contains only its native binary, so advertise that architecture only.
manifest['CodePaths'] = {TARGET: f'opendeck-mpris2-{TARGET}'}
manifest['CodePathLin'] = f'opendeck-mpris2-{TARGET}'
(folder / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
shutil.copy2(ROOT / 'target/release/opendeck-mpris2', folder / f'opendeck-mpris2-{TARGET}')
for filename in ('LICENSE', 'NOTICE.md'):
    shutil.copy2(ROOT / filename, folder / filename)
archive = ROOT / 'dist' / f'opendeck-mpris2-{manifest["Version"]}-{TARGET}.streamDeckPlugin'
with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
    for path in sorted(folder.rglob('*')):
        if path.is_file():
            output.write(path, path.relative_to(folder.parent))
print(archive)
