use super::*;
use crate::commands::{AttachmentOutcome, Settings};
use crate::vault::AttachmentInput;

#[test]
fn attachment_rules_choose_internal_folders_and_never_overwrite_or_link_after_failure() {
    let t = TestVault::new();
    t.put("Протокол.md", b"draft");
    t.put("Media/Скриншот.png", b"keep");
    t.vault
        .write_settings(&Settings {
            attachment_folders: [("png".into(), vp("Media"))].into(),
            ..Default::default()
        })
        .unwrap();
    let insert = |name| {
        t.vault.insert_attachment(
            &vp("Протокол.md"),
            AttachmentInput::Png(b"direct screenshot bytes"),
            name,
        )
    };
    assert!(
        matches!(insert("Скриншот.PNG").unwrap(), AttachmentOutcome::Created {path: Some(p), ..} if p == vp("Media/Скриншот 2.PNG"))
    );
    assert_eq!(t.bytes("Media/Скриншот.png"), b"keep");
    assert_eq!(t.bytes("Media/Скриншот 2.PNG"), b"direct screenshot bytes");
    assert!(
        t.vault
            .own_writes
            .is_own(&vp("Media/Скриншот 2.PNG"), &t.file("Media/Скриншот 2.PNG"))
    );
    assert!(insert("плохо#имя.png").is_err());
    assert!(!t.file("Media/плохо#имя.png").exists());
    std::fs::rename(t.file("Media"), t.file("Архив")).unwrap();
    assert!(insert("Фото.png").is_err());
    assert!(!t.file("Фото.png").exists());
    assert!(!t.file("Media").exists());
    assert_eq!(t.bytes("Протокол.md"), b"draft");
}

#[test]
fn attachment_same_name_created_just_before_publication_is_preserved() {
    let t = TestVault::new();
    t.put("Заметка.md", b"draft");
    let occupied = t.file("Вложение.png");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        std::fs::write(occupied, b"other").unwrap()
    }));
    let result = t
        .vault
        .insert_attachment(
            &vp("Заметка.md"),
            AttachmentInput::Png(b"mine"),
            "Вложение.png",
        )
        .unwrap();
    assert!(
        matches!(result,AttachmentOutcome::Created {path:Some(path),..} if path == vp("Вложение 2.png"))
    );
    assert_eq!(t.bytes("Вложение.png"), b"other");
    assert_eq!(t.bytes("Вложение 2.png"), b"mine");
    assert!(
        !t.vault
            .own_writes
            .is_own(&vp("Вложение.png"), &t.file("Вложение.png"))
    );
}

#[test]
fn attachment_does_not_follow_a_directory_link_outside_the_vault() {
    let t = TestVault::new();
    t.put("План.md", b"original");
    let link = t.file("Другие");
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(t.outside())
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[cfg(target_os = "linux")]
    symlink_dir(&t.outside(), &link);
    assert!(
        t.vault
            .insert_attachment(
                &vp("План.md"),
                AttachmentInput::File(&vp("Другие/secret.md")),
                "Секрет.txt"
            )
            .is_err()
    );
    assert!(!t.file("Секрет.txt").exists());
}

#[test]
fn attachment_observes_a_second_filename_that_appears_during_publication() {
    let t = TestVault::new();
    t.put("Работа/План.md", b"note");
    t.put("Исходники/Список.txt", b"other");
    let added = t.file("Исходники/Фото.png");
    crate::vault::atomic::after_temp_written(Box::new(move || {
        std::fs::write(added, b"other").unwrap()
    }));
    let outcome = t
        .vault
        .insert_attachment(
            &vp("Работа/План.md"),
            AttachmentInput::Png(b"mine"),
            "Фото.png",
        )
        .unwrap();
    assert!(
        matches!(outcome, AttachmentOutcome::Created {target:Some(target),..} if target == "Работа/Фото.png")
    );
    assert_eq!(t.bytes("Исходники/Фото.png"), b"other");
    assert_eq!(t.bytes("Работа/Фото.png"), b"mine");
}

#[test]
fn attachment_disk_error_preserves_source_and_leaves_no_copy() {
    let t = TestVault::new();
    t.put("Заметка.md", b"draft");
    t.put("Исходники/Фото.png", b"unchanged");
    std::fs::rename(t.file(".rhizomind/tmp"), t.file(".rhizomind/saved-tmp")).unwrap();
    std::fs::write(t.file(".rhizomind/tmp"), b"no writable staging directory").unwrap();
    assert!(
        t.vault
            .insert_attachment(
                &vp("Заметка.md"),
                AttachmentInput::File(&vp("Исходники/Фото.png")),
                "Фото.png"
            )
            .is_err()
    );
    assert_eq!(t.bytes("Исходники/Фото.png"), b"unchanged");
    assert_eq!(t.bytes("Заметка.md"), b"draft");
    assert!(!t.file("Фото.png").exists());
}
