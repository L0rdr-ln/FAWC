//! The window switcher: hold Alt and press Tab to walk through the windows (the most recently used
//! first), Shift+Tab goes back, letting go of Alt picks the window, Esc cancels.

use smithay::desktop::Window;
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

use crate::state::Fde;

/// The windows being chosen from, most recently used first, and the one that is selected.
pub struct Switcher {
    pub windows: Vec<Window>,
    pub index: usize,
}

impl Fde {
    /// The title of a window, whatever kind it is.
    pub fn window_title(&self, window: &Window) -> String {
        if let Some(x) = window.x11_surface() {
            return x.title();
        }
        let Some(surface) = window.wl_surface().map(|s| s.into_owned()) else { return String::new() };
        smithay::wayland::compositor::with_states(&surface, |states| {
            states.data_map.get::<XdgToplevelSurfaceData>().and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
        })
        .unwrap_or_default()
    }

    /// Alt+Tab (`delta` 1) or Alt+Shift+Tab (-1): start the switcher or move its selection.
    pub fn switcher_step(&mut self, delta: i32) {
        if self.switcher.is_none() {
            // front to back, then the minimized ones of this workspace
            let mut windows: Vec<Window> = self
                .space
                .elements()
                .rev()
                .filter(|w| self.minimize_progress(w).is_none() && !self.is_override_redirect(w))
                .cloned()
                .collect();
            windows.extend(self.minimized.iter().rev().filter(|w| self.on_current_workspace(w)).cloned());
            if windows.len() < 2 {
                return;
            }
            self.switcher = Some(Switcher { windows, index: 0 });
        }
        if let Some(sw) = &mut self.switcher {
            let n = sw.windows.len() as i32;
            sw.index = (sw.index as i32 + delta).rem_euclid(n) as usize;
        }
    }

    /// Alt was let go: the selected window comes to the front (and back from the task bar).
    pub fn switcher_commit(&mut self) {
        let Some(sw) = self.switcher.take() else { return };
        if let Some(w) = sw.windows.get(sw.index).cloned() {
            if self.minimized.contains(&w) {
                self.restore_specific(&w);
            }
            self.focus_window(&w);
        }
    }

    pub fn switcher_cancel(&mut self) {
        self.switcher = None;
    }
}
