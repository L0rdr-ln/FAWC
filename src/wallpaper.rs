//! The built-in wallpaper: a procedural sky until there are real ones.

#![allow(non_snake_case)]

fn hash(x: i32, y: i32, seed: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ (seed as u32).wrapping_mul(2147483647);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Smooth value noise, 0..1.
fn noise(x: f32, y: f32, seed: i32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sy
}

fn fbm(x: f32, y: f32, seed: i32) -> f32 {
    let (mut sum, mut amp, mut f) = (0.0, 0.5, 1.0);
    for k in 0..4 {
        sum += amp * noise(x * f, y * f, seed + k);
        amp *= 0.5;
        f *= 2.0;
    }
    sum
}

fn over(c: &mut [f32; 3], color: [f32; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    for k in 0..3 {
        c[k] += (color[k] - c[k]) * a;
    }
}

/// The built-in wallpaper: a bright Frutiger Aero day: gradient sky with soft clouds and a sun
/// with rays, hazy and green hills, a glowing swoosh of light across them and a few soap
/// bubbles. (With `stripes`, hard white blocks that make the blur easy to see.) RGBA8, top row
/// first.
pub fn procedural(w: i32, h: i32, stripes: bool) -> Vec<u8> {
    let (wf, hf) = (w as f32, h as f32);
    let aspect = wf / hf;
    // soap bubbles: position (0..1), radius (in heights)
    let bubbles: Vec<(f32, f32, f32)> = (0..14)
        .map(|i| (0.05 + 0.9 * hash(i, 1, 7), 0.10 + 0.62 * hash(i, 2, 7), 0.012 + 0.05 * hash(i, 3, 7).powi(2)))
        .collect();
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / wf;
            let fy = y as f32 / hf;
            // sky: deep blue at the top to pale aqua at the horizon
            let mut c = [0.10 + 0.50 * fy, 0.45 + 0.35 * fy, 0.85 + 0.10 * fy];
            // sun with glow and rays
            let (dx, dy) = ((fx - 0.78) * aspect, fy - 0.22);
            let d = (dx * dx + dy * dy).sqrt();
            let glow = (1.0 - d * 2.6).clamp(0.0, 1.0);
            for k in 0..3 {
                c[k] += (1.0 - c[k]) * glow * glow;
            }
            let angle = dy.atan2(dx);
            let rays = (0.5 + 0.5 * (angle * 9.0 + 1.3).sin()) * (0.6 + 0.4 * (angle * 23.0).sin().abs());
            over(&mut c, [1.0, 1.0, 0.92], rays * (1.0 - (d * 1.6).clamp(0.0, 1.0)).powi(2) * 0.22);
            // clouds: soft, drifting, brighter towards the sun
            let cloud = ((fbm(fx * 3.2 * aspect, fy * 5.0, 3) - 0.50) * 4.2).clamp(0.0, 1.0) * (1.0 - fy * 1.1).clamp(0.0, 1.0);
            over(&mut c, [0.97, 0.99, 1.0], cloud * 0.55);
            if d < 0.075 {
                c = [1.0, 0.98, 0.85];
            }
            // hazy far hills, then the near grass hills
            let far = 0.60 + 0.04 * (fx * 4.0 + 1.0).sin() + 0.02 * (fx * 11.0).cos();
            if fy > far {
                let t = ((fy - far) / (1.0 - far)).clamp(0.0, 1.0);
                over(&mut c, [0.45, 0.72, 0.62], 0.85 - 0.3 * t);
            }
            let hill = 0.68 + 0.05 * (fx * 6.0).sin() + 0.03 * (fx * 15.0).cos();
            if fy > hill {
                let t = (fy - hill) / (1.0 - hill);
                c = [0.20 - 0.1 * t, 0.62 - 0.25 * t, 0.18 - 0.08 * t];
                // a lit rim along the top of the hill and some grass texture
                let rim = (1.0 - ((fy - hill) * hf / 5.0).clamp(0.0, 1.0)).powi(2);
                over(&mut c, [0.75, 0.95, 0.55], rim * 0.35);
                let grass = noise(fx * 220.0, fy * 60.0, 11) - 0.5;
                for v in &mut c {
                    *v += grass * 0.06;
                }
            }
            // the swoosh: a glowing ribbon of light curving across the lower part
            let ribbon_y = 0.80 - 0.16 * (fx * 3.1 + 0.6).sin() * (1.0 - 0.5 * fx) - 0.10 * fx;
            let across = (fy - ribbon_y) * hf;
            let core = (-(across / (hf * 0.012)).powi(2)).exp();
            let halo = (-(across / (hf * 0.05)).powi(2)).exp();
            over(&mut c, [0.80, 1.0, 1.0], core * 0.55 + halo * 0.16);
            // bubbles
            for &(bx, by, br) in &bubbles {
                let (ddx, ddy) = ((fx - bx) * aspect, fy - by);
                let dist = (ddx * ddx + ddy * ddy).sqrt();
                if dist < br * 1.15 {
                    let r = dist / br; // 0 centre .. 1 edge
                    let rim = (-((r - 0.96) / 0.06).powi(2)).exp();
                    let body = 0.10 * r * r;
                    let (hx, hy) = (ddx / br + 0.38, ddy / br + 0.40);
                    let spot = (-(hx * hx + hy * hy) / 0.025).exp();
                    over(&mut c, [1.0, 1.0, 1.0], if r <= 1.0 { body + rim * 0.55 + spot * 0.8 } else { rim * 0.4 });
                }
            }
            // hard white stripes and dots (to show the blur)
            if stripes && (x / 24 + y / 24) % 2 == 0 && fy > 0.30 && fy < 0.62 && fx < 0.55 {
                c = [1.0, 1.0, 1.0];
            }
            let i = ((y * w + x) * 4) as usize;
            px[i] = (c[0].clamp(0.0, 1.0) * 255.0) as u8;
            px[i + 1] = (c[1].clamp(0.0, 1.0) * 255.0) as u8;
            px[i + 2] = (c[2].clamp(0.0, 1.0) * 255.0) as u8;
            px[i + 3] = 255;
        }
    }
    px
}

/// A picture loaded from a file: RGBA8, top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

/// Read a PNG, JPEG or WebP file (found out from the first bytes, not the name) as RGBA.
pub fn load_image(path: &std::path::Path) -> Result<Image, String> {
    let head = {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut b = [0u8; 4];
        let n = f.read(&mut b).map_err(|e| format!("{}: {e}", path.display()))?;
        b[..n].to_vec()
    };
    if head.starts_with(&[0xff, 0xd8]) {
        load_jpeg(path)
    } else if head.starts_with(b"RIFF") {
        load_webp(path)
    } else {
        load_png(path)
    }
}

/// Read a WebP (lossy or lossless, with or without transparency) as RGBA.
pub fn load_webp(path: &std::path::Path) -> Result<Image, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    decode_webp(std::io::BufReader::new(file)).map_err(|e| format!("{}: {e}", path.display()))
}

fn decode_webp<R: std::io::BufRead + std::io::Seek>(reader: R) -> Result<Image, String> {
    let mut dec = image_webp::WebPDecoder::new(reader).map_err(|e| e.to_string())?;
    let (w, h) = dec.dimensions();
    let has_alpha = dec.has_alpha();
    let size = dec.output_buffer_size().ok_or("picture too large")?;
    let mut data = vec![0u8; size];
    dec.read_image(&mut data).map_err(|e| e.to_string())?;
    let rgba: Vec<u8> = if has_alpha {
        data
    } else {
        data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    };
    if w == 0 || h == 0 {
        return Err("empty picture".into());
    }
    Ok(Image { width: w as i32, height: h as i32, rgba })
}

/// The wallpaper that comes with FDE (`assets/wallpaper.webp`, built into the program), used
/// when the configuration names none.
const BUNDLED: &[u8] = include_bytes!("../assets/wallpaper.webp");

pub fn bundled() -> Option<Image> {
    decode_webp(std::io::Cursor::new(BUNDLED)).ok()
}

/// The picture to show: the file `path` names; if that is empty or cannot be read, the bundled
/// one. `FDE_BUILTIN_SKY=1` asks for the generated sky instead (None), which the tests use so
/// that their pixel checks do not depend on a photograph.
pub fn configured(path: &str) -> Option<Image> {
    if !path.is_empty() {
        match load_image(std::path::Path::new(path)) {
            Ok(i) => return Some(i),
            Err(e) => eprintln!("wallpaper: {e} (using the bundled picture)"),
        }
    }
    if std::env::var_os("FDE_BUILTIN_SKY").is_some() {
        return None;
    }
    bundled()
}

/// Where theme pictures are looked for, first match wins: `$FDE_WALLPAPERS`, the user's
/// `~/.local/share/fde/wallpapers`, `/usr/share/fde/wallpapers`, and (for a built source tree)
/// `assets/wallpapers` next to the sources.
pub fn wallpaper_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("FDE_WALLPAPERS") {
        v.push(PathBuf::from(p));
    }
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))) {
        v.push(h.join("fde/wallpapers"));
    }
    v.push(PathBuf::from("/usr/share/fde/wallpapers"));
    v.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/wallpapers")));
    v
}

/// The picture for this configuration: the user's file; else the theme's own background (a
/// solid colour or a gradient), or the bundled photograph for themes that use it.
pub fn for_config(config: &crate::config::Config) -> Option<Image> {
    let path = &config.wallpaper.path;
    if !path.is_empty() {
        if let Some(i) = configured(path).filter(|_| std::path::Path::new(path).exists()) {
            return Some(i);
        }
    }
    let t = config.theme();
    if !t.wallpaper.file.is_empty() {
        // the file name as written, else the same name with another picture extension
        let stem = std::path::Path::new(&t.wallpaper.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let names: Vec<String> = std::iter::once(t.wallpaper.file.clone()).chain(["webp", "png", "jpg", "jpeg"].iter().map(|e| format!("{stem}.{e}"))).collect();
        for p in wallpaper_dirs().iter().flat_map(|d| names.iter().map(move |n| d.join(n))) {
            if p.is_file() {
                match load_image(&p) {
                    Ok(i) => return Some(i),
                    Err(e) => eprintln!("wallpaper: {e} (using the theme's colours)"),
                }
            }
        }
    }
    let c = |s: &str| crate::config::parse_color(s).unwrap_or([0.0, 0.0, 0.0]);
    let (w, h) = (256i32, 144i32);
    let (a, b) = (c(&t.wallpaper.color), c(&t.wallpaper.color2));
    match t.wallpaper.kind.as_str() {
        "solid" => {
            let px: Vec<u8> = (0..w * h).flat_map(|_| [(a[0] * 255.0) as u8, (a[1] * 255.0) as u8, (a[2] * 255.0) as u8, 255]).collect();
            Some(Image { width: w, height: h, rgba: px })
        }
        "gradient" => {
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    // top to bottom with a slight lean to the right
                    let t = ((y as f32 / (h - 1) as f32) * 0.85 + (x as f32 / (w - 1) as f32) * 0.15).clamp(0.0, 1.0);
                    for k in 0..3 {
                        px.push(((a[k] + (b[k] - a[k]) * t) * 255.0).round() as u8);
                    }
                    px.push(255);
                }
            }
            Some(Image { width: w, height: h, rgba: px })
        }
        _ => configured(""),
    }
}

/// Read a JPEG (colour or grey) as RGBA.
pub fn load_jpeg(path: &std::path::Path) -> Result<Image, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
    let data = dec.decode().map_err(|e| format!("{}: {e}", path.display()))?;
    let info = dec.info().ok_or_else(|| format!("{}: no picture information", path.display()))?;
    let (w, h) = (info.width as i32, info.height as i32);
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        jpeg_decoder::PixelFormat::L8 => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => return Err(format!("{}: JPEG kind {other:?} is not supported", path.display())),
    };
    if w == 0 || h == 0 {
        return Err(format!("{}: empty picture", path.display()));
    }
    Ok(Image { width: w, height: h, rgba })
}

/// Read a PNG (any colour type, 8 or 16 bit) as RGBA.
pub fn load_png(path: &std::path::Path) -> Result<Image, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width as i32, info.height as i32);
    let data = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: palette image was not expanded", path.display())),
    };
    if w == 0 || h == 0 {
        return Err(format!("{}: empty picture", path.display()));
    }
    Ok(Image { width: w, height: h, rgba })
}

/// The picture colour at (x, y) in picture pixels, bilinear (x, y may be fractional).
fn sample(img: &Image, x: f32, y: f32) -> [f32; 3] {
    let x = (x - 0.5).clamp(0.0, (img.width - 1) as f32);
    let y = (y - 0.5).clamp(0.0, (img.height - 1) as f32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (x1, y1) = ((x0 + 1).min(img.width - 1), (y0 + 1).min(img.height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: i32, yy: i32, c: usize| img.rgba[((yy * img.width + xx) * 4) as usize + c] as f32;
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bottom = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        *o = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// The picture laid onto a screen of `w` x `h` pixels, opaque RGBA8:
/// `fill` covers the screen (the overhang is cut off), `fit` shows all of it with black bars,
/// `stretch` distorts it to the screen, `center` shows it at its own size, `tile` repeats it.
pub fn lay_out(img: &Image, w: i32, h: i32, mode: &str) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (iw, ih) = (img.width as f32, img.height as f32);
    for y in 0..h {
        for x in 0..w {
            // where this screen pixel falls on the picture, in picture pixels; None = black
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let src: Option<(f32, f32)> = match mode {
                "stretch" => Some((px * iw / w as f32, py * ih / h as f32)),
                "fit" | "fill" => {
                    let s = if mode == "fit" { (w as f32 / iw).min(h as f32 / ih) } else { (w as f32 / iw).max(h as f32 / ih) };
                    let (ox, oy) = ((w as f32 - iw * s) / 2.0, (h as f32 - ih * s) / 2.0);
                    let (sx, sy) = ((px - ox) / s, (py - oy) / s);
                    (sx >= 0.0 && sy >= 0.0 && sx < iw && sy < ih).then_some((sx, sy))
                }
                "center" => {
                    let (sx, sy) = (px - (w as f32 - iw) / 2.0, py - (h as f32 - ih) / 2.0);
                    (sx >= 0.0 && sy >= 0.0 && sx < iw && sy < ih).then_some((sx, sy))
                }
                "tile" => Some((px.rem_euclid(iw), py.rem_euclid(ih))),
                _ => Some((px * iw / w as f32, py * ih / h as f32)),
            };
            let i = ((y * w + x) * 4) as usize;
            if let Some((sx, sy)) = src {
                let c = sample(img, sx, sy);
                out[i] = c[0].round() as u8;
                out[i + 1] = c[1].round() as u8;
                out[i + 2] = c[2].round() as u8;
            }
            out[i + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x1: left red, right blue
    fn two() -> Image {
        Image { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] }
    }

    #[test]
    fn the_modes_place_the_picture_differently() {
        let img = two();
        let at = |px: &[u8], w: i32, x: i32, y: i32| [px[((y * w + x) * 4) as usize], px[((y * w + x) * 4) as usize + 1], px[((y * w + x) * 4) as usize + 2]];
        // a 8 x 8 screen
        let stretch = lay_out(&img, 8, 8, "stretch");
        assert_eq!(at(&stretch, 8, 0, 4), [255, 0, 0]);
        assert_eq!(at(&stretch, 8, 7, 4), [0, 0, 255]);
        // fit: 2:1 picture on a square screen: bars above and below, picture in the middle band
        let fit = lay_out(&img, 8, 8, "fit");
        assert_eq!(at(&fit, 8, 0, 0), [0, 0, 0], "black bar on top");
        assert_eq!(at(&fit, 8, 0, 4), [255, 0, 0]);
        assert_eq!(at(&fit, 8, 7, 4), [0, 0, 255]);
        // fill: covers everything, the picture's middle is cut off at the sides (8x8 shows its middle 1x1... of 2x1)
        let fill = lay_out(&img, 8, 8, "fill");
        assert!(fill.chunks_exact(4).all(|p| p != [0, 0, 0, 255]), "no bars with fill");
        // center: a 2x1 picture in the middle of 8x8, black around
        let center = lay_out(&img, 8, 8, "center");
        assert_eq!(at(&center, 8, 3, 3), [255, 0, 0]);
        assert_eq!(at(&center, 8, 4, 3), [0, 0, 255]);
        assert_eq!(at(&center, 8, 0, 0), [0, 0, 0]);
        // tile: red blue red blue ...
        let tile = lay_out(&img, 8, 8, "tile");
        assert_eq!(at(&tile, 8, 0, 3), [255, 0, 0]);
        assert_eq!(at(&tile, 8, 1, 3), [0, 0, 255]);
        assert_eq!(at(&tile, 8, 2, 3), [255, 0, 0]);
        assert!(tile.chunks_exact(4).all(|p| p[3] == 255), "always opaque");
    }

    #[test]
    fn a_jpeg_is_found_by_its_content_whatever_its_name() {
        let dir = tempfile::tempdir().unwrap();
        // a solid orange 16x16 JPEG saved under a .png name
        let path = dir.path().join("really-a-jpeg.png");
        let rgb: Vec<u8> = (0..16 * 16).flat_map(|_| [230u8, 120, 20]).collect();
        jpeg_encoder::Encoder::new_file(&path, 95).unwrap().encode(&rgb, 16, 16, jpeg_encoder::ColorType::Rgb).unwrap();
        let img = load_image(&path).unwrap();
        assert_eq!((img.width, img.height), (16, 16));
        let p = &img.rgba[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4];
        assert!((p[0] as i32 - 230).abs() < 8 && (p[1] as i32 - 120).abs() < 8 && (p[2] as i32 - 20).abs() < 8 && p[3] == 255, "orange: {p:?}");
        // and a PNG still goes through load_image
        let png_path = dir.path().join("a.png");
        let file = std::fs::File::create(&png_path).unwrap();
        let mut enc = png::Encoder::new(file, 1, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&[1, 2, 3]).unwrap();
        assert_eq!(load_image(&png_path).unwrap().rgba, vec![1, 2, 3, 255]);
        assert!(load_image(&dir.path().join("nothing")).is_err());
    }

    #[test]
    fn a_webp_is_read_by_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pic.jpg"); // a lie of a name
        let rgb: Vec<u8> = (0..4 * 4).flat_map(|_| [30u8, 160, 60]).collect();
        let mut out = Vec::new();
        image_webp::WebPEncoder::new(&mut out).encode(&rgb, 4, 4, image_webp::ColorType::Rgb8).unwrap();
        std::fs::write(&path, out).unwrap();
        let img = load_image(&path).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(&img.rgba[..4], &[30, 160, 60, 255], "a lossless picture comes back exactly");
    }

    #[test]
    fn the_bundled_wallpaper_is_a_big_sky_and_grass_picture() {
        let img = bundled().expect("the wallpaper built into the program decodes");
        assert!(img.width >= 1900 && img.height >= 1200, "a full-size picture: {}x{}", img.width, img.height);
        // blue sky at the top, green grass at the bottom
        let px = |x: i32, y: i32| {
            let i = ((y * img.width + x) * 4) as usize;
            [img.rgba[i] as i32, img.rgba[i + 1] as i32, img.rgba[i + 2] as i32]
        };
        let (sky, grass) = (px(img.width / 2, 40), px(img.width / 2, img.height - 40));
        assert!(sky[2] > sky[0] + 60, "the sky is blue: {sky:?}");
        assert!(grass[1] > grass[0] + 60 && grass[1] > grass[2] + 60, "the grass is green: {grass:?}");
    }

    #[test]
    fn a_png_file_is_read_in_its_colours() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.png");
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(file, 2, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&[10, 20, 30, 40, 50, 60]).unwrap();
        let img = load_png(&path).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);
        assert!(load_png(&dir.path().join("missing.png")).is_err());
    }
}
