//! Window management: placing, focusing, maximizing, minimizing, closing, hit testing.

use std::sync::Mutex;

use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use crate::layout::{client_rect, hit, outer_rect, Hit};
use crate::state::{Fawc, WinInfo};

/// The opening animation: a short fade and zoom.
pub const OPEN_SECONDS: f32 = 0.18;

/// Ease-out: fast at first, settling at the end.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn info(window: &Window) -> &Mutex<WinInfo> {
    window.user_data().get_or_insert_threadsafe(|| Mutex::new(WinInfo::default()))
}

impl Fawc {
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .chain(self.minimized.iter())
            .find(|w| w.toplevel().map(|t| t.wl_surface() == surface).unwrap_or(false))
            .cloned()
    }

    /// Open a new window: cascaded position, focused, on top.
    pub fn map_new_window(&mut self, window: Window) {
        let b = self.style.border as i32;
        let t = self.style.titlebar as i32;
        let step = 32;
        let origin = 70 + self.cascade;
        self.cascade = (self.cascade + step) % (6 * step);
        let loc = (origin + b, origin + b + t);
        info(&window).lock().unwrap().opened = Some(std::time::Instant::now());
        self.space.map_element(window.clone(), loc, true);
        self.focus_window(&window);
    }

    /// Start the opening animation again (a window shown again; also lets tests look at its start).
    pub fn restart_open_animation(&mut self, window: &Window) {
        self.set_open_start(window, std::time::Instant::now());
    }

    /// Pretend the window appeared at `at` (a time in the future holds the animation at its start).
    pub fn set_open_start(&mut self, window: &Window, at: std::time::Instant) {
        info(window).lock().unwrap().opened = Some(at);
    }

    /// How far the opening animation of `window` is, 0 (just appeared) .. 1 (done or off).
    pub fn open_progress(&self, window: &Window) -> f32 {
        if !self.config.appearance.animations {
            return 1.0;
        }
        match info(window).lock().unwrap().opened {
            Some(t) => (t.elapsed().as_secs_f32() / OPEN_SECONDS).clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    /// Give the keyboard to `window` and bring it to the front.
    pub fn focus_window(&mut self, window: &Window) {
        if self.locked {
            return;
        }
        self.space.raise_element(window, true);
        if let Some(top) = window.toplevel() {
            let serial = SERIAL_COUNTER.next_serial();
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, Some(top.wl_surface().clone()), serial);
            }
        }
    }

    pub fn focus_top_window(&mut self) {
        let top = self.space.elements().last().cloned();
        match top {
            Some(w) => self.focus_window(&w),
            None => {
                if let Some(kb) = self.seat.get_keyboard() {
                    kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
                }
            }
        }
    }

    /// The window under `p`, with its client rectangle and where `p` is on it. Front first.
    pub fn window_under(&self, p: Point<f64, Logical>) -> Option<(Window, Rectangle<i32, Logical>, Hit)> {
        for w in self.space.elements().rev() {
            let Some(g) = self.space.element_geometry(w) else { continue };
            if let Some(h) = hit(g, &self.style, p) {
                return Some((w.clone(), g, h));
            }
        }
        None
    }

    pub fn is_maximized(&self, window: &Window) -> bool {
        info(window).lock().unwrap().restore.is_some()
    }

    pub fn set_maximized(&mut self, window: &Window, on: bool) {
        let Some(top) = window.toplevel().cloned() else { return };
        let Some(geo) = self.space.element_geometry(window) else { return };
        if on == self.is_maximized(window) {
            return;
        }
        if on {
            let screen = self.usable_area();
            let target = client_rect(screen, &self.style);
            info(window).lock().unwrap().restore = Some(geo);
            top.with_pending_state(|s| {
                s.states.set(xdg_toplevel::State::Maximized);
                s.size = Some(target.size);
            });
            self.space.map_element(window.clone(), target.loc, false);
        } else {
            let restore = info(window).lock().unwrap().restore.take();
            top.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Maximized);
                s.size = restore.map(|r| r.size);
            });
            if let Some(r) = restore {
                self.space.map_element(window.clone(), r.loc, false);
            }
        }
        top.send_configure();
    }

    pub fn toggle_maximized(&mut self, window: &Window) {
        let on = !self.is_maximized(window);
        self.set_maximized(window, on);
    }

    pub fn minimize_window(&mut self, window: &Window) {
        info(window).lock().unwrap().minimized = true;
        self.space.unmap_elem(window);
        if !self.minimized.contains(window) {
            self.minimized.push(window.clone());
        }
        self.focus_top_window();
    }

    /// Bring back the most recently minimized window.
    pub fn restore_window(&mut self) {
        if let Some(w) = self.minimized.pop() {
            info(&w).lock().unwrap().minimized = false;
            let loc = self.space.element_location(&w).unwrap_or_else(|| {
                (70 + self.style.border as i32, 70 + (self.style.border + self.style.titlebar) as i32).into()
            });
            self.space.map_element(w.clone(), loc, true);
            self.focus_window(&w);
        }
    }

    /// Bring back one particular minimized window.
    pub fn restore_specific(&mut self, window: &Window) {
        if let Some(i) = self.minimized.iter().position(|w| w == window) {
            let w = self.minimized.remove(i);
            self.minimized.push(w);
            self.restore_window();
        }
    }

    pub fn close_window(&mut self, window: &Window) {
        if let Some(top) = window.toplevel() {
            top.send_close();
        }
    }

    /// Alt+Tab: the window at the back comes to the front.
    pub fn cycle_windows(&mut self) {
        let back = self.space.elements().next().cloned();
        if let Some(w) = back {
            self.focus_window(&w);
        }
    }

    pub fn outer_of(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        self.space.element_geometry(window).map(|g| outer_rect(g, &self.style))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_easing_starts_fast_ends_at_one_and_stays_in_range() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.8, "most of the movement is early");
        assert_eq!(ease_out(2.0), 1.0);
        assert_eq!(ease_out(-1.0), 0.0);
    }
}
