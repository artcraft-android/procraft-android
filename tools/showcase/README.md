# The showcase

The README's demo (`docs/images/demo.gif`, `demo.mp4`) and screenshots were made with these scripts
on a Lenovo Legion Tab (2560 x 1600, held sideways), with ProCraft open on an empty gallery.

- `penhid.py`: a virtual pen digitizer (tip, in range, X, Y and 12-bit pressure) for Android's `hid`
  shell command, and scripts of pen strokes with real timing.
- `drive.py`: adb taps for the interface (brushes, colours by hex, sizes, layers, the colour dragged
  onto the canvas to fill) and pen strokes in screen pixels.
- `paint_dusk.py`: paints *Dusk* from a blank canvas, stage by stage
  (`new sky sun hills trees stars bloom tour gallery`).
- `to_srgb.py`: the tablet's captures hold Display P3 values; this makes sRGB JPEGs for the web.

```sh
export ANDROID_SERIAL=<the tablet>
adb shell screenrecord --size 1920x1200 --bit-rate 16M --time-limit 0 /sdcard/showcase.mp4 &
python paint_dusk.py
adb shell pkill -INT screenrecord && adb pull /sdcard/showcase.mp4

# drop the idle frames, turn P3 into sRGB, then 2x for the video and 4x for the GIF
ffmpeg -i showcase.mp4 -vf "mpdecimate=hi=768:lo=320:frac=0.33,setpts=N/(30*TB),colorspace=all=bt709:iprimaries=smpte432:itrc=srgb:ispace=bt709:irange=tv,format=yuv420p" -r 30 -an dedup.mp4
ffmpeg -i dedup.mp4 -vf "setpts=PTS/2,scale=1600:1000:flags=lanczos,tpad=stop_mode=clone:stop_duration=3" -r 30 -an -crf 23 -movflags +faststart demo.mp4
ffmpeg -i dedup.mp4 -vf "setpts=PTS/4,fps=12,scale=800:-1:flags=lanczos,tpad=stop_mode=clone:stop_duration=2.5,split[a][b];[a]palettegen=max_colors=200:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" demo.gif
```

Notes from making it: start pen strokes clear of the sidebar (left) and of any system handle on the
screen's edge; drag the colour out of the colour button with a plain swipe (holding first takes the
previous colour) starting a little below the top edge (nearer, it pulls down the system bars); and
give a fill a closed outline inside the canvas, its points no more than a stroke's width apart.
