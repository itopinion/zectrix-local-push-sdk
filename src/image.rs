use crate::{DisplayPalette, Error, ImageFormat, Result};
use std::io::{Cursor, Write};
fn linear_channel(value: u8) -> f32 {
    let value = value as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub fn prepare(png: &[u8], format: ImageFormat, palette: &DisplayPalette) -> Result<Vec<u8>> {
    let bad = |e: String| Error::InvalidImage(e);
    if png.len() > 8 * 1024 * 1024 {
        return Err(bad("PNG 超过 8 MiB".into()));
    }
    let mut reader = ::image::ImageReader::with_format(Cursor::new(png), ::image::ImageFormat::Png);
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| bad(e.to_string()))?
        .resize(400, 300, ::image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let x = (400 - img.width()) / 2;
    let y = (300 - img.height()) / 2;
    let colors: &[u8] = match format {
        ImageFormat::Mono => &[0, 255],
        ImageFormat::Gray4 => &palette.gray4,
        ImageFormat::Gray16 => &palette.gray16,
    };
    // PNG samples are treated as sRGB. Palette values and error diffusion use linear light.
    let linear: [f32; 256] = std::array::from_fn(|i| linear_channel(i as u8));
    let mut pixels = vec![255.0f32; 400 * 300];
    for (px, py, p) in img.enumerate_pixels() {
        let alpha = p[3] as f32 / 255.0;
        let luminance = 255.0
            * (0.2126 * linear[p[0] as usize]
                + 0.7152 * linear[p[1] as usize]
                + 0.0722 * linear[p[2] as usize]);
        pixels[((y + py) * 400 + x + px) as usize] = luminance * alpha + 255.0 * (1.0 - alpha);
    }
    let raw = pack_dithered(&mut pixels, format, colors);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    z.write_all(&raw)?;
    let data = z.finish()?;
    if data.len() > u16::MAX as usize {
        return Err(bad("压缩图像超过设备容量".into()));
    }
    Ok(data)
}

fn pack_dithered(pixels: &mut [f32], format: ImageFormat, colors: &[u8]) -> Vec<u8> {
    let bpp = format as usize;
    let mut raw = vec![0u8; 15000 * bpp];
    // Floyd–Steinberg error diffusion is performed on the remote host only.
    for row in 0..300usize {
        for col in 0..400usize {
            let i = row * 400 + col;
            let value = pixels[i].clamp(0.0, 255.0);
            let mut level = 0u8;
            let mut distance = f32::MAX;
            for (index, &brightness) in colors.iter().enumerate() {
                let candidate = (value - brightness as f32).abs();
                // Strict comparison gives equal/duplicate entries a stable lowest-code tie break.
                if candidate < distance {
                    distance = candidate;
                    level = index as u8;
                }
            }
            let error = value - colors[level as usize] as f32;
            let packed = if bpp == 1 { 1 - level } else { level };
            raw[i * bpp / 8] |= packed << (8 - bpp - (i * bpp % 8));
            if col + 1 < 400 {
                pixels[i + 1] += error * 7.0 / 16.0;
            }
            if row + 1 < 300 {
                if col > 0 {
                    pixels[i + 399] += error * 3.0 / 16.0;
                }
                pixels[i + 400] += error * 5.0 / 16.0;
                if col + 1 < 400 {
                    pixels[i + 401] += error / 16.0;
                }
            }
        }
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    const UNIFORM: DisplayPalette = DisplayPalette {
        gray4: [0, 85, 170, 255],
        gray16: [
            0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
        ],
    };
    #[test]
    fn full_canvas_includes_the_top_rows() {
        let image = ::image::RgbImage::from_pixel(400, 300, ::image::Rgb([0, 0, 0]));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
        let packed = prepare(png.get_ref(), ImageFormat::Mono, &UNIFORM).unwrap();
        let mut raw = vec![];
        flate2::read::ZlibDecoder::new(packed.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), 15000);
        assert!(raw.iter().all(|b| *b == 255));
    }
    #[test]
    fn full_size_pixels_keep_their_coordinates_at_all_edges() {
        let mut image = ::image::RgbImage::from_pixel(400, 300, ::image::Rgb([255, 255, 255]));
        let points = [(0, 0), (399, 0), (0, 299), (399, 299), (100, 31), (100, 32)];
        for (x, y) in points {
            image.put_pixel(x, y, ::image::Rgb([0, 0, 0]));
        }
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
        let packed = prepare(png.get_ref(), ImageFormat::Mono, &UNIFORM).unwrap();
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(packed.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let mut expected = vec![0u8; 15000];
        for (x, y) in points {
            expected[(y * 50 + x / 8) as usize] |= 1 << (7 - x % 8);
        }
        assert_eq!(raw, expected);
    }
    #[test]
    fn invalid_png_does_not_allocate_a_frame() {
        assert!(prepare(b"not PNG", ImageFormat::Mono, &UNIFORM).is_err());
    }
    #[test]
    fn native_gray_levels_are_packed_msb_first_without_second_quantization() {
        for (format, levels, row_bytes) in
            [(ImageFormat::Gray4, 4, 100), (ImageFormat::Gray16, 16, 200)]
        {
            let mut pixels: Vec<f32> = (0..120000)
                .map(|i| (i % levels * 255 / (levels - 1)) as f32)
                .collect();
            let colors: &[u8] = if format == ImageFormat::Gray4 {
                &UNIFORM.gray4
            } else {
                &UNIFORM.gray16
            };
            let raw = pack_dithered(&mut pixels, format, colors);
            assert_eq!(raw.len(), row_bytes * 300);
            if format == ImageFormat::Gray4 {
                assert!(raw.iter().all(|b| *b == 0x1b));
            } else {
                for row in raw.chunks_exact(row_bytes) {
                    for (i, byte) in row.iter().enumerate() {
                        assert_eq!(*byte, ((i * 2 % 16) << 4 | (i * 2 + 1) % 16) as u8);
                    }
                }
            }
        }
    }
    #[test]
    fn host_dithering_preserves_midgray_average_in_mono() {
        let image = ::image::RgbImage::from_pixel(400, 300, ::image::Rgb([128, 128, 128]));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
        let compressed = prepare(png.get_ref(), ImageFormat::Mono, &UNIFORM).unwrap();
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let black: u32 = raw.iter().map(|b| b.count_ones()).sum();
        // sRGB 128 is about 21.6% linear light, not 50%.
        assert!((93500..94700).contains(&black));
    }
    #[test]
    fn nonuniform_palette_drives_both_quantization_and_error_diffusion() {
        let colors = [0, 40, 180, 255];
        let raw = pack_dithered(&mut vec![80.0; 120000], ImageFormat::Gray4, &colors);
        let mut luminance = 0u64;
        for byte in raw {
            for shift in [6, 4, 2, 0] {
                luminance += colors[((byte >> shift) & 3) as usize] as u64;
            }
        }
        assert!((luminance as f64 / 120000.0 - 80.0).abs() < 0.5);
        // Exact real-device palette entry must be emitted unchanged, not rounded to uniform gray.
        let raw = pack_dithered(&mut vec![40.0; 120000], ImageFormat::Gray4, &colors);
        assert!(raw.iter().all(|byte| *byte == 0x55));
    }
    #[test]
    fn duplicate_palette_entries_choose_the_lowest_code() {
        let raw = pack_dithered(
            &mut vec![85.0; 120000],
            ImageFormat::Gray4,
            &[0, 85, 85, 255],
        );
        assert!(raw.iter().all(|byte| *byte == 0x55));
    }
}
