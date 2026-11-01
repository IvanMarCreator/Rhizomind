// Свой шаблон установщика NSIS (src-tauri/windows/nsis/installer.nsi,
// bundle.windows.nsis.template в tauri.conf.json) — копия шаблона бандлера
// Tauri с правками из PATCHES ниже и больше ничем (задача 3.4).
//
// Зачем свой шаблон:
// 1. Папка программы для текущего пользователя — %LOCALAPPDATA%\Programs\Rhizomind
//    (спецификация 0.1, С7). Шаблон Tauri ставит в %LOCALAPPDATA%\Rhizomind —
//    это папка списка хранилищ vaults.json (решение 46): программа и данные
//    пользователя оказались бы в одной папке.
// 2. В установщике нет кода установки и обновления WebView2 (загрузка с
//    go.microsoft.com, запуск EdgeUpdate) — независимо от настроек; в шаблоне
//    Tauri он отключается только режимом webviewInstallMode (инвариант 1).
// 3. Удаление всегда убирает свою запись в реестре HKCU\Software\<издатель>\
//    Rhizomind (папка установки, язык установщика); в шаблоне Tauri — только
//    с галочкой «Удалить данные приложения».
// 4. Галочка «Удалить данные приложения» (по умолчанию снята) удаляет и
//    %LOCALAPPDATA%\Rhizomind (список хранилищ); без неё данные не трогаются.
// 5. Ярлык на рабочем столе по умолчанию не создаётся (решение 107, задача
//    3.4б): галочка на странице завершения снята, тихая (/S) и пассивная (/P)
//    установка его не создают — рабочий стол бывает перенаправлен в OneDrive.
//    Ярлык меню «Пуск» — как в шаблоне Tauri (/NS отключает оба).
//
// Проверка (nsis-template.test.ts): если откатить правки, получается ровно
// шаблон из установленного @tauri-apps/cli (текст вшит в его исполняемый
// модуль). Обновили Tauri и шаблон бандлера изменился — тест падает: перенести
// правки на новый шаблон.

export interface Patch {
  /** Что это за правка — для сообщения теста. */
  readonly name: string;
  /** Текст в шаблоне Tauri. */
  readonly upstream: string;
  /** Текст в нашем шаблоне. */
  readonly ours: string;
}

const HEADER = `; Rhizomind: свой шаблон NSIS — шаблон бандлера Tauri с правками, помеченными
; «Rhizomind:». Перечень правок и проверка — tools/installer/nsis-template.ts.
`;

const WEBVIEW2_SECTION_START = 'Section WebView2\n';

export const PATCHES: readonly Patch[] = [
  {
    name: 'заголовок',
    upstream: 'Unicode true\n',
    ours: `${HEADER}Unicode true\n`,
  },
  {
    name: 'папка установки для текущего пользователя',
    upstream: '      StrCpy $INSTDIR "$LOCALAPPDATA\\${PRODUCTNAME}"\n',
    ours:
      '      ; Rhizomind: папка программ пользователя; %LOCALAPPDATA%\\Rhizomind — данные (vaults.json)\n' +
      '      StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${PRODUCTNAME}"\n',
  },
  {
    name: 'запись установщика в реестре удаляется всегда',
    upstream: '  ; Delete app data if the checkbox is selected\n',
    ours:
      '  ; Rhizomind: папка установки и язык установщика в реестре — не данные пользователя,\n' +
      '  ; удаляются всегда (кроме обновления)\n' +
      '  ${If} $UpdateMode <> 1\n' +
      '    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"\n' +
      '    DeleteRegKey /ifempty SHCTX "${MANUKEY}"\n' +
      '    DeleteRegKey HKCU "${MANUPRODUCTKEY}"\n' +
      '    DeleteRegKey /ifempty HKCU "${MANUKEY}"\n' +
      '  ${EndIf}\n' +
      '\n' +
      '  ; Delete app data if the checkbox is selected\n',
  },
  {
    name: 'галочка «Удалить данные приложения» удаляет список хранилищ',
    upstream: '    RmDir /r "$LOCALAPPDATA\\${BUNDLEID}"\n',
    ours:
      '    RmDir /r "$LOCALAPPDATA\\${BUNDLEID}"\n' +
      '    ; Rhizomind: список хранилищ (vaults.json и его копии) — только по галочке\n' +
      '    RmDir /r "$LOCALAPPDATA\\${PRODUCTNAME}"\n',
  },
  {
    name: 'галочка «Ярлык на рабочем столе» снята по умолчанию',
    upstream: '!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateOrUpdateDesktopShortcut\n',
    ours:
      '!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateOrUpdateDesktopShortcut\n' +
      '; Rhizomind: ярлык на рабочем столе — только по галочке, по умолчанию снята (решение 107)\n' +
      '!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED\n',
  },
  {
    name: 'тихая и пассивная установка не создают ярлык на рабочем столе',
    upstream:
      '  ; Create desktop shortcut for silent and passive installers\n' +
      '  ; because finish page will be skipped\n' +
      '  ${If} $PassiveMode = 1\n' +
      '  ${OrIf} ${Silent}\n' +
      '    Call CreateOrUpdateDesktopShortcut\n' +
      '  ${EndIf}\n',
    ours:
      '  ; Rhizomind: тихая (/S) и пассивная (/P) установка не создают ярлык на рабочем\n' +
      '  ; столе (решение 107: рабочий стол бывает перенаправлен в OneDrive)\n',
  },
];

/** Раздел установки WebView2 из шаблона Tauri вырезается целиком. */
export const WEBVIEW2_REMOVED =
  '; Rhizomind: раздел установки и обновления WebView2 вырезан — установщик не\n' +
  '; скачивает и не запускает установщик WebView2 (используется системный, ADR 0007)\n';

/** Вырезает `Section WebView2 … SectionEnd` из текста шаблона Tauri. */
export function cutWebView2Section(upstream: string): { text: string; section: string } {
  const start = upstream.indexOf(WEBVIEW2_SECTION_START);
  if (start === -1) throw new Error('в шаблоне Tauri нет раздела Section WebView2');
  const endMarker = '\nSectionEnd\n';
  const end = upstream.indexOf(endMarker, start);
  if (end === -1) throw new Error('у раздела Section WebView2 нет SectionEnd');
  const section = upstream.slice(start, end + endMarker.length);
  return { text: upstream.slice(0, start) + WEBVIEW2_REMOVED + upstream.slice(end + endMarker.length), section };
}

function replaceOnce(text: string, from: string, to: string, what: string): string {
  const first = text.indexOf(from);
  if (first === -1) throw new Error(`правка «${what}»: текст не найден`);
  if (text.indexOf(from, first + from.length) !== -1) throw new Error(`правка «${what}»: текст встречается больше одного раза`);
  return text.slice(0, first) + to + text.slice(first + from.length);
}

/** Шаблон Tauri → наш шаблон. */
export function applyPatches(upstream: string): string {
  let text = cutWebView2Section(upstream).text;
  for (const patch of PATCHES) text = replaceOnce(text, patch.upstream, patch.ours, patch.name);
  return text;
}

/**
 * Наш шаблон → шаблон Tauri без раздела WebView2 (его текст берётся из
 * шаблона Tauri в тесте). Каждая правка должна встречаться ровно один раз.
 */
export function revertPatches(ours: string): string {
  let text = ours;
  for (const patch of [...PATCHES].reverse()) text = replaceOnce(text, patch.ours, patch.upstream, patch.name);
  return text;
}
