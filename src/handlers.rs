//! Wayland protocol handlers.

use std::os::unix::io::OwnedFd;

use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::desktop::{layer_map_for_output, LayerSurface as DesktopLayer, PopupKind, PopupManager, Window, WindowSurfaceType};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecoMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::Client;
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::{Layer, LayerSurfaceData, LayerSurface, WlrLayerShellHandler, WlrLayerShellState};
use smithay::wayland::shell::xdg::decoration::{XdgDecorationHandler, XdgDecorationState};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_layer_shell, delegate_xdg_decoration, delegate_xdg_shell,
};

use crate::state::{client_state, Fde};

impl BufferHandler for Fde {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Fde {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        client_state(client)
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        crate::background_effect::commit(surface);
        self.popups.commit(surface);
        if let Some(window) = self.window_for_surface(surface) {
            window.on_commit();
            if let Some(top) = window.toplevel() {
                if !top.is_initial_configure_sent() {
                    top.send_configure();
                }
            }
        }
        // a layer surface (panel, wallpaper, launcher): lay the layers out again
        let found = self.all_outputs().into_iter().find_map(|o| {
            let l = layer_map_for_output(&o).layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned();
            l.map(|l| (o, l))
        });
        if let Some((layer_output, layer)) = found {
            layer_map_for_output(&layer_output).arrange();
            self.focus_layer_if_exclusive(&layer);
            // the first configure answers the first commit; arrange never sends it
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .map(|d| d.lock().unwrap().initial_configure_sent)
                    .unwrap_or(false)
            });
            if !sent {
                layer.layer_surface().send_configure();
            }
        }
        // a popup of ours needs its first configure too
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            if !popup.is_initial_configure_sent() {
                let _ = popup.send_configure();
            }
        }
        self.sync_foreign_toplevels();
    }
}

impl ShmHandler for Fde {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for Fde {}

impl SeatHandler for Fde {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // the clipboard and the primary selection go to the program that has the keyboard
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(smithay::reexports::wayland_server::Resource::id(s)).ok());
        smithay::wayland::selection::data_device::set_data_device_focus(dh, seat, client.clone());
        smithay::wayland::selection::primary_selection::set_primary_focus(dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl SelectionHandler for Fde {
    type SelectionUserData = ();

    /// A Wayland program copied something: tell the X server, whose programs paste from it.
    fn new_selection(&mut self, ty: smithay::wayland::selection::SelectionTarget, source: Option<smithay::wayland::selection::SelectionSource>, _: Seat<Self>) {
        if let Some(xwm) = &mut self.xwm {
            if let Err(e) = xwm.new_selection(ty, source.map(|s| s.mime_types())) {
                tracing::warn!("clipboard for X11 programs: {e:?}");
            }
        }
    }

    /// A Wayland program pastes what an X program copied.
    fn send_selection(&mut self, ty: smithay::wayland::selection::SelectionTarget, mime_type: String, fd: OwnedFd, _: Seat<Self>, _: &()) {
        let (Some(xwm), Some(handle)) = (self.xwm.as_mut(), self.loop_handle.clone()) else { return };
        if let Err(e) = xwm.send_selection(ty, mime_type, fd, handle) {
            tracing::warn!("paste from an X11 program: {e:?}");
        }
    }
}

impl DataDeviceHandler for Fde {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for Fde {}
impl ServerDndGrabHandler for Fde {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl XdgShellHandler for Fde {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.map_new_window(window);
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
        });
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.set_maximized(&w, true);
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.set_maximized(&w, false);
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, wl_output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            let out = wl_output.as_ref().map(|o| self.output_for_resource(Some(o)));
            self.set_fullscreen(&w, true, out);
            self.focus_window(&w);
        } else {
            surface.send_configure();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.set_fullscreen(&w, false, None);
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.minimize_window(&w);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let sid = smithay::reexports::wayland_server::Resource::id(surface.wl_surface());
        self.titles.retain(|(id, _), _| *id != sid);
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.space.unmap_elem(&w);
        }
        self.minimized.retain(|w| w.toplevel().map(|t| t != &surface).unwrap_or(true));
        self.parked.retain(|w| w.toplevel().map(|t| t != &surface).unwrap_or(true));
        self.focus_top_window();
        self.sync_foreign_toplevels();
    }
}

impl XdgDecorationHandler for Fde {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // the compositor draws the frame
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecoMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
        }
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecoMode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecoMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecoMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
        }
    }
}

impl WlrLayerShellHandler for Fde {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        // on the monitor the client names, else the first
        let output = self.output_for_resource(output.as_ref());
        tracing::info!("layer surface {namespace} on {layer:?} of {}", output.name());
        let desktop_layer = DesktopLayer::new(surface, namespace);
        let mut map = layer_map_for_output(&output);
        let _ = map.map_layer(&desktop_layer);
        map.arrange();
        drop(map);
        // a surface that asks for the keyboard exclusively (a launcher) gets it at once
        self.focus_layer_if_exclusive(&desktop_layer);
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        for output in self.all_outputs() {
            let mut map = layer_map_for_output(&output);
            let found = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            if let Some(l) = found {
                map.unmap_layer(&l);
                map.arrange();
            }
        }
        self.focus_top_window();
    }
}

delegate_compositor!(Fde);
delegate_layer_shell!(Fde);
smithay::delegate_session_lock!(Fde);

impl smithay::wayland::session_lock::SessionLockHandler for Fde {
    fn lock_state(&mut self) -> &mut smithay::wayland::session_lock::SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: smithay::wayland::session_lock::SessionLocker) {
        // nothing but the lock screen from now on; confirmed once a frame without anything else is drawn
        self.locked = true;
        self.drag = None;
        self.lock_confirmation = Some(confirmation);
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, None, smithay::utils::SERIAL_COUNTER.next_serial());
        }
        tracing::info!("session locked");
    }

    fn unlock(&mut self) {
        self.locked = false;
        self.lock_surfaces.clear();
        self.lock_confirmation = None;
        self.focus_top_window();
        tracing::info!("session unlocked");
    }

    fn new_surface(&mut self, surface: smithay::wayland::session_lock::LockSurface, wl_output: smithay::reexports::wayland_server::protocol::wl_output::WlOutput) {
        // one lock surface per monitor, each as big as its monitor
        let output = self.output_for_resource(Some(&wl_output));
        let size = self.output_logical_size(&output);
        surface.with_pending_state(|s| s.size = Some((size.w as u32, size.h as u32).into()));
        surface.send_configure();
        if self.lock_surfaces.is_empty() {
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, Some(surface.wl_surface().clone()), smithay::utils::SERIAL_COUNTER.next_serial());
            }
        }
        self.lock_surfaces.push((output, surface));
    }
}
crate::delegate_background_effect!(Fde);
delegate_shm!(Fde);
delegate_output!(Fde);
delegate_seat!(Fde);
delegate_data_device!(Fde);
delegate_xdg_shell!(Fde);
delegate_xdg_decoration!(Fde);

#[allow(dead_code)]
fn _unused(_: xdg_toplevel::State, _: &XdgDecorationState, _: &PopupManager) {}
