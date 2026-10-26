//! End to end: a real Wayland client talks to the compositor, which draws the result offscreen on
//! software GL; the test looks at the pixels and clicks the window's buttons.

use std::io::Write;
use std::os::unix::net::UnixStream;

/// The tests share XDG_RUNTIME_DIR, so they run one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::backend::renderer::gles::GlesTexture;
use smithay::backend::renderer::{Bind, ExportMem, Texture};
use smithay::backend::allocator::Fourcc;
use smithay::reexports::wayland_server::{Display, ListeningSocket};
use smithay::utils::Rectangle;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use fawc::demo::offscreen_renderer;
use fawc::glass::Glass;
use fawc::layout::{button_center, Button};
use fawc::render::create_scene;
use fawc::state::{ClientState, Fawc};

// ------------------------------------------------------------------ the client

#[derive(Default)]
struct App {
    configured: bool,
    closed: bool,
    server_side: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(_: &mut Self, wm: &xdg_wm_base::XdgWmBase, event: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(app: &mut Self, s: &xdg_surface::XdgSurface, event: xdg_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = event {
            s.ack_configure(serial);
            app.configured = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(app: &mut Self, _: &xdg_toplevel::XdgToplevel, event: xdg_toplevel::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_toplevel::Event::Close = event {
            app.closed = true;
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for App {
    fn event(app: &mut Self, _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, event: zxdg_toplevel_decoration_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            app.server_side = matches!(mode, wayland_client::WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ServerSide));
        }
    }
}

enum Msg {
    Mapped { server_side: bool },
    Closed,
}

/// ARGB8888 solid colour buffer written to a temp file.
fn client(path: std::path::PathBuf, tx: mpsc::Sender<Msg>, w: i32, h: i32, argb: u32) {
    let stream = UnixStream::connect(path).expect("connect");
    let conn = Connection::from_socket(stream).expect("connection");
    let (globals, mut queue) = registry_queue_init::<App>(&conn).expect("registry");
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ()).unwrap();
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=1, ()).unwrap();
    let deco: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();

    let mut app = App::default();
    let surface = compositor.create_surface(&qh, ());
    let xs = wm.get_xdg_surface(&surface, &qh, ());
    let top = xs.get_toplevel(&qh, ());
    top.set_title("e2e".into());
    let _d = deco.get_toplevel_decoration(&top, &qh, ());
    surface.commit();
    while !app.configured {
        queue.blocking_dispatch(&mut app).expect("dispatch");
    }

    let mut file = tempfile::tempfile_in(std::env::var("XDG_RUNTIME_DIR").unwrap()).unwrap();
    let px: Vec<u8> = (0..w * h).flat_map(|_| argb.to_le_bytes()).collect();
    file.write_all(&px).unwrap();
    use std::os::fd::AsFd;
    let pool = shm.create_pool(file.as_fd(), w * h * 4, &qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    queue.roundtrip(&mut app).unwrap();
    tx.send(Msg::Mapped { server_side: app.server_side }).unwrap();

    let t0 = Instant::now();
    while !app.closed && t0.elapsed() < Duration::from_secs(20) {
        queue.blocking_dispatch(&mut app).ok();
    }
    if app.closed {
        tx.send(Msg::Closed).unwrap();
    }
}

// ------------------------------------------------------------------ the server side of the test

struct Harness {
    display: Display<Fawc>,
    state: Fawc,
    socket: ListeningSocket,
}

impl Harness {
    fn pump(&mut self) {
        while let Some(stream) = self.socket.accept().unwrap() {
            self.display
                .handle()
                .insert_client(stream, Arc::new(ClientState::default()))
                .unwrap();
        }
        self.display.dispatch_clients(&mut self.state).unwrap();
        self.display.flush_clients().unwrap();
    }

    fn pump_for(&mut self, ms: u64) {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(ms) {
            self.pump();
            std::thread::sleep(Duration::from_millis(3));
        }
    }
}

fn pixel(buf: &[u8], w: i32, x: i32, y: i32) -> [i32; 3] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32]
}

#[test]
fn a_client_window_gets_a_glass_frame_and_can_be_closed_with_its_orb() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): e2e skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    let (tx, rx) = mpsc::channel();
    let path = dir.path().join("fawc-e2e");
    let handle = std::thread::spawn(move || client(path, tx, 300, 200, 0xffe03020));

    // 1. the window appears and the compositor asked for server side decoration
    let t0 = Instant::now();
    let server_side = loop {
        hn.pump();
        match rx.try_recv() {
            Ok(Msg::Mapped { server_side }) => break server_side,
            _ => {}
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the client never mapped its window");
        std::thread::sleep(Duration::from_millis(3));
    };
    assert!(server_side, "the compositor must offer server side decorations");
    hn.pump_for(100);
    assert_eq!(hn.state.space.elements().count(), 1);

    // 2. what is drawn: client colour inside, glass around it, the wallpaper outside
    hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
    let bytes = {
        let size = scene.size();
        let target = renderer.bind(&mut scene).unwrap();
        let m = renderer
            .copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888)
            .unwrap();
        renderer.map_texture(&m).unwrap().to_vec()
    };
    let g = hn.state.space.elements().next().and_then(|e| hn.state.space.element_geometry(e)).unwrap();
    assert_eq!((g.size.w, g.size.h), (300, 200));
    let c = pixel(&bytes, w, g.loc.x + 150, g.loc.y + 100);
    assert!(c[0] > 200 && c[1] < 70 && c[2] < 60, "the client's red shows in its area: {c:?}");
    let outside = pixel(&bytes, w, 700, 500);
    let wp = fawc::wallpaper::procedural(w, h, false);
    let wi = ((500 * w + 700) * 4) as usize;
    assert!((outside[0] - wp[wi] as i32).abs() < 6 && (outside[2] - wp[wi + 2] as i32).abs() < 6, "wallpaper outside the window: {outside:?}");
    let top = pixel(&bytes, w, g.loc.x + 150, g.loc.y - 30);
    assert!(top[2] > top[0] + 20, "the title bar is blue glass: {top:?}");
    let outer = fawc::layout::outer_rect(g, &hn.state.style);
    let corner = pixel(&bytes, w, outer.loc.x, outer.loc.y);
    assert!((corner[0] - outside[0]).abs() < 6, "the rounded corner leaves the wallpaper alone: {corner:?}");
    let orb = button_center(outer, &hn.state.style, Button::Close);
    let o = pixel(&bytes, w, orb.x as i32, orb.y as i32 + 3);
    assert!(o[0] > o[2] + 30, "the close orb is red: {o:?}");

    // the title ("e2e") is drawn in dark ink at the left of the title bar (when fonts exist)
    if fawc::text::TextRenderer::default().line("e2e", 100, 13.0, [0, 0, 0], None, 0).is_some() {
        let mut ink = 0;
        for y in (outer.loc.y + 5)..(outer.loc.y + 5 + 32) {
            for x in (outer.loc.x + 12)..(outer.loc.x + 12 + 40) {
                if (pixel(&bytes, w, x, y)).iter().sum::<i32>() < 3 * 110 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 15, "the window title should be drawn in the title bar (ink pixels: {ink})");
    }

    // 3. the window has the keyboard focus
    assert!(hn.state.seat.get_keyboard().unwrap().current_focus().is_some());

    // 4. drag the title bar
    let before = g.loc;
    let grab: smithay::utils::Point<f64, smithay::utils::Logical> = (g.loc.x as f64 + 50.0, g.loc.y as f64 - 16.0).into();
    hn.state.pointer_moved(grab, 1);
    hn.state.pointer_button(0x110, true, 2);
    hn.state.pointer_moved((grab.x + 100.0, grab.y + 40.0).into(), 3);
    hn.state.pointer_button(0x110, false, 4);
    let after = hn.state.space.element_geometry(hn.state.space.elements().next().unwrap()).unwrap().loc;
    assert_eq!((after.x - before.x, after.y - before.y), (100, 40), "dragging the title bar moves the window");

    // 5. click the close orb: the client gets xdg_toplevel.close
    let outer = fawc::layout::outer_rect(hn.state.space.element_geometry(hn.state.space.elements().next().unwrap()).unwrap(), &hn.state.style);
    let orb = button_center(outer, &hn.state.style, Button::Close);
    hn.state.pointer_moved(orb, 5);
    hn.state.pointer_button(0x110, true, 6);
    hn.state.pointer_button(0x110, false, 7);
    let t0 = Instant::now();
    loop {
        hn.pump();
        if let Ok(Msg::Closed) = rx.try_recv() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the close orb did not close the window");
        std::thread::sleep(Duration::from_millis(3));
    }
    handle.join().unwrap();
}


// ------------------------------------------------------------------ layer shell

use wayland_protocols::ext::background_effect::v1::client::{ext_background_effect_manager_v1, ext_background_effect_surface_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

#[derive(Default)]
struct PanelApp {
    configured: bool,
    size: (u32, u32),
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for PanelApp {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(PanelApp: ignore wl_compositor::WlCompositor);
delegate_noop!(PanelApp: ignore wl_surface::WlSurface);
delegate_noop!(PanelApp: ignore wl_shm::WlShm);
delegate_noop!(PanelApp: ignore wl_shm_pool::WlShmPool);
delegate_noop!(PanelApp: ignore wl_buffer::WlBuffer);
delegate_noop!(PanelApp: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(PanelApp: ignore ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);
delegate_noop!(PanelApp: ignore wayland_client::protocol::wl_region::WlRegion);
impl Dispatch<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, ()> for PanelApp {
    fn event(_: &mut Self, _: &ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _: ext_background_effect_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for PanelApp {
    fn event(app: &mut Self, s: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, width, height } = event {
            s.ack_configure(serial);
            app.size = (width, height);
            app.configured = true;
        }
    }
}

/// A 40 px panel at the bottom that reserves its space; reports when it is on screen.
fn panel_client(path: std::path::PathBuf, tx: mpsc::Sender<(u32, u32)>, argb: u32) {
    let stream = UnixStream::connect(path).expect("connect");
    let conn = Connection::from_socket(stream).expect("connection");
    let (globals, mut queue) = registry_queue_init::<PanelApp>(&conn).expect("registry");
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ()).unwrap();
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ()).unwrap();
    let mut app = PanelApp::default();
    let surface = compositor.create_surface(&qh, ());
    let ls = shell.get_layer_surface(&surface, None, zwlr_layer_shell_v1::Layer::Top, "panel".into(), &qh, ());
    ls.set_size(0, 40);
    ls.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Left | zwlr_layer_surface_v1::Anchor::Right);
    ls.set_exclusive_zone(40);
    // blur behind the right half of the panel
    let manager: ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
    let effect = manager.get_background_effect(&surface, &qh, ());
    let region = compositor.create_region(&qh, ());
    region.add(400, 0, 400, 40);
    effect.set_blur_region(Some(&region));
    region.destroy();
    surface.commit();
    while !app.configured {
        queue.blocking_dispatch(&mut app).expect("dispatch");
    }
    let (w, h) = (app.size.0 as i32, app.size.1 as i32);
    let mut file = tempfile::tempfile_in(std::env::var("XDG_RUNTIME_DIR").unwrap()).unwrap();
    let px: Vec<u8> = (0..w * h).flat_map(|_| argb.to_le_bytes()).collect();
    file.write_all(&px).unwrap();
    use std::os::fd::AsFd;
    let pool = shm.create_pool(file.as_fd(), w * h * 4, &qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    queue.roundtrip(&mut app).unwrap();
    tx.send(app.size).unwrap();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        queue.blocking_dispatch(&mut app).ok();
    }
}

#[test]
fn a_panel_reserves_space_and_maximized_windows_stay_above_it() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): layer test skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e-layer").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    let (ptx, prx) = mpsc::channel();
    let path = dir.path().join("fawc-e2e-layer");
    let p2 = path.clone();
    std::thread::spawn(move || panel_client(p2, ptx, 0xff20b040));
    let t0 = Instant::now();
    let size = loop {
        hn.pump();
        if let Ok(sz) = prx.try_recv() {
            break sz;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the panel never appeared");
        std::thread::sleep(Duration::from_millis(3));
    };
    assert_eq!(size, (800, 40), "the panel is as wide as the output and 40 px high");
    hn.pump_for(100);

    // the panel is drawn at the bottom edge, above the wallpaper
    hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
    let grab = |renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture| -> Vec<u8> {
        let size = scene.size();
        let target = renderer.bind(scene).unwrap();
        let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
        renderer.map_texture(&m).unwrap().to_vec()
    };
    let bytes = grab(&mut renderer, &mut scene);
    let p = pixel(&bytes, w, 400, 590);
    assert!(p[1] > 150 && p[0] < 80 && p[2] < 100, "the green panel at the bottom: {p:?}");
    let above = pixel(&bytes, w, 400, 540);
    assert!(above[1] < 150 || above[2] > 100, "the wallpaper above the panel: {above:?}");

    // the blur region the client asked for arrived with the commit
    {
        let map = smithay::desktop::layer_map_for_output(&hn.state.output);
        let layer = map.layers().next().expect("the panel is mapped");
        let b = fawc::background_effect::blur_bounds(layer.wl_surface()).expect("a blur region");
        assert_eq!((b.loc.x, b.loc.y, b.size.w, b.size.h), (400, 0, 400, 40));
    }

    // the usable area is what the panel leaves
    let usable = hn.state.usable_area(&hn.state.output.clone());
    assert_eq!((usable.loc.y, usable.size.h), (0, 560));

    // a window maximized now stops above the panel
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || client(path, tx, 200, 100, 0xff2030e0));
    let t0 = Instant::now();
    loop {
        hn.pump();
        if let Ok(Msg::Mapped { .. }) = rx.try_recv() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no window");
        std::thread::sleep(Duration::from_millis(3));
    }
    let win = hn.state.space.elements().next().cloned().unwrap();
    hn.state.set_maximized(&win, true);
    hn.pump_for(100);
    let outer = hn.state.outer_of(&win).unwrap();
    assert!(outer.loc.y + outer.size.h <= 560, "maximized window must stay above the panel: {outer:?}");
    drop(handle);
}

#[test]
fn the_real_task_bar_shows_a_button_for_an_open_window() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): task bar test skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e-tasks").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    // the real panel program, as the compositor starts it
    let mut panel = std::process::Command::new(env!("CARGO_BIN_EXE_fawc-panel"))
        .env("WAYLAND_DISPLAY", "fawc-e2e-tasks")
        .env_remove("DISPLAY")
        .spawn()
        .unwrap();
    hn.pump_for(500);
    let grab = |renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture| -> Vec<u8> {
        let size = scene.size();
        let target = renderer.bind(scene).unwrap();
        let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
        renderer.map_texture(&m).unwrap().to_vec()
    };
    let probe = |bytes: &[u8]| (pixel(bytes, w, 100, 592), pixel(bytes, w, 450, 592));
    hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
    let (on_button, beside) = probe(&grab(&mut renderer, &mut scene));
    assert!((on_button[0] - beside[0]).abs() < 6, "no task button yet: {on_button:?} {beside:?}");

    // open a window: a button for it appears in the bar
    let (tx, rx) = mpsc::channel();
    let path = dir.path().join("fawc-e2e-tasks");
    std::thread::spawn(move || client(path, tx, 200, 100, 0xff2030e0));
    let t0 = Instant::now();
    let mut appeared = false;
    while t0.elapsed() < Duration::from_secs(10) {
        hn.pump();
        let _ = rx.try_recv();
        hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
        let (on_button, beside) = probe(&grab(&mut renderer, &mut scene));
        if (on_button[0] - beside[0]).abs() > 12 {
            appeared = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = panel.kill();
    assert!(appeared, "the task bar never showed a button for the window");
}

#[test]
fn the_lock_screen_locks_the_session_and_unlocks_with_the_right_password() {
    use smithay::backend::input::KeyState;
    use smithay::input::keyboard::{FilterResult, Keycode};
    use smithay::utils::SERIAL_COUNTER;

    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): lock test skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e-lock").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    let mut lock = std::process::Command::new(env!("CARGO_BIN_EXE_fawc-lock"))
        .env("WAYLAND_DISPLAY", "fawc-e2e-lock")
        .env("USER", "tester")
        .env("FAWC_LOCK_CHECK", "[ \"$(tr -d '\\000')\" = secret ]")
        .env_remove("DISPLAY")
        .spawn()
        .unwrap();

    let step = |hn: &mut Harness, renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture| {
        hn.pump();
        hn.state.render_scene(renderer, &glass, scene).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    };
    let wait = |hn: &mut Harness, renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture, what: &str, cond: &dyn Fn(&Harness) -> bool| {
        let t0 = Instant::now();
        while !cond(hn) {
            assert!(t0.elapsed() < Duration::from_secs(10), "timeout: {what}");
            step(hn, renderer, scene);
        }
    };
    wait(&mut hn, &mut renderer, &mut scene, "the session locks", &|hn| hn.state.locked && !hn.state.lock_surfaces.is_empty() && hn.state.lock_confirmation.is_none());
    // let the card be drawn and shown
    for _ in 0..60 {
        step(&mut hn, &mut renderer, &mut scene);
    }
    let bytes = {
        let size = scene.size();
        let target = renderer.bind(&mut scene).unwrap();
        let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
        renderer.map_texture(&m).unwrap().to_vec()
    };
    // the login card is a lighter, bluer sheet in the middle than the wallpaper around it
    let middle = pixel(&bytes, w, 400, 300 - 90);
    let around = pixel(&bytes, w, 40, 300 - 90);
    assert!(middle != around, "the card is on screen: {middle:?} {around:?}");

    let mut type_keys = |hn: &mut Harness, codes: &[u32]| {
        for &c in codes {
            let kb = hn.state.seat.get_keyboard().unwrap();
            for st in [KeyState::Pressed, KeyState::Released] {
                kb.input::<(), _>(&mut hn.state, Keycode::new(c + 8), st, SERIAL_COUNTER.next_serial(), 0, |_, _, _| FilterResult::Forward);
            }
            hn.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    // evdev: x = 45, Enter = 28
    type_keys(&mut hn, &[45, 28]);
    for _ in 0..60 {
        step(&mut hn, &mut renderer, &mut scene);
    }
    assert!(hn.state.locked, "a wrong password does not unlock");
    // s e c r e t Enter
    type_keys(&mut hn, &[31, 18, 46, 19, 18, 20, 28]);
    wait(&mut hn, &mut renderer, &mut scene, "the right password unlocks", &|hn| !hn.state.locked);
    let _ = lock.wait();
}

#[test]
fn a_notification_sent_over_dbus_appears_as_a_bubble_and_goes_away() {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    if ["dbus-daemon", "dbus-send", "gdbus"].iter().any(|c| Command::new(c).arg("--version").output().is_err()) {
        eprintln!("no dbus-daemon / dbus-send / gdbus here: notification test skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): notification test skipped");
            return;
        }
    };
    // a private session bus
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut address = String::new();
    std::io::BufReader::new(bus.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let address = address.trim().to_string();

    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e-notify").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();
    let grab = |renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture| -> Vec<u8> {
        let size = scene.size();
        let target = renderer.bind(scene).unwrap();
        let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
        renderer.map_texture(&m).unwrap().to_vec()
    };
    hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
    let before = pixel(&grab(&mut renderer, &mut scene), w, w - 12 - 170, 12 + 70);

    struct Kill(Vec<std::process::Child>);
    impl Drop for Kill {
        fn drop(&mut self) {
            for c in &mut self.0 {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
    let daemon = Command::new(env!("CARGO_BIN_EXE_fawc-notify"))
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .env("WAYLAND_DISPLAY", "fawc-e2e-notify")
        .env_remove("DISPLAY")
        .spawn()
        .unwrap();
    let _kill = Kill(vec![bus, daemon]);

    // wait until the daemon owns its name on the bus
    let t0 = Instant::now();
    loop {
        hn.pump();
        let out = Command::new("dbus-send")
            .args(["--session", "--dest=org.freedesktop.DBus", "--print-reply", "/org/freedesktop/DBus", "org.freedesktop.DBus.NameHasOwner", "string:org.freedesktop.Notifications"])
            .env("DBUS_SESSION_BUS_ADDRESS", &address)
            .output()
            .unwrap();
        if String::from_utf8_lossy(&out.stdout).contains("true") {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the daemon never took its name on the bus");
        std::thread::sleep(Duration::from_millis(50));
    }
    hn.pump_for(300);

    let sent = Command::new("gdbus")
        .args([
            "call", "--session", "--dest", "org.freedesktop.Notifications", "--object-path", PATH_N,
            "--method", "org.freedesktop.Notifications.Notify",
            "test", "0", "", "Hello there", "The body of it", "[]", "{}", "2500",
        ])
        .env("DBUS_SESSION_BUS_ADDRESS", &address)
        .output()
        .unwrap();
    let reply = String::from_utf8_lossy(&sent.stdout).to_string();
    assert!(reply.contains("uint32 1"), "Notify returns the id: {reply} {}", String::from_utf8_lossy(&sent.stderr));

    // the bubble shows up at the top right
    let t0 = Instant::now();
    let mut shown = false;
    while t0.elapsed() < Duration::from_secs(10) {
        hn.pump();
        hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
        let now = pixel(&grab(&mut renderer, &mut scene), w, w - 12 - 170, 12 + 70);
        if (now[0] - before[0]).abs() + (now[1] - before[1]).abs() > 30 {
            shown = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(shown, "no bubble appeared at the top right");

    // and after its lifetime it is gone again
    let t0 = Instant::now();
    let mut gone = false;
    while t0.elapsed() < Duration::from_secs(10) {
        hn.pump();
        hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
        let now = pixel(&grab(&mut renderer, &mut scene), w, w - 12 - 170, 12 + 70);
        if (now[0] - before[0]).abs() + (now[1] - before[1]).abs() < 6 {
            gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(gone, "the bubble did not go away after its timeout");
}

const PATH_N: &str = "/org/freedesktop/Notifications";

#[test]
fn typing_in_the_start_menu_filters_and_enter_starts_the_program() {
    use smithay::backend::input::KeyState;
    use smithay::input::keyboard::{FilterResult, Keycode};
    use smithay::utils::SERIAL_COUNTER;

    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): start menu test skipped");
            return;
        }
    };
    // two programs of a made-up home: starting one leaves a marker file
    let home = dir.path().join("home");
    let apps = home.join(".local/share/applications");
    std::fs::create_dir_all(&apps).unwrap();
    let marker_a = dir.path().join("started-alpha");
    let marker_z = dir.path().join("started-zzterm");
    std::fs::write(apps.join("a.desktop"), format!("[Desktop Entry]\nType=Application\nName=Alpha Test\nExec=touch {}\n", marker_a.display())).unwrap();
    std::fs::write(apps.join("z.desktop"), format!("[Desktop Entry]\nType=Application\nName=Zzterm Test\nExec=touch {}\n", marker_z.display())).unwrap();

    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let socket = ListeningSocket::bind("fawc-e2e-menu").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    let mut panel = std::process::Command::new(env!("CARGO_BIN_EXE_fawc-panel"))
        .env("WAYLAND_DISPLAY", "fawc-e2e-menu")
        .env("HOME", &home)
        .env("FAWC_PANEL_OPEN_MENU", "1")
        .env_remove("DISPLAY")
        .spawn()
        .unwrap();

    // wait for the menu to be up and to hold the keyboard
    let t0 = Instant::now();
    loop {
        hn.pump();
        hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
        let focused = hn.state.seat.get_keyboard().and_then(|k| k.current_focus()).is_some();
        if focused {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the start menu never took the keyboard");
        std::thread::sleep(Duration::from_millis(10));
    }
    hn.pump_for(300); // the keymap reaches the panel

    // z z t Enter  (evdev: z = 44, t = 20, Enter = 28)
    for c in [44u32, 44, 20, 28] {
        let kb = hn.state.seat.get_keyboard().unwrap();
        for st in [KeyState::Pressed, KeyState::Released] {
            kb.input::<(), _>(&mut hn.state, Keycode::new(c + 8), st, SERIAL_COUNTER.next_serial(), 0, |_, _, _| FilterResult::Forward);
        }
        hn.pump_for(80);
    }
    let t0 = Instant::now();
    while !marker_z.exists() && t0.elapsed() < Duration::from_secs(10) {
        hn.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = panel.kill();
    let _ = panel.wait();
    assert!(marker_z.exists(), "Enter after typing \"zzt\" starts the matching program");
    assert!(!marker_a.exists(), "and not the other one");
}

#[test]
fn a_new_window_fades_and_grows_in() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): animation test skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let state = Fawc::new(&display.handle(), (w, h).into());
    assert!(state.config.appearance.animations, "on by default");
    let socket = ListeningSocket::bind("fawc-e2e-anim").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    let (tx, rx) = mpsc::channel();
    let path = dir.path().join("fawc-e2e-anim");
    std::thread::spawn(move || client(path, tx, 200, 100, 0xff2030e0));
    let t0 = Instant::now();
    loop {
        hn.pump();
        if let Ok(Msg::Mapped { .. }) = rx.try_recv() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no window");
        std::thread::sleep(Duration::from_millis(3));
    }
    let green = |hn: &mut Harness, renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture| {
        hn.state.render_scene(renderer, &glass, scene).unwrap();
        let size = scene.size();
        let target = renderer.bind(scene).unwrap();
        let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
        let bytes = renderer.map_texture(&m).unwrap().to_vec();
        // the window is dark blue (green 0x30), the sky behind it is light (lots of green)
        pixel(&bytes, w, 175, 157)[1]
    };
    // the first frames: the window is still mostly the wallpaper behind it
    let win = hn.state.space.elements().next().cloned().unwrap();
    // hold the animation at its very start (rendering on a software GL is slow, so "just now" is not exact)
    hn.state.set_open_start(&win, Instant::now() + Duration::from_secs(60));
    let early = green(&mut hn, &mut renderer, &mut scene);
    // after the animation: the window in its colour
    hn.state.restart_open_animation(&win);
    std::thread::sleep(Duration::from_millis(400));
    hn.pump();
    let late = green(&mut hn, &mut renderer, &mut scene);
    assert_eq!(hn.state.open_progress(&win), 1.0, "the animation is over");
    assert!(early > late + 25, "the window faded in: green {early} then {late}");
}

/// A client window of `w` x `h` and the server side to look at it: `scenes` are the outputs' pictures.
fn open_window(hn: &mut Harness, dir: &std::path::Path, name: &str, w: i32, h: i32) {
    let (tx, rx) = mpsc::channel();
    let path = dir.join(name);
    std::thread::spawn(move || client(path, tx, w, h, 0xff2030e0));
    let t0 = Instant::now();
    loop {
        hn.pump();
        if let Ok(Msg::Mapped { .. }) = rx.try_recv() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no window");
        std::thread::sleep(Duration::from_millis(3));
    }
    hn.pump_for(100);
}

fn grab_scene(renderer: &mut smithay::backend::renderer::gles::GlesRenderer, scene: &mut GlesTexture) -> Vec<u8> {
    let size = scene.size();
    let target = renderer.bind(scene).unwrap();
    let m = renderer.copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888).unwrap();
    renderer.map_texture(&m).unwrap().to_vec()
}

#[test]
fn a_window_on_the_second_monitor_is_drawn_there_and_the_pointer_and_maximize_follow() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): multi-monitor test skipped");
            return;
        }
    };
    let (w, h) = (800, 600);
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (w, h).into());
    state.config.appearance.animations = false;
    let second = state.add_output("fawc-1", (800, 600), 60_000);
    let socket = ListeningSocket::bind("fawc-e2e-two").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene0: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();
    let mut scene1: GlesTexture = create_scene(&mut renderer, (w, h).into()).unwrap();

    open_window(&mut hn, dir.path(), "fawc-e2e-two", 200, 100);
    let win = hn.state.space.elements().next().cloned().unwrap();

    // the window is on the first monitor: drawn there, not on the second
    let primary = hn.state.output.clone();
    hn.state.render_output(&mut renderer, &glass, &mut scene0, &primary).unwrap();
    hn.state.render_output(&mut renderer, &glass, &mut scene1, &second).unwrap();
    let g = hn.state.space.element_geometry(&win).unwrap();
    let (cx, cy) = (g.loc.x + g.size.w / 2, g.loc.y + g.size.h / 2);
    assert!(pixel(&grab_scene(&mut renderer, &mut scene0), w, cx, cy)[1] < 90, "dark window on monitor 1");
    assert!(pixel(&grab_scene(&mut renderer, &mut scene1), w, cx, cy)[1] > 120, "monitor 2 shows only sky there");

    // move it onto the second monitor (x >= 800): now it shows there
    hn.state.space.map_element(win.clone(), (900, 200), false);
    hn.state.render_output(&mut renderer, &glass, &mut scene0, &primary).unwrap();
    hn.state.render_output(&mut renderer, &glass, &mut scene1, &second).unwrap();
    assert!(pixel(&grab_scene(&mut renderer, &mut scene1), w, 900 - 800 + 100, 200 + 50)[1] < 90, "dark window on monitor 2, at its local position");
    assert!(pixel(&grab_scene(&mut renderer, &mut scene0), w, 700, 250)[1] > 120, "and nothing of it on monitor 1");

    // the pointer crosses over and cannot leave the monitors
    hn.state.pointer_moved((1200.0, 100.0).into(), 0);
    assert_eq!(hn.state.pointer_location, (1200.0, 100.0).into());
    hn.state.pointer_moved((5000.0, 100.0).into(), 0);
    assert_eq!(hn.state.pointer_location, (1599.0, 100.0).into());

    // maximize: the window fills the monitor it is on, not the first
    hn.state.set_maximized(&win, true);
    hn.pump_for(50);
    let g = hn.state.space.element_geometry(&win).unwrap();
    let area = hn.state.usable_area(&second);
    assert!(g.loc.x >= 800 && g.loc.x + g.size.w <= 1600, "inside the second monitor: {g:?}");
    assert_eq!((g.loc.x, g.loc.y), (area.loc.x + 5, area.loc.y + 5 + 32), "at the top left of that monitor's usable area, inside its frame: {g:?} {area:?}");
}

#[test]
fn a_hidpi_monitor_draws_everything_at_double_size() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): HiDPI test skipped");
            return;
        }
    };
    let display: Display<Fawc> = Display::new().unwrap();
    // a 1600x1200 monitor at scale 2: a logical space of 800x600
    let mut state = Fawc::new(&display.handle(), (1600, 1200).into());
    state.config.appearance.animations = false;
    state.output.change_current_state(None, None, Some(smithay::output::Scale::Fractional(2.0)), None);
    assert_eq!(state.output_size(), (800, 600).into());
    let socket = ListeningSocket::bind("fawc-e2e-hidpi").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (1600, 1200).into()).unwrap();

    open_window(&mut hn, dir.path(), "fawc-e2e-hidpi", 200, 100);
    let win = hn.state.space.elements().next().cloned().unwrap();
    let g = hn.state.space.element_geometry(&win).unwrap(); // logical
    hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
    let px = grab_scene(&mut renderer, &mut scene);
    let dark = |x: i32, y: i32| pixel(&px, 1600, x, y)[1] < 90;
    // the 200x100 client is 400x200 pixels: dark up to its edge, sky beyond the glass border
    let (x0, y0) = (g.loc.x * 2, g.loc.y * 2);
    eprintln!("client geometry {g:?}; samples {:?} {:?}", pixel(&px, 1600, x0 + 200, y0 + 100), pixel(&px, 1600, x0 / 2 + 100, y0 / 2 + 50));
    assert!(dark(x0 + 200, y0 + 100), "the middle of the client");
    assert!(dark(x0 + 390, y0 + 100), "still the client 390 px to the right (it would be sky at 1x)");
    assert!(dark(x0 + 100, y0 + 190), "and 190 px down");
    assert!(!dark(x0 + 400 + 40, y0 + 100), "sky beyond the frame");
    assert!(!dark(x0 + 100, y0 + 200 + 40), "sky below the frame");
    // the title bar is double height too: its glass is at 2x32 above the client
    let title = pixel(&px, 1600, x0 + 200, y0 - 40);
    let sky = pixel(&px, 1600, x0 + 200, y0 - 120);
    assert!(title != sky, "the frame is drawn 40 px above the client");
}

#[test]
fn the_task_bar_draws_at_the_monitors_scale() {
    use smithay::wayland::compositor::with_states;

    let dir = tempfile::tempdir().unwrap();
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    let mut renderer = match offscreen_renderer() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("no GL context here ({e}): panel scale test skipped");
            return;
        }
    };
    let display: Display<Fawc> = Display::new().unwrap();
    let mut state = Fawc::new(&display.handle(), (1600, 1200).into());
    state.config.appearance.animations = false;
    state.output.change_current_state(None, None, Some(smithay::output::Scale::Fractional(2.0)), None);
    let socket = ListeningSocket::bind("fawc-e2e-pscale").unwrap();
    let mut hn = Harness { display, state, socket };
    let glass = Glass::new(&mut renderer).unwrap();
    let mut scene: GlesTexture = create_scene(&mut renderer, (1600, 1200).into()).unwrap();

    let mut panel = std::process::Command::new(env!("CARGO_BIN_EXE_fawc-panel"))
        .env("WAYLAND_DISPLAY", "fawc-e2e-pscale")
        .env_remove("DISPLAY")
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    let mut result = None;
    while t0.elapsed() < Duration::from_secs(10) {
        hn.pump();
        hn.state.render_scene(&mut renderer, &glass, &mut scene).unwrap();
        let map = smithay::desktop::layer_map_for_output(&hn.state.output);
        if let Some(layer) = map.layers().next() {
            let bs = with_states(layer.wl_surface(), |s| {
                s.data_map
                    .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>()
                    .map(|d| d.lock().unwrap().buffer_scale())
            });
            if bs == Some(2) {
                result = bs;
                break;
            }
        }
        drop(map);
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = panel.kill();
    let _ = panel.wait();
    assert_eq!(result, Some(2), "the panel's buffer has scale 2 on a scale-2 monitor");
}
