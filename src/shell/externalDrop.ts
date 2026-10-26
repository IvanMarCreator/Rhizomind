// Броски извне в окно запрещены (контур хранилища, исправление КТ1): файлы и
// текст, перетащенные из Проводника или другой программы, страница не
// принимает — ни дерево, ни редактор (CodeMirror умеет вставлять содержимое
// брошенного файла), ни движок (брошенный файл он открыл бы как адрес
// `file://`; такой переход и так запрещён ядром).
//
// Настройкой движка это сделать нельзя: у WebView2 в оконном режиме
// `AllowExternalDrop = false` выключает и перетаскивание внутри страницы
// (проба CI на Windows: dragstart → dragend:none). Поэтому граница здесь:
// перетаскивание, начатое не на этой странице (не было `dragstart`), не
// принимается — над окном курсор «запрещено», бросок гасится до любых
// обработчиков страницы.

/** Ставит запрет на документ; возвращает снятие (для тестов). */
export function blockExternalDrops(doc: Document): () => void {
  /** Идёт перетаскивание, начатое на этой странице. */
  let internal = false;

  const onDragStart = () => {
    internal = true;
  };
  const onDragEnd = () => {
    internal = false;
  };
  const onDragOver = (event: DragEvent) => {
    if (internal) return;
    event.stopPropagation();
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'none';
  };
  const onDrop = (event: DragEvent) => {
    if (internal) return;
    event.stopPropagation();
    event.preventDefault();
  };

  const options = { capture: true } as const;
  doc.addEventListener('dragstart', onDragStart, options);
  doc.addEventListener('dragend', onDragEnd, options);
  doc.addEventListener('dragenter', onDragOver, options);
  doc.addEventListener('dragover', onDragOver, options);
  doc.addEventListener('drop', onDrop, options);
  return () => {
    doc.removeEventListener('dragstart', onDragStart, options);
    doc.removeEventListener('dragend', onDragEnd, options);
    doc.removeEventListener('dragenter', onDragOver, options);
    doc.removeEventListener('dragover', onDragOver, options);
    doc.removeEventListener('drop', onDrop, options);
  };
}
