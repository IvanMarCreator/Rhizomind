//! Контур хранилища: ни одна операция не выходит за корень и не попадает в
//! служебную папку ни одним способом из списка (критерий задачи 1.2).

use super::*;
use crate::commands::FileVersion;
use crate::vault::check_component;

fn rejected(component: &str) -> bool {
    check_component(component).map_err(|e| e.code) == Err(ErrorCode::InvalidPath)
}

#[test]
fn component_rejects_traversal_separators_and_absolute_paths() {
    for bad in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "/etc",
        "\\\\?\\C:\\",
        "\\\\.\\PhysicalDrive0",
        "C:",
        "a:b.md",
        "Заметка.md:поток",
    ] {
        assert!(rejected(bad), "{bad:?}");
    }
}

#[test]
fn component_rejects_windows_reserved_names_with_and_without_extension() {
    for bad in [
        "CON", "nul", "NUL.md", "com1.txt", "LPT9", "CONIN$", "conout$.md", "COM¹", "aux .md",
    ] {
        assert!(rejected(bad), "{bad:?}");
    }
    assert!(!rejected("CONSOLE.md"));
    assert!(!rejected("COM10.md"));
}

#[test]
fn component_rejects_trailing_dot_space_and_control_characters() {
    for bad in [
        "Заметка.", "Заметка ", "a\0b", "a\u{1}b", "a\nb", "a*b", "a?b", "a|b", "a<b", "a>b",
        "a\"b",
    ] {
        assert!(rejected(bad), "{bad:?}");
    }
}

#[test]
fn component_rejects_short_name_aliases_but_not_long_names_with_tilde() {
    for bad in [
        "RHIZOM~1",
        "rhizom~1",
        "RHIZO~12",
        "ПРОЕКТ~2.MD",
        "AB~1.TXT",
    ] {
        assert!(rejected(bad), "{bad:?}");
    }
    for good in [
        "Версия~2 итог.md",
        "Заметка~1.md",
        "a~b.md",
        "~1",
        "черновик~.md",
    ] {
        assert!(!rejected(good), "{good:?}");
    }
}

#[test]
fn component_accepts_ordinary_names() {
    for good in [
        "Заметка.md",
        "v1.2.md",
        "Проекты",
        ".скрытая",
        "Встреча 2.md",
        "a",
    ] {
        assert!(!rejected(good), "{good:?}");
    }
}

/// Все операции с путём от интерфейса. Каждая получает «плохой» путь как
/// путь элемента или как папку назначения.
fn every_operation(t: &TestVault, bad: &VaultPath) -> Vec<(&'static str, ErrorCode)> {
    let v = &t.vault;
    let any_version = FileVersion {
        hash: "0".repeat(64),
        size: 0,
        modified_ms: 0,
    };
    let note = |name: &str| bad.components().iter().cloned().chain([name.to_owned()]);
    let inside = VaultPath::from_components(note("x.md"));
    vec![
        ("read_note", code(v.read_note(bad))),
        ("write_note", code(v.write_note(bad, "взлом", &any_version))),
        ("write_note внутрь", code(v.write_note(&inside, "взлом", &any_version))),
        ("create_note", code(v.create_note(bad, "Новая.md"))),
        ("create_folder", code(v.create_folder(bad, "Новая"))),
        ("check_new_name", code(v.check_new_name(bad, "Новая.md", None))),
        ("rename", code(v.rename(bad, "Другое.md"))),
        ("move_entry откуда", code(v.move_entry(bad, &VaultPath::root()))),
        ("move_entry куда", code(v.move_entry(&vp("Своя.md"), bad))),
        ("trash", code(v.trash(bad, &|_| true))),
    ]
}

fn assert_never_escapes(t: &TestVault, bad: &str) {
    let bad = vp(bad);
    for (operation, code) in every_operation(t, &bad) {
        assert!(
            matches!(code, ErrorCode::InvalidPath | ErrorCode::OutsideVault),
            "{operation} {bad:?}: {code:?}"
        );
    }
    assert_eq!(
        std::fs::read(t.outside().join("secret.md")).expect("секрет на месте"),
        b"\xd1\x81\xd0\xb5\xd0\xba\xd1\x80\xd0\xb5\xd1\x82\n",
        "файл снаружи не тронут"
    );
    let outside: Vec<_> = std::fs::read_dir(t.outside())
        .expect("снаружи")
        .map(|e| e.expect("элемент").file_name())
        .collect();
    assert_eq!(outside.len(), 1, "снаружи ничего не появилось: {outside:?}");
}

#[test]
fn traversal_and_bad_components_never_leave_the_vault() {
    let t = TestVault::new();
    t.put("Своя.md", "своя\n");
    for bad in [
        "../outside/secret.md",
        "../outside",
        "Папка/../../outside",
        "./Своя.md",
        "nul/x.md",
        "Своя.md:поток",
        "C:",
    ] {
        assert_never_escapes(&t, bad);
    }
    assert_never_escapes_components(&t, &["\\\\?\\C:\\Windows"]);
    assert_never_escapes_components(&t, &["/etc"]);
    let absolute = t.outside().join("secret.md").display().to_string();
    assert_never_escapes_components(&t, &[absolute.as_str()]);
}

fn assert_never_escapes_components(t: &TestVault, components: &[&str]) {
    let bad = VaultPath::from_components(components.iter().copied());
    for (operation, code) in every_operation(t, &bad) {
        assert!(
            matches!(code, ErrorCode::InvalidPath | ErrorCode::OutsideVault),
            "{operation} {bad:?}: {code:?}"
        );
    }
}

#[test]
fn symlinks_never_leave_the_vault() {
    let t = TestVault::new();
    t.put("Своя.md", "своя\n");
    std::fs::create_dir(t.file("Папка")).expect("папка");
    symlink_dir(&t.outside(), &t.file("ссылка"));
    symlink_dir(&t.outside(), &t.file("Папка/вглубь"));
    symlink_file(&t.outside().join("secret.md"), &t.file("секрет.md"));

    for bad in ["ссылка", "ссылка/secret.md", "Папка/вглубь/secret.md", "секрет.md"] {
        assert_never_escapes(&t, bad);
    }
}

#[cfg(windows)]
#[test]
fn ntfs_junctions_never_leave_the_vault() {
    let t = TestVault::new();
    t.put("Своя.md", "своя\n");
    std::fs::create_dir(t.file("Папка")).expect("папка");
    junction::create(t.outside(), t.file("соединение")).expect("точка соединения");
    junction::create(t.outside(), t.file("Папка/вглубь")).expect("точка соединения");

    for bad in [
        "соединение",
        "соединение/secret.md",
        "Папка/вглубь/secret.md",
    ] {
        assert_never_escapes(&t, bad);
    }
}

#[test]
fn service_folder_is_unreachable_by_any_spelling() {
    let t = TestVault::new();
    std::fs::write(t.root().join(".rhizomind").join("settings.json"), "{}").expect("настройки");
    for bad in [
        ".rhizomind",
        ".rhizomind/settings.json",
        ".Rhizomind/settings.json",
        ".RHIZOMIND/settings.json",
        ".rhizomind/tmp",
        "RHIZOM~1/settings.json",
    ] {
        assert_never_escapes(&t, bad);
        assert!(
            matches!(
                code(t.vault.read_note(&vp(bad))),
                ErrorCode::InvalidPath | ErrorCode::OutsideVault
            ),
            "{bad}"
        );
    }
    assert_eq!(
        std::fs::read(t.root().join(".rhizomind").join("settings.json")).expect("на месте"),
        b"{}"
    );
}

#[test]
fn link_to_service_folder_inside_vault_is_rejected() {
    let t = TestVault::new();
    symlink_dir(&t.root().join(".rhizomind"), &t.file("служебная"));
    for bad in ["служебная", "служебная/settings.json"] {
        assert_never_escapes(&t, bad);
    }
}

#[test]
fn vault_root_itself_cannot_be_renamed_moved_or_trashed() {
    let t = TestVault::new();
    let root = VaultPath::root();
    assert_eq!(code(t.vault.rename(&root, "Другое")), ErrorCode::InvalidPath);
    assert_eq!(
        code(t.vault.move_entry(&root, &root)),
        ErrorCode::InvalidPath
    );
    assert_eq!(code(t.vault.trash(&root, &|_| true)), ErrorCode::InvalidPath);
    assert!(t.root().is_dir());
}
