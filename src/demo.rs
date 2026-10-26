//! `fawc glass-demo out.png`: renders a still scene (a bright backdrop and two glass windows)
//! offscreen and writes it as a PNG. It is how the glass look is developed and tested without
//! a display.

use smithay::backend::allocator::Fourcc;
use smithay::backend::egl::{context::EGLContext, display::EGLDisplay, native::EGLSurfacelessDisplay};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem, Frame, ImportMem, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer as BufferCoord, Physical, Rectangle, Size, Transform};

use crate::glass::{Glass, GlassStyle};

pub const W: i32 = 960;
pub const H: i32 = 540;

pub fn offscreen_renderer() -> Result<GlesRenderer, Box<dyn std::error::Error>> {
    unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay)?;
        let ctx = EGLContext::new(&display)?;
        Ok(GlesRenderer::new(ctx)?)
    }
}

/// A sky with sun, hills and a few hard stripes, so that blur is easy to see. RGBA8, top row first.
pub fn backdrop_pixels() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let fx = x as f32 / W as f32;
            let fy = y as f32 / H as f32;
            // sky: deep blue at the top to pale aqua at the horizon
            let mut c = [0.10 + 0.50 * fy, 0.45 + 0.35 * fy, 0.85 + 0.10 * fy];
            // sun with glow
            let (dx, dy) = (fx - 0.78, (fy - 0.22) * 0.56);
            let d = (dx * dx + dy * dy).sqrt();
            let glow = (1.0 - d * 6.0).clamp(0.0, 1.0);
            for k in 0..3 {
                c[k] = c[k] + (1.0 - c[k]) * glow * glow;
            }
            if d < 0.045 {
                c = [1.0, 0.98, 0.85];
            }
            // grass hills
            let hill = 0.68 + 0.05 * (fx * 6.0).sin() + 0.03 * (fx * 15.0).cos();
            if fy > hill {
                let t = (fy - hill) / (1.0 - hill);
                c = [0.20 - 0.1 * t, 0.62 - 0.25 * t, 0.18 - 0.08 * t];
            }
            // hard white stripes and dots (to show the blur)
            if (x / 24 + y / 24) % 2 == 0 && fy > 0.30 && fy < 0.62 && fx < 0.55 {
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

/// A fake client: a terminal-ish dark translucent panel with bars of "text" (premultiplied).
pub fn client_pixels(w: i32, h: i32, alpha: f32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let bar = (y % 18) < 8 && x > 14 && x < w - 14 - ((y / 18) * 37 % (w / 2));
            let (c, a) = if bar { ([0.85, 0.95, 1.0], 0.92) } else { ([0.04, 0.09, 0.16], alpha) };
            let i = ((y * w + x) * 4) as usize;
            px[i] = (c[0] * a * 255.0) as u8;
            px[i + 1] = (c[1] * a * 255.0) as u8;
            px[i + 2] = (c[2] * a * 255.0) as u8;
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    px
}

pub struct Window {
    pub outer: Rectangle<i32, Physical>,
    pub style: GlassStyle,
    pub client_alpha: f32,
}

/// Render the scene and return RGBA pixels, top row first.
pub fn render_scene(
    renderer: &mut GlesRenderer,
    glass: &Glass,
    windows: &[Window],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let size = Size::<i32, BufferCoord>::from((W, H));
    let backdrop = renderer.import_memory(&backdrop_pixels(), Fourcc::Abgr8888, size, false)?;
    let mut scene: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, size)?;

    // 1. the backdrop into the scene texture
    {
        let mut target = renderer.bind(&mut scene)?;
        let mut frame = renderer.render(&mut target, (W, H).into(), Transform::Normal)?;
        frame.render_texture_from_to(
            &backdrop,
            Rectangle::from_size((W as f64, H as f64).into()),
            Rectangle::from_size((W, H).into()),
            &[Rectangle::from_size((W, H).into())],
            &[],
            Transform::Normal,
            1.0,
            None,
            &[],
        )?;
        let _ = frame.finish()?;
    }

    // 2. each window, bottom to top: blur what is behind it, glass panel, client on top
    for win in windows {
        let margin = 24;
        let region = Rectangle::<i32, Physical>::new(
            (win.outer.loc.x - margin, win.outer.loc.y - margin).into(),
            (win.outer.size.w + 2 * margin, win.outer.size.h + 2 * margin).into(),
        )
        .intersection(Rectangle::from_size((W, H).into()))
        .unwrap_or(win.outer);
        let blurred = glass.blur(renderer, &scene, region, win.style.blur_levels)?;
        // the blurred texture covers `region`; the panel shader wants a texture that covers the
        // window, so draw the window's share of it: tell the shader through a sub-rectangle
        let inner_x = (win.outer.loc.x - region.loc.x) as f64 / region.size.w as f64;
        let inner_y = (win.outer.loc.y - region.loc.y) as f64 / region.size.h as f64;
        let inner_w = win.outer.size.w as f64 / region.size.w as f64;
        let inner_h = win.outer.size.h as f64 / region.size.h as f64;
        let bs = blurred.size();
        let src = Rectangle::<f64, BufferCoord>::new(
            (inner_x * bs.w as f64, inner_y * bs.h as f64).into(),
            (inner_w * bs.w as f64, inner_h * bs.h as f64).into(),
        );
        let client = renderer.import_memory(
            &client_pixels(
                win.outer.size.w - 2 * win.style.border as i32,
                win.outer.size.h - 2 * win.style.border as i32 - win.style.titlebar as i32,
                win.client_alpha,
            ),
            Fourcc::Abgr8888,
            Size::from((
                win.outer.size.w - 2 * win.style.border as i32,
                win.outer.size.h - 2 * win.style.border as i32 - win.style.titlebar as i32,
            )),
            false,
        )?;
        let mut target = renderer.bind(&mut scene)?;
        let mut frame = renderer.render(&mut target, (W, H).into(), Transform::Normal)?;
        glass.draw_panel_part(&mut frame, &blurred, src, win.outer, &win.style)?;
        let inner = Rectangle::<i32, Physical>::new(
            (
                win.outer.loc.x + win.style.border as i32,
                win.outer.loc.y + win.style.border as i32 + win.style.titlebar as i32,
            )
                .into(),
            (
                win.outer.size.w - 2 * win.style.border as i32,
                win.outer.size.h - 2 * win.style.border as i32 - win.style.titlebar as i32,
            )
                .into(),
        );
        frame.render_texture_from_to(
            &client,
            Rectangle::from_size((inner.size.w as f64, inner.size.h as f64).into()),
            inner,
            &[Rectangle::from_size(inner.size)],
            &[],
            Transform::Normal,
            1.0,
            None,
            &[],
        )?;
        let _ = frame.finish()?;
    }

    // 3. read the scene back
    let target = renderer.bind(&mut scene)?;
    let mapping = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888)?;
    let bytes = renderer.map_texture(&mapping)?.to_vec();
    Ok(bytes)
}

pub fn write_png(path: &str, rgba: &[u8], w: i32, h: i32, flip: bool) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    if flip {
        let stride = (w_of(rgba, h)) as usize;
        let mut out = Vec::with_capacity(rgba.len());
        for row in (0..h as usize).rev() {
            out.extend_from_slice(&rgba[row * stride..(row + 1) * stride]);
        }
        w.write_image_data(&out)?;
    } else {
        w.write_image_data(rgba)?;
    }
    Ok(())
}

fn w_of(rgba: &[u8], h: i32) -> i32 {
    (rgba.len() as i32) / h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glass::GlassStyle;

    fn px(buf: &[u8], x: i32, y: i32) -> [i32; 3] {
        let i = ((y * W + x) * 4) as usize;
        [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32]
    }

    fn lum(c: [i32; 3]) -> i32 {
        (c[0] * 3 + c[1] * 6 + c[2]) / 10
    }

    /// Renders one glass window whose client area is fully transparent over the stripes.
    fn scene() -> Option<(Vec<u8>, Vec<u8>)> {
        let mut renderer = match offscreen_renderer() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("no GL context here ({e}): glass tests skipped");
                return None;
            }
        };
        let glass = Glass::new(&mut renderer).unwrap();
        let backdrop = backdrop_pixels();
        let windows = [Window {
            outer: Rectangle::new((40, 150).into(), (420, 260).into()),
            style: GlassStyle::default(),
            client_alpha: 0.0,
        }];
        let out = render_scene(&mut renderer, &glass, &windows).unwrap();
        Some((backdrop, out))
    }

    #[test]
    fn glass_blurs_what_is_behind_the_window() {
        let Some((backdrop, out)) = scene() else { return };
        // the stripes are hard in the backdrop: find a pixel on a stripe edge inside the window's
        // client area; after blur it is neither white nor the sky behind it
        let (x, y) = (48 + 24 * 0, 150 + 5 + 32 + 40);
        let _ = (x, y);
        let mut mid_tones = 0;
        let mut sharp_in_backdrop = 0;
        for yy in 200..380 {
            for xx in 60..440 {
                let b = lum(px(&backdrop, xx, yy));
                let o = lum(px(&out, xx, yy));
                if b > 250 {
                    sharp_in_backdrop += 1;
                    if o < 245 {
                        mid_tones += 1;
                    }
                }
            }
        }
        assert!(sharp_in_backdrop > 100, "the test scene has stripes behind the window");
        assert!(mid_tones * 10 > sharp_in_backdrop * 9, "stripes behind glass must be blurred away ({mid_tones}/{sharp_in_backdrop})");
    }

    #[test]
    fn corners_are_rounded_and_untouched() {
        let Some((backdrop, out)) = scene() else { return };
        // the very corner pixel of the window is outside the rounded shape
        for (x, y) in [(40, 150), (459, 150), (40, 409), (459, 409)] {
            let (b, o) = (px(&backdrop, x, y), px(&out, x, y));
            assert!((0..3).all(|k| (b[k] - o[k]).abs() <= 3), "corner ({x},{y}) changed: {b:?} -> {o:?}");
        }
        // while the middle of the top edge is glass
        let (b, o) = (px(&backdrop, 250, 152), px(&out, 250, 152));
        assert!((0..3).any(|k| (b[k] - o[k]).abs() > 10), "top edge should be glass: {b:?} -> {o:?}");
    }

    #[test]
    fn title_bar_has_gloss() {
        let Some((_, out)) = scene() else { return };
        // inside the title bar the upper part is lighter than the lower part
        let top = lum(px(&out, 250, 150 + 5 + 4));
        let bottom = lum(px(&out, 250, 150 + 5 + 28));
        assert!(top > bottom + 15, "gloss: top {top} should be lighter than bottom {bottom}");
    }
}
