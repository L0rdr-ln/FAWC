//! fde-shot: take a screenshot of a monitor with wlr-screencopy and save it as a PNG.
//!
//!     fde-shot [--output NAME] [FILE]
//!
//! Without FILE the picture goes to `~/Pictures/Screenshot_YYYY-MM-DD_HH-MM-SS.png`
//! (`$FDE_SHOT_DIR` replaces `~/Pictures`). The compositor starts it for the Print key.

use std::io::Write;
use std::os::fd::AsFd;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols_wlr::screencopy::v1::client::{zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1};

#[derive(Default)]
struct App {
    outputs: Vec<(wl_output::WlOutput, String)>,
    buffer: Option<(u32, u32, u32, u32)>,
    buffer_done: bool,
    ready: bool,
    failed: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(app: &mut Self, output: &wl_output::WlOutput, event: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = event {
            if let Some(o) = app.outputs.iter_mut().find(|(o, _)| o == output) {
                o.1 = name;
            }
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for App {
    fn event(app: &mut Self, _: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, event: zwlr_screencopy_frame_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer { format, width, height, stride } => {
                if matches!(format, WEnum::Value(wl_shm::Format::Xrgb8888) | WEnum::Value(wl_shm::Format::Argb8888)) {
                    app.buffer = Some((width, height, stride, 0));
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => app.buffer_done = true,
            zwlr_screencopy_frame_v1::Event::Ready { .. } => app.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => app.failed = true,
            _ => {}
        }
    }
}

fn timestamp() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

fn notify(summary: &str, body: &str) {
    use std::collections::HashMap;
    let Ok(conn) = zbus::blocking::Connection::session() else { return };
    let hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
    let _ = conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("fde-shot", 0u32, "", summary, body, Vec::<&str>::new(), hints, 4000i32),
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut wanted: Option<String> = None;
    let mut file: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--output" => wanted = args.next(),
            "-h" | "--help" => {
                println!("usage: fde-shot [--output NAME] [FILE.png]");
                return Ok(());
            }
            other => file = Some(other.into()),
        }
    }

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let manager: zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1 =
        globals.bind(&qh, 1..=3, ()).map_err(|_| "this compositor has no screen capture (wlr-screencopy)")?;
    let mut app = App::default();
    for g in globals.contents().clone_list() {
        if g.interface == "wl_output" {
            let o: wl_output::WlOutput = globals.registry().bind(g.name, g.version.min(4), &qh, ());
            app.outputs.push((o, String::new()));
        }
    }
    queue.roundtrip(&mut app)?;
    let output = match &wanted {
        Some(n) => app.outputs.iter().find(|(_, name)| name == n).map(|(o, _)| o.clone()),
        None => app.outputs.first().map(|(o, _)| o.clone()),
    }
    .ok_or("no such monitor")?;

    let frame = manager.capture_output(0, &output, &qh, ());
    while !app.buffer_done && !app.failed {
        queue.blocking_dispatch(&mut app)?;
    }
    let (w, h, stride, _) = app.buffer.ok_or("the compositor offered no usable buffer")?;
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let tmp = format!("{dir}/fde-shot-{}", std::process::id());
    let mut shm_file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    shm_file.set_len((stride * h) as u64)?;
    let pool = shm.create_pool(shm_file.as_fd(), (stride * h) as i32, &qh, ());
    let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, wl_shm::Format::Xrgb8888, &qh, ());
    frame.copy(&buffer);
    while !app.ready && !app.failed {
        queue.blocking_dispatch(&mut app)?;
    }
    if app.failed {
        notify("Screenshot failed", "the compositor could not copy the screen");
        return Err("the compositor could not copy the screen".into());
    }
    // B, G, R, X in memory -> R, G, B, A
    use std::io::{Read, Seek};
    let mut raw = vec![0u8; (stride * h) as usize];
    shm_file.rewind()?;
    shm_file.read_exact(&mut raw)?;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * stride as usize + x * 4;
            rgba.extend_from_slice(&[raw[i + 2], raw[i + 1], raw[i], 255]);
        }
    }
    let path = match file {
        Some(p) => p,
        None => {
            let dir = std::env::var_os("FDE_SHOT_DIR")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Pictures")))
                .ok_or("no HOME")?;
            std::fs::create_dir_all(&dir)?;
            dir.join(format!("Screenshot_{}.png", timestamp()))
        }
    };
    let out = std::fs::File::create(&path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(out), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&rgba)?;
    writer.finish()?;
    println!("{}", path.display());
    notify("Screenshot saved", &path.display().to_string());
    std::io::stdout().flush()?;
    Ok(())
}
