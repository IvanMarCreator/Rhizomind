import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createTestVault, removeTestVault, startApp, SCREENSHOT_DIR, type Session } from './driver.ts';
import { openFromTree, TEXT, themeShots } from './kt1.ts';

const fixtures = path.join(import.meta.dirname, '..', 'fixtures', 'embeds');
const shot = themeShots('embed');
const note = [
  '# Материалы проекта', '', '![[Диаграмма.png]]', '',
  '![[Встреча]]', '', '![[Каталог.pdf]]', '![[Страница.html]]', '![[Схема.svg]]',
  '![[Брак.png]]', '![[Утрачено]]', '![[Дубль.jpeg]]', '![[https://example.invalid/Запрос.png]]',
  '`![[Внутри-кода]]`', '```', '![[В-блоке.png]]', '```', '',
].join('\n');
const modal = '[role="dialog"].embed-dialog';
const names = (vault: string) => readdirSync(vault).filter(n=>!n.startsWith('.')).sort();

describe('embed — просмотр вложений', () => {
  let session: Session, vault: string;
  const measurements: Array<{ source: string; milliseconds: number; width: number; height: number }> = [];
  beforeAll(async () => {
    vault = createTestVault('Встраивания', { 'План.md': note, 'Встреча.md':'# Повестка\n**Итоги встречи**\n![[План]]\n![[Встреча]]',
      'Брак.png':'ошибочные байты', 'Каталог.pdf':'%PDF-1.0', 'Страница.html':'<b>PRIVATE_HTML_MARKER</b>', 'Схема.svg':'<svg>PRIVATE_SVG_MARKER</svg>' });
    copyFileSync(path.join(fixtures,'diagram-4k.png'),path.join(vault,'Диаграмма.png'));
    for (const folder of ['А','Б']) {
      mkdirSync(path.join(vault,folder));
      copyFileSync(path.join(fixtures,'sample.jpg'),path.join(vault,folder,'Дубль.jpeg'));
    }
    copyFileSync(path.join(fixtures,'sample.jpg'),path.join(vault,'Фото.JPEG'));
    writeFileSync(path.join(vault,'Фотографии.md'), '# Фотографии\n![[Фото.JPEG]]\n![[А/Дубль.jpeg]]\n');
    session = await startApp({ vault });
  });
  afterAll(async () => {
    await session?.stop();
    if (vault) removeTestVault(vault);
    writeFileSync(path.join(SCREENSHOT_DIR, 'embed-timing-' + process.platform + '.json'), JSON.stringify(measurements,null,2));
  });

  it('показывает в дереве 4K PNG и JPEG через проверенные пиксели, Esc возвращает фокус', async () => {
    const b = session.browser;
    for (const [stem,name,width,height] of [['Диаграмма','Диаграмма.png',1280,720], ['Фото','Фото.JPEG',2,1]] as const) {
      const entry = b.$(`[role="treeitem"][aria-label="${stem}"]`);
      await entry.waitForDisplayed({ timeout:30000 });
      const start = Date.now();
      await entry.click();
      const preview = b.$(modal);
      await preview.waitForDisplayed();
      await b.waitUntil(async () => b.execute((selector) => {
        const img = document.querySelector<HTMLImageElement>(selector);
        return Boolean(img?.complete && img.naturalWidth > 0);
      }, modal + ' img'), { timeout:30000 });
      const frame = await b.execute((selector) => {
        const img = document.querySelector<HTMLImageElement>(selector);
        return [img?.naturalWidth,img?.naturalHeight,img?.src.startsWith('data:image/png;base64,')];
      }, modal + ' img');
      expect(frame).toEqual([width,height,true]);
      measurements.push({source:'tree/'+name,milliseconds:Date.now()-start,width,height});
      if (width===1280) await shot(b,'tree-4k');
      await b.keys('Escape');
      await preview.waitForExist({ reverse:true });
      expect(await b.execute(() => document.activeElement?.getAttribute('role'))).toBe('tree');
    }
  });

  it('показывает первый уровень Markdown, ошибки и буквальную разметку без создания файлов', async () => {
    const b=session.browser, initial=names(vault);
    const start=Date.now();
    await openFromTree(b,'План');
    const imageSelector = TEXT + ' img[alt="Диаграмма.png"]';
    await b.$(imageSelector).waitForDisplayed({timeout:30000});
    await b.waitUntil(async () => b.execute((sel) => {
      const frame = document.querySelector<HTMLImageElement>(sel);
      return frame?.naturalWidth === 1280 && frame.naturalHeight === 720;
    }, imageSelector), {timeout:30000});
    measurements.push({source:'editor/Диаграмма.png',milliseconds:Date.now()-start,width:1280,height:720});
    const fragment=b.$(TEXT+' .rz-embed-note');
    await fragment.waitForDisplayed();
    expect(await fragment.getText()).toContain('Итоги встречи');
    expect(await fragment.$$('button').length).toBe(2);
    await shot(b,'live-4k-note');
    for (const [target,reason] of [['Схема.svg','SVG'],['Страница.html','HTML'],['Каталог.pdf','PDF'],['Брак.png','повреждён'],['Дубль.jpeg','Уточните путь'],['https://example.invalid/Запрос.png','Недопустимый путь']] as const) {
      const widget=b.$(`${TEXT} [data-rz-embed="${target}"]`);
      await widget.scrollIntoView();
      await b.waitUntil(async()=> (await widget.getText()).includes(reason),{timeout:15000});
    }
    const missing=b.$(TEXT+' [data-rz-embed="Утрачено"]');
    await missing.scrollIntoView();
    await b.waitUntil(async()=> (await missing.getText()).includes('Файл не найден'),{timeout:15000});
    await missing.click(); await missing.click();
    expect(existsSync(path.join(vault,'Утрачено.md'))).toBe(false);
    const body=await b.$(TEXT).getText();
    expect(body).not.toContain('PRIVATE_HTML_MARKER');
    expect(body).not.toContain('PRIVATE_SVG_MARKER');
    expect(body).toContain('![[В-блоке.png]]');
    expect(body).toContain('![[Внутри-кода]]');
    await b.keys(['Control','e']);
    await b.waitUntil(async()=> (await b.$(TEXT).getText()).includes('![[Диаграмма.png]]'));
    expect(await b.$(TEXT+' .rz-embed img').isExisting()).toBe(false);
    await b.keys(['Control','e']);
    expect(names(vault)).toEqual(initial);
    expect(readFileSync(path.join(vault,'План.md'),'utf8')).toBe(note);
  });

  it('различает путь JPEG в заметке', async () => {
    const b=session.browser;
    await openFromTree(b,'Фотографии');
    await b.$(TEXT+' img[alt="Фото.JPEG"]').waitForDisplayed({timeout:15000});
    await b.$(TEXT+' img[alt="Дубль.jpeg"]').waitForDisplayed();
  });
});
