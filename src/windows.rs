//! Window management: placing, focusing, maximizing, minimizing, closing, hit testing.

use std::sync::Mutex;

use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use crate::layout::{client_rect, hit, outer_rect, Hit};
use crate::state::{Fawc, WinInfo};

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
        self.space.map_element(window.clone(), loc, true);
        self.focus_window(&window);
    }

    /// Give the keyboard to `window` and bring it to the front.
    pub fn focus_window(&mut self, window: &Window) {
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
            let screen = Rectangle::<i32, Logical>::from_size(self.output_size());
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
