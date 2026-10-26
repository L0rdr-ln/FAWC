//! A software mouse cursor for the backends that have no host cursor (DRM): a glassy arrow.

pub const SIZE: i32 = 28;

/// Premultiplied RGBA8, top row first. The hot spot is the top left corner (1, 1).
pub fn arrow() -> Vec<u8> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    // the arrow as a polygon (tip at 1,1), tested per pixel with 3x3 supersampling
    let poly: [(f32, f32); 7] = [(1.0, 1.0), (1.0, 18.0), (5.0, 14.2), (8.4, 21.5), (11.6, 20.0), (8.2, 12.9), (13.5, 12.9)];
    let inside = |x: f32, y: f32, grow: f32| -> bool {
        // point in polygon (even-odd); `grow` thickens it by testing a ring of offsets
        let test = |x: f32, y: f32| -> bool {
            let mut c = false;
            let mut j = poly.len() - 1;
            for i in 0..poly.len() {
                let (xi, yi) = poly[i];
                let (xj, yj) = poly[j];
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    c = !c;
                }
                j = i;
            }
            c
        };
        if grow <= 0.0 {
            return test(x, y);
        }
        for (dx, dy) in [(0.0, 0.0), (grow, 0.0), (-grow, 0.0), (0.0, grow), (0.0, -grow), (grow, grow), (-grow, -grow), (grow, -grow), (-grow, grow)] {
            if test(x + dx, y + dy) {
                return true;
            }
        }
        false
    };
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (mut body, mut rim) = (0.0f32, 0.0f32);
            for sy in 0..3 {
                for sx in 0..3 {
                    let (fx, fy) = (x as f32 + (sx as f32 + 0.5) / 3.0, y as f32 + (sy as f32 + 0.5) / 3.0);
                    if inside(fx, fy, 0.0) {
                        body += 1.0 / 9.0;
                    } else if inside(fx, fy, 1.2) {
                        rim += 1.0 / 9.0;
                    }
                }
            }
            // a white body with a soft blue shade towards the bottom, a dark blue rim
            let shade = (y as f32 / 22.0).clamp(0.0, 1.0);
            let body_c = [1.0 - 0.35 * shade, 1.0 - 0.18 * shade, 1.0];
            let rim_c = [0.05, 0.22, 0.42];
            let a = body + rim;
            let i = ((y * SIZE + x) * 4) as usize;
            if a > 0.0 {
                for k in 0..3 {
                    // premultiplied: body over rim
                    let c = body * body_c[k] + rim * rim_c[k];
                    px[i + k] = (c.clamp(0.0, 1.0) * 255.0) as u8;
                }
                px[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    #[test]
    fn arrow_has_a_tip_and_is_transparent_elsewhere() {
        let px = super::arrow();
        let at = |x: i32, y: i32| px[((y * super::SIZE + x) * 4 + 3) as usize];
        assert!(at(2, 3) > 200, "inside the arrow");
        assert!(at(25, 25) == 0 && at(20, 2) == 0, "outside is transparent");
    }
}
