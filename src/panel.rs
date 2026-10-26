//! The task bar's picture: a glass strip along the bottom with the start orb on the left and
//! the clock on the right. Pure drawing (premultiplied RGBA, top row first) so it can be tested;
//! `bin/fde-panel.rs` puts it on screen as a layer-shell client.

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
pub fn task_rect(i: usize, n: usize, width: i32, height: i32) -> (i32, i32, i32, i32) {
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

/// `width` x `height` RGBA, premultiplied, in pixels: the bar is `width / s` x `height / s`
/// logical pixels drawn at `s` pixels per logical one. `hover` brightens the orb, `clock` is
/// e.g. "14:05".
#[allow(clippy::too_many_arguments)]
pub fn draw(text: &mut TextRenderer, width: i32, height: i32, s: i32, hover: bool, clock: &str, tasks: &[Task]) -> Vec<u8> {
    let mut buf = vec![0u8; (width * height * 4) as usize];
    let sf = s as f32;
    let pal = crate::theme::palette();
    for y in 0..height {
        let t = y as f32 / (height - 1).max(1) as f32;
        // a strip in the accent colour, lighter at the top, with a gloss over the upper half
        let base = pal.tone(t);
        let alpha = (0.55 + 0.15 * t) + (0.97 - (0.55 + 0.15 * t)) * pal.solid;
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, base, alpha);
            if t < 0.5 {
                blend(px, [1.0, 1.0, 1.0], (0.20 * (1.0 - t * 2.0) + 0.05) * pal.shine);
            }
            if y < s {
                blend(px, [1.0, 1.0, 1.0], 0.55 * pal.shine.max(0.35 * pal.solid));
            }
        }
    }
    // the start orb: a green sphere with a bright highlight
    let (cx, cy) = orb_center(height / s);
    let (cx, cy) = (cx as f32 * sf, cy as f32 * sf);
    let r = ORB_SIZE as f32 / 2.0 * sf;
    for y in 0..height {
        for x in 0..(ORB_SIZE + 2 * ORB_MARGIN) * s {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            let cover = (r + 0.5 - d).clamp(0.0, 1.0); // antialiased edge
            if cover <= 0.0 {
                continue;
            }
            let ny = (dy / r).clamp(-1.0, 1.0);
            let shade = 0.5 - 0.35 * ny + if hover { 0.15 } else { 0.0 };
            let green = [
                (pal.orb[0] * (0.55 + 0.7 * shade)).min(1.0),
                (pal.orb[1] * (0.55 + 0.7 * shade)).min(1.0),
                (pal.orb[2] * (0.55 + 0.7 * shade)).min(1.0),
            ];
            let i = ((y * width + x) * 4) as usize;
            blend(&mut buf[i..i + 4], green, cover);
            // the highlight in the upper half
            if dy < 0.0 && d < r - sf {
                let h = (1.0 - (-dy / r)).clamp(0.0, 1.0) * 0.55 * cover;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], h * (1.0 - (d / r).powi(2)).max(0.0) * pal.shine.max(0.3));
            }
            // the rim
            if d > r - 1.5 * sf {
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.35 * cover);
            }
        }
    }
    // the task buttons: a glass pill each, brighter for the active window, dim when minimized
    for (i, task) in tasks.iter().enumerate() {
        let (lx0, lx1, ly0, ly1) = task_rect(i, tasks.len(), width / s, height / s);
        if lx1 - lx0 < 24 {
            continue;
        }
        let (x0, x1, y0, y1) = (lx0 * s, lx1 * s, ly0 * s, ly1 * s);
        let rad = 6.0f32 * sf;
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
                    (pal.light(0.5), 0.55 - 0.15 * t)
                } else if task.minimized {
                    (pal.dark(0.2), 0.18)
                } else {
                    (pal.light(0.15), 0.32 - 0.10 * t)
                };
                let i = ((y * width + x) * 4) as usize;
                blend(&mut buf[i..i + 4], base, a * cover);
                if t < 0.45 {
                    blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.12 * cover * pal.shine);
                }
                if d > -1.2 * sf {
                    blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.40 * cover);
                }
            }
        }
        let color = if task.minimized { [220, 235, 250] } else { [255, 255, 255] };
        if let Some(img) = text.line(&task.title, x1 - x0 - 14 * s, 13.0 * sf, color, pal.glow(0.8), 2 * s) {
            blit(&mut buf, width, height, &img, x0 + 8 * s, (height - img.height) / 2);
        }
    }
    // the clock, with a soft glow, at the right
    if let Some(img) = text.line(clock, 120 * s, 16.0 * sf, [255, 255, 255], pal.glow(0.9), 3 * s) {
        blit(&mut buf, width, height, &img, width - img.width - 14 * s, (height - img.height) / 2);
    }
    buf
}

/// The workspace dots left of the clock: `current` of `count` (1-based). Drawn over the bar.
pub fn draw_workspaces(buf: &mut [u8], width: i32, height: i32, s: i32, current: usize, count: usize) {
    if count < 2 {
        return;
    }
    let sf = s as f32;
    let r = 4.5 * sf;
    let gap = 8.0 * sf;
    let total = count as f32 * (2.0 * r) + (count as f32 - 1.0) * gap;
    let x0 = width as f32 - 112.0 * sf - total;
    let cy = height as f32 / 2.0;
    for i in 0..count {
        let cx = x0 + r + i as f32 * (2.0 * r + gap);
        let on = i + 1 == current;
        for y in (cy - r - 2.0) as i32..=(cy + r + 2.0) as i32 {
            for x in (cx - r - 2.0) as i32..=(cx + r + 2.0) as i32 {
                if x < 0 || y < 0 || x >= width || y >= height {
                    continue;
                }
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let idx = ((y * width + x) * 4) as usize;
                if on {
                    blend(&mut buf[idx..idx + 4], [1.0, 1.0, 1.0], 0.95 * cover);
                } else {
                    // a ring
                    let ring = (1.0 - ((r - d) / (1.4 * sf)).clamp(0.0, 1.0)).max(0.0);
                    blend(&mut buf[idx..idx + 4], [1.0, 1.0, 1.0], (0.12 + 0.6 * ring) * cover);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusKind {
    Volume,
    Battery,
    DoNotDisturb,
}

/// Width of the workspace dots (logical pixels), 0 with a single workspace.
fn workspace_dots_width(count: usize) -> f32 {
    if count < 2 {
        0.0
    } else {
        count as f32 * 9.0 + (count as f32 - 1.0) * 8.0 + 14.0
    }
}

/// The status labels (volume, battery) right of the task buttons and left of the workspace dots,
/// drawn over the bar. Returns where each one is, in logical x pixels, for hit testing.
pub fn draw_status(
    text: &mut TextRenderer,
    buf: &mut [u8],
    width: i32,
    height: i32,
    s: i32,
    items: &[(StatusKind, String)],
    workspaces: usize,
) -> Vec<(StatusKind, i32, i32)> {
    let sf = s as f32;
    let pal = crate::theme::palette();
    let mut right = width as f32 - (112.0 + workspace_dots_width(workspaces)) * sf;
    let mut rects = Vec::new();
    for (kind, label) in items {
        let Some(img) = text.line(label, 220 * s, 12.5 * sf, [255, 255, 255], pal.glow(0.8), 2 * s) else { continue };
        let x0 = right as i32 - img.width;
        blit(buf, width, height, &img, x0, (height - img.height) / 2);
        rects.push((*kind, x0 / s, (x0 + img.width) / s));
        right = x0 as f32 - 12.0 * sf;
    }
    rects
}

pub const MENU_W: i32 = 320;
pub const MENU_ROW: i32 = 28;
const MENU_PAD: i32 = 8;
pub const MENU_MAX_ROWS: usize = 16;
const SEARCH_H: i32 = 38;
/// the power row at the bottom of the menu
pub const POWER_H: i32 = 40;

/// Size of the start menu: a search field and room for `MENU_MAX_ROWS` entries, always.
pub fn menu_size() -> (i32, i32) {
    (MENU_W, SEARCH_H + MENU_MAX_ROWS as i32 * MENU_ROW + 2 * MENU_PAD + POWER_H)
}

/// The rectangle of power button `i` (x0, y0, x1, y1), in menu pixels.
pub fn power_rect(i: usize) -> (i32, i32, i32, i32) {
    let n = crate::power::ACTIONS.len() as i32;
    let gap = 4;
    let w = (MENU_W - 2 * MENU_PAD - (n - 1) * gap) / n;
    let (_, h) = menu_size();
    let x0 = MENU_PAD + i as i32 * (w + gap);
    (x0, h - POWER_H + 4, x0 + w, h - MENU_PAD)
}

/// Which power button is at this point of the menu?
pub fn power_at(x: f64, y: f64) -> Option<usize> {
    (0..crate::power::ACTIONS.len()).find(|&i| {
        let (x0, y0, x1, y1) = power_rect(i);
        x >= x0 as f64 && x < x1 as f64 && y >= y0 as f64 && y < y1 as f64
    })
}

/// What the power row looks like: the button under the pointer and the one waiting for a second click.
#[derive(Default, Clone, Copy)]
pub struct PowerView {
    pub hover: Option<usize>,
    pub armed: Option<usize>,
}

/// Which menu row (of `n` shown) is at height `y`?
pub fn menu_row_at(y: f64, n: usize) -> Option<usize> {
    let row = ((y - (MENU_PAD + SEARCH_H) as f64) / MENU_ROW as f64).floor();
    (row >= 0.0 && (row as usize) < n.min(MENU_MAX_ROWS)).then_some(row as usize)
}

/// The start menu at `s` pixels per logical pixel: a glass sheet with rounded corners and one
/// row per entry. The picture is `menu_size()` times `s`.
pub fn draw_menu(text: &mut TextRenderer, s: i32, query: &str, names: &[String], hover: Option<usize>, power: &PowerView) -> Vec<u8> {
    let (lw, lh) = menu_size();
    let (w, h) = (lw * s, lh * s);
    let sf = s as f32;
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let pal = crate::theme::palette();
    let rad = (12.0 - 12.0 * pal.solid) * sf;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - w as f32 / 2.0).abs() - (w as f32 / 2.0 - rad);
            let dy = (y as f32 + 0.5 - h as f32 / 2.0).abs() - (h as f32 / 2.0 - rad);
            let d = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - rad;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = y as f32 / h as f32;
            let i = ((y * w + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, pal.tone(t), (0.62 + 0.35 * pal.solid) * cover);
            if t < 0.25 {
                blend(px, [1.0, 1.0, 1.0], 0.16 * (1.0 - t * 4.0) * cover * pal.shine);
            }
            if d > -1.5 * sf {
                blend(px, [1.0, 1.0, 1.0], 0.45 * cover);
            }
        }
    }
    // the search field: a pale pill with what has been typed (or a hint)
    let (sx0, sy0, sw, sh) = (MENU_PAD * s, MENU_PAD * s, w - 2 * MENU_PAD * s, (SEARCH_H - 6) * s);
    for y in 0..sh {
        for x in 0..sw {
            let dx = (x as f32 + 0.5 - sw as f32 / 2.0).abs() - (sw as f32 / 2.0 - sh as f32 / 2.0);
            let dy = (y as f32 + 0.5 - sh as f32 / 2.0).abs();
            let d = dx.max(0.0).hypot(dy) - sh as f32 / 2.0;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                let i = (((sy0 + y) * w + sx0 + x) * 4) as usize;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.78 * cover);
                if d > -1.2 * sf {
                    blend(&mut buf[i..i + 4], [0.1, 0.3, 0.6], 0.45 * cover);
                }
            }
        }
    }
    let (shown_text, color) = if query.is_empty() { ("Search programs", [90, 120, 160]) } else { (query, [10, 40, 90]) };
    if let Some(img) = text.line(shown_text, sw - 28 * s, 14.0 * sf, color, None, 0) {
        blit(&mut buf, w, h, &img, sx0 + 14 * s, sy0 + (sh - img.height) / 2);
    }
    for (i, name) in names.iter().take(MENU_MAX_ROWS).enumerate() {
        let y0 = (MENU_PAD + SEARCH_H + i as i32 * MENU_ROW) * s;
        if hover == Some(i) {
            for y in y0..y0 + MENU_ROW * s {
                for x in MENU_PAD * s..w - MENU_PAD * s {
                    let t = (y - y0) as f32 / (MENU_ROW * s) as f32;
                    let idx = ((y * w + x) * 4) as usize;
                    blend(&mut buf[idx..idx + 4], pal.light(0.75), 0.55 - 0.2 * t);
                }
            }
        }
        let color = if hover == Some(i) { [10, 50, 100] } else { [255, 255, 255] };
        let glow = if hover == Some(i) { None } else { pal.glow(0.8) };
        if let Some(img) = text.line(name, w - 2 * MENU_PAD * s - 16 * s, 14.0 * sf, color, glow, 2 * s) {
            blit(&mut buf, w, h, &img, (MENU_PAD + 10) * s, y0 + (MENU_ROW * s - img.height) / 2);
        }
    }
    // the power row: a thin line and five small glass buttons
    {
        let (_, mh) = menu_size();
        let ly = (mh - POWER_H) * s;
        for x in MENU_PAD * s..w - MENU_PAD * s {
            let idx = ((ly * w + x) * 4) as usize;
            blend(&mut buf[idx..idx + 4], [1.0, 1.0, 1.0], 0.35);
        }
        for (i, action) in crate::power::ACTIONS.iter().enumerate() {
            let (x0, y0, x1, y1) = power_rect(i);
            let (x0, y0, x1, y1) = (x0 * s, y0 * s, x1 * s, y1 * s);
            let rad = 7.0f32 * sf;
            let armed = power.armed == Some(i);
            let hot = power.hover == Some(i);
            for y in y0..y1 {
                for x in x0..x1 {
                    let dx = (x as f32 + 0.5 - (x0 + x1) as f32 / 2.0).abs() - ((x1 - x0) as f32 / 2.0 - rad);
                    let dy = (y as f32 + 0.5 - (y0 + y1) as f32 / 2.0).abs() - ((y1 - y0) as f32 / 2.0 - rad);
                    let d = dx.max(0.0).hypot(dy.max(0.0)) - rad;
                    let cover = (0.5 - d).clamp(0.0, 1.0);
                    if cover <= 0.0 {
                        continue;
                    }
                    let t = (y - y0) as f32 / (y1 - y0) as f32;
                    let idx = ((y * w + x) * 4) as usize;
                    let (base, a) = if armed {
                        ([0.85, 0.22, 0.18], 0.85 - 0.15 * t)
                    } else if hot {
                        (pal.light(0.6), 0.6 - 0.2 * t)
                    } else {
                        (pal.light(0.2), 0.3 - 0.1 * t)
                    };
                    blend(&mut buf[idx..idx + 4], base, a * cover);
                    if t < 0.45 {
                        blend(&mut buf[idx..idx + 4], [1.0, 1.0, 1.0], 0.12 * cover * pal.shine);
                    }
                    if d > -1.2 * sf {
                        blend(&mut buf[idx..idx + 4], [1.0, 1.0, 1.0], 0.4 * cover);
                    }
                }
            }
            let label = if armed { "Sure?" } else { action.label };
            let color = if hot && !armed { [10, 50, 100] } else { [255, 255, 255] };
            let glow = if hot && !armed { None } else { pal.glow(0.7) };
            if let Some(img) = text.line(label, x1 - x0 - 4 * s, 11.0 * sf, color, glow, s) {
                blit(&mut buf, w, h, &img, x0 + (x1 - x0 - img.width) / 2, y0 + (y1 - y0 - img.height) / 2);
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
        let buf = draw(&mut text, w, h, 1, false, "12:34", &[Task { title: "Files".into(), active: true, minimized: false }, Task { title: "Terminal".into(), ..Default::default() }]);
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
        let buf = draw_menu(&mut text, 1, "", &names, Some(1), &PowerView::default());
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

    #[test]
    fn the_status_labels_are_drawn_left_of_the_dots_and_can_be_found_again() {
        let mut text = TextRenderer::default();
        let (w, h) = (600, HEIGHT);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let items = vec![(StatusKind::Volume, "Vol 45%".to_string()), (StatusKind::Battery, "Bat 87%".to_string())];
        let rects = draw_status(&mut text, &mut buf, w, h, 1, &items, 4);
        if text.line("x", 50, 12.0, [0, 0, 0], None, 0).is_none() {
            return; // no fonts on this machine
        }
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].0, StatusKind::Volume);
        assert!(rects[0].1 > rects[1].2 && rects[0].2 <= w - 112 - 60, "volume sits left of battery, both left of the dots: {rects:?}");
        assert!(buf.iter().any(|&b| b > 0), "something was drawn");
    }

    #[test]
    fn the_workspace_dots_show_which_one_is_on_and_stay_away_from_a_single_workspace() {
        let (w, h) = (400, HEIGHT);
        let mut plain = vec![0u8; (w * h * 4) as usize];
        draw_workspaces(&mut plain, w, h, 1, 1, 1);
        assert!(plain.iter().all(|&b| b == 0), "one workspace: nothing to show");
        let mut a = vec![0u8; (w * h * 4) as usize];
        let mut b = a.clone();
        draw_workspaces(&mut a, w, h, 1, 1, 4);
        draw_workspaces(&mut b, w, h, 1, 3, 4);
        assert!(a.iter().any(|&p| p > 0) && a != b, "the lit dot moves");
        // the first dot is brighter when it is the current one
        let r = 4.5f32;
        let total = 4.0 * (2.0 * r) + 3.0 * 8.0;
        let cx = (w as f32 - 112.0 - total + r) as i32;
        let at = |buf: &Vec<u8>| buf[(((h / 2) * w + cx) * 4 + 3) as usize];
        assert!(at(&a) > at(&b) + 100, "lit {} vs ring {}", at(&a), at(&b));
    }

    #[test]
    fn the_power_row_has_five_buttons_that_can_be_hit_and_do_not_overlap_the_rows() {
        let (w, h) = menu_size();
        assert_eq!(w, MENU_W);
        let mut seen = Vec::new();
        for i in 0..crate::power::ACTIONS.len() {
            let (x0, y0, x1, y1) = power_rect(i);
            assert!(x1 > x0 + 40 && y1 > y0 + 20 && x1 <= w && y1 <= h, "button {i} fits: {:?}", (x0, y0, x1, y1));
            assert_eq!(power_at((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0), Some(i));
            seen.push(x0);
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 5);
        assert_eq!(power_at(5.0, 5.0), None);
        assert_eq!(menu_row_at((h - 20) as f64, MENU_MAX_ROWS), None, "the power row is not a program row");
    }

    #[test]
    fn at_scale_two_the_bar_and_menu_are_twice_as_many_pixels_and_the_orb_is_twice_as_wide() {
        let mut text = TextRenderer::default();
        // an orb's width in pixels along the middle row: count the green pixels
        let mut orb_width = |s: i32| {
            let (w, h) = (400 * s, HEIGHT * s);
            let buf = draw(&mut text, w, h, s, false, "", &[]);
            let y = h / 2;
            (0..(ORB_SIZE + 2 * ORB_MARGIN) * s)
                .filter(|&x| {
                    let i = ((y * w + x) * 4) as usize;
                    buf[i + 1] as i32 > buf[i] as i32 + 40 && buf[i + 1] as i32 > buf[i + 2] as i32 + 20
                })
                .count() as i32
        };
        let (one, two) = (orb_width(1), orb_width(2));
        assert!((one - ORB_SIZE).abs() <= 4 && (two - 2 * ORB_SIZE).abs() <= 6, "the orb is about 32 px wide at 1x and 64 px at 2x: {one} {two}");
        let (mw, mh) = menu_size();
        assert_eq!(draw_menu(&mut text, 2, "", &[], None, &PowerView::default()).len() as i32, mw * 2 * mh * 2 * 4);
    }
}
