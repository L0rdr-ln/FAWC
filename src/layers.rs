//! Layer-shell surfaces (wallpaper, panel, launcher, notifications): keyboard focus and what
//! is under the pointer.

use smithay::desktop::{layer_map_for_output, LayerSurface, WindowSurfaceType};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer};

use crate::state::Fawc;

impl Fawc {
    /// A layer surface that wants the keyboard exclusively takes it. Called on every commit: the
    /// client sets its interactivity after creating the surface, so it is known only from the
    /// first commit on.
    pub fn focus_layer_if_exclusive(&mut self, layer: &LayerSurface) {
        let wants = layer.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive
            && matches!(layer.layer(), Layer::Top | Layer::Overlay);
        let has_it = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .map(|f| &f == layer.wl_surface())
            .unwrap_or(false);
        if wants && !has_it && !self.locked {
            self.focus_layer(layer);
        }
    }

    pub fn focus_layer(&mut self, layer: &LayerSurface) {
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, Some(layer.wl_surface().clone()), serial);
        }
    }

    /// The layer surface (and the point on its origin, in the shared space) under `p` on the
    /// given layers of the monitor `p` is on.
    pub fn layer_surface_under(
        &self,
        p: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(LayerSurface, WlSurface, Point<i32, Logical>)> {
        let output = self.output_at(p)?;
        let origin = self.output_rect(&output).loc;
        let covered = self.fullscreen_on(&output);
        let map = layer_map_for_output(&output);
        let p = p - origin.to_f64();
        for l in layers {
            if covered && *l == Layer::Top {
                continue; // a fullscreen window hides the panels
            }
            if let Some(ls) = map.layer_under(*l, p) {
                let geo = map.layer_geometry(ls)?;
                let local = p - geo.loc.to_f64();
                if let Some((s, loc)) = ls.surface_under(local, WindowSurfaceType::ALL) {
                    return Some((ls.clone(), s, origin + geo.loc + loc));
                }
            }
        }
        None
    }

    /// The area windows may use on `output`: the monitor minus what panels reserve (in the
    /// shared space).
    pub fn usable_area(&self, output: &smithay::output::Output) -> smithay::utils::Rectangle<i32, Logical> {
        let rect = self.output_rect(output);
        let zone = layer_map_for_output(output).non_exclusive_zone();
        if zone.size.w > 0 && zone.size.h > 0 {
            smithay::utils::Rectangle::new(rect.loc + zone.loc, zone.size)
        } else {
            rect
        }
    }
}
