//! Кратчайший различающий путь (спецификация С3.4).

use crate::layout::Layout;
use crate::names::name_key;

/// Текст ссылки (без скобок) на заметку `idx`: имя, если оно уникально;
/// иначе столько папок с конца пути, сколько нужно для однозначности; если
/// путь заметки целиком совпадает с концом пути другой — путь от корня с `/`.
pub fn shortest_link(l: &Layout, idx: usize) -> String {
    let path = l.note_link_path(idx);
    let group = &l.by_key[&name_key(path.last().unwrap())];
    if group.len() == 1 {
        return path.last().unwrap().clone();
    }
    let keyed = |p: &[String]| p.iter().map(|s| name_key(s)).collect::<Vec<_>>();
    let me = keyed(&path);
    let others: Vec<Vec<String>> = group
        .iter()
        .filter(|&&i| i != idx)
        .map(|&i| keyed(&l.note_link_path(i)))
        .collect();
    for k in 2..=me.len() {
        let suffix = &me[me.len() - k..];
        if !others.iter().any(|o| o.ends_with(suffix)) {
            return path[path.len() - k..].join("/");
        }
    }
    format!("/{}", path.join("/"))
}
