//! Window management: placing, focusing, maximizing, minimizing, closing, hit testing.

use smithay::wayland::seat::WaylandFocus;
use std::sync::Mutex;

use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

use crate::layout::{client_rect, hit, outer_rect, Hit};
use crate::state::{Fde, WinInfo};

/// The opening animation: a short fade and zoom.
pub const OPEN_SECONDS: f32 = 0.18;
/// Shrinking into the task bar.
pub const MINIMIZE_SECONDS: f32 = 0.22;

/// How a window looks `p` (0 .. 1) of the way into the task bar: how see-through, how big
/// (relative) and where its middle is, on the way from `center` to `target`.
pub fn minimize_pose(p: f32, center: (f64, f64), target: (f64, f64)) -> (f32, f64, (f64, f64)) {
    let e = (p.clamp(0.0, 1.0) as f64).powi(2); // slow start, quick end
    let alpha = (1.0 - e) as f32;
    let zoom = 1.0 - 0.85 * e;
    (alpha, zoom, (center.0 + (target.0 - center.0) * e, center.1 + (target.1 - center.1) * e))
}

/// Ease-out: fast at first, settling at the end.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn info(window: &Window) -> &Mutex<WinInfo> {
    window.user_data().get_or_insert_threadsafe(|| Mutex::new(WinInfo::default()))
}

/// Half of the screen a window can be snapped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Left,
    Right,
}

impl Fde {
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .chain(self.minimized.iter())
            .chain(self.parked.iter())
            .find(|w| w.wl_surface().map(|s| &*s == surface).unwrap_or(false))
            .cloned()
    }

    /// A menu, tooltip or other pop-up of an X11 program: placed by the program, no frame.
    pub fn is_override_redirect(&self, window: &Window) -> bool {
        window.x11_surface().map(|x| x.is_override_redirect()).unwrap_or(false)
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
        self.place_on_current_workspace(&window);
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
        if self.is_override_redirect(window) {
            return;
        }
        self.space.raise_element(window, true);
        // an X11 program knows it is the active one through its own window manager hints
        let others: Vec<Window> = self.space.elements().filter(|w| *w != window).cloned().collect();
        for w in others {
            if let Some(x) = w.x11_surface() {
                let _ = x.set_activated(false);
            }
        }
        if let Some(x) = window.x11_surface() {
            let _ = x.set_activated(true);
        }
        if let Some(surface) = window.wl_surface().map(|s| s.into_owned()) {
            let serial = SERIAL_COUNTER.next_serial();
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), serial);
            }
        }
    }

    pub fn focus_top_window(&mut self) {
        let top = self.space.elements().rev().find(|w| self.minimize_progress(w).is_none()).cloned();
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
            if self.minimize_progress(w).is_some() {
                continue; // on its way into the task bar
            }
            let Some(g) = self.space.element_geometry(w) else { continue };
            if self.is_fullscreen(w) || self.is_override_redirect(w) {
                if g.to_f64().contains(p) {
                    return Some((w.clone(), g, Hit::Client));
                }
                continue;
            }
            if let Some(h) = hit(g, &self.style, p) {
                return Some((w.clone(), g, h));
            }
        }
        None
    }

    pub fn is_fullscreen(&self, window: &Window) -> bool {
        info(window).lock().unwrap().fullscreen.is_some()
    }

    /// Is a window fullscreen (and showing) on this monitor? Then it covers the panels.
    pub fn fullscreen_on(&self, output: &smithay::output::Output) -> bool {
        let rect = self.output_rect(output);
        self.space.elements().any(|w| {
            self.is_fullscreen(w)
                && !info(w).lock().unwrap().minimized
                && self.space.element_geometry(w).map(|g| g.loc == rect.loc).unwrap_or(false)
        })
    }

    /// Fill `output` (or the monitor the window is on) with the window, no frame, over the
    /// panels; or put it back where it was.
    pub fn set_fullscreen(&mut self, window: &Window, on: bool, output: Option<smithay::output::Output>) {
        let top = window.toplevel().cloned();
        let x11 = window.x11_surface().cloned();
        if top.is_none() && x11.is_none() {
            return;
        }
        let Some(geo) = self.space.element_geometry(window) else { return };
        if on == self.is_fullscreen(window) {
            if on {
                if let Some(top) = &top {
                    top.send_configure();
                }
            }
            return;
        }
        if on {
            let out = output.unwrap_or_else(|| self.output_for_window(window));
            let rect = self.output_rect(&out);
            info(window).lock().unwrap().fullscreen = Some(geo);
            if let Some(top) = &top {
                top.with_pending_state(|s| {
                    s.states.set(xdg_toplevel::State::Fullscreen);
                    s.size = Some(rect.size);
                });
            }
            if let Some(x) = &x11 {
                let _ = x.set_fullscreen(true);
                let _ = x.configure(Some(rect));
            }
            self.space.map_element(window.clone(), rect.loc, false);
        } else {
            let prev = info(window).lock().unwrap().fullscreen.take();
            if let Some(top) = &top {
                top.with_pending_state(|s| {
                    s.states.unset(xdg_toplevel::State::Fullscreen);
                    s.size = prev.map(|r| r.size);
                });
            }
            if let Some(x) = &x11 {
                let _ = x.set_fullscreen(false);
                if let Some(r) = prev {
                    let _ = x.configure(Some(r));
                }
            }
            if let Some(r) = prev {
                self.space.map_element(window.clone(), r.loc, false);
            }
        }
        if let Some(top) = &top {
            top.send_configure();
        }
    }

    pub fn is_snapped(&self, window: &Window) -> bool {
        info(window).lock().unwrap().snapped.is_some()
    }

    /// Give the window this client rectangle (its size is asked of the program, its place is ours).
    fn set_client_rect(&mut self, window: &Window, rect: Rectangle<i32, Logical>) {
        if let Some(top) = window.toplevel() {
            top.with_pending_state(|s| s.size = Some(rect.size));
            top.send_pending_configure();
        }
        if let Some(x) = window.x11_surface() {
            let _ = x.configure(Some(rect));
        }
        self.space.map_element(window.clone(), rect.loc, false);
    }

    /// Fill the left or right half of the monitor the window is on.
    pub fn snap_window(&mut self, window: &Window, side: Snap) {
        if self.is_fullscreen(window) || self.minimize_progress(window).is_some() {
            return;
        }
        if self.is_maximized(window) {
            self.set_maximized(window, false);
        }
        let Some(geo) = self.space.element_geometry(window) else { return };
        let area = self.usable_area(&self.output_for_window(window));
        let half = area.size.w / 2;
        let outer = Rectangle::new(
            ((if side == Snap::Left { area.loc.x } else { area.loc.x + area.size.w - half }), area.loc.y).into(),
            (half, area.size.h).into(),
        );
        {
            let mut i = info(window).lock().unwrap();
            if i.snapped.is_none() {
                i.snapped = Some(geo);
            }
        }
        let rect = client_rect(outer, &self.style);
        self.set_client_rect(window, rect);
    }

    /// Put a snapped window back where it was.
    pub fn unsnap_window(&mut self, window: &Window) {
        let prev = info(window).lock().unwrap().snapped.take();
        if let Some(r) = prev {
            self.set_client_rect(window, r);
        }
    }

    /// A window let go at the edge of the screen: left and right snap to a half, the top maximizes.
    pub fn snap_if_at_edge(&mut self, window: &Window) {
        let p = self.pointer_location;
        let Some(out) = self.output_at(p) else { return };
        let r = self.output_rect(&out);
        if p.y <= r.loc.y as f64 + 1.0 {
            self.set_maximized(window, true);
        } else if p.x <= r.loc.x as f64 + 1.0 {
            self.snap_window(window, Snap::Left);
        } else if p.x >= (r.loc.x + r.size.w) as f64 - 2.0 {
            self.snap_window(window, Snap::Right);
        }
    }

    pub fn is_maximized(&self, window: &Window) -> bool {
        info(window).lock().unwrap().restore.is_some()
    }

    pub fn set_maximized(&mut self, window: &Window, on: bool) {
        let top = window.toplevel().cloned();
        let x11 = window.x11_surface().cloned();
        if top.is_none() && x11.is_none() {
            return;
        }
        let Some(geo) = self.space.element_geometry(window) else { return };
        if on == self.is_maximized(window) || self.is_fullscreen(window) {
            return;
        }
        if on {
            let screen = self.usable_area(&self.output_for_window(window));
            let target = client_rect(screen, &self.style);
            info(window).lock().unwrap().restore = Some(geo);
            if let Some(top) = &top {
                top.with_pending_state(|s| {
                    s.states.set(xdg_toplevel::State::Maximized);
                    s.size = Some(target.size);
                });
            }
            if let Some(x) = &x11 {
                let _ = x.set_maximized(true);
                let _ = x.configure(Some(target));
            }
            self.space.map_element(window.clone(), target.loc, false);
        } else {
            let restore = info(window).lock().unwrap().restore.take();
            if let Some(top) = &top {
                top.with_pending_state(|s| {
                    s.states.unset(xdg_toplevel::State::Maximized);
                    s.size = restore.map(|r| r.size);
                });
            }
            if let Some(x) = &x11 {
                let _ = x.set_maximized(false);
                if let Some(r) = restore {
                    let _ = x.configure(Some(r));
                }
            }
            if let Some(r) = restore {
                self.space.map_element(window.clone(), r.loc, false);
            }
        }
        if let Some(top) = &top {
            top.send_configure();
        }
    }

    pub fn toggle_maximized(&mut self, window: &Window) {
        let on = !self.is_maximized(window);
        self.set_maximized(window, on);
    }

    pub fn minimize_window(&mut self, window: &Window) {
        let mapped = self.space.elements().any(|w| w == window);
        {
            let mut i = info(window).lock().unwrap();
            i.minimized = true;
            i.last_loc = self.space.element_location(window).or(i.last_loc);
        }
        if !self.minimized.contains(window) {
            self.minimized.push(window.clone());
        }
        if mapped && self.config.appearance.animations {
            // shrink towards the task bar first; `tick_animations` takes it off the screen
            info(window).lock().unwrap().minimizing = Some(std::time::Instant::now());
        } else {
            self.space.unmap_elem(window);
        }
        self.focus_top_window();
    }

    /// How far the window's shrinking into the task bar is (0 .. 1), or None if it is not.
    pub fn minimize_progress(&self, window: &Window) -> Option<f32> {
        info(window).lock().unwrap().minimizing.map(|t| (t.elapsed().as_secs_f32() / MINIMIZE_SECONDS).clamp(0.0, 1.0))
    }

    /// Pretend the shrinking started at `at` (tests: a time in the future holds it at the start).
    pub fn set_minimize_start(&mut self, window: &Window, at: std::time::Instant) {
        info(window).lock().unwrap().minimizing = Some(at);
    }

    /// Where the window's task bar button is, in the shared space (the task bar tells us).
    pub fn set_minimize_target(&mut self, window: &Window, target: Rectangle<i32, Logical>) {
        info(window).lock().unwrap().minimize_target = Some(target);
    }

    pub fn minimize_target(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        info(window).lock().unwrap().minimize_target
    }

    /// Finish what has run its course: windows that have shrunk into the task bar leave the screen.
    /// The window's frame and shadow room as the spring grid sees it (logical, shared space).
    fn wobble_rect(&self, window: &Window) -> Option<Rectangle<f64, Logical>> {
        let g = self.space.element_geometry(window)?;
        let o = outer_rect(g, &self.style);
        let m = 24.0;
        Some(Rectangle::new(
            (o.loc.x as f64 - m, o.loc.y as f64 - m).into(),
            (o.size.w as f64 + 2.0 * m, o.size.h as f64 + 2.0 * m).into(),
        ))
    }

    /// The pointer took hold of the window: it now hangs on springs.
    pub fn start_wobble(&mut self, window: &Window, at: Point<f64, Logical>) {
        if !self.config.appearance.wobbly_windows {
            return;
        }
        let Some(rest) = self.wobble_rect(window) else { return };
        self.wobbles.retain(|(w, _)| w != window);
        let mut b = crate::wobble::Wobble::new(rest);
        b.grab = Some(b.nearest((at.x, at.y)));
        self.wobbles.push((window.clone(), b));
    }

    /// The pointer let go: the window swings free and settles.
    pub fn release_wobbles(&mut self) {
        for (_, b) in &mut self.wobbles {
            b.grab = None;
        }
    }

    fn step_wobbles(&mut self) {
        let dt = self.wobble_time.elapsed().as_secs_f64();
        self.wobble_time = std::time::Instant::now();
        let mut wobbles = std::mem::take(&mut self.wobbles);
        wobbles.retain_mut(|(w, b)| match self.wobble_rect(w) {
            Some(rest) if self.config.appearance.wobbly_windows && !info(w).lock().unwrap().minimized => {
                b.step(dt, rest);
                !b.settled()
            }
            _ => false,
        });
        self.wobbles = wobbles;
    }

    pub fn tick_animations(&mut self) {
        self.step_wobbles();
        self.update_fractional_scales();
        let done: Vec<Window> = self
            .space
            .elements()
            .filter(|w| self.minimize_progress(w).map(|p| p >= 1.0).unwrap_or(false))
            .cloned()
            .collect();
        for w in done {
            info(&w).lock().unwrap().minimizing = None;
            self.space.unmap_elem(&w);
        }
    }

    /// Bring back the most recently minimized window.
    pub fn restore_window(&mut self) {
        // the latest minimized window of the workspace being shown
        let idx = self.minimized.iter().rposition(|w| self.on_current_workspace(w));
        if let Some(w) = idx.map(|i| self.minimized.remove(i)) {
            let (last, was_shrinking) = {
                let mut i = info(&w).lock().unwrap();
                i.minimized = false;
                (i.last_loc, i.minimizing.take().is_some())
            };
            let loc = self.space.element_location(&w).or(last).unwrap_or_else(|| {
                (70 + self.style.border as i32, 70 + (self.style.border + self.style.titlebar) as i32).into()
            });
            self.space.map_element(w.clone(), loc, true);
            if !was_shrinking {
                // coming back: the same fade and zoom as a new window
                self.restart_open_animation(&w);
            }
            self.focus_window(&w);
        }
    }

    /// Bring back one particular minimized window.
    pub fn restore_specific(&mut self, window: &Window) {
        // a window of another workspace: go there first
        if !self.on_current_workspace(window) {
            let target = self.window_workspace_of(window);
            self.switch_workspace(target);
        }
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
        if let Some(x) = window.x11_surface() {
            let _ = x.close();
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

    #[test]
    fn a_minimizing_window_fades_shrinks_and_moves_to_the_button() {
        let (a0, z0, c0) = minimize_pose(0.0, (100.0, 100.0), (500.0, 700.0));
        assert_eq!((a0, z0, c0), (1.0, 1.0, (100.0, 100.0)), "at the start nothing has changed");
        let (a1, z1, c1) = minimize_pose(1.0, (100.0, 100.0), (500.0, 700.0));
        assert_eq!((a1, c1), (0.0, (500.0, 700.0)));
        assert!(z1 < 0.2, "tiny at the end");
        let (am, zm, cm) = minimize_pose(0.5, (100.0, 100.0), (500.0, 700.0));
        assert!(am > 0.0 && am < 1.0 && zm < 1.0 && zm > z1 && cm.0 > 100.0 && cm.0 < 500.0);
    }
}
