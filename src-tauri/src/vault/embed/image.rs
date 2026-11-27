//! В UI попадает только однокадровый PNG, построенный из декодированных пикселей.
use crate::commands::{EmbedOutcome, EmbedReason};
use base64::prelude::*;
use std::io::Cursor;

pub(super) const MAX_INPUT: u64 = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_SIDE: u32 = 8192;
const MAX_FRAME: usize = MAX_PIXELS as usize * 4;

fn check_size(w: u32, h: u32) -> Result<(), EmbedReason> {
    if w == 0 || h == 0 {
        return Err(EmbedReason::InvalidData);
    }
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(EmbedReason::TooLarge);
    }
    Ok(())
}

struct Frame {
    width: u32,
    height: u32,
    color: png::ColorType,
    bytes: Vec<u8>,
}

fn png_frame(bytes: &[u8]) -> Result<Frame, EmbedReason> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: MAX_FRAME });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let header = decoder.read_header_info().map_err(png_error)?;
    check_size(header.width, header.height)?;
    let mut reader = decoder.read_info().map_err(png_error)?;
    if reader.info().animation_control.is_some() {
        return Err(EmbedReason::Unsupported);
    }
    let n = reader
        .output_buffer_size()
        .filter(|n| *n <= MAX_FRAME)
        .ok_or(EmbedReason::TooLarge)?;
    let mut data = vec![0; n];
    let out = reader.next_frame(&mut data).map_err(png_error)?;
    reader.finish().map_err(png_error)?;
    data.truncate(out.buffer_size());
    Ok(Frame {
        width: out.width,
        height: out.height,
        color: out.color_type,
        bytes: data,
    })
}

fn png_error(error: png::DecodingError) -> EmbedReason {
    match error {
        png::DecodingError::LimitsExceeded => EmbedReason::TooLarge,
        _ => EmbedReason::InvalidData,
    }
}

fn jpeg_frame(bytes: &[u8]) -> Result<Frame, EmbedReason> {
    use jpeg_decoder::{Decoder, PixelFormat};
    if !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(EmbedReason::InvalidData);
    }
    let mut decoder = Decoder::new(bytes);
    decoder.set_max_decoding_buffer_size(MAX_FRAME);
    decoder.read_info().map_err(|_| EmbedReason::InvalidData)?;
    let info = decoder.info().ok_or(EmbedReason::InvalidData)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    check_size(width, height)?;
    let data = decoder.decode().map_err(|_| EmbedReason::InvalidData)?;
    let (color, data) = match info.pixel_format {
        PixelFormat::RGB24 => (png::ColorType::Rgb, data),
        PixelFormat::L8 => (png::ColorType::Grayscale, data),
        PixelFormat::CMYK32 => {
            let mut rgb = Vec::with_capacity(data.len() / 4 * 3);
            for cmyk in data.chunks_exact(4) {
                let light = 255 - u16::from(cmyk[3]);
                for &ink in &cmyk[..3] {
                    rgb.push((((255 - u16::from(ink)) * light + 127) / 255) as u8);
                }
            }
            (png::ColorType::Rgb, rgb)
        }
        PixelFormat::L16 => return Err(EmbedReason::Unsupported),
    };
    Ok(Frame {
        width,
        height,
        color,
        bytes: data,
    })
}

pub(super) fn decode(
    extension: &str,
    encoded: &[u8],
    name: String,
) -> Result<EmbedOutcome, EmbedReason> {
    if encoded.len() as u64 > MAX_INPUT {
        return Err(EmbedReason::TooLarge);
    }
    let frame = if extension == "png" {
        png_frame(encoded)?
    } else {
        jpeg_frame(encoded)?
    };
    let mut clean = Vec::new();
    let mut encoder = png::Encoder::new(&mut clean, frame.width, frame.height);
    encoder.set_color(frame.color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder
        .write_header()
        .map_err(|_| EmbedReason::InvalidData)?;
    writer
        .write_image_data(&frame.bytes)
        .map_err(|_| EmbedReason::InvalidData)?;
    writer.finish().map_err(|_| EmbedReason::InvalidData)?;
    Ok(EmbedOutcome::Image {
        name,
        width: frame.width,
        height: frame.height,
        png: BASE64_STANDARD.encode(clean),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embed_bounds_apply_before_decoding() {
        for (w, h) in [(2048, 2048), (8192, 512)] {
            assert!(check_size(w, h).is_ok());
        }
        for (w, h) in [(2049, 2048), (8193, 1), (1, 8193), (u32::MAX, u32::MAX)] {
            assert_eq!(check_size(w, h), Err(EmbedReason::TooLarge));
        }
        assert_eq!(check_size(0, 1), Err(EmbedReason::InvalidData));
    }
    #[test]
    fn embed_jpeg_returns_metadata_free_png_pixels() {
        let jpeg = include_bytes!("../../../../tests/fixtures/embeds/sample.jpg");
        let EmbedOutcome::Image {
            png, width, height, ..
        } = decode("jpg", jpeg, "Фото.jpg".into()).unwrap()
        else {
            panic!("JPEG должен открыться")
        };
        assert_eq!((width, height), (2, 1));
        let clean = BASE64_STANDARD.decode(png).unwrap();
        assert!(!clean.windows(14).any(|p| p == b"private-marker"));
        let rgb = png_frame(&clean).unwrap();
        assert_eq!(rgb.bytes.len(), 6);
        assert!(rgb.bytes[2] > rgb.bytes[1] && rgb.bytes[1] > rgb.bytes[0]);
        assert!(decode("jpg", &jpeg[..jpeg.len() - 2], String::new()).is_err());
    }

    #[test]
    fn embed_png_round_trip_discards_private_metadata_and_rejects_truncation() {
        let mut original = Vec::new();
        let mut encoder = png::Encoder::new(&mut original, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .add_text_chunk("Comment".into(), "private-marker".into())
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[19, 42, 99]).unwrap();
        writer.finish().unwrap();
        let EmbedOutcome::Image { png, .. } = decode("png", &original, String::new()).unwrap()
        else {
            panic!("ожидалось изображение")
        };
        let clean = BASE64_STANDARD.decode(png).unwrap();
        assert!(!clean.windows(14).any(|p| p == b"private-marker"));
        assert_eq!(png_frame(&clean).unwrap().bytes, [19, 42, 99]);
        original.truncate(original.len() - 7);
        assert!(decode("png", &original, String::new()).is_err());
    }
}
