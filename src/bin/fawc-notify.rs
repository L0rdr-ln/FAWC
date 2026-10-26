//! The notification daemon: owns `org.freedesktop.Notifications` on the session bus and shows
//! the notifications as glass bubbles in the top right corner (a layer-shell client).
//! Click a bubble to dismiss it.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fawc::notify::{self, Store};
use fawc::text::TextRenderer;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::background_effect::v1::client::{ext_background_effect_manager_v1, ext_background_effect_surface_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const BTN_LEFT: u32 = 0x110;
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

/// Shared between the D-Bus thread and the drawing loop.
#[derive(Default)]
struct Shared {
    store: Store,
    /// (id, reason) of notes that went away and still need the NotificationClosed signal
    closed: Vec<(u32, u32)>,
}

struct Server {
    shared: Arc<Mutex<Shared>>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "body-markup".into()]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        hints: HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let critical = matches!(hints.get("urgency").map(|v| &**v), Some(zbus::zvariant::Value::U8(2)));
        self.shared.lock().unwrap().store.notify(&app_name, replaces_id, &summary, &body, expire_timeout, critical, Instant::now())
    }

    fn close_notification(&self, id: u32) {
        let mut s = self.shared.lock().unwrap();
        if s.store.close(id) {
            s.closed.push((id, 3));
        }
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("fawc-notify".into(), "FAWC".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }
}

// ------------------------------------------------------------------ the Wayland side

#[derive(Default)]
struct App {
    generation: u32,
    configured: bool,
    pointer_y: f64,
    click: Option<f64>,
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
delegate_noop!(App: ignore ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);
delegate_noop!(App: ignore wayland_client::protocol::wl_region::WlRegion);

impl Dispatch<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, ()> for App {
    fn event(_: &mut Self, _: &ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _: ext_background_effect_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for App {
    fn event(app: &mut Self, s: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, gen: &u32, _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            s.ack_configure(serial);
            if *gen == app.generation {
                app.configured = true;
            }
        }
    }
}

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
            wl_pointer::Event::Enter { surface_y, .. } | wl_pointer::Event::Motion { surface_y, .. } => app.pointer_y = surface_y,
            wl_pointer::Event::Button { button: BTN_LEFT, state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. } => {
                app.click = Some(app.pointer_y);
            }
            _ => {}
        }
    }
}

struct Popup {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    file: Option<std::fs::File>,
    count: usize,
    drawn: Vec<(u32, String, String)>,
}

fn tempfile_in_runtime_dir() -> std::io::Result<std::fs::File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/fawc-notify-{}", std::process::id());
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(f)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let bus = zbus::blocking::connection::Builder::session()?
        .name(IFACE)?
        .serve_at(PATH, Server { shared: shared.clone() })?
        .build()?;

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ())?;
    let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=5, ())?;
    let blur: Option<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();

    let mut app = App::default();
    let mut text = TextRenderer::default();
    let mut popup: Option<Popup> = None;

    loop {
        // expire, and tell the bus about everything that went away
        let (shown, closed) = {
            let mut s = shared.lock().unwrap();
            for id in s.store.expire(Instant::now()) {
                s.closed.push((id, 1));
            }
            if let Some(y) = app.click.take() {
                let n = s.store.shown().len();
                if let Some(row) = notify::row_at(y, n) {
                    let id = s.store.shown()[row].id;
                    s.store.close(id);
                    s.closed.push((id, 2));
                }
            }
            let shown: Vec<notify::Note> = s.store.shown().into_iter().cloned().collect();
            (shown, std::mem::take(&mut s.closed))
        };
        for (id, reason) in closed {
            let _ = bus.emit_signal(None::<&str>, PATH, IFACE, "NotificationClosed", &(id, reason));
        }

        let key: Vec<(u32, String, String)> = shown.iter().map(|n| (n.id, n.summary.clone(), n.body.clone())).collect();
        if shown.is_empty() {
            if let Some(p) = popup.take() {
                p.layer.destroy();
                p.surface.destroy();
            }
        } else {
            if popup.as_ref().map(|p| p.count != shown.len()).unwrap_or(true) {
                // a different number of bubbles is a different size: make a new surface
                if let Some(p) = popup.take() {
                    p.layer.destroy();
                    p.surface.destroy();
                }
                app.generation += 1;
                app.configured = false;
                let (w, h) = notify::size(shown.len());
                let surface = compositor.create_surface(&qh, ());
                let layer = shell.get_layer_surface(&surface, None, zwlr_layer_shell_v1::Layer::Overlay, "notifications".into(), &qh, app.generation);
                layer.set_size(w as u32, h as u32);
                layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Right);
                layer.set_margin(12, 12, 0, 0);
                layer.set_exclusive_zone(0);
                if let Some(b) = &blur {
                    let effect = b.get_background_effect(&surface, &qh, ());
                    let region = compositor.create_region(&qh, ());
                    region.add(0, 0, w, h);
                    effect.set_blur_region(Some(&region));
                    region.destroy();
                }
                surface.commit();
                popup = Some(Popup { surface, layer, file: None, count: shown.len(), drawn: Vec::new() });
            }
            if let Some(p) = &mut popup {
                if app.configured && p.drawn != key {
                    let refs: Vec<&notify::Note> = shown.iter().collect();
                    let mut px = notify::draw(&mut text, &refs);
                    let (w, h) = notify::size(shown.len());
                    for c in px.chunks_exact_mut(4) {
                        c.swap(0, 2); // wl_shm ARGB8888 is B, G, R, A in memory
                    }
                    let mut f = match p.file.take() {
                        Some(f) => f,
                        None => tempfile_in_runtime_dir()?,
                    };
                    f.set_len(px.len() as u64)?;
                    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0))?;
                    f.write_all(&px)?;
                    let pool = shm.create_pool(f.as_fd(), px.len() as i32, &qh, ());
                    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ());
                    p.surface.attach(Some(&buffer), 0, 0);
                    p.surface.damage_buffer(0, 0, w, h);
                    p.surface.commit();
                    pool.destroy();
                    p.file = Some(f);
                    p.drawn = key;
                }
            }
        }

        // wait for Wayland events, but look at the D-Bus side a few times a second
        queue.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let mut fds = [libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd()), events: libc::POLLIN, revents: 0 }];
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 150) };
            if n > 0 {
                let _ = guard.read();
            }
        }
        queue.dispatch_pending(&mut app)?;
        let _ = Duration::from_millis(0);
    }
}
