/**
 * Перетаскивание строки дерева событиями HTML5 (dragstart → dragover → drop):
 * действия WebDriver не порождают событий перетаскивания HTML5 ни в
 * msedgedriver, ни в WebKitWebDriver.
 */
export async function dragRow(browser: WebdriverIO.Browser, from: string, to: string): Promise<void> {
  await browser.execute(
    (source, target) => {
      const find = (name: string) => {
        const element = document.querySelector(`[role="treeitem"][aria-label="${name}"]`);
        if (!element) throw new Error(`нет строки ${name}`);
        return element;
      };
      const data = new DataTransfer();
      const fire = (element: Element, type: string) =>
        element.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: data }));
      const [start, end] = [find(source), find(target)];
      fire(start, 'dragstart');
      fire(end, 'dragenter');
      fire(end, 'dragover');
      fire(end, 'drop');
      fire(start, 'dragend');
    },
    from,
    to,
  );
}
