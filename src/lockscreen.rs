//! The lock screen's picture: the wallpaper with a glass login card in the middle.
//! Pure drawing (RGBA, top row first) so it can be tested; `bin/fde-lock.rs` shows it.

use crate::panel::{blend, blit};
use crate::text::TextRenderer;

pub const CARD_W: i32 = 380;
pub const CARD_H: i32 = 210;

/// The picture behind the card: the wallpaper, opaque. Making it is slow, so make it once per
/// monitor and pass it to `draw` every time.
pub fn background(picture: Option<&crate::wallpaper::Image>, mode: &str, w: i32, h: i32) -> Vec<u8> {
    let mut buf = match picture {
        Some(img) => crate::wallpaper::lay_out(img, w, h, mode),
        None => crate::wallpaper::procedural(w, h, false),
    };
    for p in buf.chunks_exact_mut(4) {
        p[3] = 255;
    }
    buf
}

/// `w` x `h` pixels at `s` pixels per logical pixel. `dots` is the number of characters typed;
/// `message` a line under the field ("Wrong password").
pub fn draw(text: &mut TextRenderer, background: &[u8], w: i32, h: i32, s: i32, user: &str, dots: usize, message: &str) -> Vec<u8> {
    let mut buf = background.to_vec();
    let sf = s as f32;
    let (card_w, card_h) = (CARD_W * s, CARD_H * s);
    let (cx0, cy0) = ((w - card_w) / 2, (h - card_h) / 2);
    let pal = fde_pal();
    let rad = (16.0 - 16.0 * pal.solid) * sf;
    for y in 0..card_h {
        for x in 0..card_w {
            let dx = (x as f32 + 0.5 - card_w as f32 / 2.0).abs() - (card_w as f32 / 2.0 - rad);
            let dy = (y as f32 + 0.5 - card_h as f32 / 2.0).abs() - (card_h as f32 / 2.0 - rad);
            let d = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - rad;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = y as f32 / card_h as f32;
            let i = (((cy0 + y) * w + cx0 + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, pal.tone(t), (0.55 + 0.4 * pal.solid) * cover);
            if t < 0.3 {
                blend(px, [1.0, 1.0, 1.0], 0.20 * (1.0 - t / 0.3) * cover * pal.shine);
            }
            if d > -1.5 * sf {
                blend(px, [1.0, 1.0, 1.0], 0.5 * cover);
            }
        }
    }
    // the user's name
    if let Some(img) = text.line(user, card_w - 40 * s, 22.0 * sf, [255, 255, 255], pal.glow(0.9), 4 * s) {
        blit(&mut buf, w, h, &img, cx0 + (card_w - img.width) / 2, cy0 + 28 * s);
    }
    // the password field: a white sunken pill with one dot per character
    let (fx0, fy0, fw, fh) = (cx0 + 30 * s, cy0 + 90 * s, card_w - 60 * s, 36 * s);
    for y in 0..fh {
        for x in 0..fw {
            let dx = (x as f32 + 0.5 - fw as f32 / 2.0).abs() - (fw as f32 / 2.0 - fh as f32 / 2.0);
            let dy = (y as f32 + 0.5 - fh as f32 / 2.0).abs();
            let d = dx.max(0.0).hypot(dy) - fh as f32 / 2.0;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                let i = (((fy0 + y) * w + fx0 + x) * 4) as usize;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.80 * cover);
                if d > -1.2 * sf {
                    blend(&mut buf[i..i + 4], [0.1, 0.3, 0.6], 0.5 * cover);
                }
            }
        }
    }
    for k in 0..dots.min(((fw - 30 * s) / (16 * s)) as usize) {
        let (dx0, dy0) = (fx0 + 18 * s + k as i32 * 16 * s, fy0 + fh / 2);
        let r = 4 * s;
        for y in -r..=r {
            for x in -r..=r {
                let c = (r as f32 + 0.5 - ((x * x + y * y) as f32).sqrt()).clamp(0.0, 1.0);
                if c > 0.0 {
                    let i = (((dy0 + y) * w + dx0 + x) * 4) as usize;
                    blend(&mut buf[i..i + 4], [0.1, 0.25, 0.5], 0.9 * c);
                }
            }
        }
    }
    let line = if message.is_empty() { "Type your password and press Enter" } else { message };
    if let Some(img) = text.line(line, card_w - 40 * s, 14.0 * sf, [255, 255, 255], pal.glow(0.9), 3 * s) {
        blit(&mut buf, w, h, &img, cx0 + (card_w - img.width) / 2, cy0 + 145 * s);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_is_glass_in_the_middle_and_dots_follow_typing() {
        let mut text = TextRenderer::default();
        let (w, h) = (800, 600);
        let bg = background(None, "fill", w, h);
        let empty = draw(&mut text, &bg, w, h, 1, "alice", 0, "");
        let typed = draw(&mut text, &bg, w, h, 1, "alice", 3, "");
        let at = |b: &[u8], x: i32, y: i32| {
            let i = ((y * w + x) * 4) as usize;
            [b[i] as i32, b[i + 1] as i32, b[i + 2] as i32, b[i + 3] as i32]
        };
        assert_eq!(at(&empty, 400, 10)[3], 255, "the wallpaper is opaque");
        assert!(at(&empty, 400, 10) != at(&empty, 400, 190), "the card changes the picture in the middle");
        // the first dot is where the field starts
        let (fx0, fy0) = ((w - CARD_W) / 2 + 30, (h - CARD_H) / 2 + 90);
        let dot = at(&typed, fx0 + 18, fy0 + 18);
        let none = at(&empty, fx0 + 18, fy0 + 18);
        assert!(dot[0] < none[0] - 50, "a dark dot appears: {dot:?} vs {none:?}");
        // at 2x the card is twice as wide: its edge is at 2 * 190 px from the middle
        let big = draw(&mut text, &background(None, "fill", 1600, 1200), 1600, 1200, 2, "alice", 0, "");
        let row = |x: i32| {
            let i = ((600 * 1600 + x) * 4) as usize;
            [big[i] as i32, big[i + 1] as i32, big[i + 2] as i32]
        };
        let (inside, outside) = (row(800 + 2 * CARD_W / 2 - 30), row(800 + 2 * CARD_W / 2 + 30));
        assert!(inside != outside, "the card edge is at 2 x its logical width: {inside:?} {outside:?}");
    }
}

/// Is `password` right for `user`? Runs `$FDE_LOCK_CHECK` (a shell command that reads the
/// password on its standard input and exits 0 if it is right) or else PAM's own helper
/// `unix_chkpwd`, which checks the password of the user running it.
pub fn check_password(user: &str, password: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = match std::env::var("FDE_LOCK_CHECK") {
        Ok(c) => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(c).env("FDE_LOCK_USER", user);
            cmd
        }
        Err(_) => {
            let helper = ["/sbin/unix_chkpwd", "/usr/sbin/unix_chkpwd", "/usr/bin/unix_chkpwd"]
                .into_iter()
                .find(|p| std::path::Path::new(p).exists())
                .unwrap_or("unix_chkpwd");
            let mut cmd = Command::new(helper);
            cmd.arg(user).arg("nullok");
            cmd
        }
    };
    let Ok(mut child) = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(password.as_bytes());
        let _ = stdin.write_all(&[0]);
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn fde_pal() -> crate::theme::Palette {
    crate::theme::palette()
}
