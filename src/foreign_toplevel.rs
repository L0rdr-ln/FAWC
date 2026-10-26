//! wlr-foreign-toplevel-management: lets a task bar list the open windows, show which one is
//! active, and raise, minimize, maximize or close them.
//!
//! `sync_foreign_toplevels` compares the windows with what each bound client was last told and
//! sends the difference; it runs on commits and once per frame.


use smithay::desktop::Window;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1::{self, State, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::state::Fawc;

/// What a client was last told about one window.
#[derive(Clone, PartialEq, Default)]
struct Snapshot {
    title: String,
    app_id: String,
    activated: bool,
    maximized: bool,
    minimized: bool,
}

struct Entry {
    surface: ObjectId,
    handle: ZwlrForeignToplevelHandleV1,
    sent: Option<Snapshot>,
}

struct Manager {
    resource: ZwlrForeignToplevelManagerV1,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct ForeignToplevelState {
    managers: Vec<Manager>,
}

impl ForeignToplevelState {
    pub fn new<D>(dh: &DisplayHandle) -> ForeignToplevelState
    where
        D: GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> + 'static,
    {
        dh.create_global::<D, ZwlrForeignToplevelManagerV1, ()>(3, ());
        ForeignToplevelState::default()
    }
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for Fawc {
    fn bind(
        state: &mut Fawc,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Fawc>,
    ) {
        let resource = data_init.init(resource, ());
        state.foreign_toplevels.managers.push(Manager { resource, entries: Vec::new() });
        state.sync_foreign_toplevels();
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Fawc {
    fn request(
        state: &mut Fawc,
        _: &Client,
        manager: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Fawc>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            manager.finished();
            state.foreign_toplevels.managers.retain(|m| &m.resource != manager);
        }
    }

    fn destroyed(state: &mut Fawc, _: smithay::reexports::wayland_server::backend::ClientId, manager: &ZwlrForeignToplevelManagerV1, _: &()) {
        state.foreign_toplevels.managers.retain(|m| &m.resource != manager);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, WlSurface> for Fawc {
    fn request(
        state: &mut Fawc,
        _: &Client,
        _: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Fawc>,
    ) {
        let Some(window) = state.window_for_surface(surface) else { return };
        use zwlr_foreign_toplevel_handle_v1::Request as R;
        match request {
            R::SetMaximized => state.set_maximized(&window, true),
            R::UnsetMaximized => state.set_maximized(&window, false),
            R::SetMinimized => state.minimize_window(&window),
            R::UnsetMinimized | R::Activate { .. } => state.activate_window(&window),
            R::Close => state.close_window(&window),
            R::SetRectangle { surface: panel_surface, x, y, width, height } => {
                // the task bar says where this window's button is, in its own surface: find that
                // surface among the layer surfaces to get the place in the shared space
                for output in state.all_outputs() {
                    let origin = state.output_rect(&output).loc;
                    let map = smithay::desktop::layer_map_for_output(&output);
                    if let Some(layer) = map.layer_for_surface(&panel_surface, smithay::desktop::WindowSurfaceType::TOPLEVEL) {
                        if let Some(geo) = map.layer_geometry(layer) {
                            let r = smithay::utils::Rectangle::new((origin.x + geo.loc.x + x, origin.y + geo.loc.y + y).into(), (width, height).into());
                            drop(map);
                            state.set_minimize_target(&window, r);
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
        state.sync_foreign_toplevels();
    }

    fn destroyed(state: &mut Fawc, _: smithay::reexports::wayland_server::backend::ClientId, handle: &ZwlrForeignToplevelHandleV1, _: &WlSurface) {
        for m in &mut state.foreign_toplevels.managers {
            m.entries.retain(|e| &e.handle != handle);
        }
    }
}

fn snapshot(state: &Fawc, window: &Window) -> Option<(WlSurface, Snapshot)> {
    let top = window.toplevel()?;
    let surface = top.wl_surface().clone();
    let (title, app_id) = with_states(&surface, |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|d| {
                let d = d.lock().unwrap();
                (d.title.clone().unwrap_or_default(), d.app_id.clone().unwrap_or_default())
            })
            .unwrap_or_default()
    });
    let activated = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|f| f == surface)
        .unwrap_or(false);
    Some((
        surface,
        Snapshot {
            title,
            app_id,
            activated,
            maximized: state.is_maximized(window),
            minimized: state.minimized.contains(window),
        },
    ))
}

fn states_of(s: &Snapshot) -> Vec<u8> {
    let mut v = Vec::new();
    let mut push = |st: State| v.extend_from_slice(&(st as u32).to_ne_bytes());
    if s.maximized {
        push(State::Maximized);
    }
    if s.minimized {
        push(State::Minimized);
    }
    if s.activated {
        push(State::Activated);
    }
    v
}

impl Fawc {
    pub fn sync_foreign_toplevels(&mut self) {
        if self.foreign_toplevels.managers.is_empty() {
            return;
        }
        let windows: Vec<Window> = self.space.elements().chain(self.minimized.iter()).cloned().collect();
        let snaps: Vec<(WlSurface, Snapshot)> = windows.iter().filter_map(|w| snapshot(self, w)).collect();
        let dh = self.display_handle.clone();
        for m in &mut self.foreign_toplevels.managers {
            // windows that are gone
            m.entries.retain(|e| {
                let alive = snaps.iter().any(|(s, _)| s.id() == e.surface);
                if !alive {
                    e.handle.closed();
                }
                alive
            });
            for (surface, snap) in &snaps {
                let idx = m.entries.iter().position(|e| e.surface == surface.id());
                let idx = match idx {
                    Some(i) => i,
                    None => {
                        let Some(client) = m.resource.client() else { continue };
                        let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, WlSurface, Fawc>(
                            &dh,
                            m.resource.version(),
                            surface.clone(),
                        ) else {
                            continue;
                        };
                        m.resource.toplevel(&handle);
                        m.entries.push(Entry { surface: surface.id(), handle, sent: None });
                        m.entries.len() - 1
                    }
                };
                let e = &mut m.entries[idx];
                let old = e.sent.clone().unwrap_or_default();
                let first = e.sent.is_none();
                if first || old.title != snap.title {
                    e.handle.title(snap.title.clone());
                }
                if first || old.app_id != snap.app_id {
                    e.handle.app_id(snap.app_id.clone());
                }
                if first || old != *snap {
                    e.handle.state(states_of(snap));
                }
                if first || old != *snap {
                    e.handle.done();
                }
                e.sent = Some(snap.clone());
            }
        }
    }

    /// Bring a window back if it was minimized and give it the keyboard (a task bar click).
    pub fn activate_window(&mut self, window: &Window) {
        if self.minimized.contains(window) {
            self.restore_specific(window);
        } else {
            self.focus_window(window);
        }
    }
}

