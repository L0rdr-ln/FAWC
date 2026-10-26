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

