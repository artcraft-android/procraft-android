import { common } from '@kit.AbilityKit';
import { uniformTypeDescriptor as utd } from '@kit.ArkData';
import { BusinessError, pasteboard } from '@kit.BasicServicesKit';
import { fileIo as fs, fileUri, picker } from '@kit.CoreFileKit';
import { inputMethod } from '@kit.IMEKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { systemShare } from '@kit.ShareKit';
import { answer, softKey, softText } from 'libprocraft_harmony.so';

const DOMAIN: number = 0x5043;
const TAG: string = 'ProCraft';

/** A request from the app, as JSON (desktop/src/ohos_platform.rs). */
interface HostRequest {
  kind: string;
  title?: string;
  extensions?: string[];
  multiple?: boolean;
  into?: string;
  name?: string;
  paths?: string[];
  from?: string;
  to?: string;
  text?: string;
  show?: boolean;
}

class OpenAnswer {
  paths: string[] = [];
}

class SaveAnswer {
  uri: string = '';
}

class TextAnswer {
  text: string = '';
}

/**
 * What the app asks of the system, done here in ArkTS: the document picker (the files picked are
 * copied into the app's cache, as the app reads paths), the picker's Save (the app writes into its
 * cache, then asks for the file to be copied where the user picked), the share sheet, the
 * pasteboard and the soft keyboard. A request with an id other than 0 is answered through the
 * library's `answer`.
 */
export class Host {
  private context: common.UIAbilityContext;
  private keyboardUp: boolean = false;

  constructor(context: common.UIAbilityContext) {
    this.context = context;
  }

  handle(id: number, request: string): void {
    let r: HostRequest;
    try {
      r = JSON.parse(request) as HostRequest;
    } catch (e) {
      this.reply(id, '{}');
      return;
    }
    if (r.kind === 'open') {
      this.open(id, r);
    } else if (r.kind === 'save') {
      this.save(id, r);
    } else if (r.kind === 'copyOut') {
      this.copyOut(r);
    } else if (r.kind === 'share') {
      this.share(r);
    } else if (r.kind === 'copyText') {
      this.copyText(r);
    } else if (r.kind === 'pasteText') {
      this.pasteText(id);
    } else if (r.kind === 'keyboard') {
      this.keyboard(r.show === true);
    } else {
      this.reply(id, '{}');
    }
  }

  private reply(id: number, json: string): void {
    if (id !== 0) {
      answer(id, json);
    }
  }

  /** The document picker: the files picked, copied into `into` (each in a folder of its own). */
  private open(id: number, r: HostRequest): void {
    const options = new picker.DocumentSelectOptions();
    options.maxSelectNumber = r.multiple === true ? 500 : 1;
    const extensions: string[] = r.extensions ?? [];
    if (extensions.length > 0) {
      const endings: string = extensions.map((e: string) => '.' + e).join(',');
      options.fileSuffixFilters = [(r.title ?? 'Files') + '|' + endings];
    }
    const into: string = r.into ?? this.context.cacheDir;
    new picker.DocumentViewPicker(this.context).select(options).then((uris: Array<string>) => {
      const result = new OpenAnswer();
      for (const uri of uris) {
        const copied = this.copyIn(uri, into);
        if (copied.length > 0) {
          result.paths.push(copied);
        }
      }
      this.reply(id, JSON.stringify(result));
    }).catch((e: BusinessError) => {
      hilog.warn(DOMAIN, TAG, 'picking: %{public}s', e.message);
      this.reply(id, JSON.stringify(new OpenAnswer()));
    });
  }

  /** Copies the picked file at `uri` into a new folder in `dir`, keeping its name: its path, or ''. */
  private copyIn(uri: string, dir: string): string {
    try {
      const name: string = new fileUri.FileUri(uri).name;
      const folder: string = dir + '/' + Date.now().toString() + '-' + Math.floor(Math.random() * 1000000).toString();
      fs.mkdirSync(folder);
      const path: string = folder + '/' + name;
      const source = fs.openSync(uri, fs.OpenMode.READ_ONLY);
      fs.copyFileSync(source.fd, path);
      fs.closeSync(source);
      return path;
    } catch (e) {
      hilog.warn(DOMAIN, TAG, 'copying a picked file: %{public}s', (e as BusinessError).message);
      return '';
    }
  }

  /** The picker's Save: where the user wants `name`, as a URI ('' if nowhere). */
  private save(id: number, r: HostRequest): void {
    const options = new picker.DocumentSaveOptions();
    options.newFileNames = [r.name ?? 'ProCraft'];
    new picker.DocumentViewPicker(this.context).save(options).then((uris: Array<string>) => {
      const result = new SaveAnswer();
      result.uri = uris.length > 0 ? uris[0] : '';
      this.reply(id, JSON.stringify(result));
    }).catch((e: BusinessError) => {
      hilog.warn(DOMAIN, TAG, 'saving: %{public}s', e.message);
      this.reply(id, JSON.stringify(new SaveAnswer()));
    });
  }

  /** The file the app wrote at `from` copied to the URI `to` the picker's Save gave. */
  private copyOut(r: HostRequest): void {
    try {
      const target = fs.openSync(r.to ?? '', fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC);
      fs.copyFileSync(r.from ?? '', target.fd);
      fs.closeSync(target);
    } catch (e) {
      hilog.error(DOMAIN, TAG, 'copying a saved file out: %{public}s', (e as BusinessError).message);
    }
  }

  /** The share sheet, with the files at `paths`. */
  private share(r: HostRequest): void {
    const paths: string[] = r.paths ?? [];
    let data: systemShare.SharedData | undefined = undefined;
    for (const path of paths) {
      const dot = path.lastIndexOf('.');
      const ending: string = dot >= 0 ? path.substring(dot) : '';
      const record: systemShare.SharedRecord = {
        utd: utd.getUniformDataTypeByFilenameExtension(ending, utd.UniformDataType.FILE),
        uri: fileUri.getUriFromPath(path)
      };
      if (data === undefined) {
        data = new systemShare.SharedData(record);
      } else {
        data.addRecord(record);
      }
    }
    if (data === undefined) {
      return;
    }
    const controller = new systemShare.ShareController(data);
    controller.show(this.context, {
      selectionMode: paths.length > 1 ? systemShare.SelectionMode.BATCH : systemShare.SelectionMode.SINGLE,
      previewMode: systemShare.SharePreviewMode.DEFAULT
    }).catch((e: BusinessError) => {
      hilog.error(DOMAIN, TAG, 'sharing: %{public}s', e.message);
    });
  }

  /** Text onto the system's pasteboard (writing takes no permission). */
  private copyText(r: HostRequest): void {
    const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, r.text ?? '');
    pasteboard.getSystemPasteboard().setData(data).catch((e: BusinessError) => {
      hilog.warn(DOMAIN, TAG, 'copying: %{public}s', e.message);
    });
  }

  /**
   * The pasteboard's text, where the app may read it: reading takes ohos.permission.READ_PASTEBOARD
   * (which most apps don't get) or the system's paste button; without, '' and the app's own
   * clipboard stands.
   */
  private pasteText(id: number): void {
    const result = new TextAnswer();
    try {
      const board = pasteboard.getSystemPasteboard();
      if (board.hasDataSync()) {
        result.text = board.getDataSync().getPrimaryText();
      }
    } catch (e) {
      hilog.info(DOMAIN, TAG, 'the pasteboard can\'t be read: %{public}s', (e as BusinessError).message);
    }
    this.reply(id, JSON.stringify(result));
  }

  /** The soft keyboard up (its text typed into the app) or down. */
  private keyboard(show: boolean): void {
    if (show === this.keyboardUp) {
      return;
    }
    this.keyboardUp = show;
    const controller = inputMethod.getController();
    if (show) {
      const config: inputMethod.TextConfig = {
        inputAttribute: {
          textInputType: inputMethod.TextInputType.TEXT,
          enterKeyType: inputMethod.EnterKeyType.DONE
        }
      };
      controller.attach(true, config).then(() => {
        controller.on('insertText', (text: string) => {
          softText(text);
        });
        controller.on('deleteLeft', (length: number) => {
          softKey('Backspace', length);
        });
        controller.on('deleteRight', (length: number) => {
          softKey('Delete', length);
        });
        controller.on('sendFunctionKey', (functionKey: inputMethod.FunctionKey) => {
          softKey('Enter', 1);
        });
      }).catch((e: BusinessError) => {
        hilog.warn(DOMAIN, TAG, 'the keyboard: %{public}s', e.message);
      });
    } else {
      controller.off('insertText');
      controller.off('deleteLeft');
      controller.off('deleteRight');
      controller.off('sendFunctionKey');
      controller.detach().catch((e: BusinessError) => {
        hilog.warn(DOMAIN, TAG, 'the keyboard: %{public}s', e.message);
      });
    }
  }
}
