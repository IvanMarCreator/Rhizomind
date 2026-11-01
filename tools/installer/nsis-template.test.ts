import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { applyPatches, cutWebView2Section, PATCHES, revertPatches, WEBVIEW2_REMOVED } from './nsis-template.ts';

const root = path.join(import.meta.dirname, '..', '..');
const ours = readFileSync(path.join(root, 'src-tauri', 'windows', 'nsis', 'installer.nsi'), 'utf8').replaceAll('\r\n', '\n');

/** Исполняемый модуль @tauri-apps/cli для этой платформы (в нём вшит шаблон бандлера). */
function cliBinaries(): string[] {
  const scope = path.join(root, 'node_modules', '@tauri-apps');
  return readdirSync(scope)
    .filter((name) => name.startsWith('cli-'))
    .flatMap((name) => readdirSync(path.join(scope, name)).map((file) => path.join(scope, name, file)))
    .filter((file) => file.endsWith('.node') && existsSync(file));
}

/** Шаблон NSIS из исполняемого модуля CLI: от начала до конца по нашему откату правок. */
function upstreamFromCli(): string {
  const reverted = revertPatches(ours);
  const [prefix, suffix] = reverted.split(WEBVIEW2_REMOVED);
  if (prefix === undefined || suffix === undefined) throw new Error('в шаблоне нет пометки о вырезанном разделе WebView2');
  for (const file of cliBinaries()) {
    const binary = readFileSync(file);
    const start = binary.indexOf(prefix);
    if (start === -1) continue;
    const end = binary.indexOf(suffix, start + prefix.length);
    if (end === -1) continue;
    return binary.subarray(start, end + Buffer.byteLength(suffix)).toString('utf8');
  }
  throw new Error(
    'шаблон NSIS без наших правок не найден в @tauri-apps/cli: шаблон бандлера изменился — перенесите правки (tools/installer/nsis-template.ts)',
  );
}

describe('свой шаблон NSIS', () => {
  it('это шаблон из установленного @tauri-apps/cli с нашими правками и ничем больше', () => {
    expect(cliBinaries().length).toBeGreaterThan(0);
    expect(applyPatches(upstreamFromCli())).toBe(ours);
  });

  it('каждая правка встречается в шаблоне ровно один раз', () => {
    for (const patch of PATCHES) {
      expect(ours.split(patch.ours).length - 1, patch.name).toBe(1);
    }
  });

  it('в установщике нет кода установки и обновления WebView2', () => {
    const { section } = cutWebView2Section(upstreamFromCli());
    expect(section).toContain('NSISdl::download');
    expect(ours).not.toContain('Section WebView2');
    for (const marker of ['NSISdl', 'inetc', 'go.microsoft.com', 'MicrosoftEdgeWebview2Setup', 'EdgeUpdate" "path"', 'needsadmin']) {
      expect(ours, marker).not.toContain(marker);
    }
  });

  it('для текущего пользователя ставит в папку программ, а не в папку списка хранилищ', () => {
    expect(ours).toContain('StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${PRODUCTNAME}"');
    expect(ours).not.toContain('StrCpy $INSTDIR "$LOCALAPPDATA\\${PRODUCTNAME}"');
  });

  it('список хранилищ удаляется только по галочке «Удалить данные приложения»', () => {
    const checkbox = ours.indexOf('${If} $DeleteAppDataCheckboxState = 1');
    const removal = ours.indexOf('RmDir /r "$LOCALAPPDATA\\${PRODUCTNAME}"');
    expect(checkbox).toBeGreaterThan(-1);
    expect(removal).toBeGreaterThan(checkbox);
    expect(ours.indexOf('${EndIf}', checkbox)).toBeGreaterThan(removal);
    expect(ours.split('$LOCALAPPDATA\\${PRODUCTNAME}"').length - 1).toBe(1);
  });
});
