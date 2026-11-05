"""Builds a patch under patches/ and packs it as a .procraftplugin file, which the app imports:

    python tools/pack_patch.py touch-offset [--out DIR]

The crate builds to WebAssembly (rustup target add wasm32-unknown-unknown). The file is a zip of
the patch's craft-extension.json, with its module listed by size and SHA-256 as the app asks, and
the module as patch.wasm. It lands in target/patches unless --out says where.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = 'wasm32-unknown-unknown'
MODULE = 'patch.wasm'
MANIFEST = 'craft-extension.json'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('patch', help='its folder under patches/')
    parser.add_argument('--out', help='the folder the file goes in')
    args = parser.parse_args()

    crate = os.path.join(ROOT, 'patches', args.patch)
    target = os.path.join(ROOT, 'target', 'patches')
    subprocess.run(
        ['cargo', 'build', '--release', '--target', TARGET, '--target-dir', target],
        cwd=crate,
        check=True,
    )
    name = args.patch.replace('-', '_')
    with open(os.path.join(target, TARGET, 'release', f'{name}.wasm'), 'rb') as f:
        module = f.read()

    with open(os.path.join(crate, MANIFEST), encoding='utf-8') as f:
        manifest = json.load(f)
    manifest['files'] = [{'path': MODULE, 'sha256': hashlib.sha256(module).hexdigest(), 'size': len(module)}]

    out = os.path.abspath(args.out) if args.out else target
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, f'{args.patch}.procraftplugin')
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as file:
        file.writestr(MANIFEST, json.dumps(manifest, indent=2))
        file.writestr(MODULE, module)
    print(f'{path}: {len(module)} bytes of module')


if __name__ == '__main__':
    sys.exit(main())
