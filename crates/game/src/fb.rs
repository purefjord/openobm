//! 24-bit RGB framebuffer standing in for the MIDP LCD (240x320, no alpha —
//! `Graphics.setColor` masks to 0xRRGGBB). Out-of-bounds writes are silently
//! clipped, like AWT drawing on the oracle side (the game really does draw
//! e.g. "BACK" at y=333, below the screen).

use std::path::Path;

pub struct Fb {
    pub w: i32,
    pub h: i32,
    px: Vec<u32>, // 0xRRGGBB row-major
}

impl Fb {
    pub fn new(w: i32, h: i32) -> Self {
        assert!(w > 0 && h > 0);
        Self {
            w,
            h,
            px: vec![0; (w * h) as usize],
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        assert!(x >= 0 && y >= 0 && x < self.w && y < self.h);
        self.px[(y * self.w + x) as usize]
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, rgb: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        self.px[(y * self.w + x) as usize] = rgb & 0xFF_FF_FF;
    }

    pub fn fill(&mut self, rgb: u32) {
        self.px.fill(rgb & 0xFF_FF_FF);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: u32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, rgb);
            }
        }
    }

    /// Decode a PNG (any color type the `png` crate handles) to a framebuffer,
    /// dropping alpha — used to load oracle LCD snapshots for pixel compares.
    pub fn load_png(path: &Path) -> anyhow::Result<Self> {
        let decoder = png::Decoder::new(std::fs::File::open(path)?);
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf)?;
        let (w, h) = (info.width as i32, info.height as i32);
        let mut fb = Fb::new(w, h);
        let bpp = info.buffer_size() / (w as usize * h as usize);
        anyhow::ensure!(
            info.bit_depth == png::BitDepth::Eight && (bpp == 3 || bpp == 4),
            "unsupported PNG layout {:?}/{bpp}bpp in {}",
            info.bit_depth,
            path.display()
        );
        for y in 0..h {
            for x in 0..w {
                let i = (y as usize * w as usize + x as usize) * bpp;
                let (r, g, b) = (buf[i] as u32, buf[i + 1] as u32, buf[i + 2] as u32);
                fb.set(x, y, (r << 16) | (g << 8) | b);
            }
        }
        Ok(fb)
    }

    pub fn save_png(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(file, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        let mut data = Vec::with_capacity((self.w * self.h * 3) as usize);
        for p in &self.px {
            data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        writer.write_image_data(&data)?;
        Ok(())
    }

    /// Number of differing pixels vs `other` (must be same size).
    pub fn diff_count(&self, other: &Fb) -> usize {
        assert_eq!((self.w, self.h), (other.w, other.h));
        self.px
            .iter()
            .zip(&other.px)
            .filter(|(a, b)| a != b)
            .count()
    }

    /// Compare the top `rows` rows (same width) against a real LCD frame; the
    /// logical framebuffer is 345 tall but the device only shows the top 320.
    /// Returns a diff framebuffer (magenta where they differ, else black) plus
    /// the differing-pixel count, so a mismatch can be saved for inspection.
    pub fn diff_region(&self, real: &Fb, rows: i32) -> (Fb, usize) {
        assert_eq!(self.w, real.w, "width mismatch");
        assert!(
            rows <= self.h && rows <= real.h,
            "region taller than frames"
        );
        let mut out = Fb::new(self.w, rows);
        let mut bad = 0;
        for y in 0..rows {
            for x in 0..self.w {
                if self.get(x, y) != real.get(x, y) {
                    out.set(x, y, 0xFF_00_FF);
                    bad += 1;
                }
            }
        }
        (out, bad)
    }
}
