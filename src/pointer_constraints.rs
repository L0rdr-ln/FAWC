//! Mouse grab for games and 3D programs: pointer locking, confining, and relative motion.

use smithay::input::pointer::RelativeMotionEvent;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point};
use smithay::wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint, PointerConstraintsHandler};

use crate::state::Fde;

impl PointerConstraintsHandler for Fde {
    fn new_constraint(&mut self, surface: &WlSurface, _pointer: &smithay::input::pointer::PointerHandle<Self>) {
        // a program that already has the pointer gets its grab at once
        if self.seat.get_pointer().and_then(|p| p.current_focus()).as_ref() == Some(surface) {
            self.refresh_constraint();
        }
    }

    fn cursor_position_hint(&mut self, _: &WlSurface, _: &smithay::input::pointer::PointerHandle<Self>, _: Point<f64, Logical>) {}
}

smithay::delegate_pointer_constraints!(Fde);
smithay::delegate_relative_pointer!(Fde);

impl Fde {
    /// Tell the program under the pointer how far the mouse moved (games read this).
    pub fn send_relative_motion(&mut self, delta: Point<f64, Logical>, time_msec: u32) {
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }
        let Some(ptr) = self.seat.get_pointer() else { return };
        let Some(s) = ptr.current_focus() else { return };
        let event = RelativeMotionEvent { delta, delta_unaccel: delta, utime: time_msec as u64 * 1000 };
        ptr.relative_motion(self, Some((s, self.pointer_focus_origin)), &event);
    }

    /// May the pointer move to `pos`? Not while the program under it holds it locked, and
    /// not out of the region it is confined to.
    pub fn motion_allowed(&self, pos: Point<f64, Logical>) -> bool {
        let Some(ptr) = self.seat.get_pointer() else { return true };
        let Some(s) = ptr.current_focus() else { return true };
        let local = pos - self.pointer_focus_origin;
        with_pointer_constraint(&s, &ptr, |c| match c {
            Some(c) if c.is_active() => match &*c {
                PointerConstraint::Locked(_) => false,
                PointerConstraint::Confined(_) => {
                    c.region().map(|r| r.contains((local.x.floor() as i32, local.y.floor() as i32))).unwrap_or(true)
                }
            },
            _ => true,
        })
    }

    /// Switch on the grab of the surface that has the pointer when the pointer is inside the
    /// region it asked for.
    pub fn refresh_constraint(&mut self) {
        let Some(ptr) = self.seat.get_pointer() else { return };
        let Some(s) = ptr.current_focus() else { return };
        let local = self.pointer_location - self.pointer_focus_origin;
        with_pointer_constraint(&s, &ptr, |c| {
            if let Some(c) = c {
                let inside = c.region().map(|r| r.contains((local.x.floor() as i32, local.y.floor() as i32))).unwrap_or(true);
                if !c.is_active() && inside {
                    c.activate();
                }
            }
        });
    }
}
