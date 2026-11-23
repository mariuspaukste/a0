pub mod economy;
pub mod ecs;
pub mod era;
pub mod history;
pub mod render;
pub mod sim;
pub mod timelines;
pub mod world;

#[cfg(target_arch = "wasm32")]
mod web;
