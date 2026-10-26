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
