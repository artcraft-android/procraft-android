"""Records the human-paced take: starts screenrecord on the tablet, paints Dusk with
paint_dusk2 (which logs each span), stops the recording and pulls it, and writes the log with
the recording's start time on the same clock.

usage: python -I record_take.py <out dir>
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import drive as d  # noqa: E402

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
d.sh("rm -f /sdcard/take.mp4")
rec = subprocess.Popen([d.ADB, "shell", "screenrecord", "--size", "1920x1200", "--bit-rate", "16M",
                        "--time-limit", "0", "/sdcard/take.mp4"])
rec_start = time.time()
time.sleep(2.0)
import paint_dusk2 as p  # noqa: E402  (its clock starts here)

try:
    for name in ["new", "sky", "sun", "hills", "trees", "stars", "bloom", "tour", "gallery"]:
        t = time.time()
        p.STAGES[name]()
        print(f"{name}: {time.time() - t:.1f} s", flush=True)
finally:
    time.sleep(1.5)
    d.sh("pkill -INT screenrecord")
    rec.wait(timeout=30)
    time.sleep(2.0)
    d.adb("pull", "/sdcard/take.mp4", os.path.join(out, "take.mp4"))
    with open(os.path.join(out, "take.json"), "w", encoding="utf-8") as f:
        json.dump({"rec_start": rec_start, "t0": p.T0, "segments": p.LOG}, f, indent=1)
    print("saved", flush=True)
