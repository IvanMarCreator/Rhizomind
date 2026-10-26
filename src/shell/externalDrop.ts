// Броски извне в окно запрещены (контур хранилища, исправление КТ1): файлы и
// текст, перетащенные из Проводника или другой программы, страница не
// принимает — ни дерево, ни редактор (CodeMirror умеет вставлять содержимое
// брошенного файла), ни движок (брошенный файл он открыл бы как адрес
// `file://`; такой переход и так запрещён ядром).
//
// Настройкой движка это сделать нельзя: у WebView2 в оконном режиме
// `AllowExternalDrop = false` выключает и перетаскивание внутри страницы
// (проба CI на Windows: dragstart → dragend:none). Поэтому граница здесь:
// - файлы ОС (`types` содержит «Files») не принимаются никогда — своё
//   перетаскивание страницы файлов не несёт;
// - остальное принимается, только если перетаскивание начато на этой
//   странице (был `dragstart`); иначе над окном курсор «запрещено», бросок
//   гасится до любых обработчиков страницы.
//
// Флаг своего перетаскивания не должен застревать: источник могут убрать из
// документа до `dragend` (CodeMirror удаляет перенесённый текст, дерево
// перерисовывается), и `dragend` до документа не дойдёт. Поэтому флаг
// снимается и после броска (в следующей задаче), и по любому движению или
// нажатию мыши — во время перетаскивания страница их не получает.

/** Ставит запрет на документ; возвращает снятие (для тестов). */
export function blockExternalDrops(doc: Document): () => void {
  /** Идёт перетаскивание, начатое на этой странице. */
  let internal = false;

  const carriesFiles = (event: DragEvent) => Array.from(event.dataTransfer?.types ?? []).includes('Files');
  const refuse = (event: DragEvent) => !internal || carriesFiles(event);

  const onDragStart = () => {
    internal = true;
  };
  const reset = () => {
    internal = false;
  };
  const onDragOver = (event: DragEvent) => {
    if (!refuse(event)) return;
    event.stopPropagation();
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'none';
  };
  const onDrop = (event: DragEvent) => {
    const refused = refuse(event);
    // После броска перетаскивание закончено, даже если dragend не придёт.
    setTimeout(reset, 0);
    if (!refused) return;
    event.stopPropagation();
    event.preventDefault();
  };

  const options = { capture: true } as const;
  const listeners: [string, (event: never) => void][] = [
    ['dragstart', onDragStart],
    ['dragend', reset],
    ['pointerdown', reset],
    ['pointermove', reset],
    ['mousemove', reset],
    ['dragenter', onDragOver],
    ['dragover', onDragOver],
    ['drop', onDrop],
  ];
  for (const [type, listener] of listeners) doc.addEventListener(type, listener as EventListener, options);
  return () => {
    for (const [type, listener] of listeners) doc.removeEventListener(type, listener as EventListener, options);
  };
}
