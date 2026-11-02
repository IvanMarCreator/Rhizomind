//! Тесты файла индекса `.rhizomind/index.json` (задача 0.2-1.3): запись,
//! чтение, пересборка при сбое и смене версии, сверка при открытии, частота
//! записи и носитель только для чтения.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::commands::{CoreError, ErrorCode, IndexState};
use crate::index::{NoteEntry, rebuild};
use crate::markdown::{RULES_VERSION, parse};
use crate::vault::{ServiceRead, Vault};

fn vp(path: &str) -> VaultPath {
    VaultPath::from_components(path.split('/'))
}

/// Хранилище во временной папке.
struct TestVault {
    _dir: tempfile::TempDir,
    root: PathBuf,
    vault: Vault,
}

impl TestVault {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("временная папка");
        let root = dir.path().join("vault");
        std::fs::create_dir(&root).expect("корень");
        let vault = Vault::open(&root).expect("хранилище");
        Self {
            _dir: dir,
            root,
            vault,
        }
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = rel.split('/').fold(self.root.clone(), |p, c| p.join(c));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("папки");
        }
        std::fs::write(&path, text).expect("заметка");
        path
    }

    fn index_file(&self) -> PathBuf {
        self.root.join(".rhizomind").join(FILE_NAME)
    }

    /// Индекс, собранный с нуля.
    fn built(&self) -> RwLock<VaultIndex> {
        let index = RwLock::new(VaultIndex::new());
        rebuild(&self.vault, &index, &|_| {}).expect("собран");
        index
    }
}

/// Записи индекса по порядку пути — для сравнения.
fn sorted(index: &VaultIndex) -> Vec<(VaultPath, NoteEntry)> {
    let mut entries: Vec<_> = index
        .entries()
        .map(|(p, e)| (p.clone(), e.clone()))
        .collect();
    entries.sort_by(|a, b| a.0.components().cmp(b.0.components()));
    entries
}

fn loaded(file: &dyn IndexFile) -> VaultIndex {
    match load(file) {
        Load::Loaded(index) => *index,
        Load::Rebuild(reason) => panic!("ожидался индекс из файла, а не пересборка: {reason:?}"),
    }
}

fn rebuild_reason(file: &dyn IndexFile) -> RebuildReason {
    match load(file) {
        Load::Loaded(_) => panic!("ожидалась пересборка"),
        Load::Rebuild(reason) => reason,
    }
}

const RICH_NOTE: &str = "---\ntags: [проект, Draft]\n---\n# Заголовок\n\nТекст со ссылками [[Узел]], [[папка/Цель#Раздел|псевдоним]] и ![[картинка.png]].\n\n## Второй\n#тег/вложенный и #Тег\n";

#[test]
fn roundtrip() {
    let t = TestVault::new();
    t.put("Узел.md", "# Узел\n[[Заметка]]\n");
    t.put("папка/Цель.md", "## Раздел\n");
    t.put("папка/вложенная/Заметка.md", RICH_NOTE);
    let index = t.built();
    let mut store = IndexStore::new();
    assert_eq!(
        store.write_if_due(&t.vault, &index, Instant::now()),
        WriteOutcome::Written
    );
    assert!(t.index_file().is_file());

    let back = loaded(&t.vault);
    let before = sorted(&index.read().expect("индекс"));
    assert_eq!(sorted(&back), before);
    assert_eq!(back.status().state, IndexState::Ready);
    assert_eq!(before.len(), 3);
    let rich = &before
        .iter()
        .find(|(p, _)| p == &vp("папка/вложенная/Заметка.md"))
        .expect("заметка")
        .1;
    assert!(!rich.parsed.links.is_empty() && !rich.parsed.tags.is_empty());

    // Компактный JSON с версиями, пути компонентами.
    let raw = std::fs::read(t.index_file()).expect("файл");
    assert!(!raw.contains(&b'\n'), "без отступов и переносов");
    let value: Value = serde_json::from_slice(&raw).expect("JSON");
    assert_eq!(value["format_version"], json!(FORMAT_VERSION));
    assert_eq!(value["rules_version"], json!(RULES_VERSION));
    let paths: Vec<&Value> = value["notes"]
        .as_array()
        .expect("заметки")
        .iter()
        .map(|n| &n["p"])
        .collect();
    assert!(paths.contains(&&json!(["папка", "вложенная", "Заметка.md"])));
}

/// Файл с изменённым полем верхнего уровня.
fn with_field(t: &TestVault, field: &str, value: Value) {
    let index = t.built();
    IndexStore::new().write_now(&t.vault, &index);
    let mut file: Value =
        serde_json::from_slice(&std::fs::read(t.index_file()).expect("файл")).expect("JSON");
    file[field] = value;
    std::fs::write(t.index_file(), serde_json::to_vec(&file).expect("JSON")).expect("запись");
}

#[test]
fn wrong_format_version_rebuilds() {
    let t = TestVault::new();
    t.put("a.md", "#тег [[b]]");
    for version in [json!(0), json!(FORMAT_VERSION + 1), json!("1"), Value::Null] {
        with_field(&t, "format_version", version.clone());
        assert_eq!(
            rebuild_reason(&t.vault),
            RebuildReason::WrongVersion,
            "{version}"
        );
    }
}

#[test]
fn newer_rules_version_rebuilds() {
    let t = TestVault::new();
    t.put("a.md", "#тег [[b]]");
    with_field(&t, "rules_version", json!(RULES_VERSION + 1));
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::WrongVersion);
    with_field(&t, "rules_version", json!(RULES_VERSION - 1));
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::WrongVersion);
}

#[test]
fn truncated_rebuilds() {
    let t = TestVault::new();
    t.put("a.md", RICH_NOTE);
    t.put("b.md", RICH_NOTE);
    let index = t.built();
    IndexStore::new().write_now(&t.vault, &index);
    let raw = std::fs::read(t.index_file()).expect("файл");
    std::fs::write(t.index_file(), &raw[..raw.len() / 2]).expect("оборван");
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::Broken);

    std::fs::write(t.index_file(), b"").expect("пустой");
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::Broken);
    std::fs::write(t.index_file(), b"\xff\xfe not json").expect("мусор");
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::Broken);
}

/// Подменённый файл с путём или положениями, которые не могут быть
/// настоящими, не принимается.
#[test]
fn tampered_entries_rebuild() {
    let t = TestVault::new();
    t.put("a.md", "[[b]] #тег");
    let index = t.built();
    IndexStore::new().write_now(&t.vault, &index);
    let good: Value =
        serde_json::from_slice(&std::fs::read(t.index_file()).expect("файл")).expect("JSON");
    let tamper = |edit: &dyn Fn(&mut Value)| {
        let mut file = good.clone();
        edit(&mut file["notes"][0]);
        std::fs::write(t.index_file(), serde_json::to_vec(&file).expect("JSON")).expect("запись");
        rebuild_reason(&t.vault)
    };
    assert_eq!(
        tamper(&|n| n["p"] = json!(["..", "secret.md"])),
        RebuildReason::Broken
    );
    assert_eq!(
        tamper(&|n| n["p"] = json!([".rhizomind", "settings.json"])),
        RebuildReason::Broken
    );
    assert_eq!(tamper(&|n| n["p"] = json!([])), RebuildReason::Broken);
    assert_eq!(
        tamper(&|n| n["l"][0][5] = json!(1_000_000)),
        RebuildReason::Broken,
        "конец ссылки за концом файла"
    );
    assert_eq!(
        tamper(&|n| n["l"][0][0] = json!(7)),
        RebuildReason::Broken,
        "признак встраивания — 0 или 1"
    );
    // Теги — только такие, какие дал бы разбор (ревью безопасности 0.2-1.4).
    assert_eq!(
        tamper(&|n| n["t"][0][0] = json!("т".repeat(201))),
        RebuildReason::Broken,
        "тег длиннее 200 кодовых точек"
    );
    assert_eq!(
        tamper(&|n| n["t"][0][0] = json!("два слова")),
        RebuildReason::Broken,
        "пробел в теге"
    );
    assert_eq!(
        tamper(&|n| n["t"][0][0] = json!("2026")),
        RebuildReason::Broken
    );
    assert_eq!(tamper(&|n| n["t"][0][0] = json!("")), RebuildReason::Broken);
    // 200 кодовых точек — допустимый тег: файл загружается.
    let mut file = good.clone();
    file["notes"][0]["t"][0][0] = json!("т".repeat(200));
    std::fs::write(t.index_file(), serde_json::to_vec(&file).expect("JSON")).expect("запись");
    assert!(matches!(load(&t.vault), Load::Loaded(_)));
}

#[test]
fn over_256mb_rebuilds() {
    let t = TestVault::new();
    // Разреженный файл: место на диске не занимает и не читается.
    let file = std::fs::File::create(t.index_file()).expect("файл");
    file.set_len(MAX_INDEX_FILE_BYTES + 1).expect("размер");
    drop(file);
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::TooLarge);
}

#[test]
fn missing_file_rebuilds() {
    let t = TestVault::new();
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::Missing);
}

/// С2-9, п. 3: удалили `index.json` — индекс собирается с нуля и совпадает
/// с прежним; файл пишется снова.
#[test]
fn deleted_file_rebuilds() {
    let t = TestVault::new();
    t.put("a.md", RICH_NOTE);
    t.put("b.md", "[[a]] #тег");
    let first = t.built();
    IndexStore::new().write_now(&t.vault, &first);
    std::fs::remove_file(t.index_file()).expect("удалён");

    assert_eq!(rebuild_reason(&t.vault), RebuildReason::Missing);
    let again = t.built();
    assert_eq!(
        sorted(&again.read().expect("индекс")),
        sorted(&first.read().expect("индекс"))
    );
    let mut store = IndexStore::new();
    assert_eq!(
        store.write_if_due(&t.vault, &again, Instant::now()),
        WriteOutcome::Written
    );
    assert_eq!(
        sorted(&loaded(&t.vault)),
        sorted(&first.read().expect("индекс"))
    );
}

/// Дата изменения файла, мс.
fn set_mtime(path: &Path, ms: i64) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("файл");
    let time = std::time::UNIX_EPOCH + Duration::from_millis(ms as u64);
    file.set_modified(time).expect("дата");
}

#[test]
fn reconcile_by_mtime_and_size() {
    let t = TestVault::new();
    let a = t.put("a.md", "#старый");
    let b = t.put("b.md", "#бэ");
    set_mtime(&a, 1_700_000_000_000);
    set_mtime(&b, 1_700_000_000_000);
    let index = t.built();
    IndexStore::new().write_now(&t.vault, &index);

    // Та же длина и та же дата у «a» — правка не видна (берётся из файла
    // индекса); у «b» другая длина; «c» — новая заметка.
    std::fs::write(&a, "#дальше").expect("a");
    assert_eq!("#старый".len(), "#дальше".len());
    set_mtime(&a, 1_700_000_000_000);
    std::fs::write(&b, "#бэ-бэ").expect("b");
    set_mtime(&b, 1_700_000_000_000);
    t.put("c.md", "#цэ");

    let index = RwLock::new(loaded(&t.vault));
    reconcile(&t.vault, &index).expect("сверка");
    let index = index.read().expect("индекс");
    let tags = |path: &str| -> Vec<String> {
        index
            .entries()
            .find(|(p, _)| *p == &vp(path))
            .map(|(_, e)| e.parsed.tags.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default()
    };
    assert_eq!(tags("a.md"), ["старый"], "та же дата и размер — из индекса");
    assert_eq!(tags("b.md"), ["бэ-бэ"], "другой размер — перечитана");
    assert_eq!(tags("c.md"), ["цэ"], "новая — прочитана");

    // Другая дата при том же размере — перечитывается.
    drop(index);
    set_mtime(&a, 1_700_000_005_000);
    let index = RwLock::new(loaded(&t.vault));
    reconcile(&t.vault, &index).expect("сверка");
    let index = index.read().expect("индекс");
    let a_tags: Vec<String> = index
        .entries()
        .find(|(p, _)| *p == &vp("a.md"))
        .map(|(_, e)| e.parsed.tags.iter().map(|t| t.name.clone()).collect())
        .expect("a");
    assert_eq!(a_tags, ["дальше"]);
}

#[test]
fn removed_notes_dropped() {
    let t = TestVault::new();
    t.put("a.md", "[[b]]");
    let gone = t.put("папка/b.md", "#тег");
    let index = t.built();
    IndexStore::new().write_now(&t.vault, &index);
    std::fs::remove_file(&gone).expect("удалена");

    let index = RwLock::new(loaded(&t.vault));
    assert_eq!(index.read().expect("индекс").entries().count(), 2);
    reconcile(&t.vault, &index).expect("сверка");
    let index = index.read().expect("индекс");
    let paths: Vec<&VaultPath> = index.entries().map(|(p, _)| p).collect();
    assert_eq!(paths, [&vp("a.md")]);
    assert!(index.tags().is_empty(), "теги удалённой заметки ушли");
}

#[test]
fn no_note_text_in_file() {
    const MARKER: &str = "СЕКРЕТНЫЙМАРКЕР42";
    let t = TestVault::new();
    t.put(
        "a.md",
        &format!(
            "---\ntitle: {MARKER}\ntags: [видимый]\n---\n# Заголовок\n\nАбзац {MARKER} и [[Цель]].\n\n```\n{MARKER} #нетег [[нессылка]]\n```\n- пункт {MARKER}\n"
        ),
    );
    let index = t.built();
    assert_eq!(
        IndexStore::new().write_now(&t.vault, &index),
        WriteOutcome::Written
    );
    let raw = String::from_utf8(std::fs::read(t.index_file()).expect("файл")).expect("UTF-8");
    assert!(!raw.contains(MARKER), "текст заметки попал в индекс: {raw}");
    assert!(raw.contains("видимый") && raw.contains("Цель") && raw.contains("Заголовок"));
}

/// Носитель только для чтения: запись не удаётся — молча, индекс остаётся в
/// памяти, повтор — не раньше следующего срока и только при изменениях.
struct ReadOnlyMedia {
    writes: Cell<usize>,
}

impl IndexFile for ReadOnlyMedia {
    fn read_index(&self) -> CoreResult<ServiceRead> {
        Ok(ServiceRead::Missing)
    }

    fn write_index(&self, _bytes: &[u8]) -> CoreResult<()> {
        self.writes.set(self.writes.get() + 1);
        Err(CoreError::new(ErrorCode::IoFailed))
    }
}

#[test]
fn readonly_media_keeps_index_in_memory() {
    let t = TestVault::new();
    t.put("a.md", "[[b]] #тег");
    t.put("b.md", "текст");
    let index = t.built();
    let media = ReadOnlyMedia {
        writes: Cell::new(0),
    };
    let mut store = IndexStore::new();
    let t0 = Instant::now();
    assert_eq!(store.write_if_due(&media, &index, t0), WriteOutcome::Failed);
    assert_eq!(media.writes.get(), 1);
    {
        let index = index.read().expect("индекс");
        assert_eq!(index.status().state, IndexState::Ready);
        assert_eq!(index.entries().count(), 2);
        assert_eq!(index.tags().len(), 1);
    }
    // Без изменений — не повторяет даже после срока, при закрытии — тоже.
    let later = t0 + WRITE_INTERVAL * 3;
    assert_eq!(
        store.write_if_due(&media, &index, later),
        WriteOutcome::Unchanged
    );
    assert_eq!(store.write_now(&media, &index), WriteOutcome::Unchanged);
    assert_eq!(media.writes.get(), 1);
    // Изменение — новая попытка, снова молча.
    index
        .write()
        .expect("индекс")
        .upsert(vp("c.md"), entry_of("#новый"));
    assert_eq!(
        store.write_if_due(&media, &index, later),
        WriteOutcome::Failed
    );
    assert_eq!(media.writes.get(), 2);
    assert!(!t.index_file().exists());
}

fn entry_of(text: &str) -> NoteEntry {
    NoteEntry {
        size: text.len() as u64,
        modified_ms: 1,
        created_ms: 1,
        parsed: parse(text),
    }
}

/// Запоминает записанные байты.
#[derive(Default)]
struct Memory {
    writes: RefCell<Vec<Vec<u8>>>,
}

impl IndexFile for Memory {
    fn read_index(&self) -> CoreResult<ServiceRead> {
        Ok(match self.writes.borrow().last() {
            Some(bytes) => ServiceRead::Bytes(bytes.clone()),
            None => ServiceRead::Missing,
        })
    }

    fn write_index(&self, bytes: &[u8]) -> CoreResult<()> {
        self.writes.borrow_mut().push(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn write_throttled_30s() {
    assert_eq!(WRITE_INTERVAL, Duration::from_secs(30));
    let file = Memory::default();
    let index = RwLock::new(VaultIndex::from_entries([(vp("a.md"), entry_of("#a"))]));
    let mut store = IndexStore::new();
    let t0 = Instant::now();
    let change = |text: &str| {
        index
            .write()
            .expect("индекс")
            .upsert(vp("a.md"), entry_of(text));
    };

    assert_eq!(store.write_if_due(&file, &index, t0), WriteOutcome::Written);
    change("#b");
    assert_eq!(
        store.write_if_due(&file, &index, t0 + Duration::from_secs(29)),
        WriteOutcome::Throttled
    );
    assert_eq!(
        store.write_if_due(&file, &index, t0 + WRITE_INTERVAL),
        WriteOutcome::Written
    );
    // Без изменений — не пишет.
    assert_eq!(
        store.write_if_due(&file, &index, t0 + WRITE_INTERVAL * 5),
        WriteOutcome::Unchanged
    );
    assert_eq!(file.writes.borrow().len(), 2);
    // При закрытии — сразу, без ожидания срока, если изменился.
    change("#c");
    assert_eq!(store.write_now(&file, &index), WriteOutcome::Written);
    assert_eq!(store.write_now(&file, &index), WriteOutcome::Unchanged);
    assert_eq!(file.writes.borrow().len(), 3);
}

/// Индекс, загруженный из файла, сразу не переписывается: в файле то же.
#[test]
fn loaded_index_not_rewritten() {
    let file = Memory::default();
    let index = RwLock::new(VaultIndex::from_entries([(vp("a.md"), entry_of("#a"))]));
    IndexStore::new().write_now(&file, &index);
    let back = loaded(&file);
    let mut store = IndexStore::loaded(back.revision());
    let back = RwLock::new(back);
    assert_eq!(
        store.write_if_due(&file, &back, Instant::now()),
        WriteOutcome::Unchanged
    );
    back.write()
        .expect("индекс")
        .upsert(vp("b.md"), entry_of("#b"));
    assert_eq!(
        store.write_if_due(&file, &back, Instant::now()),
        WriteOutcome::Written
    );
}

/// Недостроенный индекс не пишется: в файле был бы неполный индекс.
#[test]
fn building_index_not_written() {
    let file = Memory::default();
    let index = RwLock::new(VaultIndex::new());
    index
        .write()
        .expect("индекс")
        .upsert(vp("a.md"), entry_of("#a"));
    let mut store = IndexStore::new();
    assert_eq!(
        store.write_if_due(&file, &index, Instant::now()),
        WriteOutcome::NotReady
    );
    assert_eq!(store.write_now(&file, &index), WriteOutcome::NotReady);
    assert!(file.writes.borrow().is_empty());
}

/// Замер (задача 0.2-1.3): `index.json` на 10 000 заметок генератора — не
/// больше 15 МБ (время записи и чтения — справочно). Запуск:
/// `cargo test -p rhizomind --lib index::store::tests::index_json_10k_size -- --ignored --nocapture`.
#[test]
#[ignore = "замер: --ignored"]
fn index_json_10k_size() {
    let dir = tempfile::tempdir().expect("временная папка");
    let root = dir.path().join("v10k");
    gen_vault::generate(&root, &gen_vault::Config::new(10_000, 7)).expect("генератор");
    let vault = Vault::open(&root).expect("хранилище");
    // В одном потоке, без `rebuild`: CI запускает замеры `--ignored`
    // одновременно, а здесь важен размер, а не скорость сборки — не отнимать
    // ядра у замеров времени (`markdown_parse_10k`, `index_build_10k`).
    let notes = vault.notes().expect("заметки");
    let index = RwLock::new(VaultIndex::from_entries(notes.iter().map(|note| {
        let entry = crate::index::read_entry(&vault, note).expect("заметка");
        (note.path.clone(), entry)
    })));
    let start = Instant::now();
    assert_eq!(
        IndexStore::new().write_now(&vault, &index),
        WriteOutcome::Written
    );
    let write = start.elapsed();
    let size = std::fs::metadata(root.join(".rhizomind").join(FILE_NAME))
        .expect("файл")
        .len();
    let start = Instant::now();
    let back = loaded(&vault);
    let read = start.elapsed();
    let notes = index.read().expect("индекс").entries().count();
    println!(
        "index_json_10k_size: {notes} заметок, index.json {size} байт ({:.2} МБ, {:.0} байт на заметку), запись {write:?}, чтение {read:?}",
        size as f64 / (1024.0 * 1024.0),
        size as f64 / notes as f64,
    );
    assert_eq!(back.entries().count(), notes);
    assert!(size <= 15 * 1024 * 1024, "{size}");
}

/// Подменённый файл с огромным массивом вместо версии: «версия не та», а
/// заголовок не строит значение массива в памяти (ревью 0.2-1.3).
#[test]
fn huge_version_value_is_wrong_version() {
    let t = TestVault::new();
    let mut raw = b"{\"format_version\":[".to_vec();
    let items = 4 * 1024 * 1024; // «0,» × 4 М ≈ 8 МБ
    raw.reserve(items * 2 + 64);
    for i in 0..items {
        if i > 0 {
            raw.push(b',');
        }
        raw.push(b'0');
    }
    raw.extend_from_slice(b"],\"rules_version\":1,\"notes\":[]}");
    std::fs::write(t.index_file(), &raw).expect("файл");
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::WrongVersion);
    // Та же подмена в rules_version и вложенный объект.
    let raw = format!(
        "{{\"format_version\":{FORMAT_VERSION},\"rules_version\":{{\"a\":[[1,2],{{\"b\":3}}]}},\"notes\":[]}}"
    );
    std::fs::write(t.index_file(), raw).expect("файл");
    assert_eq!(rebuild_reason(&t.vault), RebuildReason::WrongVersion);
    assert!(
        !store_source().contains("serde_json::Value"),
        "заголовок не разбирается в serde_json::Value"
    );
}

fn store_source() -> &'static str {
    include_str!("store.rs")
}
