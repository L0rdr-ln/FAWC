//! Text for window titles (and later the shell): shaped and rasterized by cosmic-text, with an
//! optional soft glow behind it, returned as premultiplied RGBA to upload as a texture.

use cosmic_text::{
    Attrs, Buffer, Color, Ellipsize, EllipsizeHeightLimit, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};

pub struct TextRenderer {
    fonts: FontSystem,
    cache: SwashCache,
}

/// One rendered line of text.
pub struct TextImage {
    pub width: i32,
    pub height: i32,
    /// premultiplied RGBA8, top row first
    pub rgba: Vec<u8>,
}

impl Default for TextRenderer {
    fn default() -> Self {
        TextRenderer { fonts: FontSystem::new(), cache: SwashCache::new() }
    }
}

impl TextRenderer {
    /// One line of `text`, at most `max_width` pixels wide (cut with "…"), `px` high glyphs in
    /// `color`; with a glow of `glow` (colour and strength 0..1) and `glow_radius` pixels around.
    pub fn line(
        &mut self,
        text: &str,
        max_width: i32,
        px: f32,
        color: [u8; 3],
        glow: Option<([u8; 3], f32)>,
        glow_radius: i32,
    ) -> Option<TextImage> {
        if text.is_empty() || max_width < 8 {
            return None;
        }
        let pad = if glow.is_some() { glow_radius.max(1) } else { 0 };
        let line_h = (px * 1.3).ceil();
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(px, line_h));
        buffer.set_wrap(Wrap::Word);
        buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)));
        buffer.set_size(Some((max_width - 2 * pad).max(8) as f32), Some(line_h));
        buffer.set_text(text, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);

        // the width of what was laid out
        let mut used = 0.0f32;
        for run in buffer.layout_runs() {
            used = used.max(run.line_w);
        }
        let w = (used.ceil() as i32 + 2 * pad + 1).min(max_width).max(1);
        let h = line_h as i32 + 2 * pad;
        let mut cover = vec![0u8; (w * h) as usize]; // glyph coverage 0..255
        buffer.draw(&mut self.fonts, &mut self.cache, Color::rgb(255, 255, 255), |x, y, gw, gh, c| {
            let a = c.a() as u32;
            if a == 0 {
                return;
            }
            for yy in y..y + gh as i32 {
                for xx in x..x + gw as i32 {
                    let (px_, py_) = (xx + pad, yy + pad);
                    if px_ >= 0 && py_ >= 0 && px_ < w && py_ < h {
                        let i = (py_ * w + px_) as usize;
                        cover[i] = (cover[i] as u32 + a).min(255) as u8;
                    }
                }
            }
        });

        let mut out = vec![0u8; (w * h * 4) as usize];
        // glow: the coverage blurred and widened
        if let Some((gc, strength)) = glow {
            let mut blur = cover.clone();
            box_blur(&mut blur, w, h, (glow_radius / 2).max(1));
            box_blur(&mut blur, w, h, (glow_radius / 2).max(1));
            for i in 0..(w * h) as usize {
                let a = ((blur[i] as f32 * 2.4).min(255.0) * strength) as u32;
                out[i * 4] = (gc[0] as u32 * a / 255) as u8;
                out[i * 4 + 1] = (gc[1] as u32 * a / 255) as u8;
                out[i * 4 + 2] = (gc[2] as u32 * a / 255) as u8;
                out[i * 4 + 3] = a as u8;
            }
        }
        // the glyphs over it (premultiplied "over")
        for i in 0..(w * h) as usize {
            let a = cover[i] as u32;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            for k in 0..3 {
                out[i * 4 + k] = ((color[k] as u32 * a + out[i * 4 + k] as u32 * inv) / 255) as u8;
            }
            out[i * 4 + 3] = (a + out[i * 4 + 3] as u32 * inv / 255) as u8;
        }
        Some(TextImage { width: w, height: h, rgba: out })
    }
}

/// A box blur on an 8 bit plane (rows then columns), edges treated as empty.
fn box_blur(plane: &mut [u8], w: i32, h: i32, radius: i32) {
    let r = radius.max(1);
    let win = 2 * r + 1;
    let mut tmp = vec![0u8; plane.len()];
    for y in 0..h {
        let mut sum: i32 = (-r..=r).filter(|&i| i >= 0 && i < w).map(|i| plane[(y * w + i) as usize] as i32).sum();
        for x in 0..w {
            tmp[(y * w + x) as usize] = (sum / win) as u8;
            let (add, sub) = (x + r + 1, x - r);
            if add < w {
                sum += plane[(y * w + add) as usize] as i32;
            }
            if sub >= 0 {
                sum -= plane[(y * w + sub) as usize] as i32;
            }
        }
    }
    for x in 0..w {
        let mut sum: i32 = (-r..=r).filter(|&i| i >= 0 && i < h).map(|i| tmp[(i * w + x) as usize] as i32).sum();
        for y in 0..h {
            plane[(y * w + x) as usize] = (sum / win) as u8;
            let (add, sub) = (y + r + 1, y - r);
            if add < h {
                sum += tmp[(add * w + x) as usize] as i32;
            }
            if sub >= 0 {
                sum -= tmp[(sub * w + x) as usize] as i32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_ink_and_a_glow_and_is_cut_to_the_width() {
        let mut t = TextRenderer::default();
        let img = t.line("Hello glass", 300, 13.0, [10, 30, 60], Some(([255, 255, 255], 0.9)), 4);
        let Some(img) = img else {
            eprintln!("no fonts installed: text test skipped");
            return;
        };
        assert!(img.width > 30 && img.width <= 300 && img.height >= 13);
        // dark ink pixels exist
        let ink = img.rgba.chunks(4).filter(|p| p[3] > 200 && p[0] < 80 && p[2] < 120).count();
        assert!(ink > 40, "ink pixels: {ink}");
        // and a white glow around them
        let glow = img.rgba.chunks(4).filter(|p| p[3] > 30 && p[3] < 220 && p[0] > 100).count();
        assert!(glow > 40, "glow pixels: {glow}");
        // a long title is cut to the width
        let long = t.line(&"very long title ".repeat(20), 120, 13.0, [0, 0, 0], None, 0).unwrap();
        assert!(long.width <= 120);
    }
}
