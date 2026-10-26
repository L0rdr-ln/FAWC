//! Small Wayland protocols that mostly need a global and a little policy.

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::tablet_manager::TabletSeatHandler;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData};

use crate::state::Fde;

// cursor-shape-v1: a program names a cursor ("text", "pointer") and the compositor draws it
// from the theme. The seat handler already turns that into `cursor_status`.
impl TabletSeatHandler for Fde {}
smithay::delegate_cursor_shape!(Fde);

// xdg-activation-v1: a program (or a launcher) asks for another window to come to the front.
impl XdgActivationHandler for Fde {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(&mut self, _token: XdgActivationToken, _data: XdgActivationTokenData, surface: WlSurface) {
        if self.locked {
            return;
        }
        if let Some(w) = self.window_for_surface(&surface) {
            self.restore_specific(&w);
            self.focus_window(&w);
        }
    }
}
smithay::delegate_xdg_activation!(Fde);

// linux-dmabuf-v1: GPU clients (games, browsers, video players) hand over buffers that live in
// video memory, without a copy through shared memory.
impl smithay::wayland::dmabuf::DmabufHandler for Fde {
    fn dmabuf_state(&mut self) -> &mut smithay::wayland::dmabuf::DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &smithay::wayland::dmabuf::DmabufGlobal,
        _dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: smithay::wayland::dmabuf::ImportNotifier,
    ) {
        // the format and modifier were checked against what the renderer can import; the real
        // import happens when the surface is first drawn
        let _ = notifier.successful::<Fde>();
    }
}
smithay::delegate_dmabuf!(Fde);

impl Fde {
    /// Offer GPU buffers to clients: `formats` is what the renderer can import.
    pub fn enable_dmabuf(&mut self, formats: impl IntoIterator<Item = smithay::backend::allocator::Format>) {
        if self.dmabuf_global.is_none() {
            let dh = self.display_handle.clone();
            self.dmabuf_global = Some(self.dmabuf_state.create_global::<Fde>(&dh, formats));
        }
    }
}
