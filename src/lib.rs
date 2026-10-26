//! FAWC: a Wayland compositor with real glass (blur, gloss, translucency drawn on the GPU).

pub mod apps;
pub mod background_effect;
pub mod backend_drm;
pub mod backend_winit;
pub mod clientscale;
pub mod config;
pub mod cursor;
pub mod demo;
pub mod foreign_toplevel;
pub mod glass;
pub mod handlers;
pub mod input;
pub mod layers;
pub mod lockscreen;
pub mod notify;
pub mod outputs;
pub mod layout;
pub mod panel;
pub mod render;
pub mod shell;
pub mod state;
pub mod text;
pub mod wallpaper;
pub mod windows;
