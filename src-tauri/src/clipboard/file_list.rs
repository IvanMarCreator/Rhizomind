use super::PasteData;

/// Получаем только одно имя из данных стандартного URI-списка или списка GNOME.
/// Несколько имён (в том числе смешанный список) не запускают копирование.
pub(super) fn parse(bytes: &[u8], gnome: bool) -> PasteData {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return PasteData::ExternalPath;
    };
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    if gnome && !matches!(lines.next(), Some("copy" | "cut")) {
        return PasteData::ExternalPath;
    }
    match (lines.next(), lines.next()) {
        (Some(uri), None)
            if uri
                .get(..5)
                .is_some_and(|s| s.eq_ignore_ascii_case("file:")) =>
        {
            PasteData::FileReference(uri.to_owned())
        }
        _ => PasteData::ExternalPath,
    }
}

#[test]
fn attachment_clipboard_uri_list_accepts_one_file_without_requesting_its_content() {
    assert!(
        matches!(parse(b"#selection\r\nfile:///x%20y\r\n",false), PasteData::FileReference(uri) if uri == "file:///x%20y")
    );
    assert!(matches!(
        parse(b"copy\nfile:///x", true),
        PasteData::FileReference(_)
    ));
    for data in [
        b"file:///a\nfile:///b".as_slice(),
        b"https://example.invalid/private",
        b"\xff",
    ] {
        assert!(matches!(parse(data, false), PasteData::ExternalPath));
    }
}
