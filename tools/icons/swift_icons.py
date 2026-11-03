"""Writes the iPad app's icon table (ios/ProCraft/Icons.swift) from the desktop app's
(desktop/src/icons.rs, itself written from the Android app's ui/Icons.kt), so all three show the
same icons.

usage: python -I tools/icons/swift_icons.py
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUST = os.path.join(ROOT, "desktop/src/icons.rs")
SWIFT = os.path.join(ROOT, "ios/ProCraft/Icons.swift")

ICON = re.compile(r'Icon \{ name: "([^"]+)", width: ([0-9.]+), height: ([0-9.]+), parts: &\[(.*?)\] \}', re.S)
PART = re.compile(r'Part \{ d: "([^"]*)", stroke: ([0-9.]+), fill: (true|false), even_odd: (true|false) \}')


def camel(snake):
    words = snake.lower().split("_")
    return words[0] + "".join(w.capitalize() for w in words[1:])


def swift_icon(m, indent):
    name, w, h, body = m.group(1), m.group(2), m.group(3), m.group(4)
    parts = []
    for p in PART.finditer(body):
        d, stroke, fill, even_odd = p.group(1), p.group(2), p.group(3), p.group(4)
        parts.append(f'{indent}    PcIconPart(d: "{d}", stroke: {stroke}, fill: {fill}, evenOdd: {even_odd}),')
    return f'PcIcon(name: "{name}", width: {w}, height: {h}, parts: [\n' + "\n".join(parts) + f"\n{indent}])"


def main():
    text = open(RUST, encoding="utf-8").read()
    out = []
    for m in re.finditer(r"^pub const (\w+): (Icon|&\[Icon\]|&\[\(&str, Icon\)\]) = (.*?)^(?=pub const|\Z)", text, re.S | re.M):
        name, kind, body = camel(m.group(1)), m.group(2), m.group(3)
        if kind == "Icon":
            icon = ICON.search(body)
            out.append(f"    static let {name} = {swift_icon(icon, '    ')}")
        elif kind == "&[Icon]":
            rows = [f"        {swift_icon(i, '        ')}," for i in ICON.finditer(body)]
            out.append(f"    static let {name}: [PcIcon] = [\n" + "\n".join(rows) + "\n    ]")
        else:
            rows = []
            for e in re.finditer(r'\("([^"]+)", (Icon \{.*?\] \})\)', body, re.S):
                icon = ICON.match(e.group(2))
                rows.append(f'        ("{e.group(1)}", {swift_icon(icon, "        ")}),')
            out.append(f"    static let {name}: [(String, PcIcon)] = [\n" + "\n".join(rows) + "\n    ]")
    header = """// The interface's icons, the same as the Android and desktop apps': written by
// tools/icons/swift_icons.py from desktop/src/icons.rs (don't edit by hand). Each is drawn in a
// `width` x `height` unit frame.

// swiftlint:disable all

/// The interface's icons.
enum PcIcons {
"""
    with open(SWIFT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n\n".join(out) + "\n}\n")
    print(f"{len(out)} icons written")


if __name__ == "__main__":
    sys.exit(main())
