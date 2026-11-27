//! Текстовый запрос и обновление поисковых таблиц.
use frostem::{Algorithm, Stemmer};
use rusqlite::{Connection, OptionalExtension, Result, params};
use unicode_normalization::UnicodeNormalization;

use super::NoteEntry;
use crate::{commands::VaultPath, names};

pub(crate) struct Document {
    pub entry: NoteEntry,
    pub words: String,
    pub hash: String,
}

pub(super) fn words(text: &str) -> String {
    let stemmer = Stemmer::new(Algorithm::Russian);
    let folded = text.nfc().flat_map(char::to_lowercase).collect::<String>();
    let mut result = String::with_capacity(folded.len());
    for word in folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&stemmer.stem(word));
    }
    result
}

fn title(path: &VaultPath) -> &str {
    path.file_name()
        .and_then(names::note_stem)
        .unwrap_or_default()
}

pub(super) fn store_document(db: &Connection, path: &VaultPath, doc: &Document) -> Result<bool> {
    let name = title(path);
    let path = path.components().join("/");
    let old: Option<(i64, String)> = db
        .prepare_cached("SELECT id,hash FROM documents WHERE path=?1")?
        .query_row([&path], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    if old.as_ref().is_some_and(|(_, hash)| hash == &doc.hash) {
        return Ok(false);
    }
    let id = if let Some((id, _)) = old {
        db.prepare_cached("UPDATE documents SET hash=?2 WHERE id=?1")?
            .execute(params![id, doc.hash])?;
        id
    } else {
        db.prepare_cached("INSERT INTO documents(path,title,hash) VALUES(?1,?2,?3)")?
            .execute(params![path, name.to_lowercase(), doc.hash])?;
        let id = db.last_insert_rowid();
        db.prepare_cached("INSERT INTO titles(rowid,words) VALUES(?1,?2)")?
            .execute(params![id, words(name)])?;
        id
    };
    db.prepare_cached("INSERT OR REPLACE INTO bodies(rowid,words) VALUES(?1,?2)")?
        .execute(params![id, doc.words])?;
    Ok(true)
}

pub(super) fn remove(db: &Connection, path: &VaultPath) -> Result<()> {
    let path = path.components().join("/");
    if let Some(id) = db
        .prepare_cached("DELETE FROM documents WHERE path=?1 RETURNING id")?
        .query_row([path], |r| r.get::<_, i64>(0))
        .optional()?
    {
        db.prepare_cached("DELETE FROM titles WHERE rowid=?1")?
            .execute([id])?;
        db.prepare_cached("DELETE FROM bodies WHERE rowid=?1")?
            .execute([id])?;
    }
    Ok(())
}

pub(super) fn rename(db: &Connection, from: &VaultPath, to: &VaultPath) -> Result<()> {
    if from == to {
        return Ok(());
    }
    remove(db, to)?;
    let title = title(to);
    let id = db
        .prepare_cached("UPDATE documents SET path=?1,title=?2 WHERE path=?3 RETURNING id")?
        .query_row(
            params![
                to.components().join("/"),
                title.to_lowercase(),
                from.components().join("/")
            ],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(id) = id {
        db.prepare_cached("INSERT OR REPLACE INTO titles(rowid,words) VALUES(?1,?2)")?
            .execute(params![id, words(title)])?;
    }
    Ok(())
}
