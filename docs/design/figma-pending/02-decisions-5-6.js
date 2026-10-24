// use_figma, file zBjvlOkAkVydfuwgpK5STx, page «Решения» (first page). Helper library stored in file.
const d4 = await figma.getNodeByIdAsync('6:3581');
const res = [];
for (const f of d4.findAll(n => n.name === 'Фрагмент: текст заметки')) { f.resize(960, 800); res.push(f.id); }
const hex = h => ({r:parseInt(h.slice(1,3),16)/255,g:parseInt(h.slice(3,5),16)/255,b:parseInt(h.slice(5,7),16)/255});
const cols = await figma.variables.getLocalVariableCollectionsAsync();
const vars = await figma.variables.getLocalVariablesAsync('COLOR');
const tune = {'link/unresolved':['#7A818B','#7A818A'], 'text/faint':['#8E959E','#70767E']};
for (const v of vars) if (tune[v.name]) { const c = cols.find(c=>c.id===v.variableCollectionId); const L = c.name.endsWith('Light'); v.setValueForMode(c.modes[0].modeId, hex(tune[v.name][L?0:1])); }
const RZ = await eval(figma.root.getSharedPluginData('rzm','lib'));
const linkFrag = (links) => t => {
  const ctx = {t, acc:'indigo'};
  const f = RZ.AL(ctx,'VERTICAL',{name:'Фрагмент: ссылки', w:640, pad:28, gap:18, bg:'bg/app', stroke:'border/strong', radius:6});
  const rows = [
    ['Разрешённая вики-ссылка  [[Встреча]]', ['Решение зафиксировано в ', ['Встреча','wl'], ', см. также ', ['Регламент доступа','wl'], '.']],
    ['Неразрешённая вики-ссылка (заметки ещё нет)', ['Инструкцию готовит Д. Ким: ', ['Инструкция пользователя','wu'], '.']],
    ['Обычная ссылка  [текст](http://…)', ['Процесс описан на ', ['портале регламентов','ext'], [' ↗','exti'], '.']],
  ];
  for (const [lab, segs] of rows) {
    const r = RZ.AL(ctx,'VERTICAL',{name:lab, gap:4}); RZ.add(f,r,{fw:true});
    RZ.add(r, RZ.T(ctx, lab, {size:11, w:'m', color:'text/muted'}));
    RZ.add(r, RZ.R(ctx, segs, {font:'IBM Plex Sans', size:16, lh:26, links}));
  }
  const r = RZ.AL(ctx,'VERTICAL',{name:'Строка с курсором', gap:4}); RZ.add(f,r,{fw:true});
  RZ.add(r, RZ.T(ctx, 'Строка с курсором — видна исходная разметка', {size:11, w:'m', color:'text/muted'}));
  const raw = RZ.AL(ctx,'HORIZONTAL',{ai:'CENTER', gap:1}); RZ.add(r, raw);
  RZ.add(raw, RZ.R(ctx, ['Сверить итоги с ', ['[[','mk'], 'Альфа/Встреча', [']]','mk'], ' и ', ['[','mk'], 'порталом', ['](http://portal.corp.local/alpha)','mk']], {font:'IBM Plex Sans', size:16, lh:26, links}));
  const car = figma.createRectangle(); car.resize(1.5, 21); car.fills=[RZ.paint(ctx,'A:base')]; raw.appendChild(car);
  return f;
};
const d5 = RZ.decision('5','Вид ссылок',
 'Как различаются разрешённые вики-ссылки (заметка существует, показывается её имя), неразрешённые (заметки нет — серые, приглушённые) и обычные ссылки. Строка с курсором во всех вариантах одинакова: показывает исходную разметку.',
 [
  RZ.variant('5А','Акцентный цвет без подчёркивания','Вики-ссылка — акцентный цвет, без линии (спокойнее в тексте с большим числом связей). Неразрешённая — серая с точечным подчёркиванием. Обычная — акцент + подчёркивание + стрелка ↗ «уходит наружу».', linkFrag('a')),
  RZ.variant('5Б','Цвет текста с акцентной линией','Вики-ссылка набрана цветом текста и выделена только акцентным подчёркиванием — текст читается ровно, связь заметна. Неразрешённая — просто серая. Обычная — акцентный цвет без линии, со стрелкой ↗.', linkFrag('b')),
  RZ.variant('5В','Классические подчёркнутые ссылки','Вики-ссылка и обычная ссылка выглядят как веб-ссылки (акцент + подчёркивание), обычную отличает только стрелка ↗. Неразрешённая — серая курсивом. Привычно, но текст с большим числом связей выглядит пёстро.', linkFrag('c')),
 ]);
RZ.place(d5);
const accFrag = (acc) => t => {
  const ctx = {t, acc};
  const f = RZ.AL(ctx,'VERTICAL',{name:'Фрагмент: акцент', gap:10});
  const sw = RZ.AL(ctx,'HORIZONTAL',{name:'Образцы', gap:8}); RZ.add(f, sw);
  for (const [k, lab] of [['A:base','основной'],['A:text','текст / ссылки'],['A:subtle','фон выделения']]) {
    const c = RZ.AL(ctx,'HORIZONTAL',{name:lab, gap:6, ai:'CENTER', pad:[4,10,4,4], radius:6, stroke:'border/default', bg:'bg/app'}); RZ.add(sw,c);
    const s = RZ.AL(ctx,'HORIZONTAL',{w:20,h:20,radius:4,bg:k}); RZ.add(c,s);
    RZ.add(c, RZ.T(ctx, lab, {size:11, color:'text/secondary'}));
  }
  RZ.add(f, RZ.win(ctx, {W:820, H:430, layout:'b', header:'b', tree:'a', typo:'a', links:'a', name:'Фрагмент окна'}));
  return f;
};
const d6 = RZ.decision('6','Акцентный цвет',
 'Акцент используется скупо: выбранная заметка в дереве, ссылки, отмеченные чекбоксы, курсор, метка хранилища. Для каждого акцента — три значения (основной, текстовый, фон выделения), подобранные отдельно для светлой и тёмной темы.',
 [
  RZ.variant('6А','Индиго','Спокойный сине-фиолетовый. Не спорит со статусными цветами (зелёный «Сохранено», будущие предупреждения), хорошо читается как ссылка в обеих темах. Светлая: #4A55C8, тёмная: #8E97F0.', accFrag('indigo')),
  RZ.variant('6Б','Морская волна','Сине-зелёный, «тихий» и деловой. Ближе к зелёному статусу «Сохранено», поэтому статус и акцент могут путаться. Светлая: #0E7A74, тёмная: #4CC1B6.', accFrag('teal')),
  RZ.variant('6В','Медь','Тёплый, «бумажный» акцент, выделяет продукт среди синих корпоративных программ. Ссылки меньше похожи на привычные ссылки. Светлая: #A9592A, тёмная: #E0955F.', accFrag('copper')),
 ]);
RZ.place(d6);
return { res, d5: {id:d5.id, v:d5.children.slice(2).map(c=>c.id)}, d6:{id:d6.id, v:d6.children.slice(2).map(c=>c.id)} };
