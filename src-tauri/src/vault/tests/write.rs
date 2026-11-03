//! Запись заметки: «байт в байт», атомарность, конфликт изменений, дата
//! создания, свои записи для наблюдателя.

use std::sync::{Arc, Mutex};

use super::read::generated;
use super::*;
use crate::commands::{NoteWriteResponse, ReadOnlyReason};

fn saved_version(response: NoteWriteResponse) -> crate::commands::FileVersion {
    match response {
        NoteWriteResponse::Saved { version } => version,
        other => panic!("ожидалось saved: {other:?}"),
    }
}

/// Файлы папки хранилища (без служебной).
fn names_in(t: &TestVault, rel: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(t.file(rel))
        .expect("папка")
        .map(|e| {
            e.expect("элемент")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n != ".rhizomind")
        .collect();
    names.sort();
    names
}

#[test]
fn opening_and_saving_samples_without_edits_keeps_every_byte() {
    let (t, manifest) = generated(None);
    let s = &manifest.encodings.samples;
    for sample in [
        &s.lf,
        &s.crlf,
        &s.lf_bom,
        &s.crlf_bom,
        &s.mixed,
        &s.no_final_newline,
    ] {
        let before = t.bytes(sample);
        let modified_before = std::fs::metadata(t.file(sample))
            .expect("мета")
            .modified()
            .expect("дата");
        let note = t.vault.read_note(&vp(sample)).expect(sample);

        let version = saved_version(
            t.vault
                .write_note(&vp(sample), &note.text, &note.version)
                .expect(sample),
        );

        assert_eq!(t.bytes(sample), before, "{sample}: байт в байт");
        assert_eq!(version, note.version, "{sample}: версия прежняя");
        assert_eq!(
            std::fs::metadata(t.file(sample))
                .expect("мета")
                .modified()
                .expect("дата"),
            modified_before,
            "{sample}: файл не переписан"
        );
    }
}

#[test]
fn edited_samples_keep_bom_and_line_endings() {
    let (t, manifest) = generated(None);
    let s = &manifest.encodings.samples;
    for (sample, bom, crlf) in [
        (&s.lf, false, false),
        (&s.crlf, false, true),
        (&s.lf_bom, true, false),
        (&s.crlf_bom, true, true),
    ] {
        let note = t.vault.read_note(&vp(sample)).expect(sample);
        let edited = format!("{}новая строка\nещё одна\n", note.text);

        let version = saved_version(
            t.vault
                .write_note(&vp(sample), &edited, &note.version)
                .expect(sample),
        );

        let bytes = t.bytes(sample);
        assert_eq!(bytes.starts_with(b"\xEF\xBB\xBF"), bom, "{sample}: BOM");
        let crlf_count = bytes.windows(2).filter(|w| *w == b"\r\n").count();
        let lf_count = bytes.iter().filter(|&&b| b == b'\n').count();
        if crlf {
            assert_eq!(crlf_count, lf_count, "{sample}: только CRLF");
        } else {
            assert_eq!(crlf_count, 0, "{sample}: только LF");
        }
        let reread = t.vault.read_note(&vp(sample)).expect("перечитать");
        assert_eq!(reread.text, edited, "{sample}: текст");
        assert_eq!(
            reread.version, version,
            "{sample}: версия ответа = версия на диске"
        );
    }
}

#[test]
fn mixed_line_endings_become_the_predominant_ones_after_an_edit() {
    let t = TestVault::new();
    t.put("Смесь.md", "a\r\nb\r\nc\n");
    let note = t.vault.read_note(&vp("Смесь.md")).expect("чтение");
    t.vault
        .write_note(&vp("Смесь.md"), "a\nb\nc\nd\n", &note.version)
        .expect("запись");
    assert_eq!(t.bytes("Смесь.md"), b"a\r\nb\r\nc\r\nd\r\n");
}

#[test]
fn external_change_is_kept_as_a_copy_and_editor_text_goes_to_the_note() {
    let t = TestVault::new();
    t.put("Папка/Встреча.md", "исходный\n");
    let note = t.vault.read_note(&vp("Папка/Встреча.md")).expect("чтение");
    t.put("Папка/Встреча.md", "снаружи\r\n");

    let response = t
        .vault
        .write_note(&vp("Папка/Встреча.md"), "из редактора\n", &note.version)
        .expect("запись");

    let NoteWriteResponse::ConflictSaved { version, copy_path } = response else {
        panic!("ожидался конфликт: {response:?}");
    };
    assert_eq!(t.bytes("Папка/Встреча.md"), b"\xd0\xb8\xd0\xb7 \xd1\x80\xd0\xb5\xd0\xb4\xd0\xb0\xd0\xba\xd1\x82\xd0\xbe\xd1\x80\xd0\xb0\r\n");
    assert_eq!(copy_path.components().len(), 2);
    assert_eq!(copy_path.components()[0], "Папка");
    let copy_name = copy_path.file_name().expect("имя копии");
    assert!(is_conflict_copy_name(copy_name, "Встреча"), "{copy_name}");
    let copy = std::fs::read(t.file(&copy_path.components().join("/"))).expect("копия");
    assert_eq!(
        copy,
        "снаружи\r\n".as_bytes(),
        "копия — версия с диска байт в байт"
    );
    let reread = t
        .vault
        .read_note(&vp("Папка/Встреча.md"))
        .expect("перечитать");
    assert_eq!(reread.version, version);
}

/// `Встреча (внешняя версия 2026-10-25 14-03).md`, с возможным номером
/// повтора перед `.md`.
fn is_conflict_copy_name(name: &str, stem: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(stem)
        .and_then(|r| r.strip_prefix(" (внешняя версия "))
    else {
        return false;
    };
    let digits: Vec<char> = rest.chars().take(16).collect();
    let shape: String = digits
        .iter()
        .map(|c| if c.is_ascii_digit() { 'D' } else { *c })
        .collect();
    shape == "DDDD-DD-DD DD-DD" && rest.ends_with(".md") && rest[16..].starts_with(')')
}

#[test]
fn repeated_conflicts_in_the_same_minute_get_distinct_copies() {
    let t = TestVault::new();
    t.put("Заметка.md", "0\n");
    let mut copies = Vec::new();
    for round in 1..=3 {
        let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
        t.put("Заметка.md", format!("снаружи {round}\n"));
        match t
            .vault
            .write_note(&vp("Заметка.md"), &format!("моё {round}\n"), &note.version)
            .expect("запись")
        {
            NoteWriteResponse::ConflictSaved { copy_path, .. } => copies.push(copy_path),
            other => panic!("{other:?}"),
        }
    }
    copies.sort_by(|a, b| a.components().cmp(b.components()));
    copies.dedup();
    assert_eq!(copies.len(), 3, "три разные копии: {copies:?}");
    assert_eq!(names_in(&t, "").len(), 4);
}

#[test]
fn same_content_with_new_date_is_not_a_conflict() {
    let t = TestVault::new();
    let path = t.put("Заметка.md", "текст\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("открыть")
        .set_modified(later)
        .expect("дата");

    let response = t
        .vault
        .write_note(&vp("Заметка.md"), "текст\nправка\n", &note.version)
        .expect("запись");

    assert!(
        matches!(response, NoteWriteResponse::Saved { .. }),
        "{response:?}"
    );
    assert_eq!(names_in(&t, ""), ["Заметка.md"]);
    assert_eq!(t.bytes("Заметка.md"), "текст\nправка\n".as_bytes());
}

#[test]
fn external_reformat_with_same_text_is_left_alone() {
    let t = TestVault::new();
    t.put("Заметка.md", "a\nb\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    t.put("Заметка.md", "a\r\nb\r\n");

    let version = saved_version(
        t.vault
            .write_note(&vp("Заметка.md"), "a\nb\n", &note.version)
            .expect("запись"),
    );

    assert_eq!(
        t.bytes("Заметка.md"),
        b"a\r\nb\r\n",
        "файл снаружи не переписан"
    );
    assert_eq!(version.size, 6);
    assert_eq!(names_in(&t, ""), ["Заметка.md"], "копии нет");
}

#[test]
fn missing_note_folder_and_attachment() {
    let t = TestVault::new();
    let version = crate::commands::FileVersion {
        hash: "0".repeat(64),
        size: 0,
        modified_ms: 0,
    };
    t.put("схема.png", [1, 2, 3]);
    std::fs::create_dir(t.file("Папка.md")).expect("папка");
    assert_eq!(
        code(t.vault.write_note(&vp("Нет.md"), "x", &version)),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(t.vault.write_note(&vp("схема.png"), "x", &version)),
        ErrorCode::NotANote
    );
    assert_eq!(
        code(t.vault.write_note(&vp("Папка.md"), "x", &version)),
        ErrorCode::NotANote
    );
    assert!(!t.file("Нет.md").exists(), "запись не создаёт заметку");
    assert_eq!(t.bytes("схема.png"), [1, 2, 3]);
}

#[test]
fn invalid_utf8_note_in_the_opened_version_is_read_only() {
    let t = TestVault::new();
    t.put("Битая.md", b"ok \xFF\xFE\n");
    let note = t.vault.read_note(&vp("Битая.md")).expect("чтение");
    assert_eq!(note.read_only, Some(ReadOnlyReason::InvalidUtf8));
    assert_eq!(
        code(
            t.vault
                .write_note(&vp("Битая.md"), &note.text, &note.version)
        ),
        ErrorCode::ReadOnly
    );
    assert_eq!(t.bytes("Битая.md"), b"ok \xFF\xFE\n");
}

#[test]
fn failure_in_the_middle_of_a_write_leaves_the_note_whole() {
    let t = TestVault::new();
    let original = "# Заметка\nвесь текст\n".repeat(1000);
    t.put("Заметка.md", &original);
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");

    crate::vault::atomic::fail_next_write_after(100);
    let result = t
        .vault
        .write_note(&vp("Заметка.md"), "совсем другой текст\n", &note.version);

    assert_eq!(code(result), ErrorCode::IoFailed);
    assert_eq!(t.bytes("Заметка.md"), original.as_bytes(), "заметка целая");
    assert_eq!(
        std::fs::read_dir(t.root().join(".rhizomind/tmp"))
            .expect("tmp")
            .count(),
        0,
        "временный файл убран"
    );
    let retry = t
        .vault
        .write_note(&vp("Заметка.md"), "совсем другой текст\n", &note.version)
        .expect("повтор");
    assert!(matches!(retry, NoteWriteResponse::Saved { .. }));
}

#[test]
fn leftovers_of_an_interrupted_write_are_removed_on_open() {
    let t = TestVault::new();
    let leftover = t.root().join(".rhizomind/tmp/.tmpABC123");
    std::fs::write(&leftover, "половина").expect("остаток");
    let _reopened = Vault::open(&t.root()).expect("повторное открытие");
    assert!(!leftover.exists());
}

/// Свои записи (путь, хэш, размер) и содержимое диска в момент регистрации.
type Seen = Vec<(VaultPath, String, u64, Vec<u8>)>;

#[test]
fn own_write_is_registered_before_the_file_is_replaced() {
    let mut t = TestVault::new();
    t.put("Заметка.md", "старое\n");
    let journal = crate::own_writes::OwnWrites::new();
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let disk = t.file("Заметка.md");
    let log = Arc::clone(&seen);
    journal.observe(Box::new(move |path: &VaultPath, hash: &str, size: u64| {
        let on_disk = std::fs::read(&disk).expect("диск");
        log.lock()
            .expect("журнал")
            .push((path.clone(), hash.to_owned(), size, on_disk));
    }));
    t.reopen_with(crate::vault::OpenOptions {
        own_writes: journal.clone(),
        ..Default::default()
    });
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");

    let version = saved_version(
        t.vault
            .write_note(&vp("Заметка.md"), "новое\n", &note.version)
            .expect("запись"),
    );

    let seen = seen.lock().expect("журнал");
    assert_eq!(seen.len(), 1);
    let (path, hash, size, on_disk_at_hook) = &seen[0];
    assert_eq!(*path, vp("Заметка.md"));
    assert_eq!(*hash, version.hash);
    assert_eq!(*size, version.size);
    assert_eq!(on_disk_at_hook, "старое\n".as_bytes(), "до замены файла");
    assert!(journal.contains(&vp("Заметка.md"), &version.hash, version.size));
    assert!(
        t.vault
            .is_own_write(&vp("Заметка.md"), &version.hash, version.size)
    );
    assert!(
        !t.vault
            .is_own_write(&vp("Заметка.md"), &note.version.hash, note.version.size)
    );
}

/// Дата создания сохраняется при атомарной записи (ADR 0006, уточнение
/// 25.10.2026). На Linux её не задать — проверка только на Windows.
#[cfg(windows)]
#[test]
fn write_keeps_the_creation_date() {
    let t = TestVault::new();
    let path = t.put("Старая.md", "первая версия\n");
    let created = std::fs::metadata(&path)
        .expect("мета")
        .created()
        .expect("дата создания");
    std::thread::sleep(std::time::Duration::from_millis(50));
    let note = t.vault.read_note(&vp("Старая.md")).expect("чтение");

    t.vault
        .write_note(&vp("Старая.md"), "вторая версия\n", &note.version)
        .expect("запись");

    assert_eq!(t.bytes("Старая.md"), "вторая версия\n".as_bytes());
    assert_eq!(
        std::fs::metadata(&path)
            .expect("мета")
            .created()
            .expect("дата создания"),
        created
    );
}

#[cfg(unix)]
#[test]
fn write_keeps_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let t = TestVault::new();
    let path = t.put("Заметка.md", "текст\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("права");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");
    t.vault
        .write_note(&vp("Заметка.md"), "новый\n", &note.version)
        .expect("запись");
    let mode = std::fs::metadata(&path).expect("мета").permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

// --- Запись чужой заметки по плану обновления ссылок (задача 0.2-4.5) ---

use crate::commands::RelinkFailureReason;
use crate::vault::Rebuilt;

const LIMIT: u64 = 10 * 1024 * 1024;

/// Замена `Встреча` → `Планёрка` во всём тексте (как построитель плана).
fn swap(text: &str) -> Option<String> {
    text.contains("Встреча")
        .then(|| text.replace("Встреча", "Планёрка"))
}

fn no_journal(_: &str, _: &str) -> Result<(), CoreError> {
    Ok(())
}

#[test]
fn rebuilt_note_keeps_bom_and_crlf_and_records_own_write() {
    let t = TestVault::new();
    t.put(
        "Список.md",
        b"\xEF\xBB\xBF[[\xD0\x92\xD1\x81\xD1\x82\xD1\x80\xD0\xB5\xD1\x87\xD0\xB0]]\r\n",
    );
    let mut hashes = Vec::new();

    let result = t.vault.write_note_rebuilt(
        &vp("Список.md"),
        LIMIT,
        &swap,
        &mut |before: &str, after: &str| {
            hashes.push((before.to_owned(), after.to_owned()));
            Ok(())
        },
    );

    let Ok(Rebuilt::Written(version)) = result else {
        panic!("ожидалась запись: {result:?}");
    };
    let expected = "\u{feff}[[Планёрка]]\r\n".as_bytes();
    assert_eq!(t.bytes("Список.md"), expected);
    assert_eq!(version.hash, blake3::hash(expected).to_hex().to_string());
    let read = blake3::hash(
        b"\xEF\xBB\xBF[[\xD0\x92\xD1\x81\xD1\x82\xD1\x80\xD0\xB5\xD1\x87\xD0\xB0]]\r\n",
    )
    .to_hex()
    .to_string();
    assert_eq!(
        hashes,
        [(read, version.hash.clone())],
        "журнал узнал хэш до замены"
    );
    assert!(
        t.vault
            .is_own_write(&vp("Список.md"), &version.hash, version.size)
    );
}

#[test]
fn rebuilt_nothing_to_change_leaves_the_file_alone() {
    let t = TestVault::new();
    t.put("Список.md", "без ссылок\n");
    let modified = std::fs::metadata(t.file("Список.md"))
        .expect("мета")
        .modified()
        .expect("дата");

    let result = t
        .vault
        .write_note_rebuilt(&vp("Список.md"), LIMIT, &swap, &mut no_journal);

    assert_eq!(result, Ok(Rebuilt::Unchanged));
    assert_eq!(
        std::fs::metadata(t.file("Список.md"))
            .expect("мета")
            .modified()
            .expect("дата"),
        modified
    );
}

#[test]
fn rebuilt_missing_note_is_gone_and_creates_nothing() {
    let t = TestVault::new();
    let result = t
        .vault
        .write_note_rebuilt(&vp("Нет.md"), LIMIT, &swap, &mut no_journal);
    assert_eq!(result, Ok(Rebuilt::Gone));
    assert!(!t.file("Нет.md").exists());
}

#[test]
fn rebuilt_skips_large_invalid_readonly_and_outside() {
    let t = TestVault::new();
    t.put("Большая.md", format!("Встреча{}", "x".repeat(100)));
    t.put(
        "Битая.md",
        b"\xD0\x92\xD1\x81\xD1\x82\xD1\x80\xD0\xB5\xD1\x87\xD0\xB0 \xFF",
    );
    let readonly = t.put("Только.md", "Встреча\n");
    let mut permissions = std::fs::metadata(&readonly).expect("мета").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&readonly, permissions.clone()).expect("права");
    symlink_dir(&t.outside(), &t.file("Наружу"));

    let write = |path: &str, limit: u64| {
        t.vault
            .write_note_rebuilt(&vp(path), limit, &swap, &mut no_journal)
    };
    assert_eq!(write("Большая.md", 50), Err(RelinkFailureReason::TooLarge));
    assert_eq!(write("Битая.md", LIMIT), Err(RelinkFailureReason::NotUtf8));
    assert_eq!(write("Только.md", LIMIT), Err(RelinkFailureReason::Busy));
    assert_eq!(
        write("Наружу/secret.md", LIMIT),
        Err(RelinkFailureReason::Io)
    );
    assert_eq!(
        write(".rhizomind/settings.json", LIMIT),
        Err(RelinkFailureReason::Io)
    );

    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&readonly, permissions).expect("права");
    assert_eq!(t.bytes("Только.md"), "Встреча\n".as_bytes());
    assert_eq!(
        std::fs::read(t.outside().join("secret.md")).expect("секрет"),
        "секрет\n".as_bytes()
    );
}

#[test]
fn rebuilt_journal_failure_writes_nothing() {
    let t = TestVault::new();
    t.put("Список.md", "Встреча\n");
    let result = t.vault.write_note_rebuilt(
        &vp("Список.md"),
        LIMIT,
        &swap,
        &mut |_: &str, _: &str| Err(CoreError::new(ErrorCode::IoFailed)),
    );
    assert_eq!(result, Err(RelinkFailureReason::Io));
    assert_eq!(t.bytes("Список.md"), "Встреча\n".as_bytes());
}

#[test]
fn rebuilt_external_edit_is_rebuilt_from_fresh_text() {
    let t = TestVault::new();
    let path = t.put("Список.md", "Встреча\n");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        std::fs::write(&path, "Встреча\nчужая правка\n").expect("правка");
    }));

    let result = t
        .vault
        .write_note_rebuilt(&vp("Список.md"), LIMIT, &swap, &mut no_journal);

    assert!(matches!(result, Ok(Rebuilt::Written(_))), "{result:?}");
    assert_eq!(t.bytes("Список.md"), "Планёрка\nчужая правка\n".as_bytes());
}

/// Файл уже заменён, а версию после записи прочитать не удалось (файл
/// держит антивирус, его убрали снаружи): заметка всё равно записана —
/// `Written` с хэшем и размером записанного, а не `io` (иначе повтор
/// переписал бы её второй раз без сверки хэшей).
#[test]
fn rebuilt_version_read_failure_after_replace_is_still_written() {
    let t = TestVault::new();
    let path = t.put("Список.md", "Встреча\n");
    crate::vault::after_replaced(move || {
        std::fs::remove_file(&path).expect("файл убран снаружи");
    });

    let result = t
        .vault
        .write_note_rebuilt(&vp("Список.md"), LIMIT, &swap, &mut no_journal);

    let Ok(Rebuilt::Written(version)) = result else {
        panic!("ожидалась запись: {result:?}");
    };
    let expected = "Планёрка\n".as_bytes();
    assert_eq!(version.hash, blake3::hash(expected).to_hex().to_string());
    assert_eq!(version.size, expected.len() as u64);
}

// --- Дописывание служебного файла (журнал обновления ссылок, 0.2-4.5) ---

use crate::vault::ServiceFile;

fn journal_path(t: &TestVault) -> PathBuf {
    t.root()
        .join(".rhizomind")
        .join(ServiceFile::LinkUpdate.name())
}

#[test]
fn service_append_writes_to_the_existing_file_only() {
    let t = TestVault::new();
    assert_eq!(
        code(t.vault.open_service_append(ServiceFile::LinkUpdate)),
        ErrorCode::NotFound
    );
    assert!(!journal_path(&t).exists(), "файл не создаётся");

    t.vault
        .write_service_file(ServiceFile::LinkUpdate.name(), b"head\n")
        .expect("заголовок");
    let mut file = t
        .vault
        .open_service_append(ServiceFile::LinkUpdate)
        .expect("открыт");
    file.append(b"mark\n", true).expect("дописано");
    drop(file);
    assert_eq!(
        std::fs::read(journal_path(&t)).expect("файл"),
        b"head\nmark\n"
    );
}

#[test]
fn service_append_refuses_symlinked_file() {
    let t = TestVault::new();
    let target = t.outside().join("journal.json");
    std::fs::write(&target, "снаружи\n").expect("цель");
    symlink_file(&target, &journal_path(&t));

    assert!(
        t.vault
            .open_service_append(ServiceFile::LinkUpdate)
            .is_err()
    );
    assert_eq!(
        std::fs::read(&target).expect("цель"),
        "снаружи\n".as_bytes()
    );
}

#[test]
fn service_append_refuses_symlinked_service_dir() {
    let t = TestVault::new();
    let outside_service = t.outside().join("service");
    std::fs::create_dir(&outside_service).expect("папка");
    let target = outside_service.join(ServiceFile::LinkUpdate.name());
    std::fs::write(&target, "снаружи\n").expect("цель");
    std::fs::remove_dir_all(t.root().join(".rhizomind")).expect("служебная папка");
    symlink_dir(&outside_service, &t.root().join(".rhizomind"));

    assert!(
        t.vault
            .open_service_append(ServiceFile::LinkUpdate)
            .is_err()
    );
    assert_eq!(
        std::fs::read(&target).expect("цель"),
        "снаружи\n".as_bytes()
    );
}

#[test]
fn service_append_refuses_hard_link() {
    let t = TestVault::new();
    let target = t.outside().join("journal.json");
    std::fs::write(&target, "снаружи\n").expect("цель");
    std::fs::hard_link(&target, journal_path(&t)).expect("жёсткая ссылка");

    assert!(
        t.vault
            .open_service_append(ServiceFile::LinkUpdate)
            .is_err()
    );
    assert_eq!(
        std::fs::read(&target).expect("цель"),
        "снаружи\n".as_bytes()
    );
}
