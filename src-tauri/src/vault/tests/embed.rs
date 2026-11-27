use super::{TestVault, vp as p};
use crate::commands::{EmbedOutcome, EmbedReason};

#[test]
fn embed_reads_only_the_requested_existing_object() {
    let fx = TestVault::new();
    fx.put("Проект/Схема.svg", "<svg>маркер</svg>");
    assert!(
        matches!(fx.vault.embed_path(&p("Проект/Схема.svg")).unwrap(),
        EmbedOutcome::File {name} if name == "Схема.svg")
    );
    assert!(fx.vault.embed_path(&p("../outside/secret.md")).is_err());
    assert!(fx.vault.embed_path(&p(".rhizomind/notes.md")).is_err());
    assert!(!fx.file("Нет.md").exists());
}

#[test]
fn embed_attachment_duplicates_require_a_distinguishing_suffix() {
    let fx = TestVault::new();
    for path in ["A/Схема.png", "Б/Схема.png", "Схема.png"] {
        fx.put(path, b"");
    }
    let resolve = |path, rooted| fx.vault.embed_attachment_target(&p(path), rooted).unwrap();
    assert_eq!(resolve("схема.PNG", false), Err(EmbedReason::Ambiguous));
    assert_eq!(resolve("б/схема.png", false), Ok(p("Б/Схема.png")));
    assert_eq!(resolve("схема.PNG", true), Ok(p("Схема.png")));
    assert_eq!(resolve("нет.png", false), Err(EmbedReason::Missing));
    assert!(!fx.file("нет.png").exists());
}

#[test]
fn embed_checks_the_selected_path_again_on_open() {
    let fx = TestVault::new();
    fx.put(
        "Выбор.png",
        include_bytes!("../../../../tests/fixtures/embeds/sample.png"),
    );
    let selected = fx
        .vault
        .embed_attachment_target(&p("Выбор.png"), false)
        .unwrap()
        .unwrap();
    std::fs::rename(fx.file("Выбор.png"), fx.file("Прежний.png")).unwrap();
    std::fs::hard_link(fx.outside().join("secret.md"), fx.file("Выбор.png")).unwrap();
    assert!(fx.vault.embed_path(&selected).is_err());
}
#[test]
fn embed_refuses_an_outside_hard_link() {
    let fx = TestVault::new();
    std::fs::hard_link(fx.outside().join("secret.md"), fx.file("План.md")).unwrap();
    assert!(fx.vault.embed_path(&p("План.md")).is_err());
}

#[test]
fn embed_input_and_markdown_byte_limits_have_inclusive_bounds() {
    let fx = TestVault::new();
    for (name, limit) in [("Предел.png", 8 * 1024 * 1024), ("Предел.md", 256 * 1024)] {
        let path = fx.put(name, b"");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(limit)
            .unwrap();
        let at = fx.vault.embed_path(&p(name)).unwrap();
        assert!(!matches!(
            at,
            EmbedOutcome::Unavailable {
                reason: EmbedReason::TooLarge
            }
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(limit + 1)
            .unwrap();
        assert!(matches!(
            fx.vault.embed_path(&p(name)).unwrap(),
            EmbedOutcome::Unavailable {
                reason: EmbedReason::TooLarge
            }
        ));
    }
}
#[test]
fn embed_rejects_damaged_image_and_invalid_utf8() {
    let fx = TestVault::new();
    fx.put("Ложная.png", b"not a png");
    fx.put("Текст.md", [255, 255]);
    for rel in ["Ложная.png", "Текст.md"] {
        assert!(matches!(
            fx.vault.embed_path(&p(rel)).unwrap(),
            EmbedOutcome::Unavailable {
                reason: EmbedReason::InvalidData
            }
        ));
    }
}
