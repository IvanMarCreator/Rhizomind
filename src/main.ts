import { mount } from 'svelte';
import App from './App.svelte';
import { blockExternalDrops } from './shell/externalDrop.ts';
import './theme/fonts.css';
import './theme/tokens.css';
import './shell/base.css';

// Файлы и текст извне в окно не бросаются (контур хранилища).
blockExternalDrops(document);

const target = document.getElementById('app');
if (!target) throw new Error('Нет элемента #app');

// Запуск окна (сверка версии API, настройки, раскладка) — в оболочке:
// src/shell/window.svelte.ts.
export default mount(App, { target });
