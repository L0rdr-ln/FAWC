//! Night light: a warm tint over the whole picture in the evening (a multiply at the very end of
//! drawing a monitor, so it works on every backend; screenshots are taken before it).

use crate::state::Fde;

/// The colour multipliers for a colour temperature (Tanner Helland's approximation, scaled so
/// that 6500 K means no change).
pub fn kelvin_to_rgb(kelvin: u32) -> [f32; 3] {
    fn raw(k: f64) -> [f64; 3] {
        let t = k / 100.0;
        let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
        let g = if t <= 66.0 { 99.4708025861 * t.ln() - 161.1195681661 } else { 288.1221695283 * (t - 60.0).powf(-0.0755148492) };
        let b = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.5177312231 * (t - 10.0).ln() - 305.0447927307
        };
        [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)]
    }
    let (c, w) = (raw(kelvin as f64), raw(6500.0));
    [(c[0] / w[0]) as f32, (c[1] / w[1]) as f32, (c[2] / w[2]) as f32].map(|v| v.clamp(0.0, 1.0))
}

/// Is `now` (minutes after midnight) within the warm hours? The hours may cross midnight.
pub fn in_warm_hours(now: u32, from: u32, to: u32) -> bool {
    if from == to {
        true
    } else if from < to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

fn local_minutes() -> u32 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (tm.tm_hour * 60 + tm.tm_min) as u32
    }
}

impl Fde {
    /// The tint to multiply the picture with now, if night light is on and it is its time.
    pub fn night_light_tint(&self) -> Option<[f32; 3]> {
        let d = &self.config.display;
        if !d.night_light {
            return None;
        }
        let (from, to) = (crate::config::parse_clock(&d.night_light_from), crate::config::parse_clock(&d.night_light_to));
        if let (Some(f), Some(t)) = (from, to) {
            let now = std::env::var("FDE_NIGHT_LIGHT_NOW").ok().and_then(|v| crate::config::parse_clock(&v)).unwrap_or_else(local_minutes);
            if !in_warm_hours(now, f, t) {
                return None;
            }
        }
        Some(kelvin_to_rgb(d.night_light_temperature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmer_means_less_blue_and_6500_means_no_change() {
        let neutral = kelvin_to_rgb(6500);
        assert!(neutral.iter().all(|&c| (c - 1.0).abs() < 0.01), "{neutral:?}");
        let warm = kelvin_to_rgb(3500);
        assert!(warm[0] > warm[1] && warm[1] > warm[2], "orange: {warm:?}");
        assert!(warm[2] < 0.8);
        let hot = kelvin_to_rgb(1500);
        assert!(hot[2] < warm[2] && hot[1] < warm[1]);
    }

    #[test]
    fn the_warm_hours_may_cross_midnight() {
        assert!(in_warm_hours(22 * 60, 20 * 60, 7 * 60));
        assert!(in_warm_hours(3 * 60, 20 * 60, 7 * 60));
        assert!(!in_warm_hours(12 * 60, 20 * 60, 7 * 60));
        assert!(in_warm_hours(9 * 60, 8 * 60, 17 * 60) && !in_warm_hours(18 * 60, 8 * 60, 17 * 60));
        assert!(in_warm_hours(5, 600, 600), "no hours given: always");
    }
}
