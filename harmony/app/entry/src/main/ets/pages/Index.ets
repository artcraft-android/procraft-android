import { CanvasView } from '../canvas/CanvasView';

/** For now the app is its canvas; the gallery and the interface over the canvas come next. */
@Entry
@Component
struct Index {
  build() {
    Stack() {
      CanvasView()
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#000000')
  }
}
