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
    if uri.host_str().is_some_and(|host| host != "localhost") || uri.path().starts_with("//") {
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
        if text.starts_with(r"\\")
            || text.starts_with("//")
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
