// use_figma, file zBjvlOkAkVydfuwgpK5STx. Builds 3 concepts on page «Концепты».
// Helper library is stored in the file: figma.root.getSharedPluginData('rzm','lib')
const page = figma.root.children.find(p => p.name === 'Концепты');
await figma.setCurrentPageAsync(page);
const RZ = await eval(figma.root.getSharedPluginData('rzm','lib'));
const C = [
 ['К1','Спокойный (рекомендуемый)', {layout:'b',header:'a',tree:'a',typo:'a',links:'a',acc:'indigo'}, 'Решения: 1Б · 2А · 3А · 4А · 5А · 6А. Без ленты, тихая строка над заметкой, значки в дереве, IBM Plex Sans в колонке ~70 знаков, индиго.'],
 ['К2','Близко к Obsidian', {layout:'a',header:'b',tree:'b',typo:'c',links:'b',acc:'teal'}, 'Решения: 1А · 2Б · 3Б · 4В · 5Б · 6Б. Лента иконок, путь к заметке и сегментный переключатель режима, дерево без значков, Inter, морская волна.'],
 ['К3','Явный и плотный', {layout:'c',header:'c',tree:'c',typo:'b',links:'c',acc:'copper'}, 'Решения: 1В · 2В · 3В · 4Б · 5В · 6В. Хранилище вверху панели, кнопка режима с подписью, компактное дерево, текст во всю ширину, медь.'],
];
const out = [];
for (const [code, title, combo, desc] of C) {
  const v = RZ.variant(code, title, desc, t => RZ.win({t, acc: combo.acc}, Object.assign({}, combo, {W:1280, H:800, titlebar:true, hoverFolder: t === 'L', name: code + ' · главное окно'})));
  RZ.place(v, 160);
  out.push({id: v.id, name: v.name});
}
return out;
