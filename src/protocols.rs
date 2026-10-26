//! Small Wayland protocols that mostly need a global and a little policy.

use smithay::wayland::seat::WaylandFocus;
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

// idle-inhibit-v1: a video or game keeps the screen from locking while its window is showing.
impl smithay::wayland::idle_inhibit::IdleInhibitHandler for Fde {
    fn inhibit(&mut self, surface: WlSurface) {
        if !self.idle_inhibitors.contains(&surface) {
            self.idle_inhibitors.push(surface);
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| s != &surface);
    }
}
smithay::delegate_idle_inhibit!(Fde);

impl Fde {
    /// Does a program that is showing ask for the screen to stay awake?
    pub fn idle_inhibited(&self) -> bool {
        use smithay::reexports::wayland_server::Resource;
        self.idle_inhibitors.iter().any(|s| {
            s.is_alive()
                && match self.window_for_surface(s) {
                    Some(w) => !self.minimized.contains(&w),
                    None => true, // a panel or other surface
                }
        })
    }
}

// viewporter: a program may crop and stretch its picture (video players, fractional scaling);
// the renderer already honours it. fractional-scale-v1: tell programs the exact scale of the
// monitor they are on, so they draw in whole device pixels at e.g. 1.5.
smithay::delegate_viewporter!(Fde);

impl smithay::wayland::fractional_scale::FractionalScaleHandler for Fde {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self.output_scale(&self.output.clone());
        set_preferred(&surface, scale);
    }
}
smithay::delegate_fractional_scale!(Fde);

fn set_preferred(surface: &WlSurface, scale: f64) {
    use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
    with_surface_tree_downward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            smithay::wayland::fractional_scale::with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
        },
        |_, _, _| true,
    );
}

impl Fde {
    /// Tell every window, popup and panel the exact scale of the monitor it is on.
    pub fn update_fractional_scales(&mut self) {
        let windows: Vec<_> = self.space.elements().cloned().collect();
        for w in windows {
            let scale = self.output_scale(&self.output_for_window(&w));
            if let Some(surface) = w.wl_surface().map(|s| s.into_owned()) {
                set_preferred(&surface, scale);
                for (popup, _) in smithay::desktop::PopupManager::popups_for_surface(&surface) {
                    set_preferred(popup.wl_surface(), scale);
                }
            }
        }
        for output in self.all_outputs() {
            let scale = self.output_scale(&output);
            for layer in smithay::desktop::layer_map_for_output(&output).layers() {
                set_preferred(layer.wl_surface(), scale);
            }
        }
        for (output, lock) in &self.lock_surfaces {
            set_preferred(lock.wl_surface(), self.output_scale(output));
        }
    }
}
