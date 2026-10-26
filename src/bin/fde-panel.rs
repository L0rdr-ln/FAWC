//! The task bar: a glass strip along the bottom of the screen with the start orb and a clock.
//! A plain layer-shell client; it reserves its height so windows stop above it.
//! Click the orb to start $FDE_LAUNCHER (default: $FDE_TERMINAL, else foot).

use std::io::Write;
use std::os::fd::AsFd;
use std::process::Command;
use std::time::{Duration, Instant};

use fde::panel;
use fde::text::TextRenderer;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::background_effect::v1::client::{ext_background_effect_manager_v1, ext_background_effect_surface_v1};
use xkbcommon::xkb;
use wayland_protocols_wlr::foreign_toplevel::v1::client::{zwlr_foreign_toplevel_handle_v1 as handle_mod, zwlr_foreign_toplevel_manager_v1 as manager_mod};
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
    seat: Option<wl_seat::WlSeat>,
    toplevels: Vec<Toplevel>,
    tasks_changed: bool,
    click_task: Option<usize>,
    // the start menu
    menu_surface: Option<wl_surface::WlSurface>,
    pointer_on_menu: bool,
    menu_hover: Option<usize>,
    menu_click: Option<usize>,
    power_hover: Option<usize>,
    power_armed: Option<usize>,
    power_click: Option<usize>,
    menu_configured: bool,
    menu_dirty: bool,
    toggle_menu: bool,
    close_menu: bool,
    menu_entries: usize,
    scale: fde::clientscale::ScaleTracker,
    output_names: std::collections::HashMap<wayland_client::backend::ObjectId, String>,
    // typing in the start menu
    query: String,
    query_changed: bool,
    menu_move: i32,
    menu_enter: bool,
    xkb_state: Option<xkb::State>,
}

struct Toplevel {
    handle: handle_mod::ZwlrForeignToplevelHandleV1,
    task: panel::Task,
    /// what arrives before `done`
    next: panel::Task,
}

impl Dispatch<manager_mod::ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(app: &mut Self, _: &manager_mod::ZwlrForeignToplevelManagerV1, event: manager_mod::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let manager_mod::Event::Toplevel { toplevel } = event {
            app.toplevels.push(Toplevel { handle: toplevel, task: Default::default(), next: Default::default() });
        }
    }
    wayland_client::event_created_child!(App, manager_mod::ZwlrForeignToplevelManagerV1, [
        manager_mod::EVT_TOPLEVEL_OPCODE => (handle_mod::ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<handle_mod::ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(app: &mut Self, handle: &handle_mod::ZwlrForeignToplevelHandleV1, event: handle_mod::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let Some(i) = app.toplevels.iter().position(|t| &t.handle == handle) else { return };
        match event {
            handle_mod::Event::Title { title } => app.toplevels[i].next.title = title,
            handle_mod::Event::State { state } => {
                let t = &mut app.toplevels[i].next;
                t.active = false;
                t.minimized = false;
                for c in state.chunks_exact(4) {
                    match u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) {
                        x if x == handle_mod::State::Minimized as u32 => t.minimized = true,
                        x if x == handle_mod::State::Activated as u32 => t.active = true,
                        _ => {}
                    }
                }
            }
            handle_mod::Event::Done => {
                let t = &mut app.toplevels[i];
                if t.task != t.next {
                    t.task = t.next.clone();
                    app.tasks_changed = true;
                }
            }
            handle_mod::Event::Closed => {
                app.toplevels.remove(i);
                app.tasks_changed = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(App: ignore wl_compositor::WlCompositor);
impl Dispatch<wl_surface::WlSurface, ()> for App {
    fn event(app: &mut Self, _: &wl_surface::WlSurface, event: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_surface::Event::Enter { output } => app.scale.enter(output.id()),
            wl_surface::Event::Leave { output } => app.scale.leave(&output.id()),
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(app: &mut Self, output: &wl_output::WlOutput, event: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_output::Event::Scale { factor } => app.scale.set_output_scale(output.id(), factor),
            wl_output::Event::Name { name } => {
                app.output_names.insert(output.id(), name);
            }
            _ => {}
        }
    }
}
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(App: ignore ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);
delegate_noop!(App: ignore wayland_client::protocol::wl_region::WlRegion);

impl Dispatch<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, ()> for App {
    fn event(_: &mut Self, _: &ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _: ext_background_effect_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(_: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(app: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        use std::io::Read;
        match event {
            wl_keyboard::Event::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } => {
                let mut file = std::fs::File::from(fd);
                let _ = std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0));
                let mut bytes = vec![0u8; size as usize];
                if file.read_exact(&mut bytes).is_ok() {
                    let text = String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string();
                    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                    if let Some(keymap) = xkb::Keymap::new_from_string(&ctx, text, xkb::KEYMAP_FORMAT_TEXT_V1, xkb::KEYMAP_COMPILE_NO_FLAGS) {
                        app.xkb_state = Some(xkb::State::new(&keymap));
                    }
                }
            }
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => {
                if let Some(st) = &mut app.xkb_state {
                    st.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            wl_keyboard::Event::Key { key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                // only the start menu takes the keyboard
                let Some(st) = &app.xkb_state else {
                    if key == 1 {
                        app.close_menu = true;
                    }
                    return;
                };
                let code = xkb::Keycode::new(key + 8);
                let sym = st.key_get_one_sym(code);
                if sym == xkb::Keysym::Escape {
                    if app.query.is_empty() {
                        app.close_menu = true;
                    } else {
                        app.query.clear();
                        app.query_changed = true;
                    }
                } else if sym == xkb::Keysym::Return || sym == xkb::Keysym::KP_Enter {
                    app.menu_enter = true;
                } else if sym == xkb::Keysym::Down {
                    app.menu_move = 1;
                } else if sym == xkb::Keysym::Up {
                    app.menu_move = -1;
                } else if sym == xkb::Keysym::BackSpace {
                    app.query.pop();
                    app.query_changed = true;
                } else {
                    let s = st.key_get_utf8(code);
                    if !s.is_empty() && !s.chars().any(|c| c.is_control()) {
                        app.query.push_str(&s);
                        app.query_changed = true;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(app: &mut Self, _: &wl_pointer::WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let n_menu = app.menu_entries;
        match event {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                app.pointer_on_menu = app.menu_surface.as_ref() == Some(&surface);
                app.pointer = (surface_x, surface_y);
                pointer_moved(app, n_menu);
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                app.pointer = (surface_x, surface_y);
                pointer_moved(app, n_menu);
            }
            wl_pointer::Event::Leave { .. } => {
                if app.pointer_on_menu {
                    app.close_menu = true;
                }
                app.pointer_on_menu = false;
                app.menu_hover = None;
                if app.hover {
                    app.hover = false;
                    app.hover_changed = true;
                }
            }
            wl_pointer::Event::Button { button, state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. } if button == BTN_LEFT => {
                if app.pointer_on_menu {
                    app.power_click = panel::power_at(app.pointer.0, app.pointer.1);
                    app.menu_click = if app.power_click.is_some() { None } else { panel::menu_row_at(app.pointer.1, n_menu) };
                } else if panel::on_orb(app.pointer.0, app.pointer.1, app.size.1) {
                    app.clicked_orb = true;
                } else {
                    app.click_task = panel::task_at(app.pointer.0, app.pointer.1, app.size.0, app.size.1, app.toplevels.len());
                }
            }
            _ => {}
        }
    }
}

fn pointer_moved(app: &mut App, n_menu: usize) {
    if app.pointer_on_menu {
        let power = panel::power_at(app.pointer.0, app.pointer.1);
        let row = if power.is_some() { None } else { panel::menu_row_at(app.pointer.1, n_menu) };
        if row != app.menu_hover || power != app.power_hover {
            app.menu_hover = row;
            app.power_hover = power;
            // moving away from a button that waited for its second click cancels it
            if app.power_armed.is_some() && app.power_armed != power {
                app.power_armed = None;
            }
            app.menu_dirty = true;
        }
    } else {
        let hover = panel::on_orb(app.pointer.0, app.pointer.1, app.size.1);
        if hover != app.hover {
            app.hover = hover;
            app.hover_changed = true;
        }
    }
}

/// The user data tells the bar (0) from the start menu (1).
impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u8> for App {
    fn event(app: &mut Self, s: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, which: &u8, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                s.ack_configure(serial);
                if *which == 0 {
                    app.size = (width as i32, height as i32);
                    app.configured = true;
                } else {
                    app.menu_configured = true;
                    app.menu_dirty = true;
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if *which == 0 {
                    app.closed = true;
                } else {
                    app.close_menu = true;
                }
            }
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
    let config = fde::config::Config::load().unwrap_or_default();
    let cmd = std::env::var("FDE_LAUNCHER")
        .or_else(|_| std::env::var("FDE_TERMINAL"))
        .unwrap_or_else(|_| if config.shell.launcher.is_empty() { config.shell.terminal.clone() } else { config.shell.launcher.clone() });
    if let Err(e) = Command::new(&cmd).spawn() {
        eprintln!("fde-panel: cannot start {cmd}: {e}");
    }
}

fn launch_exec(exec: &str) {
    if let Err(e) = Command::new("sh").arg("-c").arg(format!("exec {exec}")).spawn() {
        eprintln!("fde-panel: cannot start {exec}: {e}");
    }
}

/// The start menu's pieces while it is open.
struct Menu {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    file: Option<std::fs::File>,
}

fn attach(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<App>,
    surface: &wl_surface::WlSurface,
    file: &mut Option<std::fs::File>,
    mut px: Vec<u8>,
    (w, h): (i32, i32),
    scale: i32,
) -> std::io::Result<()> {
    // wl_shm ARGB8888 is B, G, R, A in memory
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let mut f = match file.take() {
        Some(f) => f,
        None => tempfile_in_runtime_dir()?,
    };
    f.set_len(px.len() as u64)?;
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0))?;
    f.write_all(&px)?;
    let pool = shm.create_pool(f.as_fd(), px.len() as i32, qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
    surface.set_buffer_scale(scale);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();
    pool.destroy();
    *file = Some(f);
    Ok(())
}

fn open_menu(
    compositor: &wl_compositor::WlCompositor,
    shell: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
    qh: &QueueHandle<App>,
    blur: &Option<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1>,
    output: Option<&wl_output::WlOutput>,
) -> Menu {
    let (w, h) = panel::menu_size();
    let surface = compositor.create_surface(qh, ());
    let layer = shell.get_layer_surface(&surface, output, zwlr_layer_shell_v1::Layer::Overlay, "start-menu".into(), qh, 1u8);
    layer.set_size(w as u32, h as u32);
    layer.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Left);
    layer.set_margin(0, 0, panel::HEIGHT + 4, 6);
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
    if let Some(b) = blur {
        let effect = b.get_background_effect(&surface, qh, ());
        let region = compositor.create_region(qh, ());
        region.add(0, 0, w, h);
        effect.set_blur_region(Some(&region));
        region.destroy();
    }
    surface.commit();
    Menu { surface, layer, file: None }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=4, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=5, ())?;
    // the window list, if the compositor offers it
    let _manager: Option<manager_mod::ZwlrForeignToplevelManagerV1> = globals.bind(&qh, 1..=3, ()).ok();

    let mut app = App { seat: Some(seat), ..App::default() };
    // the outputs tell us their scale
    let _outputs: Vec<wl_output::WlOutput> = globals
        .contents()
        .clone_list()
        .iter()
        .filter(|g| g.interface == "wl_output")
        .map(|g| globals.registry().bind(g.name, g.version.min(4), &qh, ()))
        .collect();
    queue.roundtrip(&mut app)?;
    // the monitor this task bar belongs to (the compositor starts one per monitor)
    let my_output: Option<wl_output::WlOutput> = std::env::var("FDE_PANEL_OUTPUT")
        .ok()
        .and_then(|name| _outputs.iter().find(|o| app.output_names.get(&o.id()) == Some(&name)).cloned());
    let surface = compositor.create_surface(&qh, ());
    let ls = shell.get_layer_surface(&surface, my_output.as_ref(), zwlr_layer_shell_v1::Layer::Top, "panel".into(), &qh, 0u8);
    ls.set_size(0, panel::HEIGHT as u32);
    ls.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom | zwlr_layer_surface_v1::Anchor::Left | zwlr_layer_surface_v1::Anchor::Right);
    ls.set_exclusive_zone(panel::HEIGHT);
    // ask for blur behind the whole bar, if the compositor offers it
    let blur_manager: Option<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let blur = blur_manager.as_ref().map(|m| m.get_background_effect(&surface, &qh, ()));
    surface.commit();
    while !app.configured {
        queue.blocking_dispatch(&mut app)?;
    }

    if let Some(b) = &blur {
        let region = compositor.create_region(&qh, ());
        region.add(0, 0, app.size.0, app.size.1);
        b.set_blur_region(Some(&region));
        region.destroy();
    }
    let mut text = TextRenderer::default();
    let mut shown = String::new();
    let mut shown_hover = false;
    let mut shown_ws: Option<(usize, usize)> = None;
    let mut shown_scale = 1;
    let mut last_clock = Instant::now() - Duration::from_secs(60);
    let mut current_clock = String::new();
    let mut size = (0, 0);
    let mut dirty = true;
    let mut file: Option<std::fs::File> = None;
    let mut menu: Option<Menu> = None;
    // for screenshots and tests: open the start menu at once
    app.toggle_menu = std::env::var_os("FDE_PANEL_OPEN_MENU").is_some();
    let entries = fde::apps::installed();
    let mut filtered: Vec<usize> = Vec::new();

    while !app.closed {
        if last_clock.elapsed() >= Duration::from_secs(10) {
            current_clock = clock();
            last_clock = Instant::now();
        }
        if app.tasks_changed {
            app.tasks_changed = false;
            dirty = true;
        }
        if let Some(i) = app.click_task.take() {
            if let (Some(t), Some(seat)) = (app.toplevels.get(i), app.seat.as_ref()) {
                if t.task.active && !t.task.minimized {
                    t.handle.set_minimized();
                } else {
                    t.handle.activate(seat);
                }
            }
        }
        let ws = fde::workspaces::read_workspace_state();
        if ws != shown_ws {
            dirty = true;
        }
        if app.size != size || app.hover != shown_hover || current_clock != shown || app.scale.current() != shown_scale {
            dirty = true;
        }
        if app.scale.current() != shown_scale {
            app.menu_dirty = true;
        }
        if dirty {
            size = app.size;
            let tasks: Vec<panel::Task> = app.toplevels.iter().map(|t| t.task.clone()).collect();
            let s = app.scale.current();
            let mut rgba = panel::draw(&mut text, size.0 * s, size.1 * s, s, app.hover, &current_clock, &tasks);
            shown_ws = ws;
            if let Some((cur, count)) = ws {
                panel::draw_workspaces(&mut rgba, size.0 * s, size.1 * s, s, cur, count);
            }
            attach(&shm, &qh, &surface, &mut file, rgba, (size.0 * s, size.1 * s), s)?;
            shown_scale = s;
            // tell the compositor where each window's button is: minimizing shrinks towards it
            for (i, t) in app.toplevels.iter().enumerate() {
                let (x0, x1, y0, y1) = panel::task_rect(i, tasks.len(), size.0, size.1);
                t.handle.set_rectangle(&surface, x0, y0, x1 - x0, y1 - y0);
            }
            shown = current_clock.clone();
            shown_hover = app.hover;
            dirty = false;
        }
        if app.clicked_orb {
            app.clicked_orb = false;
            app.toggle_menu = true;
        }
        // the start menu
        if app.toggle_menu {
            app.toggle_menu = false;
            if menu.is_some() {
                app.close_menu = true;
            } else if !entries.is_empty() {
                let m = open_menu(&compositor, &shell, &qh, &blur_manager, my_output.as_ref());
                app.menu_surface = Some(m.surface.clone());
                app.query.clear();
                filtered = fde::apps::filter(&entries, "");
                app.menu_entries = filtered.len().min(panel::MENU_MAX_ROWS);
                app.menu_configured = false;
                menu = Some(m);
            } else {
                launch();
            }
        }
        if menu.is_some() {
            if app.query_changed {
                app.query_changed = false;
                filtered = fde::apps::filter(&entries, &app.query);
                app.menu_entries = filtered.len().min(panel::MENU_MAX_ROWS);
                // the best match is selected, so Enter starts it
                app.menu_hover = if filtered.is_empty() { None } else { Some(0) };
                app.menu_dirty = true;
            }
            if app.menu_move != 0 {
                let n = app.menu_entries as i32;
                if n > 0 {
                    let cur = app.menu_hover.map(|h| h as i32).unwrap_or(if app.menu_move > 0 { -1 } else { n });
                    app.menu_hover = Some((cur + app.menu_move).clamp(0, n - 1) as usize);
                    app.menu_dirty = true;
                }
                app.menu_move = 0;
            }
            if app.menu_enter {
                app.menu_enter = false;
                if let Some(e) = filtered.get(app.menu_hover.unwrap_or(0)).and_then(|&i| entries.get(i)) {
                    launch_exec(&e.exec);
                    app.close_menu = true;
                }
            }
        }
        if let Some(i) = app.power_click.take() {
            let action = &fde::power::ACTIONS[i];
            if action.confirm && app.power_armed != Some(i) {
                app.power_armed = Some(i);
                app.menu_dirty = true;
            } else {
                app.power_armed = None;
                if let Err(e) = fde::power::run(action.id) {
                    eprintln!("fde-panel: {}: {e}", action.id);
                }
                app.close_menu = true;
            }
        }
        if let Some(row) = app.menu_click.take() {
            if let Some(e) = filtered.get(row).and_then(|&i| entries.get(i)) {
                launch_exec(&e.exec);
            }
            app.close_menu = true;
        }
        if app.close_menu {
            app.close_menu = false;
            if let Some(m) = menu.take() {
                m.layer.destroy();
                m.surface.destroy();
            }
            app.menu_surface = None;
            app.pointer_on_menu = false;
            app.menu_hover = None;
            app.power_hover = None;
            app.power_armed = None;
        }
        if let Some(m) = &mut menu {
            if app.menu_configured && app.menu_dirty {
                app.menu_dirty = false;
                let names: Vec<String> = filtered.iter().take(panel::MENU_MAX_ROWS).filter_map(|&i| entries.get(i)).map(|e| e.name.clone()).collect();
                let s = app.scale.current();
                let px = panel::draw_menu(&mut text, s, &app.query, &names, app.menu_hover, &panel::PowerView { hover: app.power_hover, armed: app.power_armed });
                let (mw, mh) = panel::menu_size();
                attach(&shm, &qh, &m.surface, &mut m.file, px, (mw * s, mh * s), s)?;
            }
        }
        // wait for events, but wake up for the clock
        queue.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let mut fds = [libc::pollfd { fd: guard.connection_fd().as_raw_fd_(), events: libc::POLLIN, revents: 0 }];
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 500) };
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
    let path = format!("{dir}/fde-panel-{}", std::process::id());
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
