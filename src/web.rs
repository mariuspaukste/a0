//! Browser entry points. The page calls these exported functions directly, with no
//! wasm-bindgen in between, so only numbers cross the boundary. Text and pixels are
//! returned as a pointer into wasm memory plus a length, and the page reads them from
//! there.

use std::cell::RefCell;

use crate::economy::{self, short, INPUTS, SECTORS};
use crate::ecs::Entity;
use crate::era::{self, Setting};
use crate::history::{self, Outcome};
use crate::render::{self, Camera, Framebuffer, Overlay};
use crate::sim::{format_date, group, Position, Simulation};

#[link(wasm_import_module = "env")]
extern "C" {
    /// Provided by the page (`env.console_error`).
    fn console_error(ptr: *const u8, len: usize);
}

struct App {
    sim: Simulation,
    camera: Camera,
    fb: Framebuffer,
    selected: Option<(i32, i32)>,
    /// The ship or settlers the player has picked to give orders to.
    unit: Option<Entity>,
    /// The reply to the player's last order.
    message: String,
    overlay: Overlay,
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

/// Starts a new world. `scenario` is 0 for the New World, 1 for India.
#[no_mangle]
pub extern "C" fn init(seed: u32, width: u32, height: u32, scenario: u32) {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        unsafe { console_error(message.as_ptr(), message.len()) };
    }));
    let setting = if scenario == 1 {
        Setting::India
    } else {
        Setting::NewWorld
    };
    let sim = Simulation::scenario(setting, seed as u64);
    let fb = Framebuffer::new(width as usize, height as usize);
    let camera = Camera::fit(sim.width(), sim.height(), &fb);
    let app = App {
        sim,
        camera,
        fb,
        selected: None,
        unit: None,
        message: String::new(),
        overlay: Overlay::None,
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
        if app.unit.is_some_and(|u| !app.sim.unit_exists(u)) {
            app.unit = None;
            app.message.clear();
        }
    });
}

/// Draws a frame and returns a pointer to its RGBA pixels.
#[no_mangle]
pub extern "C" fn render() -> *const u32 {
    with_app(|app| {
        render::render(
            &app.sim,
            &app.camera,
            &mut app.fb,
            app.selected,
            app.unit,
            app.overlay,
        );
        app.fb.pixels.as_ptr()
    })
}

/// Picks what's drawn over the map: 0 for nothing, 1 for land values.
#[no_mangle]
pub extern "C" fn set_overlay(overlay: u32) {
    with_app(|app| {
        app.overlay = match overlay {
            1 => Overlay::LandValue,
            _ => Overlay::None,
        }
    });
}

#[no_mangle]
pub extern "C" fn pan(dx: f32, dy: f32) {
    with_app(|app| app.camera.pan(dx, dy));
}

#[no_mangle]
pub extern "C" fn zoom(factor: f32, x: f32, y: f32) {
    with_app(|app| app.camera.zoom_at(factor, x, y, &app.fb));
}

/// A tap or click at a screen position, with `reach` the radius in screen pixels a
/// finger covers. With a unit picked, it orders the unit there (tapping the unit
/// itself lets it go). Otherwise it picks the ship or settlers under the finger, or
/// failing that inspects the tile.
///
/// Returns 0 when a tile was picked, 1 for a unit, 2 for an order, 3 for nothing.
#[no_mangle]
pub extern "C" fn tap(x: f32, y: f32, reach: f32) -> u32 {
    with_app(|app| {
        let (tx, ty) = app.camera.to_tile(x, y, &app.fb);
        let radius = (reach / app.camera.zoom).max(0.8);
        let under_finger = app.sim.unit_near(tx, ty, radius);
        if let Some(unit) = app.unit.filter(|&u| app.sim.unit_exists(u)) {
            if under_finger == Some(unit) {
                app.unit = None;
                app.message.clear();
                return 3;
            }
            app.message = app.sim.order(unit, tx.floor() as i32, ty.floor() as i32);
            return 2;
        }
        if let Some(unit) = under_finger {
            app.unit = Some(unit);
            app.message = "Tap the map to send it somewhere.".to_string();
            return 1;
        }
        let tile = (tx.floor() as i32, ty.floor() as i32);
        let on_map = tile.0 >= 0
            && tile.1 >= 0
            && (tile.0 as usize) < app.sim.width()
            && (tile.1 as usize) < app.sim.height();
        app.selected = on_map.then_some(tile);
        if on_map {
            0
        } else {
            3
        }
    })
}

/// Lets go of the picked unit.
#[no_mangle]
pub extern "C" fn release_unit() {
    with_app(|app| {
        app.unit = None;
        app.message.clear();
    });
}

/// Hands the picked unit back to the simulation.
#[no_mangle]
pub extern "C" fn automate() {
    with_app(|app| {
        if let Some(unit) = app.unit {
            app.sim.automate(unit);
            app.message = "Back on automatic.".to_string();
        }
    });
}

/// Changes history with what-if `i` (see [`history::what_ifs`]), at the picked tile
/// for those that act on a place.
#[no_mangle]
pub extern "C" fn change_history(i: u32) {
    with_app(|app| {
        let at = app.selected.map(|(x, y)| Position { x, y });
        app.message = app.sim.change_history(i as usize, at);
    });
}

#[no_mangle]
pub extern "C" fn text_ptr() -> *const u8 {
    with_app(|app| app.text.as_ptr())
}

/// The side panel, as tab-separated lines:
/// - `date<TAB>text` and `title<TAB>scenario`
/// - `heads<TAB>local peoples heading<TAB>European powers heading`
/// - `civ<TAB>#rrggbb<TAB>local (1 or 0)<TAB>name<TAB>towns<TAB>people<TAB>status`
/// - `event<TAB>text`
/// - `tile<TAB>description` and `unit<TAB>description`, with line breaks replaced by `|`
/// - `say<TAB>reply to the last order`
/// - `econ<TAB>year<TAB>population<TAB>real population<TAB>GDP<TAB>real GDP<TAB>GDP per
///   head<TAB>consumption per head<TAB>% fed<TAB>exports<TAB>Company revenue`, for the
///   last year added up
/// - `sector<TAB>name<TAB>gross output`
/// - `state<TAB>#rrggbb<TAB>name<TAB>population<TAB>GDP<TAB>per head<TAB>% fed`
/// - `chron<TAB>outcome<TAB>year<TAB>title<TAB>what happened<TAB>real history`, newest
///   first; outcome is `same`, `diff`, `none` or `you`
/// - `next<TAB>year<TAB>title` for events still to come
/// - `whatif<TAB>index<TAB>needs a tile (1 or 0)<TAB>title<TAB>description`
#[no_mangle]
pub extern "C" fn panel_text() -> usize {
    with_app(|app| {
        let rules = app.sim.rules();
        let mut out = format!("date\t{}\n", format_date(app.sim.month()));
        out += &format!("title\t{}\n", rules.title);
        out += &format!(
            "heads\t{}\t{}\n",
            rules.locals_heading, rules.powers_heading
        );
        for civ in app.sim.civ_summaries() {
            let [r, g, b] = civ.color;
            out += &format!(
                "civ\t#{r:02x}{g:02x}{b:02x}\t{}\t{}\t{}\t{}\t{}\n",
                civ.local as u8,
                civ.name,
                civ.cities,
                group(civ.population),
                civ.status
            );
        }
        for event in app.sim.events().iter().take(8) {
            out += &format!("event\t{event}\n");
        }
        if let Some((x, y)) = app.selected {
            out += &format!("tile\t{}\n", app.sim.describe_tile(x, y).replace('\n', "|"));
        }
        if let Some(text) = app.unit.and_then(|u| app.sim.describe_unit(u)) {
            out += &format!("unit\t{}\n", text.replace('\n', "|"));
        }
        if !app.message.is_empty() {
            out += &format!("say\t{}\n", app.message);
        }
        out += &history_lines(&app.sim);
        hand_over(app, out)
    })
}

/// Town names to draw over the map: `x<TAB>y<TAB>#rrggbb<TAB>text` per line.
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

fn history_lines(sim: &Simulation) -> String {
    let mut out = String::new();
    let setting = sim.rules().setting;
    let h = sim.history();
    if let Some(y) = h.economy.years.last() {
        out += &format!(
            "econ\t{}\t{}\t{}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\n",
            y.year,
            short(y.population),
            short(y.real_population),
            short(y.gdp),
            short(y.real_gdp),
            y.per_head(),
            y.consumption_per_head(),
            100.0 * y.fed(),
            short(y.exports),
            short(y.tribute),
        );
        for (name, output) in SECTORS.iter().zip(y.output) {
            out += &format!("sector\t{name}\t{}\n", short(output));
        }
        let mut states = y.states.clone();
        states.sort_by(|a, b| b.gdp.total_cmp(&a.gdp));
        for s in states {
            let [r, g, b] = s.color;
            out += &format!(
                "state\t#{r:02x}{g:02x}{b:02x}\t{}\t{}\t{}\t{:.0}\t{:.0}\n",
                s.name,
                short(s.population),
                short(s.gdp),
                s.gdp / s.population.max(1.0),
                100.0 * s.fed,
            );
        }
    }
    for e in h.chronicle.iter().rev().take(12) {
        let outcome = match e.outcome {
            Outcome::AsInHistory => "same",
            Outcome::Different => "diff",
            Outcome::DidNotHappen => "none",
            Outcome::Yours => "you",
        };
        out += &format!(
            "chron\t{outcome}\t{}\t{}\t{}\t{}\n",
            era::year(e.month),
            e.title,
            e.text,
            e.real
        );
    }
    drop(h);
    for e in history::coming(&sim.world, 3) {
        out += &format!("next\t{}\t{}\n", era::year(e.date), e.title);
    }
    let what_ifs = history::what_ifs(setting);
    for i in sim.what_ifs_now() {
        let w = &what_ifs[i];
        out += &format!(
            "whatif\t{i}\t{}\t{}\t{}\n",
            w.needs_place as u8, w.title, w.text
        );
    }
    out
}

/// The economy's yearly accounts, for the charts: `year<TAB>population<TAB>real
/// population<TAB>GDP<TAB>real GDP` per line, in people and 1990 dollars.
#[no_mangle]
pub extern "C" fn series_text() -> usize {
    with_app(|app| {
        let mut out = String::new();
        for y in &app.sim.history().economy.years {
            out += &format!(
                "{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\n",
                y.year, y.population, y.real_population, y.gdp, y.real_gdp
            );
        }
        hand_over(app, out)
    })
}

/// The input–output table: `sector<TAB>name<TAB>a(0,j)…a(3,j)<TAB>value added
/// <TAB>multiplier` per sector j (the inputs sector j buys per unit of output, and the
/// column sum of the Leontief inverse).
#[no_mangle]
pub extern "C" fn economy_text() -> usize {
    with_app(|app| {
        let inverse = economy::leontief_inverse(&INPUTS);
        let v = economy::value_added();
        let mut out = String::new();
        for (j, name) in SECTORS.iter().enumerate() {
            let column: Vec<String> = (0..4).map(|i| format!("{:.2}", INPUTS[i][j])).collect();
            let multiplier: f64 = (0..4).map(|i| inverse[i][j]).sum();
            out += &format!(
                "sector\t{name}\t{}\t{:.2}\t{multiplier:.2}\n",
                column.join("\t"),
                v[j]
            );
        }
        hand_over(app, out)
    })
}
