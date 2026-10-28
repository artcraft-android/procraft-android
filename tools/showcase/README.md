# The showcase

The README's demo (`docs/images/demo.webp`, `demo.mp4`) and screenshots were made with these scripts
on a Lenovo Legion Tab (2560 x 1600, held sideways), with ProCraft open on an empty gallery.

- `penhid.py`: a virtual pen digitizer (tip, in range, X, Y and 12-bit pressure) for Android's `hid`
  shell command, and scripts of pen strokes with real timing.
- `drive.py`: adb taps for the interface (brushes, colours by hex, sizes, layers, the colour dragged
  onto the canvas to fill) and pen strokes in screen pixels.
- `human.py`: strokes as a hand makes them: eased speed, a slight wobble, pressure that rises
  and falls, pauses between moves.
- `paint_dusk2.py`: paints *Dusk* from a blank canvas at a person's pace (menus held open, colours
  picked on the Classic square), stage by stage, and logs each span as menu or drawing.
- `record_take.py`: records a take (screenrecord) while `paint_dusk2.py` paints, with its log.
- `edit_take.py`: plays each logged span at its own speed (menus near real time, drawing about 3x),
  turns P3 into sRGB, and joins them into the video.
- `to_srgb.py`: the tablet's captures hold Display P3 values; this makes sRGB JPEGs for the web.

```sh
export ANDROID_SERIAL=<the tablet>
python record_take.py take                 # take/take.mp4 and take/take.json
python edit_take.py take 2.6 edit          # edit/demo_full.mp4 (2.6 s: the log's lag behind the video)
ffmpeg -i edit/demo_full.mp4 -vf "fps=12,scale=960:-1:flags=lanczos" -c:v libwebp_anim -loop 0 -q:v 62 demo.webp
```

Notes from making it: start pen strokes clear of the sidebar (left) and of any system handle on the
screen's edge; drag the colour out of the colour button with a plain swipe (holding first takes the
previous colour) starting a little below the top edge (nearer, it pulls down the system bars); and
give a fill a closed outline inside the canvas, its points no more than a stroke's width apart,
drawn without wobble or thin ends; a brush change forgets the size slider's place (each brush
keeps its own size); and keep taps away from the pen button at the top right edge.
