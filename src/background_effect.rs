//! ext-background-effect-v1: a client says which part of its surface should have the background
//! behind it blurred (a translucent panel, a menu). The region is double buffered, applied on
//! commit, and read by the renderer through `blur_bounds`.

use std::sync::Mutex;

use smithay::reexports::wayland_protocols::ext::background_effect::v1::server::{
    ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::compositor::{get_region_attributes, with_states, RectangleKind, RegionAttributes};

/// The blur state of one surface, kept in its data map.
#[derive(Default)]
pub struct BlurState {
    pending: Mutex<Option<RegionAttributes>>,
    current: Mutex<Option<RegionAttributes>>,
    /// has an effect object: a second one is a protocol error
    object: Mutex<bool>,
}

/// Apply the pending region of `surface` (call on every commit).
pub fn commit(surface: &WlSurface) {
    with_states(surface, |states| {
        if let Some(s) = states.data_map.get::<BlurState>() {
            let pending = s.pending.lock().unwrap().take();
            if let Some(p) = pending {
                *s.current.lock().unwrap() = if p.rects.is_empty() { None } else { Some(p) };
            }
        }
    });
}

/// The surface-local rectangle that has blur behind it (the bounding box of the added parts).
pub fn blur_bounds(surface: &WlSurface) -> Option<Rectangle<i32, Logical>> {
    with_states(surface, |states| {
        let s = states.data_map.get::<BlurState>()?;
        let cur = s.current.lock().unwrap();
        let region = cur.as_ref()?;
        let mut bounds: Option<Rectangle<i32, Logical>> = None;
        for (kind, rect) in &region.rects {
            if matches!(kind, RectangleKind::Add) {
                bounds = Some(match bounds {
                    Some(b) => b.merge(*rect),
                    None => *rect,
                });
            }
        }
        bounds
    })
}

pub struct BackgroundEffectState;

pub trait BackgroundEffectHandler {}

impl BackgroundEffectState {
    pub fn new<D>(dh: &DisplayHandle) -> BackgroundEffectState
    where
        D: GlobalDispatch<ExtBackgroundEffectManagerV1, ()> + 'static,
    {
        dh.create_global::<D, ExtBackgroundEffectManagerV1, ()>(1, ());
        BackgroundEffectState
    }
}

impl<D> GlobalDispatch<ExtBackgroundEffectManagerV1, (), D> for BackgroundEffectState
where
    D: GlobalDispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectSurfaceV1, WlSurface>
        + 'static,
{
    fn bind(
        _: &mut D,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtBackgroundEffectManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        let manager = data_init.init(resource, ());
        manager.capabilities(Capability::Blur);
    }
}

impl<D> Dispatch<ExtBackgroundEffectManagerV1, (), D> for BackgroundEffectState
where
    D: Dispatch<ExtBackgroundEffectManagerV1, ()> + Dispatch<ExtBackgroundEffectSurfaceV1, WlSurface> + 'static,
{
    fn request(
        _: &mut D,
        _: &Client,
        manager: &ExtBackgroundEffectManagerV1,
        request: ext_background_effect_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        if let ext_background_effect_manager_v1::Request::GetBackgroundEffect { id, surface } = request {
            let exists = with_states(&surface, |states| {
                let s = states.data_map.get_or_insert_threadsafe(BlurState::default);
                let mut o = s.object.lock().unwrap();
                let had = *o;
                *o = true;
                had
            });
            if exists {
                manager.post_error(
                    ext_background_effect_manager_v1::Error::BackgroundEffectExists,
                    "the surface already has a background effect object",
                );
                return;
            }
            data_init.init(id, surface);
        }
    }
}

impl<D> Dispatch<ExtBackgroundEffectSurfaceV1, WlSurface, D> for BackgroundEffectState
where
    D: Dispatch<ExtBackgroundEffectSurfaceV1, WlSurface> + 'static,
{
    fn request(
        _: &mut D,
        _: &Client,
        _: &ExtBackgroundEffectSurfaceV1,
        request: ext_background_effect_surface_v1::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, D>,
    ) {
        match request {
            ext_background_effect_surface_v1::Request::SetBlurRegion { region } => {
                let attrs = region.map(|r| get_region_attributes(&r)).unwrap_or_default();
                with_states(surface, |states| {
                    let s = states.data_map.get_or_insert_threadsafe(BlurState::default);
                    *s.pending.lock().unwrap() = Some(attrs);
                });
            }
            ext_background_effect_surface_v1::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(_: &mut D, _: smithay::reexports::wayland_server::backend::ClientId, _: &ExtBackgroundEffectSurfaceV1, surface: &WlSurface) {
        // the effect goes away with the object (on the next commit, as the protocol says)
        if surface.is_alive() {
            with_states(surface, |states| {
                if let Some(s) = states.data_map.get::<BlurState>() {
                    *s.pending.lock().unwrap() = Some(RegionAttributes::default());
                    *s.object.lock().unwrap() = false;
                }
            });
        }
    }
}

#[macro_export]
macro_rules! delegate_background_effect {
    ($ty:ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: ()
        ] => $crate::background_effect::BackgroundEffectState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: ()
        ] => $crate::background_effect::BackgroundEffectState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface
        ] => $crate::background_effect::BackgroundEffectState);
    };
}
