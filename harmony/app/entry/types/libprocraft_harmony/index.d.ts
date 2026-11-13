// The ArkTS side of harmony/bridge (libprocraft_harmony.so). Written to match
// harmony/bridge/src/canvas.rs; `ohrs build` writes the same declarations from the Rust, and
// ../../../build-rust.sh copies them over this file when it is installed.

/** One touch sample: surface pixels, pressure 0..1, the pen's tilts in degrees, time in ns. */
export interface TouchSample {
  x: number
  y: number
  pressure: number
  tiltX: number
  tiltY: number
  timeNs: number
}

/** An artwork and its canvas (the core's session, named as in the other apps). */
export declare class CanvasSession {
  /** A new artwork of `width` × `height` pixels. */
  constructor(width: number, height: number)
  /** Starts drawing into the canvas component's surface (its id as the XComponent gives it). */
  attachSurface(surfaceId: string, width: number, height: number): void
  /** The surface's new size, pixels. */
  resize(width: number, height: number): void
  /** Stops drawing into the surface; the artwork stays. */
  detach(): void
  /** A finger or pen touching down; what the touches ask of the interface, by name. */
  touchDown(id: number, pen: boolean, samples: Array<TouchSample>): Array<string>
  touchMove(id: number, samples: Array<TouchSample>): Array<string>
  touchUp(id: number, samples: Array<TouchSample>): Array<string>
  touchCancel(): Array<string>
  /** Undoes the last change; its name, if there was one. */
  undo(): string | null
  redo(): string | null
  /** The brush's colour, sRGB 0..1. */
  setColor(r: number, g: number, b: number): void
  /** The brush size slider, 0..1. */
  setBrushSize(slider: number): void
}
