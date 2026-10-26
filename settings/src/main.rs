//! fde-settings: the control panel. Every control writes `~/.config/fde/fde.toml` at once
//! (through `fde::config::set`, which keeps comments and refuses bad values); the running
//! desktop watches that file and follows within half a second. No private channel to the
//! compositor: closing this app, or editing the file by hand, changes nothing about how it works.

mod model;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use fde::config::{self, Config};
use gtk4::prelude::*;
use gtk4::{gdk, glib, Align, Box as GBox, Label, Orientation};

const CSS: &str = r#"
window.fde { background-image: linear-gradient(to bottom, #8fd0ff 0%, #d6efff 38%, #eaf7ff 100%); }
.sidebar-box { background: rgba(255,255,255,0.35); border-radius: 14px; padding: 6px; margin: 10px 0 10px 10px;
               border: 1px solid rgba(255,255,255,0.8); box-shadow: 0 1px 6px rgba(20,80,150,0.25); }
.sidebar-box list { background: transparent; }
.sidebar-box row { border-radius: 10px; padding: 8px 10px; margin: 2px 0; }
.sidebar-box row:selected { background-image: linear-gradient(to bottom, #7cc4ff, #3a95e6); color: white;
               text-shadow: 0 1px 2px rgba(10,60,120,0.7); }
.page { margin: 10px; padding: 18px 22px; background: rgba(255,255,255,0.55); border-radius: 14px;
        border: 1px solid rgba(255,255,255,0.9); box-shadow: 0 1px 6px rgba(20,80,150,0.25); }
.page-title { font-size: 20px; font-weight: bold; color: #17588f; text-shadow: 0 1px 0 white; }
.hint { color: #45688a; font-size: 12px; }
.status { color: #1d6b2b; font-size: 12px; }
.status.error { color: #b02a1c; }
scale trough { background: rgba(30,100,170,0.25); border-radius: 6px; min-height: 8px; }
scale highlight { background-image: linear-gradient(to bottom, #8fd0ff, #3a95e6); border-radius: 6px; }
scale slider { background-image: linear-gradient(to bottom, #ffffff, #cfe8fb); border: 1px solid #5a9bd6;
               border-radius: 50%; min-width: 18px; min-height: 18px; }
"#;

/// Shows "Saved" or the reason a value was refused.
#[derive(Clone)]
struct Status(Label);

impl Status {
    fn report(&self, r: Result<(), String>) {
        match r {
            Ok(()) => {
                self.0.set_text("Saved. The desktop follows within a moment.");
                self.0.remove_css_class("error");
            }
            Err(e) => {
                self.0.set_text(&format!("Not saved: {e}"));
                self.0.add_css_class("error");
            }
        }
    }
}

fn path_of_config() -> std::path::PathBuf {
    path()
}

fn path() -> std::path::PathBuf {
    config::user_path().unwrap_or_else(|| "fde.toml".into())
}

/// Run `save` once, a moment after the last call: a slider drag makes one write, not hundreds.
fn debounced(save: impl Fn() + 'static) -> impl Fn() + Clone + 'static {
    let save = Rc::new(save);
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    move || {
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let save = save.clone();
        let pending2 = pending.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            pending2.borrow_mut().take();
            save();
        });
        *pending.borrow_mut() = Some(id);
    }
}

fn page(title: &str, hint: &str) -> (GBox, GBox) {
    let outer = GBox::new(Orientation::Vertical, 10);
    outer.add_css_class("page");
    let t = Label::new(Some(title));
    t.add_css_class("page-title");
    t.set_halign(Align::Start);
    outer.append(&t);
    let h = Label::new(Some(hint));
    h.add_css_class("hint");
    h.set_halign(Align::Start);
    h.set_wrap(true);
    outer.append(&h);
    let body = GBox::new(Orientation::Vertical, 10);
    outer.append(&body);
    (outer, body)
}

fn row(label: &str, widget: &impl IsA<gtk4::Widget>) -> GBox {
    let r = GBox::new(Orientation::Horizontal, 12);
    let l = Label::new(Some(label));
    l.set_halign(Align::Start);
    l.set_width_chars(18);
    l.set_xalign(0.0);
    r.append(&l);
    widget.set_hexpand(true);
    r.append(widget);
    r
}

fn scale(min: f64, max: f64, step: f64, value: f64) -> gtk4::Scale {
    let s = gtk4::Scale::with_range(Orientation::Horizontal, min, max, step);
    s.set_value(value);
    s.set_draw_value(true);
    s.set_hexpand(true);
    s
}

fn appearance_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Appearance", "The glass of windows, the task bar and menus. Changes apply to the running desktop.");
    let a = &cfg.appearance;
    let rgba = gdk::RGBA::parse(a.accent.as_str()).unwrap_or(gdk::RGBA::new(0.29, 0.66, 0.93, 1.0));
    let dialog = gtk4::ColorDialog::new();
    let accent = gtk4::ColorDialogButton::new(Some(dialog));
    accent.set_rgba(&rgba);
    accent.set_halign(Align::Start);
    let transparency = scale(0.0, 1.0, 0.01, a.transparency as f64);
    let blur = scale(1.0, 6.0, 1.0, a.blur as f64);
    let gloss = scale(0.0, 1.0, 0.01, a.gloss as f64);
    let animations = gtk4::Switch::new();
    animations.set_active(a.animations);
    animations.set_halign(Align::Start);
    let (acc2, tr2, bl2, gl2, an2, st2) = (accent.clone(), transparency.clone(), blur.clone(), gloss.clone(), animations.clone(), status.clone());
    let save = debounced(move || {
        let c = acc2.rgba();
        let hex = model::hex_color(c.red(), c.green(), c.blue());
        st2.report(config::set(&path(), &model::appearance(&hex, tr2.value(), bl2.value() as u32, gl2.value(), an2.is_active())));
    });
    for s in [&transparency, &blur, &gloss] {
        let save = save.clone();
        s.connect_value_changed(move |_| save());
    }
    let save2 = save.clone();
    accent.connect_rgba_notify(move |_| save2());
    let save3 = save.clone();
    animations.connect_active_notify(move |_| save3());
    body.append(&row("Accent colour", &accent));
    body.append(&row("Glass tint", &transparency));
    body.append(&row("Blur (levels)", &blur));
    body.append(&row("Gloss", &gloss));
    body.append(&row("Animations", &animations));
    outer
}

fn wallpaper_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Wallpaper", "A PNG, JPEG or WebP picture for the desktop background. Leave it empty for the built-in sky.");
    let path = gtk4::Entry::new();
    path.set_text(&cfg.wallpaper.path);
    path.set_placeholder_text(Some("built-in sky"));
    let choose = gtk4::Button::with_label("Choose a picture…");
    let sky = gtk4::Button::with_label("Use the built-in sky");
    let modes = gtk4::StringList::new(&config::WALLPAPER_MODES);
    let mode = gtk4::DropDown::new(Some(modes), gtk4::Expression::NONE);
    mode.set_selected(config::WALLPAPER_MODES.iter().position(|m| *m == cfg.wallpaper.mode).unwrap_or(0) as u32);
    mode.set_halign(Align::Start);
    let (p2, m2, s2) = (path.clone(), mode.clone(), status.clone());
    let save = debounced(move || {
        let chosen = config::WALLPAPER_MODES.get(m2.selected() as usize).copied().unwrap_or("fill");
        s2.report(config::set(&path_of_config(), &model::wallpaper(&p2.text(), chosen)));
    });
    let s = save.clone();
    path.connect_changed(move |_| s());
    let s = save.clone();
    mode.connect_selected_notify(move |_| s());
    let p3 = path.clone();
    sky.connect_clicked(move |_| p3.set_text(""));
    let p4 = path.clone();
    choose.connect_clicked(move |b| {
        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Choose a wallpaper");
        let filter = gtk4::FileFilter::new();
        filter.set_name(Some("Pictures (PNG, JPEG, WebP)"));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        filter.add_mime_type("image/webp");
        let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        let p5 = p4.clone();
        let parent = b.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        dialog.open(parent.as_ref(), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(p) = file.path() {
                    p5.set_text(&p.to_string_lossy());
                }
            }
        });
    });
    let buttons = GBox::new(Orientation::Horizontal, 8);
    buttons.append(&choose);
    buttons.append(&sky);
    body.append(&row("Picture", &path));
    body.append(&buttons);
    body.append(&row("How it fits", &mode));
    outer
}

fn music_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Music", "Background music for the session: your own files, played in a loop by fde-music. Takes effect at once if it is running, else at the next session.");
    let enabled = gtk4::Switch::new();
    enabled.set_active(cfg.music.enabled);
    enabled.set_halign(Align::Start);
    let path = gtk4::Entry::new();
    path.set_text(&cfg.music.path);
    path.set_placeholder_text(Some("a music file or a folder, e.g. ~/Music"));
    let choose = gtk4::Button::with_label("Choose a folder…");
    let volume = scale(0.0, 1.0, 0.01, cfg.music.volume as f64);
    let shuffle = gtk4::Switch::new();
    shuffle.set_active(cfg.music.shuffle);
    shuffle.set_halign(Align::Start);
    let (e2, p2, v2, sh2, s2) = (enabled.clone(), path.clone(), volume.clone(), shuffle.clone(), status.clone());
    let save = debounced(move || {
        s2.report(config::set(&path_of_config(), &model::music(e2.is_active(), &p2.text(), v2.value(), sh2.is_active())));
    });
    let s = save.clone();
    enabled.connect_active_notify(move |_| s());
    let s = save.clone();
    path.connect_changed(move |_| s());
    let s = save.clone();
    volume.connect_value_changed(move |_| s());
    let s = save.clone();
    shuffle.connect_active_notify(move |_| s());
    let p3 = path.clone();
    choose.connect_clicked(move |b| {
        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Choose the music folder");
        let p4 = p3.clone();
        let parent = b.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        dialog.select_folder(parent.as_ref(), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(p) = file.path() {
                    p4.set_text(&p.to_string_lossy());
                }
            }
        });
    });
    body.append(&row("Play music", &enabled));
    body.append(&row("Music", &path));
    body.append(&choose);
    body.append(&row("Volume", &volume));
    body.append(&row("Shuffle", &shuffle));
    outer
}

fn keyboard_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Keyboard", "Layout is an xkb name such as us, de, fr or us(intl). Leave empty for the system default.");
    let layout = gtk4::Entry::new();
    layout.set_text(&cfg.input.keyboard_layout);
    layout.set_placeholder_text(Some("system default"));
    let rate = scale(1.0, 80.0, 1.0, cfg.input.repeat_rate as f64);
    let delay = scale(150.0, 1500.0, 10.0, cfg.input.repeat_delay as f64);
    let (l2, r2, d2, s2) = (layout.clone(), rate.clone(), delay.clone(), status.clone());
    let save = debounced(move || {
        s2.report(config::set(&path(), &model::keyboard(&l2.text(), r2.value() as u32, d2.value() as u32)));
    });
    let s = save.clone();
    layout.connect_changed(move |_| s());
    for sc in [&rate, &delay] {
        let s = save.clone();
        sc.connect_value_changed(move |_| s());
    }
    body.append(&row("Layout", &layout));
    body.append(&row("Repeat rate (/s)", &rate));
    body.append(&row("Repeat delay (ms)", &delay));
    outer
}

fn shell_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Desktop", "Programs and the task bar. The terminal is what Alt+Return starts.");
    let terminal = gtk4::Entry::new();
    terminal.set_text(&cfg.shell.terminal);
    let launcher = gtk4::Entry::new();
    launcher.set_text(&cfg.shell.launcher);
    launcher.set_placeholder_text(Some("same as the terminal"));
    let panel = gtk4::Switch::new();
    panel.set_active(cfg.shell.panel);
    panel.set_halign(Align::Start);
    let (t2, l2, p2, s2) = (terminal.clone(), launcher.clone(), panel.clone(), status.clone());
    let save = debounced(move || {
        s2.report(config::set(&path(), &model::shell(&t2.text(), &l2.text(), p2.is_active())));
    });
    for e in [&terminal, &launcher] {
        let s = save.clone();
        e.connect_changed(move |_| s());
    }
    let s = save.clone();
    panel.connect_active_notify(move |_| s());
    body.append(&row("Terminal", &terminal));
    body.append(&row("Start orb runs", &launcher));
    body.append(&row("Task bar", &panel));
    let note = Label::new(Some("The task bar starts and stops with the next session."));
    note.add_css_class("hint");
    note.set_halign(Align::Start);
    body.append(&note);
    outer
}

fn session_page(cfg: &Config, status: &Status) -> GBox {
    let (outer, body) = page("Session", "Lock the screen by itself when nobody uses the keyboard or mouse. 0 turns it off.");
    let minutes = scale(0.0, 60.0, 1.0, (cfg.session.idle_lock_seconds / 60) as f64);
    let (m2, s2) = (minutes.clone(), status.clone());
    let save = debounced(move || s2.report(config::set(&path(), &model::session(m2.value() as u32))));
    minutes.connect_value_changed(move |_| save());
    body.append(&row("Lock after (minutes)", &minutes));
    let key = Label::new(Some("Alt+L locks at any time."));
    key.add_css_class("hint");
    key.set_halign(Align::Start);
    body.append(&key);
    outer
}

fn about_page() -> GBox {
    let (outer, body) = page("About", "FDE: a Frutiger Aero desktop on Wayland.");
    for line in [
        format!("Settings file: {}", path().display()),
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        "Edit the file by hand if you like: it is applied the same way.".to_string(),
    ] {
        let l = Label::new(Some(&line));
        l.set_halign(Align::Start);
        l.set_selectable(true);
        body.append(&l);
    }
    outer
}

fn build(app: &gtk4::Application, quit_after: Option<u64>) {
    let cfg = Config::load().unwrap_or_default();
    let window = gtk4::ApplicationWindow::new(app);
    window.set_title(Some("FDE Settings"));
    window.set_default_size(820, 520);
    window.add_css_class("fde");

    let status = Status(Label::new(Some("")));
    status.0.add_css_class("status");
    status.0.set_halign(Align::Start);
    status.0.set_margin_start(14);
    status.0.set_margin_bottom(8);

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.add_titled(&appearance_page(&cfg, &status), Some("appearance"), "Appearance");
    stack.add_titled(&wallpaper_page(&cfg, &status), Some("wallpaper"), "Wallpaper");
    stack.add_titled(&music_page(&cfg, &status), Some("music"), "Music");
    stack.add_titled(&keyboard_page(&cfg, &status), Some("keyboard"), "Keyboard");
    stack.add_titled(&shell_page(&cfg, &status), Some("shell"), "Desktop");
    stack.add_titled(&session_page(&cfg, &status), Some("session"), "Session");
    stack.add_titled(&about_page(), Some("about"), "About");
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    // the page list: one row per page, selecting a row shows the page
    let sidebar = gtk4::ListBox::new();
    sidebar.set_size_request(190, -1);
    sidebar.set_selection_mode(gtk4::SelectionMode::Single);
    let pages = [("appearance", "Appearance"), ("wallpaper", "Wallpaper"), ("music", "Music"), ("keyboard", "Keyboard"), ("shell", "Desktop"), ("session", "Session"), ("about", "About")];
    for (_, title) in pages {
        let l = Label::new(Some(title));
        l.set_halign(Align::Start);
        sidebar.append(&l);
    }
    let stack2 = stack.clone();
    sidebar.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            if let Some((id, _)) = pages.get(row.index() as usize) {
                stack2.set_visible_child_name(id);
            }
        }
    });
    if let Some(first) = sidebar.row_at_index(0) {
        sidebar.select_row(Some(&first));
    }
    let side_box = GBox::new(Orientation::Vertical, 0);
    side_box.add_css_class("sidebar-box");
    side_box.append(&sidebar);

    let main = GBox::new(Orientation::Horizontal, 0);
    main.append(&side_box);
    let right = GBox::new(Orientation::Vertical, 0);
    right.append(&stack);
    right.append(&status.0);
    main.append(&right);
    window.set_child(Some(&main));
    window.present();

    if let Some(ms) = quit_after {
        let app = app.clone();
        glib::timeout_add_local_once(Duration::from_millis(ms), move || app.quit());
    }
}

fn main() -> glib::ExitCode {
    // `--quit-after MS` closes the window by itself: for smoke tests
    let args: Vec<String> = std::env::args().collect();
    let quit_after = args.iter().position(|a| a == "--quit-after").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
    let app = gtk4::Application::builder().application_id("org.fde.Settings").flags(gtk4::gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_startup(|_| {
        if let Some(display) = gdk::Display::default() {
            let css = gtk4::CssProvider::new();
            css.load_from_data(CSS);
            gtk4::style_context_add_provider_for_display(&display, &css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    });
    app.connect_activate(move |app| build(app, quit_after));
    // our own arguments are not GTK's
    app.run_with_args::<&str>(&[])
}
