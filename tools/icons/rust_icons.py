"""Writes the desktop app's icon table (desktop/src/icons.rs) from the Android app's
(ui/Icons.kt), so both show the same icons: every icon given as path data there (`ours`, `drawn`
and `icon` with literal parts). Icons computed in Kotlin (loops, helpers) are listed as skipped.

usage: python -I tools/icons/rust_icons.py
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
KOTLIN = os.path.join(ROOT, "android/app/src/main/java/io/github/artcraftandroid/procraft/ui/Icons.kt")
RUST = os.path.join(ROOT, "desktop/src/icons.rs")

NUM = r"(-?[0-9.]+)f?"
STR = r'"([^"]*)"'


def snake(name):
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).upper()


def floats(text):
    return float(text.rstrip("f"))


def parse_call(body):
    """An icon from a call's text, or None if it is computed."""
    body = body.strip()
    m = re.fullmatch(r"ours\(\s*" + STR + r"\s*,\s*" + NUM + r"\s*,(?:\s*" + NUM + r"\s*,)?\s*((?:" + STR + r"\s*,?\s*)+)\)", body, re.S)
    if m:
        name, w, h, paths = m.group(1), floats(m.group(2)), m.group(3), m.group(4)
        h = floats(h) if h else w
        parts = [(d, 0.0, True, True) for d in re.findall(STR, paths)]
        return name, w, h, parts
    m = re.fullmatch(r"drawn\(\s*" + STR + r"\s*,\s*" + NUM + r"\s*,\s*" + NUM + r"\s*,\s*((?:" + STR + r"\s*,?\s*)+)\)", body, re.S)
    if m:
        name, frame, width = m.group(1), floats(m.group(2)), floats(m.group(3))
        parts = [(d, width, False, False) for d in re.findall(STR, m.group(4))]
        return name, frame, frame, parts
    # in the 24 frame: stroked `width` wide, or filled
    m = re.fullmatch(r"outline\(\s*" + STR + r"\s*,\s*" + NUM + r"\s*,\s*((?:" + STR + r"\s*,?\s*)+)\)", body, re.S)
    if m:
        name, width = m.group(1), floats(m.group(2))
        return name, 24.0, 24.0, [(d, width, False, False) for d in re.findall(STR, m.group(3))]
    # filled and outlined 1.2 wide, in the 24 frame
    m = re.fullmatch(r"solid\(\s*" + STR + r"\s*,\s*((?:" + STR + r"\s*,?\s*)+)\)", body, re.S)
    if m:
        return m.group(1), 24.0, 24.0, [(d, 1.2, True, False) for d in re.findall(STR, m.group(2))]
    m = re.fullmatch(r"filled\(\s*" + STR + r"\s*,\s*" + STR + r"\s*\)", body, re.S)
    if m:
        return m.group(1), 24.0, 24.0, [(m.group(2), 0.0, True, False)]
    m = re.fullmatch(r"icon\(\s*" + STR + r"\s*,(.*)\)", body, re.S)
    if m:
        name, rest = m.group(1), m.group(2)
        vw = re.search(r"viewport\s*=\s*" + NUM, rest)
        vh = re.search(r"viewportHeight\s*=\s*" + NUM, rest)
        w = floats(vw.group(1)) if vw else 24.0
        h = floats(vh.group(1)) if vh else w
        args = re.sub(r"viewport(Height)?\s*=\s*" + NUM, "", rest)
        parts = []
        for p in re.finditer(r"Part\(\s*" + STR + r"\s*,\s*" + NUM + r"\s*,\s*(true|false)\s*(?:,\s*evenOdd\s*=\s*(true|false))?\s*\)", args):
            parts.append((p.group(1), floats(p.group(2)), p.group(3) == "true", p.group(4) == "true"))
        leftover = re.sub(r"Part\([^)]*\)", "", args).strip(" ,\n")
        if not parts or leftover:
            return None
        return name, w, h, parts
    return None


def rust_icon(icon):
    name, w, h, parts = icon
    lines = [f'Icon {{ name: "{name}", width: {w!r}, height: {h!r}, parts: &[']
    for d, stroke, fill, even_odd in parts:
        lines.append(f'    Part {{ d: "{d}", stroke: {stroke!r}, fill: {str(fill).lower()}, even_odd: {str(even_odd).lower()} }},')
    lines.append("] }")
    return "\n".join(lines)


def main():
    text = open(KOTLIN, encoding="utf-8").read()
    out, skipped = [], []
    # each `val Name by lazy { ... }`, on one line or several (path data holds no braces)
    lines = text.splitlines()
    vals = []
    i = 0
    while i < len(lines):
        m = re.match(r"\s{4}val (\w+)(?::[^=]*)? by lazy \{(.*)$", lines[i])
        i += 1
        if not m:
            continue
        body, depth = m.group(2), 1 + m.group(2).count("{") - m.group(2).count("}")
        while depth > 0 and i < len(lines):
            body += " " + lines[i]
            depth += lines[i].count("{") - lines[i].count("}")
            i += 1
        vals.append((m.group(1), body.strip()[:-1].strip()))
    for val, body in vals:
        if body.startswith("listOf("):
            calls = re.findall(r'(ours\("[^"]+".*?"\))\s*,?\s*(?=ours\(|\)\s*$)', body[len("listOf("):].strip(), re.S)
            icons = [parse_call(c) for c in calls]
            if icons and all(icons):
                rows = "\n".join(f"    {rust_icon(i)}," for i in icons)
                out.append(f"pub const {snake(val)}: &[Icon] = &[\n{rows}\n];")
            else:
                skipped.append(val)
            continue
        if body.startswith("mapOf("):
            entries = re.findall(r'"([^"]+)" to (ours\(.*?"\))', body, re.S)
            icons = [(k, parse_call(v)) for k, v in entries]
            if all(i for _, i in icons) and icons:
                rows = "\n".join(f'    ("{k}", {rust_icon(i)}),' for k, i in icons)
                out.append(f"pub const {snake(val)}: &[(&str, Icon)] = &[\n{rows}\n];")
            else:
                skipped.append(val)
            continue
        icon = parse_call(body)
        if icon:
            out.append(f"pub const {snake(val)}: Icon = {rust_icon(icon)};")
        else:
            skipped.append(val)
    header = f"""//! The interface's icons, the same as the Android app's: written by tools/icons/rust_icons.py
//! from its ui/Icons.kt (don't edit by hand). Each is drawn in a `width` x `height` unit frame.
//! Not here (computed in Kotlin): {", ".join(skipped) or "none"}.

#![allow(dead_code)]
#![cfg_attr(rustfmt, rustfmt_skip)]

/// An icon: its parts, in a `width` x `height` unit frame.
pub struct Icon {{
    pub name: &'static str,
    pub width: f32,
    pub height: f32,
    pub parts: &'static [Part],
}}

/// A path filled (even-odd or not) or stroked `stroke` wide, with round caps and joins.
pub struct Part {{
    pub d: &'static str,
    pub stroke: f32,
    pub fill: bool,
    pub even_odd: bool,
}}
"""
    with open(RUST, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n" + "\n\n".join(out) + "\n")
    print(f"{len(out)} icons written, skipped: {skipped}")


if __name__ == "__main__":
    sys.exit(main())
