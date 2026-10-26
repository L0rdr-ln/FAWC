//! Running on the hardware: the login session (libseat), the GPU (DRM/KMS through GBM and EGL),
//! the monitors and the input devices (libinput). One GPU, every connected monitor is an output
//! of its own, placed side by side (`[outputs.NAME]` in the configuration sets scale, mode and
//! position). Monitors plugged in while running are not noticed yet: restart FAWC.

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
use smithay::backend::udev::primary_gpu;
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
use crate::state::{ClientState, Fawc};

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
fn render_and_queue(state: &mut Fawc, gpu: &mut Gpu, idx: usize) {
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
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<Fawc> = EventLoop::try_new()?;
    let mut display: Display<Fawc> = Display::new()?;
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
    let (mut drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), true)?;
    let gbm = GbmDevice::new(drm_fd.clone())?;
    let egl = unsafe { EGLDisplay::new(gbm.clone())? };
    let context = EGLContext::new(&egl)?;
    let mut renderer = unsafe { GlesRenderer::new(context)? };

    // --- every connected monitor, each on a CRTC of its own
    let config = crate::config::Config::load().unwrap_or_default();
    let res = drm.resource_handles()?;
    let mut monitors: Vec<Monitor> = Vec::new();
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
        // a CRTC this connector can use that no other monitor has taken
        let taken: Vec<crtc::Handle> = monitors.iter().map(|m| m.crtc).collect();
        let crtc = info
            .encoders()
            .iter()
            .filter_map(|&e| drm.get_encoder(e).ok())
            .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
            .find(|c| !taken.contains(c));
        match crtc {
            Some(crtc) => {
                tracing::info!("monitor {name} ({}x{}@{})", mode.size().0, mode.size().1, mode.vrefresh());
                monitors.push(Monitor { connector: handle, crtc, mode, name });
            }
            None => tracing::warn!("monitor {name}: no free CRTC for it"),
        }
    }
    let first = monitors.first().ok_or("no monitor is connected")?;
    let (w, h) = (first.mode.size().0 as i32, first.mode.size().1 as i32);
    let allocator_for = |gbm: &GbmDevice<DrmDeviceFd>| GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
    let exporter_for = |gbm: &GbmDevice<DrmDeviceFd>| GbmFramebufferExporter::new(gbm.clone(), DrmNode::from_file(drm_fd.as_fd()).ok());

    let mut state = Fawc::new_named(&dh, &first.name, (w, h).into());
    state.draw_cursor = true;
    state.session = Some(session.clone());
    let outputs: Vec<Output> = std::iter::once(state.output.clone())
        .chain(monitors.iter().skip(1).map(|m| state.add_output(&m.name, (m.mode.size().0 as i32, m.mode.size().1 as i32), m.mode.vrefresh() as i32 * 1000)))
        .collect();

    let renderer_formats = renderer.egl_context().dmabuf_render_formats().iter().copied().collect::<Vec<_>>();
    let glass = Glass::new(&mut renderer)?;
    let mut gpu_outputs = Vec::new();
    for (monitor, output) in monitors.iter().zip(outputs.iter()) {
        let surface = drm.create_surface(monitor.crtc, monitor.mode, &[monitor.connector])?;
        let compositor: Compositor = DrmCompositor::new(
            output,
            surface,
            None,
            allocator_for(&gbm),
            exporter_for(&gbm),
            [Fourcc::Argb8888, Fourcc::Xrgb8888],
            renderer_formats.clone(),
            drm.cursor_size(),
            Some(gbm.clone()),
        )?;
        let (mw, mh) = (monitor.mode.size().0 as i32, monitor.mode.size().1 as i32);
        let scene = create_scene(&mut renderer, (mw, mh).into())?;
        gpu_outputs.push(OutputGpu { output: output.clone(), scene, compositor, crtc: monitor.crtc, queued: false });
    }
    let gpu = Rc::new(RefCell::new(Gpu { renderer, glass, outputs: gpu_outputs }));

    // --- the Wayland socket and the clients
    let socket = ListeningSocket::bind_auto("wayland", 1..33)?;
    let socket_name = socket.socket_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tracing::info!("listening on WAYLAND_DISPLAY={socket_name}");
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    crate::shell::start_session_clients(&crate::config::Config::load().unwrap_or_default());
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
    let _ = (&mut drm, &mut session);

    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
        // an idle compositor still needs a frame going: restart the cycle if it stalled
        let mut g = gpu.borrow_mut();
        for idx in 0..count {
            if !g.outputs[idx].queued {
                render_and_queue(&mut state, &mut g, idx);
            }
        }
    }
    Ok(())
}
