//! Раскладка хранилища: дерево папок, имена заметок и вложений.
//! Всё решается до записи на диск, чтобы ссылки могли вести на любую заметку.

use crate::names::{is_valid_windows_name, name_key};
use std::collections::{HashMap, HashSet};

/// Наибольшая глубина вложенности папок.
pub const MAX_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    Mixed,
}

/// Кодировка и переносы строк файла заметки.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub eol: Eol,
    pub bom: bool,
    pub final_newline: bool,
}

impl Style {
    pub const LF: Style = Style {
        eol: Eol::Lf,
        bom: false,
        final_newline: true,
    };
}

/// Особое содержимое заметки вместо обычного текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    None,
    AllLinks,
    CodeNotLinks,
    InvalidUtf8,
    Big,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub folder: usize,
    pub name: String,
    pub headings: Vec<String>,
    pub style: Style,
    pub special: Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Png,
    Pdf,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub folder: usize,
    pub file_name: String,
    pub kind: AttachmentKind,
}

#[derive(Default)]
pub struct Layout {
    /// Папки; индекс 0 — корень (пустой путь).
    pub folders: Vec<Vec<String>>,
    /// Заметки в порядке создания (порядок записи на диск).
    pub notes: Vec<Note>,
    /// Число образцов: они идут первыми в `notes`.
    pub samples: usize,
    pub attachments: Vec<Attachment>,
    folder_index: HashMap<Vec<String>, usize>,
    /// Занятые имена в папке: (папка, ключ имени с расширением).
    siblings: HashSet<(usize, String)>,
    /// Ключ имени заметки (без `.md`) → заметки с этим именем.
    pub by_key: HashMap<String, Vec<usize>>,
}

impl Layout {
    pub fn new() -> Self {
        let mut layout = Layout::default();
        layout.folders.push(Vec::new());
        layout.folder_index.insert(Vec::new(), 0);
        layout
    }

    pub fn depth(&self, folder: usize) -> usize {
        self.folders[folder].len()
    }

    /// Папка по пути из корня; недостающие папки создаются.
    pub fn ensure_folder(&mut self, parts: &[&str]) -> usize {
        let mut current = 0;
        for i in 0..parts.len() {
            let path: Vec<String> = parts[..=i].iter().map(|s| s.to_string()).collect();
            current = match self.folder_index.get(&path) {
                Some(&idx) => idx,
                None => self
                    .add_folder(current, parts[i])
                    .expect("имя папки занято"),
            };
        }
        current
    }

    /// Новая папка в `parent`, если имя свободно и допустимо.
    pub fn add_folder(&mut self, parent: usize, name: &str) -> Option<usize> {
        assert!(
            is_valid_windows_name(name),
            "недопустимое имя папки {name:?}"
        );
        if self.depth(parent) >= MAX_DEPTH || !self.siblings.insert((parent, name_key(name))) {
            return None;
        }
        let mut path = self.folders[parent].clone();
        path.push(name.to_string());
        let idx = self.folders.len();
        self.folders.push(path.clone());
        self.folder_index.insert(path, idx);
        Some(idx)
    }

    /// Свободно ли имя заметки `name` (без `.md`) в папке.
    pub fn note_name_free(&self, folder: usize, name: &str) -> bool {
        !self
            .siblings
            .contains(&(folder, name_key(&format!("{name}.md"))))
    }

    pub fn add_note(
        &mut self,
        folder: usize,
        name: &str,
        headings: Vec<String>,
        style: Style,
        special: Special,
    ) -> usize {
        assert!(
            is_valid_windows_name(&format!("{name}.md")),
            "недопустимое имя заметки {name:?}"
        );
        assert!(
            self.siblings
                .insert((folder, name_key(&format!("{name}.md")))),
            "имя {name:?} занято"
        );
        let idx = self.notes.len();
        self.notes.push(Note {
            folder,
            name: name.to_string(),
            headings,
            style,
            special,
        });
        self.by_key.entry(name_key(name)).or_default().push(idx);
        idx
    }

    pub fn add_attachment(&mut self, folder: usize, file_name: &str, kind: AttachmentKind) -> bool {
        assert!(
            is_valid_windows_name(file_name),
            "недопустимое имя вложения {file_name:?}"
        );
        if !self.siblings.insert((folder, name_key(file_name))) {
            return false;
        }
        self.attachments.push(Attachment {
            folder,
            file_name: file_name.to_string(),
            kind,
        });
        true
    }

    /// Путь заметки от корня через `/`, с `.md`.
    pub fn note_path(&self, idx: usize) -> String {
        let note = &self.notes[idx];
        join(&self.folders[note.folder], &format!("{}.md", note.name))
    }

    /// Путь заметки без `.md` — в виде, как он пишется в ссылке.
    pub fn note_link_path(&self, idx: usize) -> Vec<String> {
        let note = &self.notes[idx];
        let mut parts = self.folders[note.folder].clone();
        parts.push(note.name.clone());
        parts
    }

    pub fn attachment_path(&self, idx: usize) -> String {
        let a = &self.attachments[idx];
        join(&self.folders[a.folder], &a.file_name)
    }
}

pub fn join(folder: &[String], file: &str) -> String {
    if folder.is_empty() {
        file.to_string()
    } else {
        format!("{}/{}", folder.join("/"), file)
    }
}
