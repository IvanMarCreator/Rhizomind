//! Чтение заметки: текст без BOM с переносами `\n`, версия файла, «только
//! чтение» для невалидного UTF-8 и файлов больше лимита из настроек.

use super::*;
use crate::commands::ReadOnlyReason;

/// Хранилище генератора `tools/gen-vault` (образцы кодировок) и его манифест.
pub fn generated(big_mb: Option<u64>) -> (TestVault, gen_vault::Manifest) {
    let t = TestVault::new();
    let generated = t.dir.path().join("generated");
    let mut config = gen_vault::Config::new(400, 7);
    config.big_mb = big_mb;
    let manifest = gen_vault::generate(&generated, &config).expect("генератор");
    copy_dir(&generated, &t.root());
    (t, manifest)
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("папка генератора") {
        let entry = entry.expect("элемент");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("тип").is_dir() {
            std::fs::create_dir_all(&target).expect("папка");
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("копия");
        }
    }
}

#[test]
fn encoding_samples_read_without_bom_and_with_lf() {
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
        let note = t.vault.read_note(&vp(sample)).expect(sample);
        let disk = t.bytes(sample);
        let expected = String::from_utf8(disk.clone())
            .expect("UTF-8")
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n");
        assert_eq!(note.text, expected, "{sample}");
        assert!(!note.text.contains('\r'), "{sample}");
        assert_eq!(note.read_only, None, "{sample}");
    }
}

#[test]
fn version_is_hash_size_and_modification_time_of_the_file() {
    let t = TestVault::new();
    let path = t.put("Заметка.md", "# Заметка\r\nтекст\r\n");
    let note = t.vault.read_note(&vp("Заметка.md")).expect("чтение");

    let bytes = std::fs::read(&path).expect("байты");
    assert_eq!(note.version.hash, blake3::hash(&bytes).to_hex().as_str());
    assert_eq!(note.version.hash.len(), 64);
    assert_eq!(note.version.size, bytes.len() as u64);
    let modified = std::fs::metadata(&path)
        .expect("метаданные")
        .modified()
        .expect("дата")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("после 1970")
        .as_millis() as i64;
    assert_eq!(note.version.modified_ms, modified);
}

#[test]
fn invalid_utf8_opens_read_only_with_replacement_characters() {
    let (t, manifest) = generated(None);
    let sample = &manifest.encodings.samples.invalid_utf8;
    let note = t.vault.read_note(&vp(sample)).expect("чтение");
    assert_eq!(note.read_only, Some(ReadOnlyReason::InvalidUtf8));
    assert!(note.text.contains('\u{fffd}'));
}

#[test]
fn file_larger_than_settings_limit_opens_read_only() {
    let (t, manifest) = generated(Some(2));
    std::fs::write(
        t.root().join(".rhizomind").join("settings.json"),
        r#"{ "format_version": 1, "max_edit_size_bytes": 1048576 }"#,
    )
    .expect("настройки");
    let big = manifest
        .encodings
        .samples
        .big
        .as_ref()
        .expect("большой файл");

    let note = t.vault.read_note(&vp(&big.path)).expect("чтение");

    assert_eq!(
        note.read_only,
        Some(ReadOnlyReason::TooLarge {
            limit_bytes: 1_048_576
        })
    );
    assert_eq!(note.version.size, big.bytes);
    let small = t
        .vault
        .read_note(&vp(&manifest.encodings.samples.lf))
        .expect("малый");
    assert_eq!(small.read_only, None);
}

#[test]
fn not_a_note_missing_and_folder() {
    let t = TestVault::new();
    t.put("схема.png", [0x89, b'P', b'N', b'G']);
    std::fs::create_dir(t.file("Папка.md")).expect("папка с именем заметки");
    assert_eq!(
        code(t.vault.read_note(&vp("схема.png"))),
        ErrorCode::NotANote
    );
    assert_eq!(
        code(t.vault.read_note(&vp("Папка.md"))),
        ErrorCode::NotANote
    );
    assert_eq!(code(t.vault.read_note(&vp("Нет.md"))), ErrorCode::NotFound);
    assert_eq!(
        code(t.vault.read_note(&vp("Нет/Нет.md"))),
        ErrorCode::NotFound
    );
}
