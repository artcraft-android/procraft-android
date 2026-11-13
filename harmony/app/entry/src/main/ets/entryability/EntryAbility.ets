import { AbilityConstant, UIAbility, Want } from '@kit.AbilityKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { window } from '@kit.ArkUI';

const DOMAIN: number = 0x5043;
const TAG: string = 'ProCraft';

/** The app's one window: the canvas page, drawn under the system bars. */
export default class EntryAbility extends UIAbility {
  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(DOMAIN, TAG, 'started');
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, TAG, 'no page: %{public}s', JSON.stringify(err));
        return;
      }
      windowStage.getMainWindowSync().setWindowLayoutFullScreen(true).catch((e: Error) => {
        hilog.warn(DOMAIN, TAG, 'not full screen: %{public}s', e.message);
      });
    });
  }
}
