import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (target === null) throw new Error('Нет элемента #app в index.html');

export default mount(App, { target });
