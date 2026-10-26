//! Workspaces (virtual desktops): windows live on one of a few workspaces; only the windows of the
//! one being shown are on the screen, the others are parked until their workspace comes back.
//! Alt+1 .. Alt+9 switch, Alt+Shift+1 .. 9 move the front window, Alt+Ctrl+Left/Right step.

use smithay::desktop::Window;

use crate::state::Fde;

impl Fde {
    pub fn workspace_count(&self) -> usize {
        self.config.session.workspaces.clamp(1, 9) as usize
    }

    /// The workspace a window lives on.
    pub fn window_workspace_of(&self, window: &Window) -> usize {
        self.window_workspace(window)
    }

    fn window_workspace(&self, window: &Window) -> usize {
        window.user_data().get::<std::sync::Mutex<crate::state::WinInfo>>().map(|m| m.lock().unwrap().workspace).unwrap_or(0)
    }

    fn set_window_workspace(&self, window: &Window, n: usize) {
        window
            .user_data()
            .get_or_insert_threadsafe(|| std::sync::Mutex::new(crate::state::WinInfo::default()))
            .lock()
            .unwrap()
            .workspace = n;
    }

    /// Is this window on the workspace that is being shown?
    pub fn on_current_workspace(&self, window: &Window) -> bool {
        self.window_workspace(window) == self.workspace
    }

    /// Show workspace `n` (0 is the first).
    pub fn switch_workspace(&mut self, n: usize) {
        if n >= self.workspace_count() || n == self.workspace {
            return;
        }
        // take the windows of this workspace off the screen, back to front
        let leaving: Vec<Window> = self.space.elements().cloned().collect();
        for w in leaving {
            if let Some(loc) = self.space.element_location(&w) {
                w.user_data().get_or_insert_threadsafe(|| std::sync::Mutex::new(crate::state::WinInfo::default())).lock().unwrap().last_loc = Some(loc);
            }
            self.space.unmap_elem(&w);
            self.parked.push(w);
        }
        self.workspace = n;
        // bring back the windows of the new one (the minimized ones stay minimized)
        let (back, stay): (Vec<Window>, Vec<Window>) =
            std::mem::take(&mut self.parked).into_iter().partition(|w| self.window_workspace(w) == n && !self.minimized.contains(w));
        self.parked = stay;
        for w in back {
            let loc = w
                .user_data()
                .get::<std::sync::Mutex<crate::state::WinInfo>>()
                .and_then(|m| m.lock().unwrap().last_loc)
                .unwrap_or((70, 70).into());
            self.space.map_element(w, loc, false);
        }
        self.focus_top_window();
        self.sync_foreign_toplevels();
        self.hud = Some((std::time::Instant::now(), format!("Workspace {}", n + 1)));
        self.write_workspace_state();
    }

    pub fn switch_workspace_by(&mut self, delta: i32) {
        let n = self.workspace_count() as i32;
        let to = (self.workspace as i32 + delta).rem_euclid(n) as usize;
        self.switch_workspace(to);
    }

    /// Send `window` to workspace `n`; it leaves the screen if that is not the one being shown.
    pub fn move_window_to_workspace(&mut self, window: &Window, n: usize) {
        if n >= self.workspace_count() {
            return;
        }
        self.set_window_workspace(window, n);
        if n != self.workspace {
            self.space.unmap_elem(window);
            self.parked.push(window.clone());
            self.focus_top_window();
            self.sync_foreign_toplevels();
        }
    }

    /// New windows open on the workspace being shown.
    pub fn place_on_current_workspace(&self, window: &Window) {
        self.set_window_workspace(window, self.workspace);
    }

    /// Alt+N: notifications on or off; the screen says which.
    pub fn toggle_do_not_disturb(&mut self) {
        let on = !crate::notify::dnd_on();
        crate::notify::set_dnd(on);
        self.hud = Some((std::time::Instant::now(), if on { "Do not disturb: on".to_string() } else { "Do not disturb: off".to_string() }));
    }

    /// Tell the task bar which workspace is shown: `$XDG_RUNTIME_DIR/fde/workspace` ("2 4").
    pub fn write_workspace_state(&self) {
        let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join("fde")) else { return };
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("workspace"), format!("{} {}\n", self.workspace + 1, self.workspace_count()));
    }
}

/// The workspace the running compositor shows and how many there are: (shown, count), 1-based.
pub fn read_workspace_state() -> Option<(usize, usize)> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let text = std::fs::read_to_string(std::path::Path::new(&dir).join("fde/workspace")).ok()?;
    let mut it = text.split_whitespace().map(|v| v.parse::<usize>().ok());
    let (a, b) = (it.next()??, it.next()??);
    (a >= 1 && a <= b).then_some((a, b))
}
