//! Desktop notifications: the picture of the bubbles (pure drawing, for tests) and the state a
//! `org.freedesktop.Notifications` server keeps. `bin/fde-notify.rs` is the program.

use crate::panel::{blend, blit};
use crate::text::TextRenderer;

pub const W: i32 = 340;
pub const BUBBLE_H: i32 = 76;
pub const GAP: i32 = 8;
pub const MAX_SHOWN: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// when it goes away; None = stays until dismissed
    pub expires: Option<std::time::Instant>,
    pub critical: bool,
}

/// What the server knows: the notes in order of arrival and the next id.
#[derive(Default)]
pub struct Store {
    pub notes: Vec<Note>,
    next_id: u32,
}

impl Store {
    /// Add a note (or replace the one with `replaces` as id); returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn notify(
        &mut self,
        app: &str,
        replaces: u32,
        summary: &str,
        body: &str,
        timeout_ms: i32,
        critical: bool,
        now: std::time::Instant,
    ) -> u32 {
        let expires = if critical || timeout_ms == 0 {
            None
        } else {
            let ms = if timeout_ms < 0 { 6000 } else { timeout_ms as u64 };
            Some(now + std::time::Duration::from_millis(ms))
        };
        let strip = |s: &str| strip_markup(s);
        let note = |id| Note { id, app: app.to_string(), summary: strip(summary), body: strip(body), expires, critical };
        if replaces != 0 {
            if let Some(n) = self.notes.iter_mut().find(|n| n.id == replaces) {
                *n = note(replaces);
                return replaces;
            }
        }
        self.next_id += 1;
        let id = self.next_id;
        self.notes.push(note(id));
        id
    }

    /// Remove one note; true if it existed.
    pub fn close(&mut self, id: u32) -> bool {
        let before = self.notes.len();
        self.notes.retain(|n| n.id != id);
        self.notes.len() != before
    }

    /// Remove expired notes and return their ids.
    pub fn expire(&mut self, now: std::time::Instant) -> Vec<u32> {
        let gone: Vec<u32> = self.notes.iter().filter(|n| n.expires.map(|e| e <= now).unwrap_or(false)).map(|n| n.id).collect();
        self.notes.retain(|n| !gone.contains(&n.id));
        gone
    }

    /// The notes on screen, newest first.
    pub fn shown(&self) -> Vec<&Note> {
        self.notes.iter().rev().take(MAX_SHOWN).collect()
    }
}

/// Notification bodies may carry a little HTML (`<b>`, `<i>`, `<a>`); show the text only.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

pub fn size(n: usize) -> (i32, i32) {
    let n = n.min(MAX_SHOWN) as i32;
    (W, n * BUBBLE_H + (n - 1).max(0) * GAP)
}

/// Which bubble is at height `y` (0 = the top one)?
pub fn row_at(y: f64, n: usize) -> Option<usize> {
    let step = (BUBBLE_H + GAP) as f64;
    let i = (y / step).floor();
    let within = y - i * step;
    (i >= 0.0 && (i as usize) < n.min(MAX_SHOWN) && within < BUBBLE_H as f64).then_some(i as usize)
}

/// The bubbles, newest on top, as one premultiplied RGBA picture of `size(n)` times `s`.
pub fn draw(text: &mut TextRenderer, s: i32, notes: &[&Note]) -> Vec<u8> {
    let (lw, lh) = size(notes.len());
    let (w, h) = (lw * s, lh * s);
    let sf = s as f32;
    let mut buf = vec![0u8; (w * h * 4).max(0) as usize];
    let pal = crate::theme::palette();
    let rad = (12.0 - 12.0 * pal.solid) * sf;
    let bubble_h = BUBBLE_H * s;
    for (i, note) in notes.iter().take(MAX_SHOWN).enumerate() {
        let y0 = i as i32 * (BUBBLE_H + GAP) * s;
        for y in 0..bubble_h {
            for x in 0..w {
                let dx = (x as f32 + 0.5 - w as f32 / 2.0).abs() - (w as f32 / 2.0 - rad);
                let dy = (y as f32 + 0.5 - bubble_h as f32 / 2.0).abs() - (bubble_h as f32 / 2.0 - rad);
                let d = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - rad;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let t = y as f32 / bubble_h as f32;
                let idx = (((y0 + y) * w + x) * 4) as usize;
                let px = &mut buf[idx..idx + 4];
                let base = if note.critical { [0.85, 0.30, 0.20] } else { pal.tone(t) };
                blend(px, base, 0.70 * cover);
                if t < 0.4 {
                    blend(px, [1.0, 1.0, 1.0], 0.18 * (1.0 - t / 0.4) * cover * pal.shine);
                }
                if d > -1.5 * sf {
                    blend(px, [1.0, 1.0, 1.0], 0.5 * cover);
                }
            }
        }
        if let Some(img) = text.line(&note.summary, w - 32 * s, 15.0 * sf, [255, 255, 255], pal.glow(0.9), 2 * s) {
            blit(&mut buf, w, h, &img, 16 * s, y0 + 12 * s);
        }
        if let Some(img) = text.line(&note.body, w - 32 * s, 13.0 * sf, [235, 245, 255], pal.glow(0.7), 2 * s) {
            blit(&mut buf, w, h, &img, 16 * s, y0 + 40 * s);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn notes_get_ids_replace_expire_and_close() {
        let t0 = Instant::now();
        let mut s = Store::default();
        let a = s.notify("app", 0, "Hi", "there", -1, false, t0);
        let b = s.notify("app", 0, "Second", "", 1000, false, t0);
        let c = s.notify("app", 0, "Sticky", "", 0, false, t0);
        assert_eq!((a, b, c), (1, 2, 3));
        assert_eq!(s.notify("app", a, "Hi again", "", -1, false, t0), a, "replacing keeps the id");
        assert_eq!(s.notes.len(), 3);
        assert!(s.expire(t0 + Duration::from_millis(500)).is_empty());
        assert_eq!(s.expire(t0 + Duration::from_millis(1500)), vec![b]);
        assert_eq!(s.expire(t0 + Duration::from_secs(60)), vec![a], "the default lifetime ends; sticky stays");
        assert!(s.close(c) && !s.close(c));
    }

    #[test]
    fn markup_is_stripped_and_the_newest_is_on_top() {
        assert_eq!(strip_markup("<b>Bold</b> &amp; <a href=\"x\">link</a>"), "Bold & link");
        let t0 = Instant::now();
        let mut s = Store::default();
        for i in 0..7 {
            s.notify("a", 0, &format!("n{i}"), "", -1, false, t0);
        }
        let shown = s.shown();
        assert_eq!(shown.len(), MAX_SHOWN);
        assert_eq!(shown[0].summary, "n6");
    }

    #[test]
    fn bubbles_are_drawn_with_gaps_and_hit_testing_matches() {
        let t0 = Instant::now();
        let mut s = Store::default();
        s.notify("a", 0, "One", "first", -1, false, t0);
        s.notify("a", 0, "Two", "second", -1, false, t0);
        let mut text = TextRenderer::default();
        let shown = s.shown();
        let buf = draw(&mut text, 1, &shown);
        let (w, h) = size(2);
        assert_eq!(h, 2 * BUBBLE_H + GAP);
        let alpha = |x: i32, y: i32| buf[((y * w + x) * 4 + 3) as usize];
        assert!(alpha(w / 2, 5) > 100 && alpha(w / 2, BUBBLE_H + GAP + 5) > 100, "both bubbles are on");
        assert_eq!(alpha(w / 2, BUBBLE_H + GAP / 2), 0, "and the gap between them is clear");
        assert_eq!(row_at(10.0, 2), Some(0));
        assert_eq!(row_at((BUBBLE_H + GAP + 10) as f64, 2), Some(1));
        assert_eq!(row_at((BUBBLE_H + 2) as f64, 2), None);
    }
}
