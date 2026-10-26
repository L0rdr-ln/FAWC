//! The task bar's picture: a glass strip along the bottom with the start orb on the left and
//! the clock on the right. Pure drawing (premultiplied RGBA, top row first) so it can be tested;
//! `bin/fawc-panel.rs` puts it on screen as a layer-shell client.

use crate::text::TextRenderer;

pub const HEIGHT: i32 = 40;
pub const ORB_SIZE: i32 = 32;
pub const ORB_MARGIN: i32 = 6;

/// Is the point on the start orb?
pub fn on_orb(x: f64, y: f64, height: i32) -> bool {
    let (cx, cy) = orb_center(height);
    let r = ORB_SIZE as f64 / 2.0;
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn orb_center(height: i32) -> (f64, f64) {
    ((ORB_MARGIN + ORB_SIZE / 2) as f64, height as f64 / 2.0)
}

fn blend(px: &mut [u8], rgb: [f32; 3], a: f32) {
    // source-over of a premultiplied colour (rgb * a, a)
    let inv = 1.0 - a;
    for c in 0..3 {
        px[c] = (rgb[c] * 255.0 * a + px[c] as f32 * inv).round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (255.0 * a + px[3] as f32 * inv).round().clamp(0.0, 255.0) as u8;
}

/// `width` x `height` RGBA, premultiplied. `hover` brightens the orb, `clock` is e.g. "14:05".
pub fn draw(text: &mut TextRenderer, width: i32, height: i32, hover: bool, clock: &str) -> Vec<u8> {
    let mut buf = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let t = y as f32 / (height - 1).max(1) as f32;
        // an aqua strip, lighter at the top, with a gloss over the upper half
        let base = [0.18 + 0.10 * (1.0 - t), 0.52 + 0.16 * (1.0 - t), 0.86 + 0.06 * (1.0 - t)];
        let alpha = 0.55 + 0.15 * t;
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, base, alpha);
            if t < 0.5 {
                blend(px, [1.0, 1.0, 1.0], 0.20 * (1.0 - t * 2.0) + 0.05);
            }
            if y == 0 {
                blend(px, [1.0, 1.0, 1.0], 0.55);
            }
        }
    }
    // the start orb: a green sphere with a bright highlight
    let (cx, cy) = orb_center(height);
    let r = ORB_SIZE as f32 / 2.0;
    for y in 0..height {
        for x in 0..ORB_SIZE + 2 * ORB_MARGIN {
            let (dx, dy) = (x as f32 + 0.5 - cx as f32, y as f32 + 0.5 - cy as f32);
            let d = (dx * dx + dy * dy).sqrt();
            let cover = (r + 0.5 - d).clamp(0.0, 1.0); // antialiased edge
            if cover <= 0.0 {
                continue;
            }
            let ny = (dy / r).clamp(-1.0, 1.0);
            let shade = 0.5 - 0.35 * ny + if hover { 0.15 } else { 0.0 };
            let green = [0.10 + 0.25 * shade, 0.55 + 0.40 * shade, 0.15 + 0.20 * shade];
            let i = ((y * width + x) * 4) as usize;
            blend(&mut buf[i..i + 4], green, cover);
            // the highlight in the upper half
            if dy < 0.0 && d < r - 1.0 {
                let h = (1.0 - (-dy / r)).clamp(0.0, 1.0) * 0.55 * cover;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], h * (1.0 - (d / r).powi(2)).max(0.0));
            }
            // the rim
            if d > r - 1.5 {
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.35 * cover);
            }
        }
    }
    // the clock, with a soft glow, at the right
    if let Some(img) = text.line(clock, 120, 16.0, [255, 255, 255], Some(([20, 70, 140], 0.9)), 3) {
        let x0 = width - img.width - 14;
        let y0 = (height - img.height) / 2;
        for yy in 0..img.height {
            for xx in 0..img.width {
                let (px_, py_) = (x0 + xx, y0 + yy);
                if px_ < 0 || py_ < 0 || px_ >= width || py_ >= height {
                    continue;
                }
                let s = ((yy * img.width + xx) * 4) as usize;
                let d = ((py_ * width + px_) * 4) as usize;
                let inv = 255 - img.rgba[s + 3] as u32;
                for c in 0..4 {
                    buf[d + c] = (img.rgba[s + c] as u32 + buf[d + c] as u32 * inv / 255).min(255) as u8;
                }
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_orb_is_green_and_the_strip_is_blue_with_a_clock() {
        let mut text = TextRenderer::default();
        let (w, h) = (400, HEIGHT);
        let buf = draw(&mut text, w, h, false, "12:34");
        let at = |x: i32, y: i32| {
            let i = ((y * w + x) * 4) as usize;
            [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32, buf[i + 3] as i32]
        };
        let orb = at(ORB_MARGIN + ORB_SIZE / 2, h / 2 + 6);
        assert!(orb[1] > orb[0] + 40 && orb[1] > orb[2] + 20, "a green orb: {orb:?}");
        let strip = at(150, h - 4);
        assert!(strip[2] > strip[0] + 30 && strip[3] > 100, "a blue translucent strip: {strip:?}");
        // some white-ish ink where the clock is (if the machine has fonts at all)
        let mut ink = 0;
        for y in 0..h {
            for x in (w - 90)..(w - 10) {
                if at(x, y)[0] > 200 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 0 || text.line("x", 50, 16.0, [0; 3], None, 0).is_none(), "the clock leaves ink");
        assert!(on_orb(22.0, 20.0, h) && !on_orb(100.0, 20.0, h));
    }
}
