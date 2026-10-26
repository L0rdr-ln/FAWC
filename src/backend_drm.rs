//! Running on the hardware: the login session (libseat), the GPU (DRM/KMS through GBM and EGL),
//! the monitors and the input devices (libinput). One GPU, all connected monitors showing the
//! same picture side by side is not done yet: for now the first connected monitor is used.

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
use smithay::reexports::drm::control::{connector, crtc, Device as ControlDevice, ModeTypeFlags};
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

/// Everything the render callback needs besides the compositor state.
struct Gpu {
    renderer: GlesRenderer,
    glass: Glass,
    scene: GlesTexture,
    compositor: Compositor,
    crtc: crtc::Handle,
    queued: bool,
}

fn render_and_queue(state: &mut Fawc, gpu: &mut Gpu) {
    if gpu.queued {
        return;
    }
    if let Err(e) = state.render_scene(&mut gpu.renderer, &gpu.glass, &mut gpu.scene) {
        tracing::error!("render failed: {e}");
        return;
    }
    // the scene texture is the only element on screen; a new buffer every frame = full damage
    let buffer = TextureBuffer::from_texture(&gpu.renderer, gpu.scene.clone(), 1, Transform::Normal, None);
    let element = TextureRenderElement::from_texture_buffer((0.0, 0.0), &buffer, Some(1.0), None, None, Kind::Unspecified);
    match gpu.compositor.render_frame::<_, _>(&mut gpu.renderer, &[element], [0.0, 0.0, 0.0, 1.0], FrameFlags::DEFAULT) {
        Ok(res) => {
            if !res.is_empty {
                match gpu.compositor.queue_frame(()) {
                    Ok(()) => gpu.queued = true,
                    Err(e) => tracing::error!("queue_frame: {e}"),
                }
            }
        }
        Err(e) => tracing::error!("render_frame: {e}"),
    }
    // frame callbacks for the clients
    let elapsed = state.start_time.elapsed();
    for window in state.space.elements() {
        window.send_frame(&state.output, elapsed, Some(Duration::ZERO), |_, _| Some(state.output.clone()));
    }
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

    // --- the first connected monitor
    let res = drm.resource_handles()?;
    let mut chosen = None;
    for &handle in res.connectors() {
        let info = drm.get_connector(handle, true)?;
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let mode = info
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .copied()
            .unwrap_or(info.modes()[0]);
        // a CRTC this connector can use
        let crtc = info
            .encoders()
            .iter()
            .filter_map(|&e| drm.get_encoder(e).ok())
            .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
            .next();
        if let Some(crtc) = crtc {
            tracing::info!("monitor {} ({}x{}@{})", info, mode.size().0, mode.size().1, mode.vrefresh());
            chosen = Some((handle, crtc, mode));
            break;
        }
    }
    let (conn, crtc, mode) = chosen.ok_or("no monitor is connected")?;
    let surface = drm.create_surface(crtc, mode, &[conn])?;
    let allocator = GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
    let exporter = GbmFramebufferExporter::new(gbm.clone(), DrmNode::from_file(drm_fd.as_fd()).ok());
    let (w, h) = (mode.size().0 as i32, mode.size().1 as i32);

    let mut state = Fawc::new(&dh, (w, h).into());
    state.draw_cursor = true;
    state.session = Some(session.clone());

    let renderer_formats = renderer.egl_context().dmabuf_render_formats().iter().copied().collect::<Vec<_>>();
    let compositor: Compositor = DrmCompositor::new(
        &state.output,
        surface,
        None,
        allocator,
        exporter,
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        renderer_formats,
        drm.cursor_size(),
        Some(gbm.clone()),
    )?;
    let glass = Glass::new(&mut renderer)?;
    let scene = create_scene(&mut renderer, (w, h).into())?;
    let gpu = Rc::new(RefCell::new(Gpu { renderer, glass, scene, compositor, crtc, queued: false }));

    // --- the Wayland socket and the clients
    let socket = ListeningSocket::bind_auto("wayland", 1..33)?;
    let socket_name = socket.socket_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    tracing::info!("listening on WAYLAND_DISPLAY={socket_name}");
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    crate::shell::start_session_clients();
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
            if c == gpu.crtc {
                if let Err(e) = gpu.compositor.frame_submitted() {
                    tracing::warn!("frame_submitted: {e}");
                }
                gpu.queued = false;
                render_and_queue(state, &mut gpu);
            }
        }
        DrmEvent::Error(e) => tracing::error!("drm: {e}"),
    })?;

    if let Ok(cmd) = std::env::var("FAWC_AUTOSTART") {
        let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
    }

    // the first frame starts the vblank cycle
    render_and_queue(&mut state, &mut gpu.borrow_mut());
    let _ = (&mut drm, &mut session);

    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
        // an idle compositor still needs a frame going: restart the cycle if it stalled
        let mut g = gpu.borrow_mut();
        if !g.queued {
            render_and_queue(&mut state, &mut g);
        }
    }
    Ok(())
}
