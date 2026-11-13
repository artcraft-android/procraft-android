"""Builds the plugins under plugins/ and packs each as a .procraft-plugin file, which the app imports:

    python tools/pack_plugin.py [touch-offset | extract-lines] [--out DIR]
    python tools/pack_plugin.py --bundle

With no name it packs them all. What goes in is what the manifest's `holds` says: for `hooks`, the
folder's crate built to WebAssembly (rustup target add wasm32-unknown-unknown), and for code on the
`picker` hook its picture.wgsl too; for `filter`, its text. A file is a zip of the craft-extension.json, with those listed by size and SHA-256 as the app
asks, and them (plugin.wasm, filter.wgsl or both). They land in target/plugins unless --out says
where. A file packed again from the same folder comes out the same, byte for byte.

--bundle packs them all into crates/core/bundled-plugins, the plugins that come inside the app, and
writes sources.txt there: a digest of each folder as it was packed, which the core's tests check
against plugins/. Run it whenever anything under plugins/ changes, and commit what it writes.
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
PICTURE = 'picture.wgsl'
MANIFEST = 'craft-extension.json'
TARGET_DIR = os.path.join(ROOT, 'target', 'plugins')
BUNDLED = os.path.join(ROOT, 'crates', 'core', 'bundled-plugins')
SOURCES = 'sources.txt'
EXTENSION = '.procraft-plugin'


def built(name, folder):
    """The module of the crate in `folder`, with no local path in it: a panic's location names its
    source file, so the repository and cargo's home go in under fixed names (a module packed
    anywhere comes out the same, and names nobody's folders)."""
    env = dict(os.environ)
    cargo_home = env.get('CARGO_HOME') or os.path.join(os.path.expanduser('~'), '.cargo')
    flags = [f'--remap-path-prefix={ROOT}=/procraft', f'--remap-path-prefix={cargo_home}=/cargo']
    env.pop('RUSTFLAGS', None)
    env['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags)
    subprocess.run(
        ['cargo', 'build', '--release', '--target', TARGET, '--target-dir', TARGET_DIR],
        cwd=folder,
        check=True,
        env=env,
    )
    with open(os.path.join(TARGET_DIR, TARGET, 'release', f"{name.replace('-', '_')}.wasm"), 'rb') as f:
        return f.read()


def entry(inside):
    """A zip entry for `inside`, the same on every run and system: no clock, no owner."""
    info = zipfile.ZipInfo(inside, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = 0o644 << 16
    return info


def digest(folder):
    """The SHA-256 of what `folder` holds: each file's path, size and bytes in order of path, with
    `target` and names starting with a dot left out. The core's tests work it out the same way."""
    paths = []
    for here, dirs, names in os.walk(folder):
        dirs[:] = [d for d in dirs if d != 'target' and not d.startswith('.')]
        for name in names:
            if not name.startswith('.'):
                paths.append(os.path.relpath(os.path.join(here, name), folder).replace(os.sep, '/'))
    sha = hashlib.sha256()
    for path in sorted(paths):
        with open(os.path.join(folder, path), 'rb') as f:
            data = f.read()
        sha.update(f'{path}\n{len(data)}\n'.encode())
        sha.update(data)
    return sha.hexdigest()


def source_digest(name):
    """What `name`'s packed file is made from: its folder, and the plugin kit too when it is built
    with it (a change to the kit is a change to it). The core's tests work it out the same way."""
    folder = os.path.join(PLUGINS, name)
    mine = digest(folder)
    cargo = os.path.join(folder, 'Cargo.toml')
    if os.path.isfile(cargo) and 'path = "../kit"' in open(cargo, encoding='utf-8').read():
        return hashlib.sha256(f'{mine}{digest(os.path.join(PLUGINS, "kit"))}'.encode()).hexdigest()
    return mine


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
    # a picker brings its picture
    if 'picker' in manifest.get('hooks', []):
        with open(os.path.join(folder, PICTURE), 'rb') as f:
            files[PICTURE] = f.read()
    manifest['files'] = [
        {'path': path, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)} for path, data in files.items()
    ]
    place = os.path.abspath(out) if out else TARGET_DIR
    os.makedirs(place, exist_ok=True)
    path = os.path.join(place, f'{name}{EXTENSION}')
    with zipfile.ZipFile(path, 'w') as file:
        file.writestr(entry(MANIFEST), json.dumps(manifest, indent=2))
        for inside, data in files.items():
            file.writestr(entry(inside), data)
    print(f"{path}: {', '.join(f'{len(data)} bytes of {inside}' for inside, data in files.items())}")


def bundle(names):
    """Every plugin packed into the core's folder, what was there before gone, and their digests."""
    os.makedirs(BUNDLED, exist_ok=True)
    for old in os.listdir(BUNDLED):
        if old.endswith(EXTENSION) or old == SOURCES:
            os.remove(os.path.join(BUNDLED, old))
    for name in names:
        pack(name, BUNDLED)
    with open(os.path.join(BUNDLED, SOURCES), 'w', encoding='utf-8', newline='') as f:
        f.writelines(f'{name} {source_digest(name)}\n' for name in names)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('name', nargs='?', help='its folder under plugins/; all of them if left out')
    parser.add_argument('--out', help='the folder the files go in')
    parser.add_argument('--bundle', action='store_true', help='all of them, into the core, to come inside the app')
    args = parser.parse_args()
    names = sorted(n for n in os.listdir(PLUGINS) if os.path.isfile(os.path.join(PLUGINS, n, MANIFEST)))
    if args.bundle:
        if args.name or args.out:
            parser.error('--bundle packs them all, into crates/core/bundled-plugins')
        bundle(names)
        return
    if args.name and args.name not in names:
        parser.error(f'no plugins/{args.name}')
    for name in [args.name] if args.name else names:
        pack(name, args.out)


if __name__ == '__main__':
    sys.exit(main())
