fn main() {
    // Свои команды ядра перечисляются явно: команда вне этого списка интерфейсу
    // недоступна (ADR 0009). Пока команд нет; список заполняет задача 0.4.
    let manifest = tauri_build::AppManifest::new().commands(&[]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri-build: не удалось подготовить сборку");
}
