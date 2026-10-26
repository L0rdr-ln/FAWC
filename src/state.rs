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

/// A window's last picture: the texture holds the window and its shadow room.
#[derive(Clone)]
pub struct Snapshot {
    pub texture: smithay::backend::renderer::gles::GlesTexture,
    /// where it is, logical, the shadow room included
    pub rect: Rectangle<i32, Logical>,
    pub scale: f64,
}

/// A window that has gone and is fading out.
pub struct Ghost {
    pub snapshot: Snapshot,
    pub since: Instant,
}

/// Per-window state kept in the window's user data.
#[derive(Debug, Default, Clone)]
pub struct WinInfo {
    /// where the window was before it was maximized
    pub restore: Option<Rectangle<i32, Logical>>,
    /// where the window was before it was snapped to half the screen
    pub snapped: Option<Rectangle<i32, Logical>>,
    /// where the window was before it went fullscreen (set while it is fullscreen)
    pub fullscreen: Option<Rectangle<i32, Logical>>,
    pub minimized: bool,
    /// the window was asked to close (a snapshot of it is kept for the closing animation)
    pub close_requested: bool,
    /// the workspace the window lives on (0 = the first)
    pub workspace: usize,
    /// when the window appeared, for the opening animation
    pub opened: Option<std::time::Instant>,
    /// the window is shrinking into the task bar since this moment
    pub minimizing: Option<std::time::Instant>,
    /// where its task bar button is (set by the task bar), to shrink towards
    pub minimize_target: Option<Rectangle<i32, Logical>>,
    /// where it was when it was minimized
    pub last_loc: Option<Point<i32, Logical>>,
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

pub struct Fde {
    pub start_time: Instant,
    pub display_handle: DisplayHandle,
    pub running: bool,

    pub space: Space<Window>,
    pub popups: PopupManager,
    /// the first monitor (the nested window has only this one); panels and notifications go here
    pub output: Output,
    /// every other monitor, in the order they appeared
    /// the modes each monitor offers (from the graphics driver), for the settings app
    pub available_modes: std::collections::HashMap<String, Vec<(i32, i32, i32)>>,
    pub extra_outputs: Vec<Output>,
    /// the Wayland globals of the extra outputs (to remove them when a monitor goes)
    pub output_globals: Vec<(Output, smithay::reexports::wayland_server::backend::GlobalId)>,
    /// the session programs (task bars ...) have been started
    pub session_started: bool,
    pub style: GlassStyle,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub session_lock_state: smithay::wayland::session_lock::SessionLockManagerState,
    /// the session is locked: only `lock_surface` is shown and gets input
    pub locked: bool,
    pub lock_surfaces: Vec<(Output, smithay::wayland::session_lock::LockSurface)>,
    pub lock_confirmation: Option<smithay::wayland::session_lock::SessionLocker>,
    pub foreign_toplevels: crate::foreign_toplevel::ForeignToplevelState,
    pub background_effect_state: crate::background_effect::BackgroundEffectState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Fde>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: smithay::wayland::selection::primary_selection::PrimarySelectionState,
    /// windows that are bending on their springs
    /// the X11 window manager, once XWayland is up
    pub xwm: Option<smithay::xwayland::X11Wm>,
    /// the event loop, for transfers between X11 and Wayland clipboards
    pub loop_handle: Option<smithay::reexports::calloop::LoopHandle<'static, Fde>>,
    pub x11_display: Option<u32>,
    pub xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState,
    pub screencopy: crate::screencopy::Screencopy,
    pub wobbles: Vec<(Window, crate::wobble::Wobble)>,
    pub switcher: Option<crate::switcher::Switcher>,
    /// the graphics are drawn by the CPU (a VM without 3D): `appearance.performance = "auto"` then uses fast mode
    pub software_renderer: bool,
    /// counts every reason to draw again; a monitor that has drawn this number is up to date
    pub redraw_serial: u64,
    /// the shortcut cheat sheet is showing
    pub help_open: bool,
    /// Alt was let go while the switcher was open: commit after the key event is handled
    pub switcher_commit: bool,
    /// the last picture of windows that were asked to close, by their surface
    pub snapshots: std::collections::HashMap<smithay::reexports::wayland_server::backend::ObjectId, Snapshot>,
    /// windows that are gone and fade out
    pub ghosts: Vec<Ghost>,
    /// the workspace being shown, and windows on the others (taken off the screen)
    pub workspace: usize,
    pub parked: Vec<Window>,
    /// "Workspace 2", shown for a moment after a switch
    pub hud: Option<(std::time::Instant, String)>,
    pub wobble_time: Instant,
    pub viewporter_state: smithay::wayland::viewporter::ViewporterState,
    pub fractional_scale_state: smithay::wayland::fractional_scale::FractionalScaleManagerState,
    pub idle_inhibit_state: smithay::wayland::idle_inhibit::IdleInhibitManagerState,
    /// surfaces whose program asked the screen not to lock (a video, a game)
    pub idle_inhibitors: Vec<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    pub dmabuf_global: Option<smithay::wayland::dmabuf::DmabufGlobal>,
    pub xdg_activation_state: smithay::wayland::xdg_activation::XdgActivationState,
    pub cursor_shape_state: smithay::wayland::cursor_shape::CursorShapeManagerState,
    pub pointer_constraints_state: smithay::wayland::pointer_constraints::PointerConstraintsState,
    pub relative_pointer_state: smithay::wayland::relative_pointer::RelativePointerManagerState,
    /// where the surface that has the pointer starts (for pointer constraint regions)
    pub pointer_focus_origin: Point<f64, Logical>,
    pub seat: Seat<Fde>,

    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    pub drag: Option<Drag>,
    /// windows hidden by the minimize button, newest last
    pub minimized: Vec<Window>,
    pub cascade: i32,
    pub terminal: String,
    /// the last key or pointer event, for the idle lock
    pub last_activity: std::time::Instant,
    idle_lock_started: bool,
    pub config: crate::config::Config,
    config_watcher: crate::config::Watcher,
    config_checked: std::time::Instant,
    /// the wallpaper as a texture, per size in physical pixels, made when needed
    pub wallpapers: std::collections::HashMap<(i32, i32), smithay::backend::renderer::gles::GlesTexture>,
    /// the wallpaper picture from the configuration (None = the built-in sky)
    pub wallpaper_image: Option<crate::wallpaper::Image>,
    pub text: crate::text::TextRenderer,
    /// draw our own mouse cursor (backends without a host cursor)
    pub draw_cursor: bool,
    /// the cursor theme and the textures made from it, by name and scale
    pub cursor_theme: crate::cursortheme::CursorTheme,
    pub cursor_textures: std::collections::HashMap<String, (smithay::backend::renderer::gles::GlesTexture, crate::cursortheme::CursorImage)>,
    /// the cursor the compositor wants where no client has the pointer (borders, title bars)
    pub hover_cursor: &'static str,
    /// the login session, on the DRM backend (virtual terminal switching)
    pub session: Option<smithay::backend::session::libseat::LibSeatSession>,
    /// rendered window titles, by surface
    pub titles: std::collections::HashMap<(smithay::reexports::wayland_server::backend::ObjectId, i32), TitleTex>,
}

/// The wallpaper picture: the file the configuration names, else the one built into the program
/// (None = the generated sky, see `wallpaper::configured`).
pub fn load_wallpaper(config: &crate::config::Config) -> Option<crate::wallpaper::Image> {
    crate::wallpaper::for_config(config)
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
    if let Some(c) = client.get_data::<ClientState>() {
        return &c.compositor_state;
    }
    // the X server is a client of its own kind
    &client.get_data::<smithay::xwayland::XWaylandClientData>().expect("client data").compositor_state
}

impl Fde {
    pub fn new(dh: &DisplayHandle, size: Size<i32, Logical>) -> Fde {
        Fde::new_named(dh, "fde-0", size)
    }

    /// `size` is the monitor's mode in physical pixels; `name` is its connector name.
    pub fn new_named(dh: &DisplayHandle, name: &str, size: Size<i32, Logical>) -> Fde {
        let compositor_state = CompositorState::new::<Fde>(dh);
        let xdg_shell_state = XdgShellState::new::<Fde>(dh);
        let xdg_decoration_state = XdgDecorationState::new::<Fde>(dh);
        let layer_shell_state = WlrLayerShellState::new::<Fde>(dh);
        let session_lock_state = smithay::wayland::session_lock::SessionLockManagerState::new::<Fde, _>(dh, |_| true);
        let foreign_toplevels = crate::foreign_toplevel::ForeignToplevelState::new::<Fde>(dh);
        let background_effect_state = crate::background_effect::BackgroundEffectState::new::<Fde>(dh);
        let shm_state = ShmState::new::<Fde>(dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Fde>(dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Fde>(dh);
        let primary_selection_state = smithay::wayland::selection::primary_selection::PrimarySelectionState::new::<Fde>(dh);
        crate::screencopy::create_global(dh);
        let xwayland_shell_state = smithay::wayland::xwayland_shell::XWaylandShellState::new::<Fde>(dh);
        let viewporter_state = smithay::wayland::viewporter::ViewporterState::new::<Fde>(dh);
        let fractional_scale_state = smithay::wayland::fractional_scale::FractionalScaleManagerState::new::<Fde>(dh);
        let idle_inhibit_state = smithay::wayland::idle_inhibit::IdleInhibitManagerState::new::<Fde>(dh);
        let dmabuf_state = smithay::wayland::dmabuf::DmabufState::new();
        let xdg_activation_state = smithay::wayland::xdg_activation::XdgActivationState::new::<Fde>(dh);
        let cursor_shape_state = smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Fde>(dh);
        let pointer_constraints_state = smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Fde>(dh);
        let relative_pointer_state = smithay::wayland::relative_pointer::RelativePointerManagerState::new::<Fde>(dh);
        let mut seat = seat_state.new_wl_seat(dh, "seat0");
        let config = match crate::config::Config::load() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("configuration: {e} (using the defaults)");
                crate::config::Config::default()
            }
        };
        let xkb = smithay::input::keyboard::XkbConfig { layout: &config.input.keyboard_layout, ..Default::default() };
        seat.add_keyboard(xkb, config.input.repeat_delay, config.input.repeat_rate).expect("keyboard");
        let mut style = GlassStyle::default();
        config.apply_style(&mut style);
        seat.add_pointer();
        let wallpaper_image = load_wallpaper(&config);
        let cursor_theme = crate::cursortheme::CursorTheme::load(&config.appearance.cursor_theme, config.appearance.cursor_size);

        let output = Output::new(
            name.to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "FDE".into(),
                model: "virtual".into(),
            },
        );
        let mode = OutputMode { size: (size.w, size.h).into(), refresh: 60_000 };
        output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
        output.set_preferred(mode);
        let _global = output.create_global::<Fde>(dh);
        let mut space = Space::default();
        space.map_output(&output, (0, 0));
        // scale and position from the configuration
        if let Some(oc) = config.outputs.get(name) {
            output.change_current_state(None, None, Some(smithay::output::Scale::Fractional(oc.scale)), Some((oc.x.unwrap_or(0), oc.y.unwrap_or(0)).into()));
            space.map_output(&output, (oc.x.unwrap_or(0), oc.y.unwrap_or(0)));
        }

        Fde {
            start_time: Instant::now(),
            display_handle: dh.clone(),
            running: true,
            space,
            popups: PopupManager::default(),
            output,
            available_modes: Default::default(),
            extra_outputs: Vec::new(),
            output_globals: Vec::new(),
            session_started: false,
            style,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            session_lock_state,
            locked: false,
            lock_surfaces: Vec::new(),
            lock_confirmation: None,
            foreign_toplevels,
            background_effect_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            xwm: None,
            loop_handle: None,
            x11_display: None,
            xwayland_shell_state,
            screencopy: Default::default(),
            wobbles: Vec::new(),
            switcher: None,
            software_renderer: false,
            redraw_serial: 1,
            help_open: false,
            switcher_commit: false,
            snapshots: Default::default(),
            ghosts: Vec::new(),
            workspace: 0,
            parked: Vec::new(),
            hud: None,
            wobble_time: Instant::now(),
            viewporter_state,
            fractional_scale_state,
            idle_inhibit_state,
            idle_inhibitors: Vec::new(),
            dmabuf_state,
            dmabuf_global: None,
            xdg_activation_state,
            cursor_shape_state,
            pointer_constraints_state,
            relative_pointer_state,
            pointer_focus_origin: (0.0, 0.0).into(),
            seat,
            pointer_location: (size.w as f64 / 2.0, size.h as f64 / 2.0).into(),
            cursor_status: CursorImageStatus::default_named(),
            drag: None,
            minimized: Vec::new(),
            cascade: 0,
            terminal: std::env::var("FDE_TERMINAL").unwrap_or_else(|_| config.shell.terminal.clone()),
            last_activity: std::time::Instant::now(),
            idle_lock_started: false,
            config,
            config_watcher: crate::config::Watcher::new(),
            config_checked: std::time::Instant::now(),
            wallpapers: std::collections::HashMap::new(),
            wallpaper_image,
            text: crate::text::TextRenderer::default(),
            draw_cursor: false,
            cursor_theme,
            cursor_textures: std::collections::HashMap::new(),
            hover_cursor: "default",
            session: None,
            titles: std::collections::HashMap::new(),
        }
    }

    /// Reload the configuration if a file changed (looked at twice a second at most): the look,
    /// the terminal and the keyboard follow at once; a file that does not parse changes nothing.
    pub fn check_config(&mut self, force: bool) {
        if !force && self.config_checked.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        self.config_checked = std::time::Instant::now();
        if !self.config_watcher.changed() {
            return;
        }
        let new = match crate::config::Config::load() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("configuration not applied: {e}");
                return;
            }
        };
        if new == self.config {
            return;
        }
        tracing::info!("configuration reloaded");
        self.request_redraw();
        new.apply_style(&mut self.style);
        if std::env::var_os("FDE_TERMINAL").is_none() {
            self.terminal = new.shell.terminal.clone();
        }
        if let Some(kb) = self.seat.get_keyboard() {
            kb.change_repeat_info(new.input.repeat_rate, new.input.repeat_delay);
            if new.input.keyboard_layout != self.config.input.keyboard_layout {
                let xkb = smithay::input::keyboard::XkbConfig { layout: &new.input.keyboard_layout, ..Default::default() };
                if let Err(e) = kb.set_xkb_config(self, xkb) {
                    tracing::warn!("cannot set the keyboard layout: {e:?}");
                }
            }
        }
        if new.wallpaper != self.config.wallpaper || new.appearance.theme != self.config.appearance.theme {
            self.wallpaper_image = load_wallpaper(&new);
            self.wallpapers.clear();
        }
        if new.appearance != self.config.appearance || new.explicit != self.config.explicit {
            self.titles.clear(); // new colours for the title text
        }
        if new.appearance.cursor_theme != self.config.appearance.cursor_theme || new.appearance.cursor_size != self.config.appearance.cursor_size {
            self.cursor_theme = crate::cursortheme::CursorTheme::load(&new.appearance.cursor_theme, new.appearance.cursor_size);
            self.cursor_textures.clear();
        }
        let outputs_changed = new.outputs != self.config.outputs;
        self.config = new;
        if outputs_changed {
            let cfg = self.config.clone();
            self.apply_output_config(&cfg);
        }
    }

    /// Something happened on the keyboard or pointer: the user is there.
    pub fn note_activity(&mut self) {
        self.last_activity = std::time::Instant::now();
        self.idle_lock_started = false;
    }

    /// Lock the session once the user has been idle for `session.idle_lock_seconds` (0 = never).
    /// Returns true when it started the lock screen just now.
    pub fn check_idle(&mut self) -> bool {
        let limit = self.config.session.idle_lock_seconds as u64;
        if limit == 0 || self.locked || self.idle_lock_started {
            return false;
        }
        if self.last_activity.elapsed() < std::time::Duration::from_secs(limit) {
            return false;
        }
        if self.idle_inhibited() {
            return false;
        }
        self.idle_lock_started = true;
        tracing::info!("idle for {limit} s: locking");
        crate::shell::start_sibling("fde-lock");
        true
    }

    /// The size of the first monitor in logical pixels.
    pub fn output_size(&self) -> Size<i32, Logical> {
        self.output_logical_size(&self.output)
    }

    /// Set the first monitor's mode (physical pixels), e.g. after the nested window was resized.
    pub fn resize_output(&mut self, size: Size<i32, Logical>) {
        let mode = OutputMode { size: (size.w, size.h).into(), refresh: 60_000 };
        self.output.change_current_state(Some(mode), None, None, None);
        self.output.set_preferred(mode);
    }
}
