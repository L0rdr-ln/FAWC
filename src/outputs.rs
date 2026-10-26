//! Monitors: several outputs side by side in one logical space, each with its own scale
//! (HiDPI: 2 physical pixels per logical one, or 1.5 ...). Windows live in the shared space and
//! may be on any of them; each monitor is drawn from its own part of it.

use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use crate::state::Fawc;

impl Fawc {
    /// Every monitor, the first one first.
    pub fn all_outputs(&self) -> Vec<Output> {
        std::iter::once(self.output.clone()).chain(self.extra_outputs.iter().cloned()).collect()
    }

    pub fn output_scale(&self, output: &Output) -> f64 {
        output.current_scale().fractional_scale()
    }

    /// The monitor's size in logical pixels: its mode divided by its scale.
    pub fn output_logical_size(&self, output: &Output) -> Size<i32, Logical> {
        let scale = self.output_scale(output);
        output
            .current_mode()
            .map(|m| ((m.size.w as f64 / scale).round() as i32, (m.size.h as f64 / scale).round() as i32).into())
            .unwrap_or((1280, 720).into())
    }

    /// Where the monitor is in the shared logical space.
    pub fn output_rect(&self, output: &Output) -> Rectangle<i32, Logical> {
        let loc = self.space.output_geometry(output).map(|g| g.loc).unwrap_or_default();
        Rectangle::new(loc, self.output_logical_size(output))
    }

    /// Add a monitor: `mode_px` is its mode in physical pixels. Scale, mode and position come from
    /// `[outputs.NAME]` in the configuration; without a position it goes to the right of the others.
    pub fn add_output(&mut self, name: &str, mode_px: (i32, i32), refresh_mhz: i32) -> Output {
        let output = Output::new(
            name.to_string(),
            PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "FAWC".into(), model: name.to_string() },
        );
        let mode = Mode { size: mode_px.into(), refresh: refresh_mhz };
        let oc = self.config.outputs.get(name).cloned().unwrap_or_default();
        let to_the_right = self
            .all_outputs()
            .iter()
            .map(|o| {
                let r = self.output_rect(o);
                r.loc.x + r.size.w
            })
            .max()
            .unwrap_or(0);
        let loc: Point<i32, Logical> = (oc.x.unwrap_or(to_the_right), oc.y.unwrap_or(0)).into();
        output.change_current_state(Some(mode), Some(Transform::Normal), Some(Scale::Fractional(oc.scale)), Some(loc));
        output.set_preferred(mode);
        let _global = output.create_global::<Fawc>(&self.display_handle);
        self.space.map_output(&output, loc);
        self.extra_outputs.push(output.clone());
        tracing::info!("output {name}: {}x{} at scale {} placed at {},{}", mode_px.0, mode_px.1, oc.scale, loc.x, loc.y);
        output
    }

    /// The monitor under `p`, if any.
    pub fn output_at(&self, p: Point<f64, Logical>) -> Option<Output> {
        self.all_outputs().into_iter().find(|o| {
            let r = self.output_rect(o);
            p.x >= r.loc.x as f64 && p.y >= r.loc.y as f64 && p.x < (r.loc.x + r.size.w) as f64 && p.y < (r.loc.y + r.size.h) as f64
        })
    }

    /// `p` moved onto the nearest monitor if it is on none (the pointer cannot leave the screens).
    pub fn clamp_to_outputs(&self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        let mut best: Option<(f64, Point<f64, Logical>)> = None;
        for o in self.all_outputs() {
            let r = self.output_rect(&o);
            let q: Point<f64, Logical> = (
                p.x.clamp(r.loc.x as f64, (r.loc.x + r.size.w - 1) as f64),
                p.y.clamp(r.loc.y as f64, (r.loc.y + r.size.h - 1) as f64),
            )
                .into();
            let d = (q.x - p.x).powi(2) + (q.y - p.y).powi(2);
            if best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
                best = Some((d, q));
            }
        }
        best.map(|(_, q)| q).unwrap_or(p)
    }

    /// The monitor a window is mostly on: the one under its middle, else the nearest to it.
    pub fn output_for_window(&self, window: &smithay::desktop::Window) -> Output {
        let Some(g) = self.space.element_geometry(window) else { return self.output.clone() };
        let middle: Point<f64, Logical> = ((g.loc.x + g.size.w / 2) as f64, (g.loc.y + g.size.h / 2) as f64).into();
        self.output_at(middle).unwrap_or_else(|| {
            let p = self.clamp_to_outputs(middle);
            self.output_at(p).unwrap_or_else(|| self.output.clone())
        })
    }

    /// The monitor with this layer-shell / protocol output resource, else the first one.
    pub fn output_for_resource(&self, wl: Option<&smithay::reexports::wayland_server::protocol::wl_output::WlOutput>) -> Output {
        wl.and_then(Output::from_resource).filter(|o| self.all_outputs().contains(o)).unwrap_or_else(|| self.output.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::wayland_server::Display;

    fn state() -> (Display<Fawc>, Fawc) {
        let display: Display<Fawc> = Display::new().unwrap();
        let s = Fawc::new(&display.handle(), (800, 600).into());
        (display, s)
    }

    #[test]
    fn outputs_sit_side_by_side_and_the_pointer_stays_on_them() {
        let (_d, mut s) = state();
        let second = s.add_output("fawc-1", (640, 480), 60_000);
        assert_eq!(s.output_rect(&s.output.clone()), Rectangle::new((0, 0).into(), (800, 600).into()));
        assert_eq!(s.output_rect(&second), Rectangle::new((800, 0).into(), (640, 480).into()));
        assert_eq!(s.output_at((100.0, 100.0).into()).unwrap(), s.output);
        assert_eq!(s.output_at((900.0, 100.0).into()).unwrap(), second);
        assert!(s.output_at((900.0, 550.0).into()).is_none(), "below the shorter monitor there is nothing");
        // clamped onto the nearest monitor
        assert_eq!(s.clamp_to_outputs((2000.0, 100.0).into()), (1439.0, 100.0).into());
        assert_eq!(s.clamp_to_outputs((1000.0, 590.0).into()), (1000.0, 479.0).into());
        assert_eq!(s.clamp_to_outputs((900.0, 590.0).into()), (799.0, 590.0).into(), "equally near: the closer monitor wins");
        assert_eq!(s.clamp_to_outputs((-5.0, -5.0).into()), (0.0, 0.0).into());
    }

    #[test]
    fn scale_makes_the_logical_size_smaller() {
        let (_d, mut s) = state();
        let o = s.add_output("fawc-1", (3840, 2160), 60_000);
        o.change_current_state(None, None, Some(Scale::Fractional(2.0)), None);
        assert_eq!(s.output_logical_size(&o), (1920, 1080).into());
        o.change_current_state(None, None, Some(Scale::Fractional(1.5)), None);
        assert_eq!(s.output_logical_size(&o), (2560, 1440).into());
    }
}
