"""Writes the third-party licences the app ships with to its assets (licenses.txt), for the
settings page's Licenses screen. Run from anywhere after changing dependencies:

    python tools/licenses.py

Rust crates: those the Android library links (normal dependencies for aarch64-linux-android;
proc macros and build-only crates don't ship), each with its own licence and notice files.
Texts shared by many crates (the Apache and Mozilla licences) are written once, at the end.
Android libraries: listed by hand below (all under the Apache License 2.0)."""

import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'apps', 'procraft-android', 'android', 'app', 'src', 'main', 'assets', 'licenses.txt')
TARGET = 'aarch64-linux-android'
LICENCE_FILE = re.compile(r'^(licen[cs]e|copying|notice|unlicense)', re.IGNORECASE)

# The Android side, from app/build.gradle.kts and what those pull in.
ANDROID = [
    ('Android Jetpack (AndroidX core, activity, lifecycle, input motion prediction)', 'https://developer.android.com/jetpack'),
    ('Jetpack Compose (UI, Material 3)', 'https://developer.android.com/jetpack/compose'),
    ('Kotlin standard library and kotlinx.coroutines', 'https://kotlinlang.org'),
    ('JetBrains annotations, JSpecify, Guava ListenableFuture', 'https://github.com/google/guava'),
    ('JNA (Java Native Access), used under the Apache License 2.0 of its dual licence', 'https://github.com/java-native-access/jna'),
]


# standard texts for crates that ship none, by the name in their licence expression
STANDARD = [('Apache License 2.0', 'Apache-2.0'), ('Mozilla Public License 2.0', 'MPL-2.0')]


def standard_text(folder, title):
    """A crate's standard licence text, from another crate in the registry that ships it."""
    registry = os.path.dirname(folder)
    first = {'Apache License 2.0': 'Apache License', 'Mozilla Public License 2.0': 'Mozilla Public License Version 2.0'}[title]
    for crate in sorted(os.listdir(registry)):
        path = os.path.join(registry, crate)
        if not os.path.isdir(path):
            continue
        for f in sorted(os.listdir(path)):
            if LICENCE_FILE.match(f) and os.path.isfile(os.path.join(path, f)):
                text = open(os.path.join(path, f), encoding='utf-8', errors='replace').read().strip()
                if text.startswith(first) or text[:200].lstrip().startswith(first):
                    return text
    sys.exit(f'no {title} text in the registry')


def normal(text):
    return re.sub(r'\s+', ' ', text).strip()


def main():
    meta = json.loads(subprocess.run(
        ['cargo', 'metadata', '--format-version', '1', '--filter-platform', TARGET],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    packages = {p['id']: p for p in meta['packages']}
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    workspace = set(meta['workspace_members'])
    root = next(i for i in workspace if packages[i]['name'] == 'procraft-android')

    # the crates the library links: normal dependencies, not proc macros
    seen, stack = set(), [root]
    while stack:
        node = nodes[stack.pop()]
        for dep in node['deps']:
            if not any(k['kind'] is None for k in dep['dep_kinds']):
                continue
            target_kinds = {k for t in packages[dep['pkg']]['targets'] for k in t['kind']}
            if 'proc-macro' in target_kinds or dep['pkg'] in seen:
                continue
            seen.add(dep['pkg'])
            stack.append(dep['pkg'])
    crates = sorted((packages[i] for i in seen if i not in workspace), key=lambda p: (p['name'], p['version']))

    shared = {}  # normalised text -> (title, text)
    lines = ['ProCraft is made with the following third-party software, under the licences given.', '',
             'The interface font, Inter, is under the SIL Open Font License 1.1 (its text above).', '',
             'The app keeps Lucide icons (lucide.dev), under the ISC License (its text above).', '',
             'The 3D view environments are panoramas from Poly Haven (polyhaven.com), under CC0 (public',
             'domain); their authors are listed in assets/environments/README.md.', '',
             '== Rust crates ==', '']
    for p in crates:
        folder = os.path.dirname(p['manifest_path'])
        files = sorted(f for f in os.listdir(folder) if LICENCE_FILE.match(f) and os.path.isfile(os.path.join(folder, f)))
        if p.get('license_file') and os.path.basename(p['license_file']) not in files:
            files.append(p['license_file'])
        lines.append(f"{p['name']} {p['version']} ({p.get('license') or 'see its licence file'})")
        if p.get('repository'):
            lines.append(p['repository'])
        if not files:
            # none in the package: under a standard licence its expression allows, the text at the end
            expression = p.get('license') or ''
            for title, marker in STANDARD:
                if marker in expression:
                    text = standard_text(folder, title)
                    shared.setdefault(normal(text), (title, text))
                    lines.append(f'  (no licence file in the package) used under the {title} (its text at the end)')
                    break
            else:
                sys.exit(f"{p['name']}: no licence file and no standard text for {expression!r}")
        for f in files:
            text = open(os.path.join(folder, f), encoding='utf-8', errors='replace').read().strip()
            key = normal(text)
            # the long standard texts once, at the end
            if 'Apache License' in text[:200] and 'Version 2.0' in text[:300]:
                shared.setdefault(key, ('Apache License 2.0', text))
                lines.append(f'  {f}: the Apache License 2.0 (its text at the end)')
            elif 'Mozilla Public License' in text[:300] and '2.0' in text[:300]:
                shared.setdefault(key, ('Mozilla Public License 2.0', text))
                lines.append(f'  {f}: the Mozilla Public License 2.0 (its text at the end; the source is on crates.io)')
            else:
                lines.append(f'  {f}:')
                lines.extend('    ' + l for l in text.splitlines())
        lines.append('')

    lines += ['== Android libraries ==', '']
    for name, url in ANDROID:
        lines += [f'{name}: Apache License 2.0', url, '']

    lines += ['== Licence texts ==', '']
    written = set()
    for title, text in sorted(shared.values(), key=lambda t: t[0]):
        if title in written:
            continue
        written.add(title)
        lines += [f'-- {title} --', '', text, '']
    if 'Apache License 2.0' not in written:
        sys.exit('no Apache License 2.0 text found among the crates')

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(lines).rstrip() + '\n')
    print(f'{len(crates)} crates, {len(ANDROID)} Android groups -> {os.path.relpath(OUT, ROOT)}')


if __name__ == '__main__':
    main()
