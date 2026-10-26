//! XWayland: old X11 programs (Steam, many games, Wine) run inside FDE. FDE starts the X server
//! (`Xwayland`) and is its window manager: managed X windows get the same glass frame as
//! Wayland ones, pop-ups and menus (override-redirect windows) are drawn bare at the place the
//! program asked for.

use smithay::wayland::seat::WaylandFocus;
use std::process::Stdio;

use smithay::desktop::Window;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::selection::data_device::{
    clear_data_device_selection, current_data_device_selection_userdata, request_data_device_client_selection, set_data_device_selection,
};
use smithay::wayland::selection::primary_selection::{
    clear_primary_selection, current_primary_selection_userdata, request_primary_client_selection, set_primary_selection,
};
use smithay::wayland::selection::SelectionTarget;
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, XwmId};
use smithay::xwayland::{X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler};

use crate::state::Fde;

/// Start the X server; the window manager part starts when it reports ready.
pub fn start(handle: LoopHandle<'static, Fde>, dh: &DisplayHandle) {
    if std::env::var_os("FDE_NO_XWAYLAND").is_some() {
        return;
    }
    let spawned = XWayland::spawn(dh, None, std::iter::empty::<(String, String)>(), true, Stdio::null(), Stdio::null(), |_| {});
    let (xwayland, client) = match spawned {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!("no XWayland ({e}): X11 programs will not run");
            return;
        }
    };
    let h2 = handle.clone();
    let _ = handle.insert_source(xwayland, move |event, _, state| match event {
        XWaylandEvent::Ready { x11_socket, display_number } => match X11Wm::start_wm(h2.clone(), x11_socket, client.clone()) {
            Ok(wm) => {
                tracing::info!("XWayland ready on DISPLAY=:{display_number}");
                state.xwm = Some(wm);
                state.loop_handle = Some(h2.clone());
                state.x11_display = Some(display_number);
                std::env::set_var("DISPLAY", format!(":{display_number}"));
            }
            Err(e) => tracing::warn!("XWayland window manager: {e}"),
        },
        XWaylandEvent::Error => tracing::warn!("XWayland failed to start"),
    });
}

impl XWaylandShellHandler for Fde {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(&mut self, _: XwmId, _: WlSurface, surface: X11Surface) {
        // the window may have been mapped before its picture arrived: give it the keyboard now
        if let Some(w) = self.x11_window(&surface) {
            if self.space.elements().last() == Some(&w) {
                self.focus_window(&w);
            }
        }
    }
}
smithay::delegate_xwayland_shell!(Fde);

impl Fde {
    /// The window of this X11 surface.
    pub fn x11_window(&self, surface: &X11Surface) -> Option<Window> {
        self.space
            .elements()
            .chain(self.minimized.iter())
            .find(|w| w.x11_surface().map(|x| x == surface).unwrap_or(false))
            .cloned()
    }

    /// Tell an X11 window where it is (its client rectangle in the shared space); menus and
    /// dialogs position themselves from this.
    pub fn sync_x11_position(&self, window: &Window) {
        if let (Some(x), Some(g)) = (window.x11_surface(), self.space.element_geometry(window)) {
            let _ = x.configure(Some(g));
        }
    }

    fn forget_x11_window(&mut self, surface: &X11Surface) {
        if let Some(w) = self.x11_window(surface) {
            self.space.unmap_elem(&w);
            self.minimized.retain(|m| m != &w);
            self.parked.retain(|m| m != &w);
            self.wobbles.retain(|(m, _)| m != &w);
            self.focus_top_window();
            self.sync_foreign_toplevels();
        }
    }
}

impl XwmHandler for Fde {
    fn xwm_state(&mut self, _: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("the window manager runs")
    }

    fn new_window(&mut self, _: XwmId, _: X11Surface) {}
    fn new_override_redirect_window(&mut self, _: XwmId, _: X11Surface) {}

    fn map_window_request(&mut self, _: XwmId, surface: X11Surface) {
        let _ = surface.set_mapped(true);
        let window = Window::new_x11_window(surface.clone());
        self.map_new_window(window.clone());
        let size = surface.geometry().size;
        if let Some(loc) = self.space.element_location(&window) {
            let _ = surface.configure(Some(Rectangle::new(loc, (size.w.max(64), size.h.max(48)).into())));
        }
        self.sync_foreign_toplevels();
    }

    fn mapped_override_redirect_window(&mut self, _: XwmId, surface: X11Surface) {
        let loc = surface.geometry().loc;
        self.space.map_element(Window::new_x11_window(surface), loc, false);
    }

    fn unmapped_window(&mut self, _: XwmId, surface: X11Surface) {
        self.forget_x11_window(&surface);
    }

    fn destroyed_window(&mut self, _: XwmId, surface: X11Surface) {
        self.forget_x11_window(&surface);
    }

    fn configure_request(&mut self, _: XwmId, surface: X11Surface, x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>, _: Option<Reorder>) {
        let mut geo = surface.geometry();
        if let Some(window) = self.x11_window(&surface) {
            // the compositor keeps the place of a window it manages; the size is the program's
            if let Some(g) = self.space.element_geometry(&window) {
                geo.loc = g.loc;
            }
        } else {
            if let Some(x) = x {
                geo.loc.x = x;
            }
            if let Some(y) = y {
                geo.loc.y = y;
            }
        }
        if let Some(w) = w {
            geo.size.w = w as i32;
        }
        if let Some(h) = h {
            geo.size.h = h as i32;
        }
        let _ = surface.configure(Some(geo));
    }

    fn configure_notify(&mut self, _: XwmId, surface: X11Surface, geometry: Rectangle<i32, Logical>, _: Option<u32>) {
        // pop-ups move themselves
        if surface.is_override_redirect() {
            if let Some(w) = self.x11_window(&surface) {
                self.space.map_element(w, geometry.loc, false);
            }
        }
    }

    fn maximize_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.set_maximized(&w, true);
        }
    }

    fn unmaximize_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.set_maximized(&w, false);
        }
    }

    fn fullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.set_fullscreen(&w, true, None);
            self.focus_window(&w);
        }
    }

    fn unfullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.set_fullscreen(&w, false, None);
        }
    }

    fn minimize_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.minimize_window(&w);
        }
    }

    fn unminimize_request(&mut self, _: XwmId, surface: X11Surface) {
        if let Some(w) = self.x11_window(&surface) {
            self.restore_specific(&w);
        }
    }

    // clipboard and primary selection between X11 and Wayland programs
    fn allow_selection_access(&mut self, _: XwmId, _: SelectionTarget) -> bool {
        true
    }

    /// An X program wants what a Wayland program has copied.
    fn send_selection(&mut self, _: XwmId, selection: SelectionTarget, mime_type: String, fd: std::os::fd::OwnedFd) {
        let seat = self.seat.clone();
        let sent = match selection {
            SelectionTarget::Clipboard => request_data_device_client_selection(&seat, mime_type, fd).map_err(|e| format!("{e:?}")),
            SelectionTarget::Primary => request_primary_client_selection(&seat, mime_type, fd).map_err(|e| format!("{e:?}")),
        };
        if let Err(e) = sent {
            tracing::warn!("X11 paste: {e}");
        }
    }

    /// An X program copied something: Wayland programs see it as a selection of the compositor.
    fn new_selection(&mut self, _: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        let (dh, seat) = (self.display_handle.clone(), self.seat.clone());
        match selection {
            SelectionTarget::Clipboard => set_data_device_selection(&dh, &seat, mime_types, ()),
            SelectionTarget::Primary => set_primary_selection(&dh, &seat, mime_types, ()),
        }
    }

    fn cleared_selection(&mut self, _: XwmId, selection: SelectionTarget) {
        let (dh, seat) = (self.display_handle.clone(), self.seat.clone());
        match selection {
            SelectionTarget::Clipboard => {
                if current_data_device_selection_userdata(&seat).is_some() {
                    clear_data_device_selection(&dh, &seat);
                }
            }
            SelectionTarget::Primary => {
                if current_primary_selection_userdata(&seat).is_some() {
                    clear_primary_selection(&dh, &seat);
                }
            }
        }
    }

    fn resize_request(&mut self, _: XwmId, _: X11Surface, _: u32, _: ResizeEdge) {}
    fn move_request(&mut self, _: XwmId, _: X11Surface, _: u32) {}
}
