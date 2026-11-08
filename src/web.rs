//! Browser entry points. The page calls these exported functions directly, with no
//! wasm-bindgen in between, so only numbers cross the boundary. Text and pixels are
//! returned as a pointer into wasm memory plus a length, and the page reads them from
//! there.

use std::cell::RefCell;

use crate::render::{self, Camera, Framebuffer};
use crate::sim::{Simulation, MAP_HEIGHT, MAP_WIDTH};

extern "C" {
    /// Provided by the page (`env.console_error`).
    fn console_error(ptr: *const u8, len: usize);
}

struct App {
    sim: Simulation,
    camera: Camera,
    fb: Framebuffer,
    selected: Option<(i32, i32)>,
    /// Holds the last string handed to the page, so it stays valid until the next call.
    text: String,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|app| {
        f(app
            .borrow_mut()
            .as_mut()
            .expect("init() must be called first"))
    })
}

/// Stores `text` and returns its length; the page then reads it at `text_ptr()`.
fn hand_over(app: &mut App, text: String) -> usize {
    app.text = text;
    app.text.len()
}

#[no_mangle]
pub extern "C" fn init(seed: u32, width: u32, height: u32) {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        unsafe { console_error(message.as_ptr(), message.len()) };
    }));
    let sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, seed as u64);
    let fb = Framebuffer::new(width as usize, height as usize);
    let camera = Camera::fit(MAP_WIDTH, MAP_HEIGHT, &fb);
    let app = App {
        sim,
        camera,
        fb,
        selected: None,
        text: String::new(),
    };
    APP.with(|slot| *slot.borrow_mut() = Some(app));
}

#[no_mangle]
pub extern "C" fn resize(width: u32, height: u32) {
    with_app(|app| app.fb.resize(width as usize, height as usize));
}

#[no_mangle]
pub extern "C" fn step(months: u32) {
    with_app(|app| {
        for _ in 0..months {
            app.sim.step_month();
        }
    });
}

/// Draws a frame and returns a pointer to its RGBA pixels.
#[no_mangle]
pub extern "C" fn render() -> *const u32 {
    with_app(|app| {
        render::render(&app.sim, &app.camera, &mut app.fb, app.selected);
        app.fb.pixels.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn pan(dx: f32, dy: f32) {
    with_app(|app| app.camera.pan(dx, dy));
}

#[no_mangle]
pub extern "C" fn zoom(factor: f32, x: f32, y: f32) {
    with_app(|app| app.camera.zoom_at(factor, x, y, &app.fb));
}

/// Selects the tile under a screen position, or clears the selection off the map.
#[no_mangle]
pub extern "C" fn select(x: f32, y: f32) {
    with_app(|app| {
        let (tx, ty) = app.camera.to_tile(x, y, &app.fb);
        let tile = (tx.floor() as i32, ty.floor() as i32);
        let on_map = tile.0 >= 0
            && tile.1 >= 0
            && (tile.0 as usize) < MAP_WIDTH
            && (tile.1 as usize) < MAP_HEIGHT;
        app.selected = on_map.then_some(tile);
    });
}

#[no_mangle]
pub extern "C" fn text_ptr() -> *const u8 {
    with_app(|app| app.text.as_ptr())
}

/// The side panel, as tab-separated lines:
/// `date<TAB>text`, `civ<TAB>#rrggbb<TAB>name<TAB>cities<TAB>people<TAB>settlers`,
/// `event<TAB>text` and `tile<TAB>description`, with the description's line breaks
/// replaced by `|`.
#[no_mangle]
pub extern "C" fn panel_text() -> usize {
    with_app(|app| {
        let mut out = format!("date\t{}\n", crate::sim::format_date(app.sim.month()));
        for civ in app.sim.civ_summaries() {
            let [r, g, b] = civ.color;
            out += &format!(
                "civ\t#{r:02x}{g:02x}{b:02x}\t{}\t{}\t{}\t{}\n",
                civ.name, civ.cities, civ.population, civ.settlers
            );
        }
        for event in app.sim.events().iter().take(8) {
            out += &format!("event\t{event}\n");
        }
        if let Some((x, y)) = app.selected {
            out += &format!("tile\t{}\n", app.sim.describe_tile(x, y).replace('\n', "|"));
        }
        hand_over(app, out)
    })
}

/// City names to draw over the map: `x<TAB>y<TAB>#rrggbb<TAB>text` per line.
#[no_mangle]
pub extern "C" fn labels_text() -> usize {
    with_app(|app| {
        let mut out = String::new();
        for label in render::city_labels(&app.sim, &app.camera, &app.fb) {
            let [r, g, b] = label.color;
            out += &format!(
                "{}\t{}\t#{r:02x}{g:02x}{b:02x}\t{}\n",
                label.x, label.y, label.text
            );
        }
        hand_over(app, out)
    })
}
