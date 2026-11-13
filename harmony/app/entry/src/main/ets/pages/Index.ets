import { common } from '@kit.AbilityKit';
import { display } from '@kit.ArkUI';
import { startApp } from 'libprocraft_harmony.so';

/**
 * For now the app is the desktop app's interface (egui) in a full-screen canvas component: its
 * native side (libprocraft_harmony.so, named by `libraryname`) takes the surface, the touches, the
 * mouse, the keys and the frames, and draws everything itself. The ArkUI interface over the core's
 * CanvasSession (canvas/CanvasView.ets) comes later.
 */
@Entry
@Component
struct Index {
  aboutToAppear(): void {
    // before the canvas component loads the library's native side: the app's folder and the
    // display's pixels per vp
    const context = getContext(this) as common.UIAbilityContext;
    startApp(context.filesDir, display.getDefaultDisplaySync().densityPixels);
  }

  build() {
    XComponent({ id: 'procraft', type: XComponentType.SURFACE, libraryname: 'procraft_harmony' })
      .width('100%')
      .height('100%')
      // the keys of a hardware keyboard come to the focused component
      .focusable(true)
      .defaultFocus(true)
  }
}
