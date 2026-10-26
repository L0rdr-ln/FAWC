//! The built-in wallpaper: a procedural sky until there are real ones.

#![allow(non_snake_case)]

/// A sky with sun and hills (and optionally hard stripes, so that blur is easy to see). RGBA8, top row first.
pub fn procedural(w: i32, h: i32, stripes: bool) -> Vec<u8> {
    let (W, H) = (w, h);
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let fx = x as f32 / W as f32;
            let fy = y as f32 / H as f32;
            // sky: deep blue at the top to pale aqua at the horizon
            let mut c = [0.10 + 0.50 * fy, 0.45 + 0.35 * fy, 0.85 + 0.10 * fy];
            // sun with glow
            let aspect = W as f32 / H as f32;
            let (dx, dy) = ((fx - 0.78) * aspect, fy - 0.22); // round in pixels, sized by the height
            let d = (dx * dx + dy * dy).sqrt();
            let glow = (1.0 - d * 2.6).clamp(0.0, 1.0);
            for k in 0..3 {
                c[k] = c[k] + (1.0 - c[k]) * glow * glow;
            }
            if d < 0.075 {
                c = [1.0, 0.98, 0.85];
            }
            // grass hills
            let hill = 0.68 + 0.05 * (fx * 6.0).sin() + 0.03 * (fx * 15.0).cos();
            if fy > hill {
                let t = (fy - hill) / (1.0 - hill);
                c = [0.20 - 0.1 * t, 0.62 - 0.25 * t, 0.18 - 0.08 * t];
            }
            // hard white stripes and dots (to show the blur)
            if stripes && (x / 24 + y / 24) % 2 == 0 && fy > 0.30 && fy < 0.62 && fx < 0.55 {
                c = [1.0, 1.0, 1.0];
            }
            let i = ((y * W + x) * 4) as usize;
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
