//! End to end: a real Wayland client talks to the compositor, which draws the result offscreen on
//! software GL; the test looks at the pixels and clicks the window's buttons.

use std::io::Write;
use std::os::unix::net::UnixStream;
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
    let state = Fawc::new(&display.handle(), (w, h).into());
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
