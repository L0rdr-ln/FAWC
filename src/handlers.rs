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

use crate::state::{client_state, Fawc};

impl BufferHandler for Fawc {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Fawc {
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
        let layer = layer_map_for_output(&self.output)
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .cloned();
        if let Some(layer) = layer {
            layer_map_for_output(&self.output).arrange();
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
    }
}

impl ShmHandler for Fawc {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for Fawc {}

impl SeatHandler for Fawc {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl SelectionHandler for Fawc {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Fawc {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for Fawc {}
impl ServerDndGrabHandler for Fawc {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl XdgShellHandler for Fawc {
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

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.minimize_window(&w);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.titles.remove(&smithay::reexports::wayland_server::Resource::id(surface.wl_surface()));
        if let Some(w) = self.window_for_surface(surface.wl_surface()) {
            self.space.unmap_elem(&w);
        }
        self.minimized.retain(|w| w.toplevel().map(|t| t != &surface).unwrap_or(true));
        self.focus_top_window();
    }
}

impl XdgDecorationHandler for Fawc {
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

impl WlrLayerShellHandler for Fawc {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        tracing::info!("layer surface {namespace} on {layer:?}");
        let desktop_layer = DesktopLayer::new(surface, namespace);
        let mut map = layer_map_for_output(&self.output);
        let _ = map.map_layer(&desktop_layer);
        map.arrange();
        drop(map);
        // a surface that asks for the keyboard exclusively (a launcher) gets it at once
        self.focus_layer_if_exclusive(&desktop_layer);
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        let mut map = layer_map_for_output(&self.output);
        let found = map.layers().find(|l| l.layer_surface() == &surface).cloned();
        if let Some(l) = found {
            map.unmap_layer(&l);
        }
        map.arrange();
        drop(map);
        self.focus_top_window();
    }
}

delegate_compositor!(Fawc);
delegate_layer_shell!(Fawc);
crate::delegate_background_effect!(Fawc);
delegate_shm!(Fawc);
delegate_output!(Fawc);
delegate_seat!(Fawc);
delegate_data_device!(Fawc);
delegate_xdg_shell!(Fawc);
delegate_xdg_decoration!(Fawc);

#[allow(dead_code)]
fn _unused(_: xdg_toplevel::State, _: &XdgDecorationState, _: &PopupManager) {}
