//! Чистое лексическое сопоставление текста ОС с уже известным корнем.
//! Ни преобразование URI, ни разбор компонентов не обращаются к файловой системе.
use super::contour;
use crate::commands::{VaultPath, checked_path};
use std::path::{Component, Path, PathBuf};

fn file_uri(text: &str) -> Option<PathBuf> {
    if text.contains(['\\', '?', '#']) || !text.get(5..)?.starts_with('/') {
        return None;
    }
    let bytes = text.as_bytes();
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte != b'%' {
            continue;
        }
        let hex = text.get(offset + 1..offset + 3)?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        if matches!(u8::from_str_radix(hex, 16).ok()?, b'/' | b'\\' | 0) {
            return None;
        }
    }
    // URL разбирает и нормализует . и .. самостоятельно; отвергаем их до разбора.
    if text.split('/').any(|part| {
        matches!(
            part.to_ascii_lowercase().replace("%2e", ".").as_str(),
            "." | ".."
        )
    }) {
        return None;
    }
    let uri = url::Url::parse(text).ok()?;
    if uri.path().starts_with("//") {
        return None;
    }
    uri.to_file_path().ok()
}

pub(super) fn relative(root: &Path, reference: &str) -> Option<VaultPath> {
    let text = reference.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(text);
    if text.len() > 128 * 1024 || text.chars().any(char::is_control) {
        return None;
    }
    let path = if text
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
    {
        file_uri(text)?
    } else {
        if (!cfg!(windows) && (text.starts_with("//") || text.starts_with(r"\\")))
            || text
                .split(['/', '\\'])
                .any(|part| matches!(part, "." | ".."))
        {
            return None;
        }
        PathBuf::from(text)
    };
    if !path.is_absolute() {
        return None;
    }
    // Разрешены только обычный путь на диске и обычный UNC. Из буфера не
    // принимаются пространства имён устройств и verbatim-префиксы. Уже
    // открытый корень может быть каноническим: strip_root сравнивает компоненты.
    #[cfg(windows)]
    if !matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(..))
    ) {
        return None;
    }
    let tail = contour::strip_root(root, &path)?;
    let parts = tail
        .components()
        .map(|part| match part {
            Component::Normal(part) => part.to_str().filter(|part| !part.starts_with('.')),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        return None;
    }
    let path = VaultPath::from_components(parts);
    checked_path(&path, "source.path").ok()?;
    Some(path)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn attachment_clipboard_accepts_only_matching_open_unc_root_without_io() {
        let plain = Path::new(r"\\rhz-fixture.invalid\Share\Vault");
        let canonical = Path::new(r"\\?\UNC\rhz-fixture.invalid\Share\Vault");
        let raw = r"\\RHZ-FIXTURE.INVALID\share\vault\Папка\Фото +1.png";
        let uri =
            "file://rhz-fixture.invalid/Share/Vault/%D0%9F%D0%B0%D0%BF%D0%BA%D0%B0/Фото%20+1.png";
        let expected = VaultPath::from_components(["Папка", "Фото +1.png"]);

        for root in [plain, canonical] {
            for valid in [raw, uri] {
                assert_eq!(relative(root, valid), Some(expected.clone()), "{valid}");
            }
            for outside in [
                r"\\other.invalid\Share\Vault\file.png",
                r"\\rhz-fixture.invalid\Share2\Vault\file.png",
                r"\\rhz-fixture.invalid\Share\Vault-copy\file.png",
                r"\\?\UNC\rhz-fixture.invalid\Share\Vault\file.png",
                r"\\.\UNC\rhz-fixture.invalid\Share\Vault\file.png",
                r"\\?\GLOBALROOT\Device\file.png",
                "file://other.invalid/Share/Vault/file.png",
                "file://rhz-fixture.invalid/Share2/Vault/file.png",
                "file://rhz-fixture.invalid/Share/Vault-copy/file.png",
                "file://rhz-fixture.invalid/Share/Vault/a%2fb.png",
                "file://rhz-fixture.invalid/Share/Vault/%2e%2e/file.png",
            ] {
                assert_eq!(relative(root, outside), None, "{outside}");
            }
        }
        for network in [raw, uri] {
            assert_eq!(relative(Path::new(r"C:\Vault"), network), None);
        }
    }
}
