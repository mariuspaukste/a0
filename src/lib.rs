pub mod ecs;
pub mod render;
pub mod sim;
pub mod world;

#[cfg(target_arch = "wasm32")]
mod web;
