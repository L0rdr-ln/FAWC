//! The lock screen's picture: the wallpaper with a glass login card in the middle.
//! Pure drawing (RGBA, top row first) so it can be tested; `bin/fawc-lock.rs` shows it.

use crate::panel::{blend, blit};
use crate::text::TextRenderer;

pub const CARD_W: i32 = 380;
pub const CARD_H: i32 = 210;

/// `dots` is the number of characters typed; `message` a line under the field ("Wrong password").
pub fn draw(text: &mut TextRenderer, w: i32, h: i32, user: &str, dots: usize, message: &str) -> Vec<u8> {
    let mut buf = crate::wallpaper::procedural(w, h, false);
    // the wallpaper has no transparency; make sure alpha is full
    for p in buf.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let (cx0, cy0) = ((w - CARD_W) / 2, (h - CARD_H) / 2);
    let rad = 16.0f32;
    for y in 0..CARD_H {
        for x in 0..CARD_W {
            let dx = (x as f32 + 0.5 - CARD_W as f32 / 2.0).abs() - (CARD_W as f32 / 2.0 - rad);
            let dy = (y as f32 + 0.5 - CARD_H as f32 / 2.0).abs() - (CARD_H as f32 / 2.0 - rad);
            let d = dx.max(0.0).hypot(dy.max(0.0)) - rad;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = y as f32 / CARD_H as f32;
            let i = (((cy0 + y) * w + cx0 + x) * 4) as usize;
            let px = &mut buf[i..i + 4];
            blend(px, [0.25 + 0.15 * (1.0 - t), 0.58 + 0.12 * (1.0 - t), 0.92], 0.55 * cover);
            if t < 0.3 {
                blend(px, [1.0, 1.0, 1.0], 0.20 * (1.0 - t / 0.3) * cover);
            }
            if d > -1.5 {
                blend(px, [1.0, 1.0, 1.0], 0.5 * cover);
            }
        }
    }
    // the user's name
    if let Some(img) = text.line(user, CARD_W - 40, 22.0, [255, 255, 255], Some(([20, 70, 140], 0.9)), 4) {
        blit(&mut buf, w, h, &img, cx0 + (CARD_W - img.width) / 2, cy0 + 28);
    }
    // the password field: a white sunken pill with one dot per character
    let (fx0, fy0, fw, fh) = (cx0 + 30, cy0 + 90, CARD_W - 60, 36);
    for y in 0..fh {
        for x in 0..fw {
            let dx = (x as f32 + 0.5 - fw as f32 / 2.0).abs() - (fw as f32 / 2.0 - fh as f32 / 2.0);
            let dy = (y as f32 + 0.5 - fh as f32 / 2.0).abs();
            let d = dx.max(0.0).hypot(dy) - fh as f32 / 2.0;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                let i = (((fy0 + y) * w + fx0 + x) * 4) as usize;
                blend(&mut buf[i..i + 4], [1.0, 1.0, 1.0], 0.80 * cover);
                if d > -1.2 {
                    blend(&mut buf[i..i + 4], [0.1, 0.3, 0.6], 0.5 * cover);
                }
            }
        }
    }
    for k in 0..dots.min(((fw - 30) / 16) as usize) {
        let (dx0, dy0) = (fx0 + 18 + k as i32 * 16, fy0 + fh / 2);
        for y in -4..=4i32 {
            for x in -4..=4i32 {
                let c = (4.5 - ((x * x + y * y) as f32).sqrt()).clamp(0.0, 1.0);
                if c > 0.0 {
                    let i = (((dy0 + y) * w + dx0 + x) * 4) as usize;
                    blend(&mut buf[i..i + 4], [0.1, 0.25, 0.5], 0.9 * c);
                }
            }
        }
    }
    let line = if message.is_empty() { "Type your password and press Enter" } else { message };
    if let Some(img) = text.line(line, CARD_W - 40, 14.0, [255, 255, 255], Some(([20, 70, 140], 0.9)), 3) {
        blit(&mut buf, w, h, &img, cx0 + (CARD_W - img.width) / 2, cy0 + 145);
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
        let empty = draw(&mut text, w, h, "alice", 0, "");
        let typed = draw(&mut text, w, h, "alice", 3, "");
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
    }
}

/// Is `password` right for `user`? Runs `$FAWC_LOCK_CHECK` (a shell command that reads the
/// password on its standard input and exits 0 if it is right) or else PAM's own helper
/// `unix_chkpwd`, which checks the password of the user running it.
pub fn check_password(user: &str, password: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = match std::env::var("FAWC_LOCK_CHECK") {
        Ok(c) => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(c).env("FAWC_LOCK_USER", user);
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
