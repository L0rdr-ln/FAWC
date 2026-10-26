//! Running on the hardware: the login session (libseat), the GPU (DRM/KMS through GBM and EGL),
//! the monitors and the input devices (libinput). One GPU, every connected monitor is an output
//! of its own, placed side by side (`[outputs.NAME]` in the configuration sets scale, mode and
//! position). Monitors plugged in or out while running are picked up (the kernel's change event
//! for the GPU triggers a new look at the connectors); the first monitor cannot go away.

use std::cell::RefCell;
use std::os::fd::AsFd;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmNode};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::element::texture::{TextureBuffer, TextureRenderElement};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{primary_gpu, UdevBackend, UdevEvent};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::output::Output;
use smithay::reexports::drm::control::{self as drm_mode, connector, crtc, Device as ControlDevice, ModeTypeFlags};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::{Display, ListeningSocket};
use smithay::utils::{DeviceFd, Transform};

use crate::glass::Glass;
use crate::render::create_scene;
use crate::state::{ClientState, Fde};

type Compositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    (),
    DrmDeviceFd,
>;

/// One monitor on the GPU.
struct OutputGpu {
    output: Output,
    scene: GlesTexture,
    compositor: Compositor,
    crtc: crtc::Handle,
    queued: bool,
}

/// Everything the render callback needs besides the compositor state.
struct Gpu {
    renderer: GlesRenderer,
    glass: Glass,
    outputs: Vec<OutputGpu>,
}

/// Draw monitor `idx` and hand the frame to the display (unless one is already on its way).
fn render_and_queue(state: &mut Fde, gpu: &mut Gpu, idx: usize) {
    let Gpu { renderer, glass, outputs } = gpu;
    let Some(og) = outputs.get_mut(idx) else { return };
    if og.queued {
        return;
    }
    if let Err(e) = state.render_output(renderer, glass, &mut og.scene, &og.output) {
        tracing::error!("render failed: {e}");
        return;
    }
    // the scene texture is the only element on screen; a new buffer every frame = full damage
    let buffer = TextureBuffer::from_texture(&*renderer, og.scene.clone(), 1, Transform::Normal, None);
    let element = TextureRenderElement::from_texture_buffer((0.0, 0.0), &buffer, Some(1.0), None, None, Kind::Unspecified);
    match og.compositor.render_frame::<_, _>(renderer, &[element], [0.0, 0.0, 0.0, 1.0], FrameFlags::DEFAULT) {
        Ok(res) => {
            if !res.is_empty {
                match og.compositor.queue_frame(()) {
                    Ok(()) => og.queued = true,
                    Err(e) => tracing::error!("queue_frame: {e}"),
                }
            }
        }
        Err(e) => tracing::error!("render_frame: {e}"),
    }
    // frame callbacks for the clients that are (partly) on this monitor
    let elapsed = state.start_time.elapsed();
    for window in state.space.elements() {
        if state.space.outputs_for_element(window).contains(&og.output) {
            window.send_frame(&og.output, elapsed, Some(Duration::ZERO), |_, _| Some(og.output.clone()));
        }
    }
}

/// A connected monitor and what to show on it.
struct Monitor {
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: drm_mode::Mode,
    name: String,
    /// every mode of the monitor (width, height, millihertz), for the settings app
    all_modes: Vec<(i32, i32, i32)>,
}

/// A connected, enabled monitor that has not been given a CRTC yet.
struct Candidate {
    connector: connector::Handle,
    mode: drm_mode::Mode,
    name: String,
    all_modes: Vec<(i32, i32, i32)>,
    crtcs: Vec<crtc::Handle>,
}

/// Every connected monitor the configuration does not switch off, with its mode and the CRTCs it
/// could use.
fn probe(drm: &DrmDevice, config: &crate::config::Config) -> Result<Vec<Candidate>, Box<dyn std::error::Error>> {
    let res = drm.resource_handles()?;
    let mut found = Vec::new();
    for &handle in res.connectors() {
        let info = drm.get_connector(handle, true)?;
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let oc = config.outputs.get(&name).cloned().unwrap_or_default();
        if !oc.enabled {
            tracing::info!("monitor {name} is switched off in the configuration");
            continue;
        }
        let modes = info.modes();
        let preferred = modes.iter().find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED)).copied().unwrap_or(modes[0]);
        // the configured mode if the monitor has it, else its preferred one
        let mode = match crate::config::parse_mode(&oc.mode) {
            Some((w, h, hz)) => modes
                .iter()
                .filter(|m| m.size() == (w as u16, h as u16))
                .min_by(|a, b| {
                    let want = hz.unwrap_or(f64::MAX);
                    let da = (a.vrefresh() as f64 - want).abs();
                    let db = (b.vrefresh() as f64 - want).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied()
                .unwrap_or_else(|| {
                    tracing::warn!("monitor {name} has no {w}x{h} mode: using its preferred one");
                    preferred
                }),
            None => preferred,
        };
        let crtcs: Vec<crtc::Handle> = info
            .encoders()
            .iter()
            .filter_map(|&e| drm.get_encoder(e).ok())
            .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
            .collect();
        let mut all_modes: Vec<(i32, i32, i32)> = modes.iter().map(|m| (m.size().0 as i32, m.size().1 as i32, m.vrefresh() as i32 * 1000)).collect();
        all_modes.dedup();
        found.push(Candidate { connector: handle, mode, name, all_modes, crtcs });
    }
    Ok(found)
}

/// Give each candidate a CRTC that is not in `taken` (and take it).
fn assign(candidates: Vec<Candidate>, taken: &mut Vec<crtc::Handle>) -> Vec<Monitor> {
    let mut monitors = Vec::new();
    for c in candidates {
        match c.crtcs.iter().copied().find(|x| !taken.contains(x)) {
            Some(crtc) => {
                taken.push(crtc);
                tracing::info!("monitor {} ({}x{}@{})", c.name, c.mode.size().0, c.mode.size().1, c.mode.vrefresh());
                monitors.push(Monitor { connector: c.connector, crtc, mode: c.mode, name: c.name, all_modes: c.all_modes });
            }
            None => tracing::warn!("monitor {}: no free CRTC for it", c.name),
        }
    }
    monitors
}

/// The GPU pieces a monitor's compositor needs.
struct Hardware {
    drm: Rc<RefCell<DrmDevice>>,
    gbm: GbmDevice<DrmDeviceFd>,
    drm_fd: DrmDeviceFd,
    formats: Vec<smithay::backend::allocator::Format>,
}

/// Start driving `monitor` as `output`: a surface on its CRTC, a compositor, a scene texture.
fn create_output_gpu(hw: &Hardware, renderer: &mut GlesRenderer, monitor: &Monitor, output: &Output) -> Result<OutputGpu, Box<dyn std::error::Error>> {
    let mut drm = hw.drm.borrow_mut();
    let surface = drm.create_surface(monitor.crtc, monitor.mode, &[monitor.connector])?;
    let allocator = GbmAllocator::new(hw.gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
    let exporter = GbmFramebufferExporter::new(hw.gbm.clone(), DrmNode::from_file(hw.drm_fd.as_fd()).ok());
    let compositor: Compositor = DrmCompositor::new(
        output,
        surface,
        None,
        allocator,
        exporter,
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        hw.formats.clone(),
        drm.cursor_size(),
        Some(hw.gbm.clone()),
    )?;
    let (mw, mh) = (monitor.mode.size().0 as i32, monitor.mode.size().1 as i32);
    let scene = create_scene(renderer, (mw, mh).into())?;
    Ok(OutputGpu { output: output.clone(), scene, compositor, crtc: monitor.crtc, queued: false })
}

/// A monitor was plugged in or out: look again at what is connected.
fn rescan(state: &mut Fde, gpu: &mut Gpu, hw: &Hardware) {
    let config = state.config.clone();
    let candidates = match probe(&hw.drm.borrow(), &config) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("cannot look at the monitors: {e}");
            return;
        }
    };
    // monitors that are gone (the first one stays: it carries the panels)
    let gone: Vec<Output> = gpu
        .outputs
        .iter()
        .map(|o| o.output.clone())
        .filter(|o| *o != state.output && !candidates.iter().any(|c| c.name == o.name()))
        .collect();
    for output in gone {
        tracing::info!("monitor {} unplugged", output.name());
        gpu.outputs.retain(|o| o.output != output);
        state.remove_output(&output);
    }
    // monitors that are new
    let known: Vec<String> = gpu.outputs.iter().map(|o| o.output.name()).collect();
    let mut taken: Vec<crtc::Handle> = gpu.outputs.iter().map(|o| o.crtc).collect();
    let fresh: Vec<Candidate> = candidates.into_iter().filter(|c| !known.contains(&c.name)).collect();
    for monitor in assign(fresh, &mut taken) {
        state.available_modes.insert(monitor.name.clone(), monitor.all_modes.clone());
        let output = state.add_output(&monitor.name, (monitor.mode.size().0 as i32, monitor.mode.size().1 as i32), monitor.mode.vrefresh() as i32 * 1000);
        match create_output_gpu(hw, &mut gpu.renderer, &monitor, &output) {
            Ok(og) => {
                gpu.outputs.push(og);
                let idx = gpu.outputs.len() - 1;
                render_and_queue(state, gpu, idx);
            }
            Err(e) => {
                tracing::error!("cannot drive monitor {}: {e}", monitor.name);
                state.remove_output(&output);
            }
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<Fde> = EventLoop::try_new()?;
    let mut display: Display<Fde> = Display::new()?;
    let dh = display.handle();

    // --- the login session: access to the GPU and the input devices without being root
    let (mut session, session_notifier) = LibSeatSession::new().map_err(|e| format!("libseat: {e:?} (is seatd or logind running, and are you on a console?)"))?;
    let seat_name = session.seat();
    tracing::info!("seat {seat_name}");

    // --- the GPU
    let gpu_path = primary_gpu(&seat_name)?.ok_or("no GPU found for this seat")?;
    tracing::info!("GPU {}", gpu_path.display());
    let fd = session.open(&gpu_path, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)?;
    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), true)?;
    let gbm = GbmDevice::new(drm_fd.clone())?;
    let egl = unsafe { EGLDisplay::new(gbm.clone())? };
    let context = EGLContext::new(&egl)?;
    let mut renderer = unsafe { GlesRenderer::new(context)? };

    // --- every connected monitor, each on a CRTC of its own
    let config = crate::config::Config::load().unwrap_or_default();
    let monitors = assign(probe(&drm, &config)?, &mut Vec::new());
    let first = monitors.first().ok_or("no monitor is connected")?;
    let (w, h) = (first.mode.size().0 as i32, first.mode.size().1 as i32);

    let mut state = Fde::new_named(&dh, &first.name, (w, h).into());
    for m in &monitors {
        state.available_modes.insert(m.name.clone(), m.all_modes.clone());
    }
    state.draw_cursor = true;
    state.session = Some(session.clone());
    state.enable_dmabuf(smithay::backend::renderer::ImportDma::dmabuf_formats(&renderer));
    let outputs: Vec<Output> = std::iter::once(state.output.clone())
        .chain(monitors.iter().skip(1).map(|m| state.add_output(&m.name, (m.mode.size().0 as i32, m.mode.size().1 as i32), m.mode.vrefresh() as i32 * 1000)))
        .collect();

    let formats = renderer.egl_context().dmabuf_render_formats().iter().copied().collect::<Vec<_>>();
    let glass = Glass::new(&mut renderer)?;
    let hw = Rc::new(Hardware { drm: Rc::new(RefCell::new(drm)), gbm: gbm.clone(), drm_fd: drm_fd.clone(), formats });
    let mut gpu_outputs = Vec::new();
    for (monitor, output) in monitors.iter().zip(outputs.iter()) {
        gpu_outputs.push(create_output_gpu(&hw, &mut renderer, monitor, output)?);
    }
    let gpu = Rc::new(RefCell::new(Gpu { renderer, glass, outputs: gpu_outputs }));

    // --- the Wayland socket and the clients
    let socket = ListeningSocket::bind_auto("wayland", 1..33)?;
    let socket_name = socket.socket_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tracing::info!("listening on WAYLAND_DISPLAY={socket_name}");
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    state.start_session();
    crate::shell::watch_signals();
    crate::xwayland::start(event_loop.handle(), &dh);
    event_loop.handle().insert_source(Generic::new(socket, Interest::READ, Mode::Level), |_, socket, state| {
        while let Some(stream) = socket.accept()? {
            let _ = state.display_handle.insert_client(stream, Arc::new(ClientState::default()));
        }
        Ok(PostAction::Continue)
    })?;
    let poll_fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop.handle().insert_source(Generic::new(poll_fd, Interest::READ, Mode::Level), move |_, _, state| {
        display.dispatch_clients(state).expect("dispatch");
        display.flush_clients().expect("flush");
        Ok(PostAction::Continue)
    })?;

    // --- input devices
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput.udev_assign_seat(&seat_name).map_err(|_| "libinput: cannot assign the seat")?;
    let libinput_backend = LibinputInputBackend::new(libinput.clone());
    event_loop.handle().insert_source(libinput_backend, |event, _, state| state.process_input_event(event))?;

    // --- the session coming and going (virtual terminal switches)
    let mut libinput_for_session = libinput.clone();
    event_loop.handle().insert_source(session_notifier, move |event, _, _state| match event {
        SessionEvent::PauseSession => {
            libinput_for_session.suspend();
            tracing::info!("session paused");
        }
        SessionEvent::ActivateSession => {
            if libinput_for_session.resume().is_err() {
                tracing::error!("cannot resume the input devices");
            }
            tracing::info!("session active");
        }
    })?;

    // --- monitors plugged in or out while running: the kernel says the GPU's connectors changed
    let our_node = DrmNode::from_file(drm_fd.as_fd())?;
    let udev = UdevBackend::new(&seat_name)?;
    let (gpu_hotplug, hw_hotplug) = (gpu.clone(), hw.clone());
    event_loop.handle().insert_source(udev, move |event, _, state| {
        if let UdevEvent::Changed { device_id } = event {
            if device_id == our_node.dev_id() {
                rescan(state, &mut gpu_hotplug.borrow_mut(), &hw_hotplug);
            }
        }
    })?;

    // --- vblank: the last frame is on screen, draw the next
    let gpu_vblank = gpu.clone();
    event_loop.handle().insert_source(drm_notifier, move |event, _, state| match event {
        DrmEvent::VBlank(c) => {
            let mut gpu = gpu_vblank.borrow_mut();
            if let Some(idx) = gpu.outputs.iter().position(|o| o.crtc == c) {
                if let Err(e) = gpu.outputs[idx].compositor.frame_submitted() {
                    tracing::warn!("frame_submitted: {e}");
                }
                gpu.outputs[idx].queued = false;
                render_and_queue(state, &mut gpu, idx);
            }
        }
        DrmEvent::Error(e) => tracing::error!("drm: {e}"),
    })?;

    // the first frame of every monitor starts its vblank cycle
    let count = gpu.borrow().outputs.len();
    for idx in 0..count {
        render_and_queue(&mut state, &mut gpu.borrow_mut(), idx);
    }
    let _ = &mut session;

    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
        if crate::shell::termination_requested() {
            tracing::info!("asked to terminate: ending the session");
            state.running = false;
        }
        // an idle compositor still needs a frame going: restart the cycle if it stalled
        let mut g = gpu.borrow_mut();
        for idx in 0..g.outputs.len() {
            if !g.outputs[idx].queued {
                render_and_queue(&mut state, &mut g, idx);
            }
        }
    }
    Ok(())
}
