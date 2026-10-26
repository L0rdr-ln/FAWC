//! The lock screen: asks the compositor to lock the session (ext-session-lock-v1), shows a glass
//! login card on every output and unlocks when the user's password is typed.

use std::io::{Read, Write};
use std::os::fd::AsFd;

use fde::lockscreen;
use fde::text::TextRenderer;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::session_lock::v1::client::{ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1};
use xkbcommon::xkb;

struct Output {
    surface: wl_surface::WlSurface,
    lock_surface: ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
    size: (i32, i32),
    scale: i32,
    /// the wallpaper at this monitor's size and scale, made once
    background: Option<((i32, i32), Vec<u8>)>,
    file: Option<std::fs::File>,
}

struct App {
    outputs: Vec<Output>,
    locked: bool,
    finished: bool,
    user: String,
    typed: String,
    message: String,
    redraw: bool,
    unlock: bool,
    xkb_state: Option<xkb::State>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);
/// The user data is the index of the output in `App::outputs`.
impl Dispatch<wl_output::WlOutput, usize> for App {
    fn event(app: &mut Self, _: &wl_output::WlOutput, event: wl_output::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Scale { factor } = event {
            if let Some(o) = app.outputs.get_mut(*idx) {
                o.scale = factor.max(1);
                app.redraw = true;
            }
        }
    }
}
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore ext_session_lock_manager_v1::ExtSessionLockManagerV1);

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for App {
    fn event(app: &mut Self, _: &ext_session_lock_v1::ExtSessionLockV1, event: ext_session_lock_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_session_lock_v1::Event::Locked => app.locked = true,
            ext_session_lock_v1::Event::Finished => app.finished = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, usize> for App {
    fn event(app: &mut Self, s: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, event: ext_session_lock_surface_v1::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let ext_session_lock_surface_v1::Event::Configure { serial, width, height } = event {
            s.ack_configure(serial);
            app.outputs[*i].size = (width as i32, height as i32);
            app.redraw = true;
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(_: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(app: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_keyboard::Event::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } => {
                let mut text = String::new();
                let mut file = std::fs::File::from(fd);
                let _ = std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0));
                let mut bytes = vec![0u8; size as usize];
                if file.read_exact(&mut bytes).is_ok() {
                    text = String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string();
                }
                let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                if let Some(keymap) = xkb::Keymap::new_from_string(&ctx, text, xkb::KEYMAP_FORMAT_TEXT_V1, xkb::KEYMAP_COMPILE_NO_FLAGS) {
                    app.xkb_state = Some(xkb::State::new(&keymap));
                }
            }
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => {
                if let Some(st) = &mut app.xkb_state {
                    st.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            wl_keyboard::Event::Key { key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                let Some(st) = &app.xkb_state else { return };
                let code = xkb::Keycode::new(key + 8);
                let sym = st.key_get_one_sym(code);
                if sym == xkb::Keysym::Return || sym == xkb::Keysym::KP_Enter {
                    app.unlock = true;
                } else if sym == xkb::Keysym::BackSpace {
                    app.typed.pop();
                    app.message.clear();
                    app.redraw = true;
                } else if sym == xkb::Keysym::Escape {
                    app.typed.clear();
                    app.message.clear();
                    app.redraw = true;
                } else {
                    let s = st.key_get_utf8(code);
                    if !s.is_empty() && !s.chars().any(|c| c.is_control()) {
                        app.typed.push_str(&s);
                        app.message.clear();
                        app.redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}

fn tempfile_in_runtime_dir() -> std::io::Result<std::fs::File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{dir}/fde-lock-{}", std::process::id());
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(f)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let manager: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=5, ())?;

    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_else(|_| "user".into());
    let mut app = App {
        outputs: Vec::new(),
        locked: false,
        finished: false,
        user: user.clone(),
        typed: String::new(),
        message: String::new(),
        redraw: false,
        unlock: false,
        xkb_state: None,
    };

    let lock = manager.lock(&qh, ());
    // one lock surface per output
    for (name, interface, version) in globals.contents().clone_list().iter().map(|g| (g.name, g.interface.clone(), g.version)) {
        if interface == "wl_output" {
            let output: wl_output::WlOutput = globals.registry().bind(name, version.min(4), &qh, app.outputs.len());
            let surface = compositor.create_surface(&qh, ());
            let idx = app.outputs.len();
            let lock_surface = lock.get_lock_surface(&surface, &output, &qh, idx);
            app.outputs.push(Output { surface, lock_surface, size: (0, 0), scale: 1, background: None, file: None });
        }
    }
    if app.outputs.is_empty() {
        eprintln!("fde-lock: no outputs");
        lock.destroy();
        return Ok(());
    }

    // the same wallpaper as the desktop
    let config = fde::config::Config::load().unwrap_or_default();
    let (picture, mode) = (fde::wallpaper::configured(&config.wallpaper.path), config.wallpaper.mode.clone());
    let mut text = TextRenderer::default();
    while !app.finished {
        if app.unlock {
            app.unlock = false;
            if lockscreen::check_password(&user, &app.typed) {
                lock.unlock_and_destroy();
                conn.flush()?;
                queue.roundtrip(&mut app)?;
                return Ok(());
            }
            app.typed.clear();
            app.message = "Wrong password, try again".into();
            app.redraw = true;
        }
        if app.redraw {
            app.redraw = false;
            for o in &mut app.outputs {
                let (lw, lh) = o.size;
                if lw <= 0 || lh <= 0 {
                    continue;
                }
                let s = o.scale;
                let (w, h) = (lw * s, lh * s);
                if o.background.as_ref().map(|(sz, _)| *sz != (w, h)).unwrap_or(true) {
                    o.background = Some(((w, h), lockscreen::background(picture.as_ref(), &mode, w, h)));
                }
                let bg = &o.background.as_ref().expect("background").1;
                let mut px = lockscreen::draw(&mut text, bg, w, h, s, &app.user, app.typed.chars().count(), &app.message);
                for p in px.chunks_exact_mut(4) {
                    p.swap(0, 2);
                }
                let mut f = match o.file.take() {
                    Some(f) => f,
                    None => tempfile_in_runtime_dir()?,
                };
                f.set_len(px.len() as u64)?;
                std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0))?;
                f.write_all(&px)?;
                let pool = shm.create_pool(f.as_fd(), px.len() as i32, &qh, ());
                let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &qh, ());
                o.surface.set_buffer_scale(s);
                o.surface.attach(Some(&buffer), 0, 0);
                o.surface.damage_buffer(0, 0, w, h);
                o.surface.commit();
                pool.destroy();
                o.file = Some(f);
            }
        }
        queue.blocking_dispatch(&mut app)?;
    }
    eprintln!("fde-lock: the compositor refused or ended the lock");
    let _ = &app.outputs[0].lock_surface;
    Ok(())
}
