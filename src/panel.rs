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

/// One window in the task list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Task {
    pub title: String,
    pub active: bool,
    pub minimized: bool,
}

const TASKS_LEFT: i32 = ORB_MARGIN * 2 + ORB_SIZE + 6;
const TASK_GAP: i32 = 4;
const TASK_MAX_W: i32 = 170;
const CLOCK_ROOM: i32 = 90;

/// The x range and the y range of task button `i` of `n` in a panel `width` wide.
fn task_rect(i: usize, n: usize, width: i32, height: i32) -> (i32, i32, i32, i32) {
    let avail = (width - TASKS_LEFT - CLOCK_ROOM).max(0);
    let w = ((avail - TASK_GAP * (n as i32 - 1).max(0)) / n.max(1) as i32).clamp(0, TASK_MAX_W);
    let x0 = TASKS_LEFT + i as i32 * (w + TASK_GAP);
    (x0, x0 + w, 5, height - 5)
}

/// Which task button is at (x, y)?
pub fn task_at(x: f64, y: f64, width: i32, height: i32, n: usize) -> Option<usize> {
    (0..n).find(|&i| {
        let (x0, x1, y0, y1) = task_rect(i, n, width, height);
        x >= x0 as f64 && x < x1 as f64 && y >= y0 as f64 && y < y1 as f64
    })
}

fn orb_center(height: i32) -> (f64, f64) {
    ((ORB_MARGIN + ORB_SIZE / 2) as f64, height as f64 / 2.0)
}

pub(crate) fn blend(px: &mut [u8], rgb: [f32; 3], a: f32) {
    // source-over of a premultiplied colour (rgb * a, a)
    let inv = 1.0 - a;
    for c in 0..3 {
        px[c] = (rgb[c] * 255.0 * a + px[c] as f32 * inv).round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (255.0 * a + px[3] as f32 * inv).round().clamp(0.0, 255.0) as u8;
}

/// Composite a rendered line of text (premultiplied) onto `buf` at (x0, y0).
pub(crate) fn blit(buf: &mut [u8], width: i32, height: i32, img: &crate::text::TextImage, x0: i32, y0: i32) {
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

/// `width` x `height` RGBA, premultiplied. `hover` brightens the orb, `clock` is e.g. "14:05".
pub fn draw(text: &mut TextRenderer, width: i32, height: i32, hover: bool, clock: &str, tasks: &[Task]) -> Vec<u8> {
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
    // the task buttons: a glass pill each, brighter for the active window, dim when minimized
    for (i, task) in tasks.iter().enumerate() {
        let (x0, x1, y0, y1) = task_rect(i, tasks.len(), width, height);
        if x1 - x0 < 24 {
            continue;
        }
        let rad = 6.0f32;
        for y in y0..y1 {
            for x in x0..x1 {
                // rounded rectangle coverage
                let dx = (x as f32 + 0.5 - (x0 + x1) as f32 / 2.0).abs() - ((x1 - x0) as f32 / 2.0 - rad);
                let dy = (y as f32 + 0.5 - (y0 + y1) as f32 / 2.0).abs() - ((y1 - y0) as f32 / 2.0 - rad);
                let d = dx.max(0.0).hypot(dy.max(0.0)) - rad;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let t = (y - y0) as f32 / (y1 - y0) as f32;
                let (base, a) = if task.active {
                    ([0.55, 0.80, 1.0], 0.55 - 0.15 * t)
                } else if task.minimized {
                    ([0.20, 0.45, 0.75], 0.18)
                } else {
                    ([0.35, 0.65, 0.95], 0.32 - 0.10 * t)
                };
                let i = ((y * width + x) * 4) as usize;
                blend(&mut buf[i..i + 4], base, a * cover);
                if t < 0.45 {
                    blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.12 * cover);
                }
                if d > -1.2 {
                    blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.40 * cover);
                }
            }
        }
        let color = if task.minimized { [220, 235, 250] } else { [255, 255, 255] };
        if let Some(img) = text.line(&task.title, x1 - x0 - 14, 13.0, color, Some(([20, 70, 140], 0.8)), 2) {
            let tx = x0 + 8;
            let ty = (height - img.height) / 2;
            for yy in 0..img.height {
                for xx in 0..img.width {
                    let (px_, py_) = (tx + xx, ty + yy);
                    if px_ < 0 || py_ < 0 || px_ >= width || py_ >= height {
                        continue;
                    }
                    let sidx = ((yy * img.width + xx) * 4) as usize;
                    let d = ((py_ * width + px_) * 4) as usize;
                    let inv = 255 - img.rgba[sidx + 3] as u32;
                    for c in 0..4 {
                        buf[d + c] = (img.rgba[sidx + c] as u32 + buf[d + c] as u32 * inv / 255).min(255) as u8;
                    }
                }
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

pub const MENU_W: i32 = 280;
pub const MENU_ROW: i32 = 28;
const MENU_PAD: i32 = 8;
pub const MENU_MAX_ROWS: usize = 16;
const SEARCH_H: i32 = 38;

/// Size of the start menu: a search field and room for `MENU_MAX_ROWS` entries, always.
pub fn menu_size() -> (i32, i32) {
    (MENU_W, SEARCH_H + MENU_MAX_ROWS as i32 * MENU_ROW + 2 * MENU_PAD)
}

/// Which menu row (of `n` shown) is at height `y`?
pub fn menu_row_at(y: f64, n: usize) -> Option<usize> {
    let row = ((y - (MENU_PAD + SEARCH_H) as f64) / MENU_ROW as f64).floor();
    (row >= 0.0 && (row as usize) < n.min(MENU_MAX_ROWS)).then_some(row as usize)
}

/// The start menu: a glass sheet with rounded corners and one row per entry.
pub fn draw_menu(text: &mut TextRenderer, query: &str, names: &[String], hover: Option<usize>) -> Vec<u8> {
    let (w, h) = menu_size();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let rad = 12.0f32;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - w as f32 / 2.0).abs() - (w as f32 / 2.0 - rad);
            let dy = (y as f32 + 0.5 - h as f32 / 2.0).abs() - (h as f32 / 2.0 - rad);
            let d = dx.max(0.0).hypot(dy.max(0.0)) - rad;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = y as f32 / h as f32;
            let i = ((y * w + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, [0.20 + 0.10 * (1.0 - t), 0.55 + 0.12 * (1.0 - t), 0.90], 0.62 * cover);
            if t < 0.25 {
                blend(px, [1.0, 1.0, 1.0], 0.16 * (1.0 - t * 4.0) * cover);
            }
            if d > -1.5 {
                blend(px, [1.0, 1.0, 1.0], 0.45 * cover);
            }
        }
    }
    // the search field: a pale pill with what has been typed (or a hint)
    let (sx0, sy0, sw, sh) = (MENU_PAD, MENU_PAD, w - 2 * MENU_PAD, SEARCH_H - 6);
    for y in 0..sh {
        for x in 0..sw {
            let dx = (x as f32 + 0.5 - sw as f32 / 2.0).abs() - (sw as f32 / 2.0 - sh as f32 / 2.0);
            let dy = (y as f32 + 0.5 - sh as f32 / 2.0).abs();
            let d = dx.max(0.0).hypot(dy) - sh as f32 / 2.0;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                let i = (((sy0 + y) * w + sx0 + x) * 4) as usize;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.78 * cover);
                if d > -1.2 {
                    blend(&mut buf[i..i + 4], [0.1, 0.3, 0.6], 0.45 * cover);
                }
            }
        }
    }
    let (shown_text, color) = if query.is_empty() { ("Search programs", [90, 120, 160]) } else { (query, [10, 40, 90]) };
    if let Some(img) = text.line(shown_text, sw - 28, 14.0, color, None, 0) {
        blit(&mut buf, w, h, &img, sx0 + 14, sy0 + (sh - img.height) / 2);
    }
    for (i, name) in names.iter().take(MENU_MAX_ROWS).enumerate() {
        let y0 = MENU_PAD + SEARCH_H + i as i32 * MENU_ROW;
        if hover == Some(i) {
            for y in y0..y0 + MENU_ROW {
                for x in MENU_PAD..w - MENU_PAD {
                    let t = (y - y0) as f32 / MENU_ROW as f32;
                    let idx = ((y * w + x) * 4) as usize;
                    blend(&mut buf[idx..idx + 4], [0.75, 0.92, 1.0], 0.55 - 0.2 * t);
                }
            }
        }
        let color = if hover == Some(i) { [10, 50, 100] } else { [255, 255, 255] };
        let glow = if hover == Some(i) { None } else { Some(([20, 70, 140], 0.8)) };
        if let Some(img) = text.line(name, w - 2 * MENU_PAD - 16, 14.0, color, glow, 2) {
            let (tx, ty) = (MENU_PAD + 10, y0 + (MENU_ROW - img.height) / 2);
            for yy in 0..img.height {
                for xx in 0..img.width {
                    let (px_, py_) = (tx + xx, ty + yy);
                    if px_ < 0 || py_ < 0 || px_ >= w || py_ >= h {
                        continue;
                    }
                    let sidx = ((yy * img.width + xx) * 4) as usize;
                    let d = ((py_ * w + px_) * 4) as usize;
                    let inv = 255 - img.rgba[sidx + 3] as u32;
                    for c in 0..4 {
                        buf[d + c] = (img.rgba[sidx + c] as u32 + buf[d + c] as u32 * inv / 255).min(255) as u8;
                    }
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
        let buf = draw(&mut text, w, h, false, "12:34", &[Task { title: "Files".into(), active: true, minimized: false }, Task { title: "Terminal".into(), ..Default::default() }]);
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
        // two task buttons: the first is the active, lighter one
        let (a, b) = (at(TASKS_LEFT + 80, h - 8), at(TASKS_LEFT + 80 + 174, h - 8));
        assert!(a[0] > b[0], "the active task is lighter: {a:?} {b:?}");
        assert_eq!(task_at((TASKS_LEFT + 5) as f64, 20.0, w, h, 2), Some(0));
        assert_eq!(task_at((TASKS_LEFT + TASK_MAX_W + TASK_GAP + 5) as f64, 20.0, w, h, 2), Some(1));
        assert_eq!(task_at(300.0, 2.0, w, h, 2), None);
        assert!(on_orb(22.0, 20.0, h) && !on_orb(100.0, 20.0, h));
    }

    #[test]
    fn the_menu_has_rows_a_highlight_and_rounded_corners() {
        let mut text = TextRenderer::default();
        let names: Vec<String> = ["Files", "Terminal", "Settings"].iter().map(|s| s.to_string()).collect();
        let (w, h) = menu_size();
        let buf = draw_menu(&mut text, "", &names, Some(1));
        let at = |x: i32, y: i32| {
            let i = ((y * w + x) * 4) as usize;
            [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32, buf[i + 3] as i32]
        };
        assert_eq!(at(0, 0)[3], 0, "the corner is transparent");
        assert!(at(w / 2, h / 2)[3] > 100, "the sheet is translucent, not empty");
        let (hi, plain) = (at(w - 20, MENU_PAD + SEARCH_H + MENU_ROW + 14), at(w - 20, MENU_PAD + SEARCH_H + 14));
        assert!(hi[0] > plain[0] + 40, "the hovered row is lighter: {hi:?} {plain:?}");
        assert_eq!(menu_row_at((MENU_PAD + SEARCH_H + MENU_ROW + 3) as f64, 3), Some(1));
        assert_eq!(menu_row_at(2.0, 3), None, "the search field is not a row");
        assert_eq!(menu_row_at((MENU_PAD + SEARCH_H + 3 * MENU_ROW + 3) as f64, 3), None, "below the last row");
    }
}
