import { common } from '@kit.AbilityKit';
import { display } from '@kit.ArkUI';
import { penHover, registerHost, startApp } from 'libprocraft_harmony.so';
import { Host } from '../host/Host';

/**
 * For now the app is the desktop app's interface (egui) in a full-screen canvas component: its
 * native side (libprocraft_harmony.so, named by `libraryname`) takes the surface, the touches, the
 * mouse and the keys, and draws everything itself. What it asks of the system (pickers, sharing,
 * the pasteboard, the soft keyboard) comes back here (host/Host.ets). The ArkUI interface over the
 * core's CanvasSession (canvas/CanvasView.ets) comes later.
 */
@Entry
@Component
struct Index {
  aboutToAppear(): void {
    // before the canvas component loads the library's native side: ArkTS's side of the system,
    // the app's folders and the display's pixels per vp
    const context = getContext(this) as common.UIAbilityContext;
    const host = new Host(context);
    registerHost((id: number, request: string) => {
      host.handle(id, request);
    });
    startApp(context.filesDir, context.cacheDir, display.getDefaultDisplaySync().densityPixels);
  }

  build() {
    XComponent({ id: 'procraft', type: XComponentType.SURFACE, libraryname: 'procraft_harmony' })
      .width('100%')
      .height('100%')
      // the keys of a hardware keyboard come to the focused component
      .focusable(true)
      .defaultFocus(true)
      // the pen hovering, for the brush's outline (its touches come to the native side)
      .onHoverMove((event: HoverEvent) => {
        if (event.sourceTool === SourceTool.Pen) {
          penHover(event.x ?? 0, event.y ?? 0, event.tiltX, event.tiltY);
        }
      })
  }
}
