"""Builds the plugins under plugins/ and packs each as a .procraftplugin file, which the app imports:

    python tools/pack_plugin.py [touch-offset | extract-lines] [--out DIR]

With no name it packs them all. What goes in is what the manifest's `holds` says: for `hooks`, the
folder's crate built to WebAssembly (rustup target add wasm32-unknown-unknown); for `filter`, its
text. A file is a zip of the craft-extension.json, with those listed by size and SHA-256 as the app
asks, and them (plugin.wasm, filter.wgsl or both). They land in target/plugins unless --out says
where.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PLUGINS = os.path.join(ROOT, 'plugins')
TARGET = 'wasm32-unknown-unknown'
MODULE = 'plugin.wasm'
FILTER = 'filter.wgsl'
MANIFEST = 'craft-extension.json'
TARGET_DIR = os.path.join(ROOT, 'target', 'plugins')


def built(name, folder):
    """The module of the crate in `folder`."""
    subprocess.run(
        ['cargo', 'build', '--release', '--target', TARGET, '--target-dir', TARGET_DIR],
        cwd=folder,
        check=True,
    )
    with open(os.path.join(TARGET_DIR, TARGET, 'release', f"{name.replace('-', '_')}.wasm"), 'rb') as f:
        return f.read()


def pack(name, out):
    """The file `name`, of what its folder's manifest says it holds."""
    folder = os.path.join(PLUGINS, name)
    with open(os.path.join(folder, MANIFEST), encoding='utf-8') as f:
        manifest = json.load(f)
    files = {}
    if 'hooks' in manifest['holds']:
        files[MODULE] = built(name, folder)
    if 'filter' in manifest['holds']:
        with open(os.path.join(folder, FILTER), 'rb') as f:
            files[FILTER] = f.read()
    manifest['files'] = [
        {'path': path, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)} for path, data in files.items()
    ]
    place = os.path.abspath(out) if out else TARGET_DIR
    os.makedirs(place, exist_ok=True)
    path = os.path.join(place, f'{name}.procraftplugin')
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as file:
        file.writestr(MANIFEST, json.dumps(manifest, indent=2))
        for inside, data in files.items():
            file.writestr(inside, data)
    print(f"{path}: {', '.join(f'{len(data)} bytes of {inside}' for inside, data in files.items())}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('name', nargs='?', help='its folder under plugins/; all of them if left out')
    parser.add_argument('--out', help='the folder the files go in')
    args = parser.parse_args()
    names = sorted(os.listdir(PLUGINS))
    if args.name and args.name not in names:
        parser.error(f'no plugins/{args.name}')
    for name in [args.name] if args.name else names:
        pack(name, args.out)


if __name__ == '__main__':
    sys.exit(main())
