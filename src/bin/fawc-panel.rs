//! The task bar: a glass strip along the bottom of the screen with the start orb and a clock.
//! A plain layer-shell client; it reserves its height so windows stop above it.
//! Click the orb to start $FAWC_LAUNCHER (default: $FAWC_TERMINAL, else foot).

use std::io::Write;
use std::os::fd::AsFd;
use std::process::Command;
use std::time::{Duration, Instant};

use fawc::panel;
use fawc::text::TextRenderer;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const BTN_LEFT: u32 = 0x110;

#[derive(Default)]
struct App {
    configured: bool,
    closed: bool,
    size: (i32, i32),
    pointer: (f64, f64),
    over_panel: bool,
    clicked_orb: bool,
    hover_changed: bool,
    hover: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(_: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(app: &mut Self, _: &wl_pointer::WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_pointer::Event::Enter { surface_x, surface_y, .. } | wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                app.pointer = (surface_x, surface_y);
                app.over_panel = true;
                let hover = panel::on_orb(surface_x, surface_y, app.size.1);
                if hover != app.hover {
                    app.hover = hover;
                    app.hover_changed = true;
                }
            }
            wl_pointer::Event::Leave { .. } => {
                app.over_panel = false;
                if app.hover {
                    app.hover = false;
                    app.hover_changed = true;
                }
            }
            wl_pointer::Event::Button { button, state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. }
                if button == BTN_LEFT && panel::on_orb(app.pointer.0, app.pointer.1, app.size.1) =>
            {
                app.clicked_orb = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for App {
    fn event(app: &mut Self, s: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                s.ack_configure(serial);
                app.size = (width as i32, height as i32);
                app.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => app.closed = true,
            _ => {}
        }
    }
}

fn clock() -> String {
    Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn launch() {
    let cmd = std::env::var("FAWC_LAUNCHER")
        .or_else(|_| std::env::var("FAWC_TERMINAL"))
        .unwrap_or_else(|_| "foot".into());
    if let Err(e) = Command::new(&cmd).spawn() {
        eprintln!("fawc-panel: cannot start {cmd}: {e}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ())?;
    let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=5, ())?;

    let mut app = App::default();
    let surface = compositor.create_surface(&qh, ());
    let ls = shell.get_layer_surface(&surface, None, zwlr_layer_shell_v1::Layer::Top, "panel".into(), &qh, ());
    ls.set_size(0, panel::HEIGHT as u32);
    ls.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Left | zwlr_layer_surface_v1::Anchor::Right);
    ls.set_exclusive_zone(panel::HEIGHT);
    surface.commit();
    while !app.configured {
        queue.blocking_dispatch(&mut app)?;
    }

    let mut text = TextRenderer::default();
    let mut shown = String::new();
    let mut shown_hover = false;
    let mut last_clock = Instant::now() - Duration::from_secs(60);
    let mut current_clock = String::new();
    let mut size = (0, 0);
    let mut dirty = true;
    let mut file: Option<std::fs::File> = None;

    while !app.closed {
        if last_clock.elapsed() >= Duration::from_secs(10) {
            current_clock = clock();
            last_clock = Instant::now();
        }
        if app.size != size || app.hover != shown_hover || current_clock != shown {
            dirty = true;
        }
        if dirty {
            size = app.size;
            let (w, h) = size;
            let rgba = panel::draw(&mut text, w, h, app.hover, &current_clock);
            // wl_shm ARGB8888 is B, G, R, A in memory
            let mut px = rgba;
            for p in px.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let f = match file.take() {
                Some(f) => f,
                None => tempfile_in_runtime_dir()?,
            };
            f.set_len(px.len() as u64)?;
            let mut f = f;
            std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0))?;
            f.write_all(&px)?;
            let pool = shm.create_pool(f.as_fd(), px.len() as i32, &qh, ());
            let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ());
            surface.attach(Some(&buffer), 0, 0);
            surface.damage_buffer(0, 0, w, h);
            surface.commit();
            pool.destroy();
            file = Some(f);
            shown = current_clock.clone();
            shown_hover = app.hover;
            dirty = false;
        }
        if app.clicked_orb {
            app.clicked_orb = false;
            launch();
        }
        // wait for events, but wake up for the clock
        queue.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let mut fds = [libc::pollfd { fd: guard.connection_fd().as_raw_fd_(), events: libc::POLLIN, revents: 0 }];
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 5000) };
            if n > 0 {
                let _ = guard.read();
            }
        }
        queue.dispatch_pending(&mut app)?;
        // a buffer is single use: the server releases it when done; we keep it simple and leak none
    }
    Ok(())
}

fn tempfile_in_runtime_dir() -> std::io::Result<std::fs::File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/fawc-panel-{}", std::process::id());
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(f)
}

trait AsRawFdExt {
    fn as_raw_fd_(&self) -> i32;
}
impl<T: std::os::fd::AsFd> AsRawFdExt for T {
    fn as_raw_fd_(&self) -> i32 {
        std::os::fd::AsRawFd::as_raw_fd(&self.as_fd())
    }
}
