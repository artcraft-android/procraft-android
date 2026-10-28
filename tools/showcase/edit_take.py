"""Cuts the take into the README video: each logged span played at its own speed (the first colour
pick, brush pick and new layer close to real time so they can be read, repeats faster, fills and
the layers slow enough to see, drawing at about 3x), the take's P3 colours made sRGB, the spans
joined, then a full MP4 and an animated WebP and GIF for the README.

usage: python -I edit_take.py <take dir> <offset s> <out dir>
"""
import json
import os
import subprocess
import sys

take, offset, out = sys.argv[1], float(sys.argv[2]), sys.argv[3]
os.makedirs(out, exist_ok=True)
j = json.load(open(os.path.join(take, "take.json"), encoding="utf-8"))
segs = sorted(j["segments"], key=lambda s: s["start"])
seen = {}


def speed(s):
    kind, label = s["kind"], s["label"]
    if kind == "draw":
        return 1.6 if label == "sun circle" else 3.0
    family = label.split(" ")[0]
    n = seen.get(family, 0)
    seen[family] = n + 1
    if family == "new":
        return 1.6 if label == "new canvas" else (1.5 if n == 0 else 3.0)
    if family == "colour":
        return 1.5 if n < 2 else 3.0
    if family == "brush":
        return 1.6 if n == 0 else 2.6
    if family in ("size", "opacity"):
        return 2.0
    if family == "drop":
        return 1.2
    if family == "bloom":
        return 1.3
    if family == "layers":
        return 0.9
    return 2.0


plan = []
t = 0.0
for s in segs:
    if s["label"] == "gallery":
        break
    a, b = s["start"] + offset, s["end"] + offset
    if a > t + 0.05:
        plan.append((t, a, 6.0, "idle"))
    plan.append((max(a, t), b, speed(s), s["label"]))
    t = b
plan.append((t, t + 1.2, 0.8, "end hold"))

colour = "colorspace=all=bt709:iprimaries=smpte432:itrc=srgb:ispace=bt709:irange=tv"
parts = []
for i, (a, b, sp, label) in enumerate(plan):
    if b - a < 0.05:
        continue
    part = os.path.join(out, f"part_{i:03d}.mp4")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", f"{a:.3f}", "-to", f"{b:.3f}", "-i", os.path.join(take, "take.mp4"),
                    "-vf", f"setpts=(PTS-STARTPTS)/{sp},fps=30,{colour},scale=1600:1000:flags=lanczos,format=yuv420p",
                    "-an", "-c:v", "libx264", "-crf", "18", "-preset", "veryfast", part], check=True)
    parts.append(part)
with open(os.path.join(out, "parts.txt"), "w", encoding="utf-8") as f:
    for p in parts:
        f.write(f"file '{p}'\n")
full = os.path.join(out, "demo_full.mp4")
subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", os.path.join(out, "parts.txt"),
                "-c:v", "libx264", "-crf", "23", "-preset", "slow", "-pix_fmt", "yuv420p", "-movflags", "+faststart", full], check=True)
total = sum((b - a) / sp for a, b, sp, _ in plan)
print("parts", len(parts), "length about", round(total, 1), "s")
