import { describe, expect, it } from 'vitest';
import { scanSource } from './scan.ts';

const kinds = (text: string, file = 'a.ts') => scanSource(text, file).map((f) => f.what);

describe('scanSource: сетевой код в TypeScript', () => {
  it('находит вызов fetch со строкой и номером строки', () => {
    expect(scanSource('const a = 1;\nfetch("https://x.test");\n', 'a.ts')).toEqual([
      { line: 2, what: 'fetch', severity: 'network' },
    ]);
  });

  it('не срабатывает на слова в строках и комментариях', () => {
    expect(kinds('// fetch(url)\nconst s = "new WebSocket(x)";\n/* XMLHttpRequest */')).toEqual([]);
  });

  it.each([
    ['fetch(u)', 'fetch'],
    ['new Request(u)', 'Request'],
    ['new XMLHttpRequest()', 'XMLHttpRequest'],
    ['new WebSocket(u)', 'WebSocket'],
    ['new EventSource(u)', 'EventSource'],
    ['new RTCPeerConnection()', 'RTCPeerConnection'],
    ['new WebTransport(u)', 'WebTransport'],
    ['importScripts(u)', 'importScripts'],
  ])('находит глобал %s', (code, what) => {
    expect(kinds(code)).toEqual([what]);
  });

  it.each([
    ['window.fetch(u)', 'fetch'],
    ['globalThis.WebSocket', 'WebSocket'],
    ['self.XMLHttpRequest', 'XMLHttpRequest'],
    ['navigator.sendBeacon(u, d)', 'sendBeacon'],
  ])('находит обращение через объект %s', (code, what) => {
    expect(kinds(code)).toEqual([what]);
  });

  it('деструктуризация из window — и сетевое имя, и подозрительный объект', () => {
    expect(kinds('const { fetch: f } = window')).toEqual(['fetch', 'глобальный объект window без свойства']);
  });
});

describe('scanSource: переходы, окна и фоновые потоки', () => {
  it.each([
    ['window.open(u)', 'window.open'],
    ['open(u)', 'window.open'],
    ['globalThis.open(u, "_blank")', 'window.open'],
    ['location = u', 'присваивание location'],
    ['window.location = u', 'присваивание location'],
    ['document.location.href = u', 'присваивание location'],
    ['location.href += "x"', 'присваивание location'],
    ['location.assign(u)', 'location.assign'],
    ['window.location.replace(u)', 'location.replace'],
    ['new Worker(u)', 'Worker'],
    ['new SharedWorker(u)', 'SharedWorker'],
    ['navigator.serviceWorker.register(u)', 'serviceWorker'],
  ])('находит %s', (code, what) => {
    expect(kinds(code)).toEqual([what]);
  });

  it('не трогает чтение location и свой метод open', () => {
    expect(kinds('const h = location.hash;\ndialog.open();\nconst o = { open: 1 };')).toEqual([]);
  });
});

describe('scanSource: вычисляемые обращения — подозрительно', () => {
  it.each([
    ["window['fe' + 'tch'](u)", 'window[<вычисляемое>]'],
    ['globalThis[name]', 'globalThis[<вычисляемое>]'],
    ['self[`f${x}`]', 'self[<вычисляемое>]'],
    ['eval(code)', 'eval'],
    ['new Function(code)', 'Function'],
    ['const w = window;\nw[x]', 'w[<вычисляемое>]'],
    ['let w; w = self; w[x]', 'w[<вычисляемое>]'],
    ['use(globalThis)', 'глобальный объект globalThis без свойства'],
  ])('%s', (code, what) => {
    expect(scanSource(code, 'a.ts')).toEqual([{ line: code.split('\n').length, what, severity: 'suspicious' }]);
  });

  it('псевдоним глобального объекта проверяется так же, как сам объект', () => {
    expect(kinds('const w = window;\nw.open(u);\nw.addEventListener("x", f);')).toEqual(['window.open']);
  });

  it('сравнение с глобальным объектом не подозрительно', () => {
    expect(kinds('if (t === window || t !== self) {}')).toEqual([]);
  });

  it('обращение через скобки к сетевому имени строкой — сеть', () => {
    expect(scanSource("window['fetch'](u)", 'a.ts')).toEqual([{ line: 1, what: 'fetch', severity: 'network' }]);
  });

  it('обычные обращения к глобальному объекту не подозрительны', () => {
    expect(kinds("window.addEventListener('x', f);\nwindow['matchMedia']('(x)');\ntypeof window;")).toEqual([]);
  });
});

describe('scanSource: сетевые модули', () => {
  it.each([
    ['import http from "http";', 'import http'],
    ['import * as n from "node:net";', 'import node:net'],
    ['import { request } from "node:https";', 'import node:https'],
    ['import "tls";', 'import tls'],
    ['import type { Agent } from "undici";', 'import undici'],
    ['export { get } from "node:http2";', 'import node:http2'],
    ['export * from "dgram";', 'import dgram'],
    ['const d = require("dns");', 'import dns'],
    ['const w = await import("ws");', 'import ws'],
    ['import { fetch as f } from "@tauri-apps/plugin-http";', 'import @tauri-apps/plugin-http'],
    ['import { openUrl } from "@tauri-apps/plugin-opener";', 'import @tauri-apps/plugin-opener'],
  ])('находит %s', (code, what) => {
    expect(kinds(code)).toContain(what);
  });

  it('не трогает обычные модули', () => {
    expect(kinds('import { mount } from "svelte";\nimport x from "./http.ts";')).toEqual([]);
  });

  it('динамический import() с внешним адресом — сеть', () => {
    expect(scanSource('await import("https://cdn.test/x.js");', 'a.ts')).toEqual([
      { line: 1, what: 'import https://cdn.test/x.js', severity: 'network' },
    ]);
  });

  it('динамический import() и require() с вычисляемым именем — подозрительно', () => {
    expect(scanSource('await import(name);\nrequire("ht" + "tp");', 'a.ts')).toEqual([
      { line: 1, what: 'import(<вычисляемое>)', severity: 'suspicious' },
      { line: 2, what: 'require(<вычисляемое>)', severity: 'suspicious' },
    ]);
  });
});

describe('scanSource: Svelte и HTML', () => {
  it('находит сеть в <script lang="ts"> с правильной строкой', () => {
    const svelte = '<script lang="ts">\n  let n: number = 1;\n  fetch("/x");\n</script>\n<h1>{n}</h1>\n';
    expect(scanSource(svelte, 'App.svelte')).toEqual([{ line: 3, what: 'fetch', severity: 'network' }]);
  });

  it('находит сеть в выражениях разметки', () => {
    const svelte = '<button onclick={() => new WebSocket(u)}>x</button>\n';
    expect(kinds(svelte, 'B.svelte')).toEqual(['WebSocket']);
  });

  it('находит сеть в <script module>', () => {
    expect(kinds('<script module>\n  export const x = navigator.sendBeacon;\n</script>', 'M.svelte')).toEqual([
      'sendBeacon',
    ]);
  });

  it.each([
    ['iframe', '<iframe></iframe>'],
    ['object', '<object></object>'],
    ['embed', '<embed>'],
    ['script', '<script></script>'],
    ['link', '<link>'],
    ['form', '<form></form>'],
    ['base', '<base>'],
    ['meta', '<meta>'],
  ])(
    'элемент <%s> в разметке — подозрительно',
    (tag, markup) => {
      expect(scanSource(`<div>\n${markup}\n</div>`, 'E.svelte')).toEqual([
        { line: 2, what: `элемент <${tag}>`, severity: 'suspicious' },
      ]);
    },
  );

  it('текст разметки и стили не считаются кодом', () => {
    const svelte = '<p>fetch(url) и new WebSocket</p>\n<style>p { color: red; }</style>\n';
    expect(kinds(svelte, 'T.svelte')).toEqual([]);
  });

  it('в HTML находит внешний адрес в src и href', () => {
    const html = '<!doctype html>\n<link rel="icon" href="/x.svg">\n<script src="https://cdn.test/a.js"></script>\n';
    expect(scanSource(html, 'index.html')).toEqual([
      { line: 3, what: 'внешний адрес https://cdn.test/a.js', severity: 'network' },
    ]);
  });

  it('в HTML проверяет код встроенных скриптов', () => {
    expect(kinds('<html><script>\nfetch(u)\n</script></html>', 'index.html')).toEqual(['fetch']);
  });
});

describe('scanSource: разметка Svelte — внешние адреса и загрузки (ревью 0.2, п. 10)', () => {
  it.each([
    ['<a href="https://x.test/">x</a>', 'внешний адрес https://x.test/'],
    ['<img src="//cdn.test/a.png" alt="">', 'внешний адрес //cdn.test/a.png'],
    ['<button formaction="https://x.test/send">x</button>', 'внешний адрес https://x.test/send'],
    ['<div action="wss://x.test/s"></div>', 'внешний адрес wss://x.test/s'],
  ])('%s — сеть', (markup, what) => {
    expect(scanSource(`<div>\n${markup}\n</div>`, 'L.svelte')).toEqual([{ line: 2, what, severity: 'network' }]);
  });

  it('атрибут download — сеть (загрузка файла)', () => {
    expect(scanSource('<a href="/x" download>x</a>', 'D.svelte')).toEqual([
      { line: 1, what: 'атрибут download', severity: 'network' },
    ]);
  });

  it('<svelte:element> — подозрительно (имя элемента вычисляется)', () => {
    expect(scanSource('<svelte:element this={tag}></svelte:element>', 'S.svelte')).toEqual([
      { line: 1, what: 'элемент <svelte:element>', severity: 'suspicious' },
    ]);
  });

  it('свои адреса в атрибутах не трогает', () => {
    expect(kinds('<a href="/notes/x">x</a>\n<img src="./a.png" alt="">', 'O.svelte')).toEqual([]);
  });
});

describe('scanSource: обходы через глобальный объект и строки (ревью 0.2, п. 10)', () => {
  it.each([
    ['document.defaultView.fetch(u)', 'fetch', 'network'],
    ['document.defaultView[x]', 'document.defaultView[<вычисляемое>]', 'suspicious'],
    ['const w = document.defaultView;\nw[x]', 'w[<вычисляемое>]', 'suspicious'],
    ['navigator[x](u)', 'navigator[<вычисляемое>]', 'suspicious'],
    ['setTimeout("fetch(u)", 1)', 'setTimeout(<строка>)', 'suspicious'],
    ['setInterval(`x`, 1)', 'setInterval(<строка>)', 'suspicious'],
    ['window.setTimeout("x" + y)', 'setTimeout(<строка>)', 'suspicious'],
    ['document.createElement("script")', 'createElement(script)', 'suspicious'],
    ['document.createElement("IFRAME")', 'createElement(iframe)', 'suspicious'],
    ['new Image().src = u', 'Image', 'suspicious'],
    ['new Audio(u)', 'Audio', 'suspicious'],
    ['form.submit()', 'form.submit', 'network'],
    ['el.requestSubmit()', 'form.submit', 'network'],
  ])('%s', (code, what, severity) => {
    expect(scanSource(code, 'a.ts')).toEqual([{ line: code.split('\n').length, what, severity }]);
  });

  it('обычные таймеры и элементы не трогает', () => {
    expect(kinds('setTimeout(() => f(), 1);\nsetInterval(tick, 5);\ndocument.createElement("div");')).toEqual([]);
    // Вычисляемое имя элемента — так работает сам Svelte (create_element).
    expect(kinds('document.createElement(tag)')).toEqual([]);
  });

  it('проверяет и .tsx', () => {
    expect(kinds('const a = <div onClick={() => fetch(u)} />;', 'A.tsx')).toEqual(['fetch']);
  });
});
