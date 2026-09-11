//! PNG decoding + `.cml` frame extraction for sprite rendering.
//!
//! The game's images are indexed (palette) PNGs, often with a `tRNS`
//! transparency chunk. We decode to RGBA8 (palette/low-bit-depth expanded, alpha
//! from `tRNS`) so the renderer can blit frames with transparency.
//!
//! A `.cml` animation frame (see `formats::cml`) is a sub-rectangle of one of
//! these sheets plus a draw offset and an optional horizontal flip — exactly the
//! parameters `g.java::a(Graphics, d, ...)` uses (`var_short_a/b` = source x/y,
//! `var_short_c/d` = width/height, `var_byte_b/c` = offset, `e` = flip).

use anyhow::{Context, Result};
use formats::cml::Flags;

/// An RGBA8 image.
pub struct Png {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Decode a PNG (indexed/grayscale/rgb/rgba, any bit depth) to RGBA8.
pub fn load_png(bytes: &[u8]) -> Result<Png> {
    let mut dec = png::Decoder::new(bytes);
    // EXPAND: palette -> RGB, sub-8-bit grayscale -> 8-bit, tRNS -> alpha.
    // STRIP_16: 16-bit channels -> 8-bit.
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().context("png header")?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("png decode")?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => {
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                out[i * 4] = buf[i * 3];
                out[i * 4 + 1] = buf[i * 3 + 1];
                out[i * 4 + 2] = buf[i * 3 + 2];
                out[i * 4 + 3] = 255;
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                let g = buf[i];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
                out[i * 4 + 3] = 255;
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                let g = buf[i * 2];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
                out[i * 4 + 3] = buf[i * 2 + 1];
            }
            out
        }
        png::ColorType::Indexed => {
            anyhow::bail!("indexed color survived EXPAND (unexpected)")
        }
    };
    Ok(Png { w, h, rgba })
}

/// A `.cml` frame's render parameters, named after the `d.java` fields they map
/// to (`g.java::a(int[], d)`).
#[derive(Debug, Clone, Copy)]
pub struct FrameView {
    pub src_x: i32,  // var_short_a
    pub src_y: i32,  // var_short_b
    pub width: i32,  // var_short_c
    pub height: i32, // var_short_d
    pub off_x: i32,  // var_byte_b
    pub off_y: i32,  // var_byte_c
    pub flip: bool,  // e == 1
}

impl FrameView {
    /// Interpret a `.cml` flag block as a renderable frame.
    pub fn from_flags(f: &Flags) -> Self {
        Self {
            src_x: f[1],
            src_y: f[2],
            width: f[3],
            height: f[4],
            off_x: f[5],
            off_y: f[6],
            flip: f[8] == 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_view_maps_cml_fields() {
        // [id, src_x, src_y, w, h, off_x, off_y, loop, flip, _]
        let f: Flags = [1, 105, 0, 23, 39, 2, 0, 0, 1, 0];
        let v = FrameView::from_flags(&f);
        assert_eq!((v.src_x, v.src_y, v.width, v.height), (105, 0, 23, 39));
        assert_eq!((v.off_x, v.off_y), (2, 0));
        assert!(v.flip);
    }

    #[test]
    fn decodes_indexed_png_with_transparency_to_rgba() {
        // Synthetic pixels exercise palette expansion and tRNS without a game asset.
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(vec![255, 0, 0, 0, 255, 0]);
            encoder.set_trns(vec![255, 0]);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 1]).unwrap();
        }
        let png = load_png(&bytes).expect("decode synthetic palette PNG");
        assert_eq!((png.w, png.h), (2, 1));
        assert_eq!(png.rgba, vec![255, 0, 0, 255, 0, 255, 0, 0]);
    }
}
