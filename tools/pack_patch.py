"""Builds patches under patches/ and filters under plugins/ and packs each as a .procraftplugin
file, which the app imports:

    python tools/pack_patch.py [touch-offset | extract-lines] [--out DIR]

With no name it packs them all. Each patch's crate builds to WebAssembly (rustup target add
wasm32-unknown-unknown); a filter is its text. A file is a zip of the craft-extension.json, with
what goes with it listed by size and SHA-256 as the app asks, and that (patch.wasm or filter.wgsl).
They land in target/patches unless --out says where.
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
FILTERS = os.path.join(ROOT, 'plugins')
TARGET = 'wasm32-unknown-unknown'
MODULE = 'patch.wasm'
FILTER = 'filter.wgsl'
MANIFEST = 'craft-extension.json'
TARGET_DIR = os.path.join(ROOT, 'target', 'patches')


def write(folder, name, crate, file_name, data, out):
    """The file `name`, of the manifest in `folder` and `data` as `file_name`."""
    with open(os.path.join(folder, MANIFEST), encoding='utf-8') as f:
        manifest = json.load(f)
    manifest['files'] = [{'path': file_name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}]
    place = os.path.abspath(out) if out else TARGET_DIR
    os.makedirs(place, exist_ok=True)
    path = os.path.join(place, f'{name}.procraftplugin')
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as file:
        file.writestr(MANIFEST, json.dumps(manifest, indent=2))
        file.writestr(file_name, data)
    print(f'{path}: {len(data)} bytes of {crate}')


def pack_filter(name, out):
    folder = os.path.join(FILTERS, name)
    with open(os.path.join(folder, FILTER), 'rb') as f:
        write(folder, name, 'code', FILTER, f.read(), out)


def pack(patch, out):
    crate = os.path.join(PATCHES, patch)
    target = TARGET_DIR
    subprocess.run(
        ['cargo', 'build', '--release', '--target', TARGET, '--target-dir', target],
        cwd=crate,
        check=True,
    )
    name = patch.replace('-', '_')
    with open(os.path.join(target, TARGET, 'release', f'{name}.wasm'), 'rb') as f:
        module = f.read()

    write(crate, patch, 'module', MODULE, module, out)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('name', nargs='?', help='its folder under patches/ or plugins/; all of them if left out')
    parser.add_argument('--out', help='the folder the files go in')
    args = parser.parse_args()
    patches, filters = sorted(os.listdir(PATCHES)), sorted(os.listdir(FILTERS))
    for patch in patches if not args.name else [args.name] if args.name in patches else []:
        pack(patch, args.out)
    for name in filters if not args.name else [args.name] if args.name in filters else []:
        pack_filter(name, args.out)


if __name__ == '__main__':
    sys.exit(main())
