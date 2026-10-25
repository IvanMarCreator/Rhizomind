import { mount } from 'svelte';
import { checkApiVersion } from './api/index.ts';
import App from './App.svelte';
import './theme.css';

const target = document.getElementById('app');
if (!target) throw new Error('Нет элемента #app');

// Первый вызов ядра — сверка версии API (С5.5). Что показать пользователю при
// несовпадении, решает оболочка окна (задача 2.2); пока — запись в консоль.
checkApiVersion().catch((error: unknown) => console.error(error));

export default mount(App, { target });
