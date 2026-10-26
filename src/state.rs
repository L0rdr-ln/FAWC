//! The compositor state: protocol globals, windows, the pointer and what the user is doing with it.

use std::time::Instant;

use smithay::desktop::{PopupManager, Space, Window};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::{Client, DisplayHandle};
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;

use crate::glass::GlassStyle;

/// Per-window state kept in the window's user data.
#[derive(Debug, Default, Clone)]
pub struct WinInfo {
    /// where the window was before it was maximized
    pub restore: Option<Rectangle<i32, Logical>>,
    pub minimized: bool,
}

/// What a drag with the pointer is doing.
#[derive(Debug, Clone)]
pub enum Drag {
    Move { window: Window, grab_offset: Point<f64, Logical> },
    Resize { window: Window, edges: Edges, start: Rectangle<i32, Logical>, pointer_start: Point<f64, Logical> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

pub struct Fawc {
    pub start_time: Instant,
    pub display_handle: DisplayHandle,
    pub running: bool,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub output: Output,
    pub style: GlassStyle,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub session_lock_state: smithay::wayland::session_lock::SessionLockManagerState,
    /// the session is locked: only `lock_surface` is shown and gets input
    pub locked: bool,
    pub lock_surface: Option<smithay::wayland::session_lock::LockSurface>,
    pub lock_confirmation: Option<smithay::wayland::session_lock::SessionLocker>,
    pub foreign_toplevels: crate::foreign_toplevel::ForeignToplevelState,
    pub background_effect_state: crate::background_effect::BackgroundEffectState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Fawc>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<Fawc>,

    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    pub drag: Option<Drag>,
    /// windows hidden by the minimize button, newest last
    pub minimized: Vec<Window>,
    pub cascade: i32,
    pub terminal: String,
    /// the wallpaper as a texture of the output size, made when needed
    pub wallpaper: Option<smithay::backend::renderer::gles::GlesTexture>,
    pub text: crate::text::TextRenderer,
    /// draw our own mouse cursor (backends without a host cursor)
    pub draw_cursor: bool,
    pub cursor_texture: Option<smithay::backend::renderer::gles::GlesTexture>,
    /// the login session, on the DRM backend (virtual terminal switching)
    pub session: Option<smithay::backend::session::libseat::LibSeatSession>,
    /// rendered window titles, by surface
    pub titles: std::collections::HashMap<smithay::reexports::wayland_server::backend::ObjectId, TitleTex>,
}

/// A window title as a texture, and what it was made from.
pub struct TitleTex {
    pub title: String,
    pub max_width: i32,
    pub focused: bool,
    pub texture: smithay::backend::renderer::gles::GlesTexture,
    pub size: (i32, i32),
    /// how far the glow reaches around the text (the texture is this much bigger on every side)
    pub pad: i32,
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

pub fn client_state(client: &Client) -> &CompositorClientState {
    &client.get_data::<ClientState>().expect("client data").compositor_state
}

impl Fawc {
    pub fn new(dh: &DisplayHandle, size: Size<i32, Logical>) -> Fawc {
        let compositor_state = CompositorState::new::<Fawc>(dh);
        let xdg_shell_state = XdgShellState::new::<Fawc>(dh);
        let xdg_decoration_state = XdgDecorationState::new::<Fawc>(dh);
        let layer_shell_state = WlrLayerShellState::new::<Fawc>(dh);
        let session_lock_state = smithay::wayland::session_lock::SessionLockManagerState::new::<Fawc, _>(dh, |_| true);
        let foreign_toplevels = crate::foreign_toplevel::ForeignToplevelState::new::<Fawc>(dh);
        let background_effect_state = crate::background_effect::BackgroundEffectState::new::<Fawc>(dh);
        let shm_state = ShmState::new::<Fawc>(dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Fawc>(dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Fawc>(dh);
        let mut seat = seat_state.new_wl_seat(dh, "seat0");
        seat.add_keyboard(Default::default(), 600, 25).expect("keyboard");
        seat.add_pointer();

        let output = Output::new(
            "fawc-0".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "FAWC".into(),
                model: "virtual".into(),
            },
        );
        let mode = OutputMode { size: (size.w, size.h).into(), refresh: 60_000 };
        output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
        output.set_preferred(mode);
        let _global = output.create_global::<Fawc>(dh);
        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        Fawc {
            start_time: Instant::now(),
            display_handle: dh.clone(),
            running: true,
            space,
            popups: PopupManager::default(),
            output,
            style: GlassStyle::default(),
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            session_lock_state,
            locked: false,
            lock_surface: None,
            lock_confirmation: None,
            foreign_toplevels,
            background_effect_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
            pointer_location: (size.w as f64 / 2.0, size.h as f64 / 2.0).into(),
            cursor_status: CursorImageStatus::default_named(),
            drag: None,
            minimized: Vec::new(),
            cascade: 0,
            terminal: std::env::var("FAWC_TERMINAL").unwrap_or_else(|_| "foot".into()),
            wallpaper: None,
            text: crate::text::TextRenderer::default(),
            draw_cursor: false,
            cursor_texture: None,
            session: None,
            titles: std::collections::HashMap::new(),
        }
    }

    pub fn output_size(&self) -> Size<i32, Logical> {
        self.output
            .current_mode()
            .map(|m| (m.size.w, m.size.h).into())
            .unwrap_or((1280, 720).into())
    }

    pub fn resize_output(&mut self, size: Size<i32, Logical>) {
        let mode = OutputMode { size: (size.w, size.h).into(), refresh: 60_000 };
        self.output.change_current_state(Some(mode), None, None, None);
        self.output.set_preferred(mode);
    }
}
