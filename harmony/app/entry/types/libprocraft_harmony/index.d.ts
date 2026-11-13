// The ArkTS side of harmony/bridge (libprocraft_harmony.so). Written to match
// harmony/bridge/src/egui_host.rs and canvas.rs; `ohrs build` writes the same declarations from the Rust, and
// ../../../build-rust.sh copies them over this file when it is installed.

/**
 * Starts the app (for now the desktop app's interface, hosted in the canvas component whose
 * `libraryname` is 'procraft_harmony'): its folders (`context.filesDir`, `context.cacheDir`) and
 * the display's pixels per vp. Before the canvas component is made, after registerHost.
 */
export declare function startApp(filesDir: string, cacheDir: string, density: number): void

/**
 * ArkTS's side of what the app asks of the system (host/Host.ets): each request as JSON, on the
 * interface's thread; one with an id other than 0 is answered through answer().
 */
export declare function registerHost(host: (id: number, request: string) => void): void

/** The answer to request `id`, as JSON. */
export declare function answer(id: number, json: string): void

/** Text the soft keyboard typed. */
export declare function softText(text: string): void

/** A key the soft keyboard pressed `count` times: 'Backspace', 'Delete' or 'Enter'. */
export declare function softKey(name: string, count: number): void

/** The pen hovering over the canvas component at (x, y) vp, with its tilts in degrees. */
export declare function penHover(x: number, y: number, tiltX: number, tiltY: number): void

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
