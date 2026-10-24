fn main() {
    // Свои команды ядра перечисляются явно: команда вне этого списка интерфейсу
    // недоступна (ADR 0009). Пока команд нет; список заполняет задача 0.4.
    let manifest = tauri_build::AppManifest::new().commands(&[]);

    // Манифест Windows встраивается компоновщиком во все исполняемые файлы
    // крейта, а не только в приложение: иначе `cargo test` на Windows падает
    // с STATUS_ENTRYPOINT_NOT_FOUND (нужны Common Controls 6).
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    embed_windows_manifest();

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(manifest)
            .windows_attributes(windows),
    )
    .expect("tauri-build: не удалось подготовить сборку");
}

fn embed_windows_manifest() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(&dir).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", path.display());
}
