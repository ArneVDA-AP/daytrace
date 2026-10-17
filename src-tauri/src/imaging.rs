//! Image helpers on the capture hot path. A 2x2 box average handles the common exact halvings
//! (2880x1440 → 1440x720 style, e.g. 1440p monitors to 720p) in a few milliseconds without the
//! large float buffer of a Triangle resize; other ratios fall back to the image crate.

use image::{imageops::FilterType, DynamicImage, RgbImage, RgbaImage};

/// Halve an RGBA image by averaging 2x2 blocks, producing RGB.
fn box_half_rgba(src: &RgbaImage) -> RgbImage {
    let (w, h) = (src.width() / 2, src.height() / 2);
    let s = src.as_raw();
    let stride = src.width() as usize * 4;
    let mut out = vec![0u8; w as usize * h as usize * 3];
    for y in 0..h as usize {
        let r0 = &s[(2 * y) * stride..(2 * y + 1) * stride];
        let r1 = &s[(2 * y + 1) * stride..(2 * y + 2) * stride];
        let o = &mut out[y * w as usize * 3..(y + 1) * w as usize * 3];
        for x in 0..w as usize {
            let i = x * 8;
            for c in 0..3 {
                let sum = r0[i + c] as u16 + r0[i + 4 + c] as u16 + r1[i + c] as u16 + r1[i + 4 + c] as u16;
                o[x * 3 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    RgbImage::from_raw(w, h, out).expect("buffer size matches")
}

/// Downscale to at most `target_h` pixels high (aspect kept), returning RGB.
pub fn downscale_rgba(img: RgbaImage, target_h: u32) -> RgbImage {
    if img.height() <= target_h {
        return DynamicImage::ImageRgba8(img).to_rgb8();
    }
    if img.height() == target_h * 2 {
        return box_half_rgba(&img);
    }
    let w = ((img.width() as f64 * target_h as f64 / img.height() as f64).round() as u32).max(1);
    DynamicImage::ImageRgba8(img).resize_exact(w, target_h, FilterType::Triangle).to_rgb8()
}

/// Downscale an RGB image to at most `target_h` high.
pub fn downscale_rgb(img: RgbImage, target_h: u32) -> RgbImage {
    if img.height() <= target_h {
        return img;
    }
    if img.height() == target_h * 2 {
        let rgba = DynamicImage::ImageRgb8(img).to_rgba8();
        return box_half_rgba(&rgba);
    }
    let w = ((img.width() as f64 * target_h as f64 / img.height() as f64).round() as u32).max(1);
    DynamicImage::ImageRgb8(img).resize_exact(w, target_h, FilterType::Triangle).to_rgb8()
}

/// JPEG-encode with 4:2:0 chroma subsampling (SIMD encoder).
pub fn encode_jpeg(img: &RgbImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity((img.width() * img.height() / 8) as usize);
    let mut enc = jpeg_encoder::Encoder::new(&mut buf, quality);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    enc.encode(img.as_raw(), img.width() as u16, img.height() as u16, jpeg_encoder::ColorType::Rgb)?;
    Ok(buf)
}

/// Anthropic image token estimate: one token per 28x28 patch.
pub fn image_tokens(w: u32, h: u32) -> usize {
    (w.div_ceil(28) * h.div_ceil(28)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn box_half_averages() {
        let mut img = RgbaImage::new(4, 2);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba([if x < 2 { 0 } else { 200 }, 100, 50, 255]);
        }
        let out = downscale_rgba(img, 1);
        assert_eq!(out.dimensions(), (2, 1));
        assert_eq!(out.get_pixel(0, 0).0, [0, 100, 50]);
        assert_eq!(out.get_pixel(1, 0).0, [200, 100, 50]);
        let jpg = encode_jpeg(&out, 80).unwrap();
        assert!(image::load_from_memory(&jpg).is_ok());
        assert_eq!(image_tokens(1720, 720), 62 * 26);
    }
}
