//! Проверка заголовка и строк DIB до выделения памяти для RGB.
use super::PasteData;

pub(super) fn to_png(dib: &[u8]) -> Result<PasteData, String> {
    let invalid = || "неподдерживаемая битовая карта скриншота".to_owned();
    let word = |at: usize| {
        dib.get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
            .ok_or_else(invalid)
    };
    let header = word(0)? as usize;
    if !matches!(header, 40 | 52 | 56 | 108 | 124) || header > dib.len() {
        return Err(invalid());
    }
    let width = word(4)? as i32;
    let height = word(8)? as i32;
    let format = word(12)?;
    let depth = format >> 16;
    let compression = word(16)?;
    if width <= 0
        || height == 0
        || format & 0xffff != 1
        || !matches!(depth, 24 | 32)
        || !matches!(compression, 0 | 3)
        || word(32)? != 0
    {
        return Err(invalid());
    }
    if compression == 3 && (word(40)? != 0xff0000 || word(44)? != 0xff00 || word(48)? != 0xff) {
        return Err(invalid());
    }
    let start = if header == 40 && compression == 3 {
        52
    } else {
        header
    };
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    if w.checked_mul(h)
        .is_none_or(|pixels| pixels > 16 * 1024 * 1024)
    {
        return Ok(PasteData::TooLarge);
    }
    let pixel = depth as usize / 8;
    let stride = (w * pixel).next_multiple_of(4);
    let rows = dib.get(start..start + stride * h).ok_or_else(invalid)?;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let row = if height > 0 { h - 1 - y } else { y };
        for p in rows[row * stride..row * stride + w * pixel].chunks_exact(pixel) {
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        }
    }
    let mut image = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut image, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        let mut writer = encoder.write_header().map_err(|_| invalid())?;
        writer.write_image_data(&rgb).map_err(|_| invalid())?;
        writer.finish().map_err(|_| invalid())?;
    }
    Ok(PasteData::Png(image))
}

#[test]
fn attachment_dib_reads_padded_bgr_and_rejects_excessive_dimensions() {
    let mut dib = vec![0; 44];
    dib[..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&1i32.to_le_bytes());
    dib[8..12].copy_from_slice(&1i32.to_le_bytes());
    dib[12..16].copy_from_slice(&(24u32 << 16 | 1).to_le_bytes());
    dib[40..].copy_from_slice(&[0x10, 0x20, 0x30, 0x00]);
    let PasteData::Png(file) = to_png(&dib).unwrap() else {
        panic!("PNG")
    };
    let mut decoder = png::Decoder::new(std::io::Cursor::new(file))
        .read_info()
        .unwrap();
    let mut rgb = [0; 3];
    decoder.next_frame(&mut rgb).unwrap();
    assert_eq!(rgb, [0x30, 0x20, 0x10]);
    dib[4..8].copy_from_slice(&i32::MAX.to_le_bytes());
    assert!(matches!(to_png(&dib).unwrap(), PasteData::TooLarge));
}
