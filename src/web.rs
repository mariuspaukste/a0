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
use crate::timelines::Timelines;

#[link(wasm_import_module = "env")]
extern "C" {
    /// Provided by the page (`env.console_error`).
    fn console_error(ptr: *const u8, len: usize);
}

struct App {
    /// Your timeline, and once you change history, the unchanged one.
    t: Timelines,
    /// Whether the map and the World tab show the unchanged timeline.
    unchanged_view: bool,
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

/// The timeline on the map: yours, or the unchanged one when the player looks at it.
fn shown(t: &Timelines, unchanged_view: bool) -> &Simulation {
    match &t.unchanged {
        Some(u) if unchanged_view => u,
        _ => &t.yours,
    }
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
    let t = Timelines::new(setting, seed as u64);
    let fb = Framebuffer::new(width as usize, height as usize);
    let camera = Camera::fit(t.yours.width(), t.yours.height(), &fb);
    let app = App {
        t,
        unchanged_view: false,
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
        app.t.step(months);
        if app.unit.is_some_and(|u| !app.t.yours.unit_exists(u)) {
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
            shown(&app.t, app.unchanged_view),
            &app.camera,
            &mut app.fb,
            app.selected,
            app.unit.filter(|_| !app.unchanged_view),
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
        // Units in the unchanged timeline can't be given orders: it's left alone.
        let mine = !app.unchanged_view;
        let under_finger = app.t.yours.unit_near(tx, ty, radius).filter(|_| mine);
        if let Some(unit) = app.unit.filter(|&u| mine && app.t.yours.unit_exists(u)) {
            if under_finger == Some(unit) {
                app.unit = None;
                app.message.clear();
                return 3;
            }
            app.message = app
                .t
                .yours
                .order(unit, tx.floor() as i32, ty.floor() as i32);
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
            && (tile.0 as usize) < app.t.yours.width()
            && (tile.1 as usize) < app.t.yours.height();
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
            app.t.yours.automate(unit);
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
        app.message = app.t.change_history(i as usize, at);
    });
}

/// Replays up to `months` months of the unchanged timeline, after your first change.
/// Returns how many months it still has to go.
#[no_mangle]
pub extern "C" fn catch_up(months: u32) -> u32 {
    with_app(|app| app.t.catch_up(months))
}

/// Shows the unchanged timeline on the map and in the World tab (1), or yours (0).
#[no_mangle]
pub extern "C" fn view_unchanged(on: u32) {
    with_app(|app| {
        app.unchanged_view = on == 1 && app.t.unchanged.is_some();
        if app.unchanged_view {
            app.unit = None;
        }
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
/// - `cmp<TAB>year of your first change<TAB>months the unchanged timeline is behind
///   <TAB>showing it (1 or 0)`, once you have changed history
/// - `econ<TAB>year<TAB>population<TAB>real population<TAB>GDP<TAB>real GDP<TAB>GDP per
///   head<TAB>consumption per head<TAB>% fed<TAB>exports<TAB>Company revenue<TAB>population
///   unchanged<TAB>GDP unchanged`, for the last year added up (the last two are empty
///   until the unchanged timeline has got there)
/// - `sector<TAB>name<TAB>gross output`
/// - `state<TAB>#rrggbb<TAB>name<TAB>population<TAB>GDP<TAB>per head<TAB>% fed`
/// - `chron<TAB>outcome<TAB>year<TAB>title<TAB>what happened<TAB>real history<TAB>outcome
///   unchanged<TAB>what happened unchanged`, newest first; outcomes are `same`, `diff`,
///   `none` or `you`, and the last two are empty unless it went differently there
/// - `next<TAB>year<TAB>title` for events still to come
/// - `whatif<TAB>index<TAB>needs a tile (1 or 0)<TAB>title<TAB>description`
#[no_mangle]
pub extern "C" fn panel_text() -> usize {
    with_app(|app| {
        let sim = shown(&app.t, app.unchanged_view);
        let rules = sim.rules();
        let mut out = format!("date\t{}\n", format_date(sim.month()));
        out += &format!("title\t{}\n", rules.title);
        out += &format!(
            "heads\t{}\t{}\n",
            rules.locals_heading, rules.powers_heading
        );
        for civ in sim.civ_summaries() {
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
        for event in sim.events().iter().take(8) {
            out += &format!("event\t{event}\n");
        }
        if let Some((x, y)) = app.selected {
            out += &format!("tile\t{}\n", sim.describe_tile(x, y).replace('\n', "|"));
        }
        if let Some(text) = app.unit.and_then(|u| app.t.yours.describe_unit(u)) {
            out += &format!("unit\t{}\n", text.replace('\n', "|"));
        }
        if !app.message.is_empty() {
            out += &format!("say\t{}\n", app.message);
        }
        if let Some(m) = app.t.changed_at {
            out += &format!(
                "cmp\t{}\t{}\t{}\n",
                era::year(m),
                app.t.behind(),
                app.unchanged_view as u8
            );
        }
        out += &history_lines(&app.t);
        hand_over(app, out)
    })
}

/// Town names to draw over the map: `x<TAB>y<TAB>#rrggbb<TAB>text` per line.
#[no_mangle]
pub extern "C" fn labels_text() -> usize {
    with_app(|app| {
        let mut out = String::new();
        let sim = shown(&app.t, app.unchanged_view);
        for label in render::city_labels(sim, &app.camera, &app.fb) {
            let [r, g, b] = label.color;
            out += &format!(
                "{}\t{}\t#{r:02x}{g:02x}{b:02x}\t{}\n",
                label.x, label.y, label.text
            );
        }
        hand_over(app, out)
    })
}

fn outcome_word(o: Outcome) -> &'static str {
    match o {
        Outcome::AsInHistory => "same",
        Outcome::Different => "diff",
        Outcome::DidNotHappen => "none",
        Outcome::Yours => "you",
    }
}

fn history_lines(t: &Timelines) -> String {
    let mut out = String::new();
    let sim = &t.yours;
    let setting = sim.rules().setting;
    let h = sim.history();
    if let Some(y) = h.economy.years.last() {
        let (pop, gdp) = t
            .unchanged_year(y.year)
            .map_or((String::new(), String::new()), |u| {
                (short(u.population), short(u.gdp))
            });
        out += &format!(
            "econ\t{}\t{}\t{}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\t{pop}\t{gdp}\n",
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
        let (theirs, their_text) = t
            .unchanged_entry(e)
            .map_or(("", String::new()), |u| (outcome_word(u.outcome), u.text));
        out += &format!(
            "chron\t{}\t{}\t{}\t{}\t{}\t{theirs}\t{their_text}\n",
            outcome_word(e.outcome),
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
/// population<TAB>GDP<TAB>real GDP<TAB>population unchanged<TAB>GDP unchanged` per
/// line, in people and 1990 dollars. The unchanged figures are -1 for years the
/// unchanged timeline hasn't reached, or before your first change.
#[no_mangle]
pub extern "C" fn series_text() -> usize {
    with_app(|app| {
        let mut out = String::new();
        let from = app.t.changed_at.map(era::year);
        let unchanged = app.t.unchanged.as_ref().map(|u| u.history());
        let theirs = unchanged
            .as_ref()
            .map(|h| &h.economy.years[..])
            .unwrap_or(&[]);
        for y in &app.t.yours.history().economy.years {
            let other = theirs
                .iter()
                .find(|u| u.year == y.year)
                .filter(|_| from.is_some_and(|f| y.year + 1 >= f));
            let (up, ug) = other.map_or((-1.0, -1.0), |u| (u.population, u.gdp));
            out += &format!(
                "{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{up:.0}\t{ug:.0}\n",
                y.year, y.population, y.real_population, y.gdp, y.real_gdp
            );
        }
        drop(unchanged);
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
