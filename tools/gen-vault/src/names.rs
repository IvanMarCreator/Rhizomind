//! Имена файлов и папок: ключ имени и правила Windows (спецификация С3.2).

use unicode_normalization::UnicodeNormalization;

/// Ключ имени: NFC → нижний регистр → NFC. Совпадение ключей — одно и то же
/// имя для ссылок и для проверки дубликатов в папке (С3.2, С3.4).
pub fn name_key(name: &str) -> String {
    let nfc: String = name.nfc().collect();
    nfc.to_lowercase().nfc().collect()
}

const FORBIDDEN: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Имя допустимо в Windows: без запрещённых и управляющих символов, не
/// зарезервировано (в том числе с расширением), без точки или пробела в конце.
pub fn is_valid_windows_name(name: &str) -> bool {
    if name.is_empty() || name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    if name.chars().any(|c| FORBIDDEN.contains(&c) || c.is_control()) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end();
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}
