//! Mouse cursors from the user's cursor theme (XCursor files), with the built-in arrow as the
//! fallback when the theme or a cursor in it is missing.

use std::collections::HashMap;

/// One cursor picture: premultiplied RGBA, top row first, and where its "tip" is.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorImage {
    pub width: i32,
    pub height: i32,
    pub xhot: i32,
    pub yhot: i32,
    pub rgba: Vec<u8>,
}

pub struct CursorTheme {
    theme: Option<xcursor::CursorTheme>,
    size: u32,
    cache: HashMap<String, CursorImage>,
}

impl CursorTheme {
    /// `name` empty means `$XCURSOR_THEME`, else "default"; `size` 0 means `$XCURSOR_SIZE`, else 24.
    pub fn load(name: &str, size: u32) -> CursorTheme {
        let name = if !name.is_empty() {
            name.to_string()
        } else {
            std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into())
        };
        let size = if size > 0 { size } else { std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24) };
        CursorTheme { theme: Some(xcursor::CursorTheme::load(&name)), size, cache: HashMap::new() }
    }

    /// A theme with nothing in it: only the built-in arrow.
    pub fn builtin_only() -> CursorTheme {
        CursorTheme { theme: None, size: 24, cache: HashMap::new() }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// The cursor called `name` ("default", "text", "pointer", "ew-resize" ...) at about
    /// `size * scale` pixels. Never fails: the built-in arrow stands in for anything missing.
    pub fn image(&mut self, name: &str, scale: u32) -> CursorImage {
        let key = format!("{name}@{scale}");
        if let Some(i) = self.cache.get(&key) {
            return i.clone();
        }
        let img = self.from_theme(name, scale).unwrap_or_else(builtin_arrow);
        self.cache.insert(key, img.clone());
        img
    }

    fn from_theme(&self, name: &str, scale: u32) -> Option<CursorImage> {
        let theme = self.theme.as_ref()?;
        // a few names have older spellings in themes
        let candidates: &[&str] = match name {
            "default" => &["default", "left_ptr"],
            "pointer" => &["pointer", "hand2", "hand1"],
            "text" => &["text", "xterm"],
            "ew-resize" => &["ew-resize", "sb_h_double_arrow", "h_double_arrow"],
            "ns-resize" => &["ns-resize", "sb_v_double_arrow", "v_double_arrow"],
            "nwse-resize" => &["nwse-resize", "bd_double_arrow", "size_fdiag"],
            "nesw-resize" => &["nesw-resize", "fd_double_arrow", "size_bdiag"],
            "grabbing" => &["grabbing", "closedhand"],
            "wait" => &["wait", "watch"],
            _ => &[],
        };
        let own = [name];
        let names = if candidates.is_empty() { &own[..] } else { candidates };
        for n in names {
            if let Some(path) = theme.load_icon(n) {
                let data = std::fs::read(path).ok()?;
                let images = xcursor::parser::parse_xcursor(&data)?;
                let want = (self.size * scale) as i32;
                // the picture whose nominal size is nearest the wanted one
                let best = images.iter().min_by_key(|i| (i.size as i32 - want).abs())?;
                return Some(from_xcursor(best));
            }
        }
        None
    }
}

fn from_xcursor(i: &xcursor::parser::Image) -> CursorImage {
    // XCursor files hold non-premultiplied ARGB words (bytes B, G, R, A on a little-endian
    // machine). The crate's `pixels_rgba` is those bytes as they are in the file, whatever its
    // name says.
    let mut rgba = Vec::with_capacity(i.pixels_rgba.len());
    for p in i.pixels_rgba.chunks_exact(4) {
        let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
        rgba.extend_from_slice(&[(r * a / 255) as u8, (g * a / 255) as u8, (b * a / 255) as u8, a as u8]);
    }
    CursorImage { width: i.width as i32, height: i.height as i32, xhot: i.xhot as i32, yhot: i.yhot as i32, rgba }
}

fn builtin_arrow() -> CursorImage {
    CursorImage {
        width: crate::cursor::SIZE,
        height: crate::cursor::SIZE,
        xhot: 1,
        yhot: 1,
        rgba: crate::cursor::arrow(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a minimal XCursor file with one image of `size` x `size` in one colour.
    fn write_cursor(path: &std::path::Path, sizes: &[(u32, [u8; 4])], hot: (u32, u32)) {
        let mut out = Vec::new();
        let n = sizes.len() as u32;
        out.extend_from_slice(b"Xcur");
        out.extend_from_slice(&16u32.to_le_bytes()); // header size
        out.extend_from_slice(&0x10000u32.to_le_bytes()); // version
        out.extend_from_slice(&n.to_le_bytes());
        let mut offset = 16 + 12 * n;
        let mut chunks = Vec::new();
        for (size, color) in sizes {
            out.extend_from_slice(&0xfffd0002u32.to_le_bytes()); // image
            out.extend_from_slice(&size.to_le_bytes()); // subtype = nominal size
            out.extend_from_slice(&offset.to_le_bytes());
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&36u32.to_le_bytes()); // chunk header size
            chunk.extend_from_slice(&0xfffd0002u32.to_le_bytes());
            chunk.extend_from_slice(&size.to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes()); // version
            chunk.extend_from_slice(&size.to_le_bytes()); // width
            chunk.extend_from_slice(&size.to_le_bytes()); // height
            chunk.extend_from_slice(&hot.0.to_le_bytes());
            chunk.extend_from_slice(&hot.1.to_le_bytes());
            chunk.extend_from_slice(&0u32.to_le_bytes()); // delay
            for _ in 0..size * size {
                // ARGB, little endian: B G R A
                chunk.extend_from_slice(&[color[2], color[1], color[0], color[3]]);
            }
            offset += chunk.len() as u32;
            chunks.push(chunk);
        }
        for c in chunks {
            out.extend_from_slice(&c);
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn a_theme_in_xcursor_path_is_used_with_the_nearest_size_and_old_names_work() {
        let dir = tempfile::tempdir().unwrap();
        let cursors = dir.path().join("mytheme/cursors");
        std::fs::create_dir_all(&cursors).unwrap();
        // "left_ptr" (the old name of "default") in two sizes, red and green; a resize cursor
        write_cursor(&cursors.join("left_ptr"), &[(16, [255, 0, 0, 255]), (32, [0, 255, 0, 255])], (3, 4));
        write_cursor(&cursors.join("sb_h_double_arrow"), &[(24, [0, 0, 255, 128])], (12, 12));
        std::env::set_var("XCURSOR_PATH", dir.path());
        let mut t = CursorTheme::load("mytheme", 30);
        let default = t.image("default", 1);
        assert_eq!((default.width, default.xhot, default.yhot), (32, 3, 4), "size 30 is nearest to 32: the green one");
        assert_eq!(&default.rgba[..4], &[0, 255, 0, 255]);
        let small = CursorTheme::load("mytheme", 16).image("default", 1);
        assert_eq!(&small.rgba[..4], &[255, 0, 0, 255], "size 16 picks the red one");
        // 2x scale wants 60 px: still the biggest there is
        assert_eq!(t.image("default", 2).width, 32);
        // an old name stands in, and the pixels are premultiplied
        let ew = t.image("ew-resize", 1);
        assert_eq!((ew.width, ew.xhot), (24, 12));
        assert_eq!(&ew.rgba[..4], &[0, 0, 128, 128], "blue at half alpha, premultiplied");
        // a cursor the theme does not have: the built-in arrow
        assert_eq!(t.image("not-a-cursor", 1), builtin_arrow());
        assert_eq!(CursorTheme::builtin_only().image("default", 1), builtin_arrow());
    }
}
