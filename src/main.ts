import { mount } from 'svelte';
import App from './App.svelte';
import { blockEngineContextMenu, blockEngineZoom } from './shell/engineShortcuts.ts';
import { blockExternalDrops } from './shell/externalDrop.ts';
import './theme/fonts.css';
import './theme/tokens.css';
import './shell/base.css';

// Файлы и текст извне в окно не бросаются (контур хранилища).
blockExternalDrops(document);
// Масштаб движка Ctrl+колесом не меняется (решение 94); сочетания клавиш
// движка гасит оболочка окна (src/shell/Shell.svelte).
blockEngineZoom(document);
// Меню движка не открывается; свои меню — дерево и правка текста (решение 97).
blockEngineContextMenu(document);

const target = document.getElementById('app');
if (!target) throw new Error('Нет элемента #app');

// Запуск окна (сверка версии API, настройки, раскладка) — в оболочке:
// src/shell/window.svelte.ts.
export default mount(App, { target });
