//! SQLite обращается только к оперативной памяти. Проверенный образ передаётся
//! между SQLite и файловым API хранилища, как и другие атомарные служебные записи.
use std::sync::OnceLock;

use rusqlite::{Connection, MAIN_DB, Result, config::DbConfig};

const SCHEMA: &str = "
CREATE TABLE link_index(id INTEGER PRIMARY KEY CHECK(id=1), snapshot BLOB NOT NULL);
CREATE TABLE documents(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, title TEXT NOT NULL, hash TEXT NOT NULL);
CREATE VIRTUAL TABLE bodies USING fts5(words, content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 0');
CREATE VIRTUAL TABLE titles USING fts5(words, content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 0');
";

fn connection() -> Result<Connection> {
    let db = Connection::open_in_memory()?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA temp_store=MEMORY")?;
    Ok(db)
}

pub(super) fn new_database() -> Result<Connection> {
    let db = connection()?;
    db.execute_batch(SCHEMA)?;
    Ok(db)
}

type Schema = Vec<(String, String, Option<String>)>;

fn schema(db: &Connection) -> Result<Schema> {
    db.prepare("SELECT type,name,sql FROM main.sqlite_schema ORDER BY name,type")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

pub(super) fn from_image(bytes: &[u8]) -> Result<Connection> {
    static EXPECTED: OnceLock<Schema> = OnceLock::new();
    let mut db = connection()?;
    db.deserialize_read_exact(MAIN_DB, bytes, bytes.len(), false)?;
    // Базу могли заменить, пока окно было закрыто. Разбираем только схему
    // сборки приложения, прежде чем обращаться к таблицам или начинать запись.
    if EXPECTED.get().is_none() {
        let _ = EXPECTED.set(schema(&new_database()?)?);
    }
    if Some(&schema(&db)?) != EXPECTED.get() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let check: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(db)
}

pub(super) fn snapshot(db: &mut Connection, links: &[u8], max_bytes: u64) -> Result<Vec<u8>> {
    db.execute("INSERT OR REPLACE INTO link_index VALUES(1,?1)", [links])?;
    let bytes: i64 = db.query_row(
        "SELECT page_count*page_size FROM pragma_page_count,pragma_page_size",
        [],
        |r| r.get(0),
    )?;
    if u64::try_from(bytes).unwrap_or(u64::MAX) > max_bytes {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_TOOBIG),
            None,
        ));
    }
    Ok(db.serialize(MAIN_DB)?.to_vec())
}

pub(super) fn links(db: &Connection) -> Result<Vec<u8>> {
    db.query_row("SELECT snapshot FROM link_index WHERE id=1", [], |r| {
        r.get(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_database_build_options_and_memory_only_temp() {
        let db = new_database().unwrap();
        for option in ["ENABLE_FTS5", "TEMP_STORE=3", "OMIT_LOAD_EXTENSION"] {
            assert!(
                db.query_row("SELECT sqlite_compileoption_used(?1)", [option], |r| r
                    .get::<_, bool>(0))
                    .unwrap(),
                "{option}"
            );
        }
        assert_eq!(
            db.query_row("PRAGMA temp_store", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
