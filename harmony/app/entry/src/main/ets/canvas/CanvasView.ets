import { display } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { CanvasSession, TouchSample } from 'libprocraft_harmony.so';

const DOMAIN: number = 0x5043;
const TAG: string = 'ProCraft';

/**
 * The canvas component's surface, handed to the core as it comes, changes size and goes. The core
 * makes the native window from the surface id and draws into it on its own render thread.
 */
class SurfaceKeeper extends XComponentController {
  session: CanvasSession | null = null;
  private attached: boolean = false;

  onSurfaceChanged(surfaceId: string, rect: SurfaceRect): void {
    const session = this.session;
    if (session === null) {
      return;
    }
    const width = Math.round(rect.surfaceWidth);
    const height = Math.round(rect.surfaceHeight);
    if (this.attached) {
      session.resize(width, height);
      return;
    }
    try {
      session.attachSurface(surfaceId, width, height);
      this.attached = true;
    } catch (e) {
      hilog.error(DOMAIN, TAG, 'no canvas: %{public}s', (e as Error).message);
    }
  }

  onSurfaceDestroyed(surfaceId: string): void {
    if (this.attached && this.session !== null) {
      this.session.detach();
    }
    this.attached = false;
  }
}

/** The canvas: the core's drawing, and every touch and pen sample on it passed to the core. */
@Component
export struct CanvasView {
  private session: CanvasSession | null = null;
  private keeper: SurfaceKeeper = new SurfaceKeeper();
  // surface pixels per vp (touch coordinates are in vp, the surface in pixels)
  private scale: number = 1;

  aboutToAppear(): void {
    // a new artwork the size of the screen, for now
    const screen = display.getDefaultDisplaySync();
    this.session = new CanvasSession(Math.max(screen.width, screen.height), Math.min(screen.width, screen.height));
    this.keeper.session = this.session;
    this.scale = this.getUIContext().vp2px(1);
  }

  aboutToDisappear(): void {
    this.session?.detach();
  }

  build() {
    XComponent({ type: XComponentType.SURFACE, controller: this.keeper })
      .width('100%')
      .height('100%')
      .onTouch((event: TouchEvent) => {
        this.touched(event);
      })
  }

  private touched(event: TouchEvent): void {
    const session = this.session;
    if (session === null) {
      return;
    }
    const pen = event.sourceTool === SourceTool.Pen;
    for (const touch of event.changedTouches) {
      const samples = this.samplesOf(event, touch);
      let asked: Array<string> = [];
      if (touch.type === TouchType.Down) {
        asked = session.touchDown(touch.id, pen, samples);
      } else if (touch.type === TouchType.Move) {
        asked = session.touchMove(touch.id, samples);
      } else if (touch.type === TouchType.Up) {
        asked = session.touchUp(touch.id, samples);
      } else if (touch.type === TouchType.Cancel) {
        asked = session.touchCancel();
      }
      this.act(session, asked);
    }
  }

  /**
   * A touch's samples in this event: the raw points ArkUI kept since the last frame, else the
   * event's own (resampled) point.
   */
  private samplesOf(event: TouchEvent, touch: TouchObject): Array<TouchSample> {
    const out: Array<TouchSample> = [];
    for (const point of event.getHistoricalPoints()) {
      if (point.touchObject.id === touch.id) {
        out.push(this.sample(event, point.touchObject.x, point.touchObject.y, point.timestamp));
      }
    }
    if (out.length === 0) {
      out.push(this.sample(event, touch.x, touch.y, event.timestamp));
    }
    return out;
  }

  /**
   * One sample in surface pixels. Pressure and tilt are the event's (one pen at a time); the
   * native event API has them for each kept point too, which a later version reads.
   */
  private sample(event: TouchEvent, x: number, y: number, timeNs: number): TouchSample {
    const pen = event.sourceTool === SourceTool.Pen;
    const sample: TouchSample = {
      x: x * this.scale,
      y: y * this.scale,
      pressure: pen ? event.pressure : 1,
      tiltX: pen ? event.tiltX : 0,
      tiltY: pen ? event.tiltY : 0,
      timeNs: timeNs
    };
    return sample;
  }

  /** What the touches asked for that the canvas alone can do; the rest waits for the interface. */
  private act(session: CanvasSession, asked: Array<string>): void {
    for (const command of asked) {
      if (command === 'Undo') {
        session.undo();
      } else if (command === 'Redo') {
        session.redo();
      }
    }
  }
}
