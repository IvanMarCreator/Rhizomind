//! Текст заметки и его вид на диске: BOM и переносы строк (правило «байт в
//! байт», С3.3, `commands::note`).
//!
//! Интерфейс работает с текстом без BOM и с переносами `\n`. Ядро помнит
//! вид файла на диске и при записи возвращает тексту тот же вид.

/// Метка порядка байтов UTF-8.
pub const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Текст файла для интерфейса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Без BOM, `\r\n` → `\n` (одиночный `\r` остаётся как есть).
    pub text: String,
    /// Файл не в UTF-8: неверные байты заменены на U+FFFD.
    pub invalid_utf8: bool,
}

/// Байты файла → текст для интерфейса.
pub fn decode(bytes: &[u8]) -> Decoded {
    let body = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let (text, invalid_utf8) = match std::str::from_utf8(body) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (String::from_utf8_lossy(body).into_owned(), true),
    };
    Decoded {
        text: normalize(&text),
        invalid_utf8,
    }
}

/// `\r\n` → `\n`.
pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Вид файла на диске: BOM и преобладающие переносы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Format {
    /// Файл начинается с BOM.
    pub bom: bool,
    /// Переносов `\r\n` больше, чем одиночных `\n`.
    pub crlf: bool,
}

impl Format {
    /// Вид существующего файла.
    pub fn of(bytes: &[u8]) -> Self {
        let crlf = bytes.windows(2).filter(|w| *w == b"\r\n").count();
        let lf = bytes.iter().filter(|&&b| b == b'\n').count() - crlf;
        Self {
            bom: bytes.starts_with(BOM),
            crlf: crlf > lf,
        }
    }

    /// Текст из интерфейса → байты файла в этом виде.
    pub fn encode(self, text: &str) -> Vec<u8> {
        let text = normalize(text);
        let body = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        };
        let mut bytes = Vec::with_capacity(body.len() + BOM.len());
        if self.bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_file_round_trips_byte_for_byte() {
        let disk = b"\xEF\xBB\xBF# A\r\nb\r\n".to_vec();
        let decoded = decode(&disk);
        assert_eq!(decoded.text, "# A\nb\n");
        assert_eq!(Format::of(&disk).encode(&decoded.text), disk);
    }

    #[test]
    fn predominant_line_ending_wins_and_ties_go_to_lf() {
        assert!(Format::of(b"a\r\nb\r\nc\n").crlf);
        assert!(!Format::of(b"a\r\nb\nc\n").crlf);
        assert!(!Format::of(b"a\r\nb\n").crlf);
        assert!(!Format::of(b"no breaks").crlf);
    }

    #[test]
    fn invalid_utf8_is_marked_and_replaced() {
        let decoded = decode(b"ok \xFF\xFE end");
        assert!(decoded.invalid_utf8);
        assert!(decoded.text.contains('\u{fffd}'));
    }
}
