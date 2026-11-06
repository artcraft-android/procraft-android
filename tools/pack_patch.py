"""Builds patches under patches/ and packs each as a .procraftplugin file, which the app imports:

    python tools/pack_patch.py [touch-offset] [--out DIR]

With no name it packs them all. Each crate builds to WebAssembly (rustup target add
wasm32-unknown-unknown). A file is a zip of the patch's craft-extension.json, with its module
listed by size and SHA-256 as the app asks, and the module as patch.wasm. They land in
target/patches unless --out says where.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PATCHES = os.path.join(ROOT, 'patches')
TARGET = 'wasm32-unknown-unknown'
MODULE = 'patch.wasm'
MANIFEST = 'craft-extension.json'


def pack(patch, out):
    crate = os.path.join(PATCHES, patch)
    target = os.path.join(ROOT, 'target', 'patches')
    subprocess.run(
        ['cargo', 'build', '--release', '--target', TARGET, '--target-dir', target],
        cwd=crate,
        check=True,
    )
    name = patch.replace('-', '_')
    with open(os.path.join(target, TARGET, 'release', f'{name}.wasm'), 'rb') as f:
        module = f.read()

    with open(os.path.join(crate, MANIFEST), encoding='utf-8') as f:
        manifest = json.load(f)
    manifest['files'] = [{'path': MODULE, 'sha256': hashlib.sha256(module).hexdigest(), 'size': len(module)}]

    folder = os.path.abspath(out) if out else target
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, f'{patch}.procraftplugin')
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as file:
        file.writestr(MANIFEST, json.dumps(manifest, indent=2))
        file.writestr(MODULE, module)
    print(f'{path}: {len(module)} bytes of module')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('patch', nargs='?', help='its folder under patches/; all of them if left out')
    parser.add_argument('--out', help='the folder the files go in')
    args = parser.parse_args()
    for patch in [args.patch] if args.patch else sorted(os.listdir(PATCHES)):
        pack(patch, args.out)


if __name__ == '__main__':
    sys.exit(main())
