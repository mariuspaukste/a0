//! Software renderer: draws the world into a buffer of RGBA pixels that the browser
//! copies onto a canvas. No graphics library involved.
//!
//! The map follows the seasons: snow lies in the north in winter, forests turn in the
//! autumn, and fields go from bare earth to green to gold at the September harvest.

use std::collections::BTreeMap;

use crate::ecs::{Entity, World};
use crate::era::{self, Culture, Setting};
use crate::history::War;
use crate::sim::{
    group, Building, Burning, City, Civ, Drought, LandValue, Owner, Position, Settler, Ship,
    Simulation, Tiles,
};
use crate::trade::Trade;
use crate::world::{hash, Map, Terrain};

type Rgb = [u8; 3];

const SEA: Rgb = [20, 44, 92];
const ROAD: Rgb = [128, 108, 84];
const WOOD: Rgb = [96, 66, 40];
const EARTH: Rgb = [150, 120, 84];
const SNOW: Rgb = [236, 240, 246];
const ICE: Rgb = [196, 214, 232];
const WORKSHOP: Rgb = [116, 104, 146];
const CHURCH: Rgb = [238, 234, 224];
const FIRE: Rgb = [250, 150, 40];
const SAIL: Rgb = [244, 240, 226];
const WHITE: Rgb = [250, 250, 250];
const GOLD: Rgb = [255, 204, 51];
const INK: Rgb = [16, 18, 28];
/// Plague and other epidemics.
const SICK: Rgb = [238, 64, 64];
/// Hunger, and failed monsoons.
const HUNGRY: Rgb = [244, 168, 56];

/// Below this zoom (screen pixels per tile) buildings are too small to make out, so
/// towns are drawn as markers instead.
const DETAIL_ZOOM: f32 = 5.0;
/// Below this zoom towns get a marker, and states their name.
const OVERVIEW_ZOOM: f32 = 9.0;

pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    /// Device pixels per CSS pixel, so markers and lines keep their size on screens
    /// of any density.
    pub scale: f32,
    /// One `u32` per pixel. On little-endian wasm its bytes are R, G, B, A, which is
    /// exactly the layout the canvas `ImageData` expects.
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            pixels: vec![0; width * height],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let scale = self.scale;
        *self = Self::new(width, height);
        self.scale = scale;
    }

    /// A filled circle.
    fn disc(&mut self, cx: f32, cy: f32, r: f32, color: Rgb) {
        let y0 = (cy - r).floor() as i32;
        let y1 = (cy + r).ceil() as i32;
        for y in y0..y1 {
            let dy = y as f32 + 0.5 - cy;
            let w2 = r * r - dy * dy;
            if w2 <= 0.0 {
                continue;
            }
            let w = w2.sqrt();
            let row = Rect::new((cx - w).round() as i32, y, (cx + w).round() as i32, y + 1);
            self.fill(row, color);
        }
    }

    /// Diagonal stripes of `color` over `r`, continuous from tile to tile.
    fn hatch(&mut self, r: Rect, color: Rgb, period: i32, width: i32) {
        let x0 = r.x0.clamp(0, self.width as i32);
        let x1 = r.x1.clamp(0, self.width as i32);
        let y0 = r.y0.clamp(0, self.height as i32);
        let y1 = r.y1.clamp(0, self.height as i32);
        let c = pack(color);
        for y in y0..y1 {
            for x in x0..x1 {
                if (x + y).rem_euclid(period) < width {
                    self.pixels[y as usize * self.width + x as usize] = c;
                }
            }
        }
    }

    /// A square dot of side `w` centred on a point.
    fn dot(&mut self, x: f32, y: f32, w: f32, color: Rgb) {
        let h = w / 2.0;
        let r = Rect::new(
            (x - h).round() as i32,
            (y - h).round() as i32,
            (x + h).round().max((x - h).round() + 1.0) as i32,
            (y + h).round().max((y - h).round() + 1.0) as i32,
        );
        self.fill(r, color);
    }

    /// A straight line of width `w`.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), w: f32, color: Rgb) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = (len / (w * 0.5).max(0.5)).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.dot(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, w, color);
        }
    }

    /// A circle outline of thickness `t`, dashed if `dash` is set.
    fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32, color: Rgb, dash: bool) {
        let steps = ((std::f32::consts::TAU * r / (t * 0.6)) as usize).clamp(16, 4000);
        let half = t / 2.0;
        for i in 0..steps {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            // Dashes and gaps about four line-widths long.
            if dash && ((a * r) / (4.0 * t)) as usize % 2 == 1 {
                continue;
            }
            let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
            let dot = Rect::new(
                (x - half).round() as i32,
                (y - half).round() as i32,
                (x + half).round() as i32,
                (y + half).round() as i32,
            );
            self.fill(dot, color);
        }
    }

    fn clear(&mut self, color: Rgb) {
        self.pixels.fill(pack(color));
    }

    /// Blends `color` over the pixels in `r`, by `t` from 0 (none) to 1 (all).
    fn tint(&mut self, r: Rect, color: Rgb, t: f32) {
        let x0 = r.x0.clamp(0, self.width as i32) as usize;
        let x1 = r.x1.clamp(0, self.width as i32) as usize;
        let y0 = r.y0.clamp(0, self.height as i32) as usize;
        let y1 = r.y1.clamp(0, self.height as i32) as usize;
        for y in y0..y1 {
            for p in &mut self.pixels[y * self.width + x0..y * self.width + x1] {
                let old = [
                    (*p & 255) as u8,
                    (*p >> 8 & 255) as u8,
                    (*p >> 16 & 255) as u8,
                ];
                *p = pack(mix(old, color, t));
            }
        }
    }

    fn fill(&mut self, r: Rect, color: Rgb) {
        let x0 = r.x0.clamp(0, self.width as i32) as usize;
        let x1 = r.x1.clamp(0, self.width as i32) as usize;
        let y0 = r.y0.clamp(0, self.height as i32) as usize;
        let y1 = r.y1.clamp(0, self.height as i32) as usize;
        if x0 >= x1 {
            return;
        }
        let c = pack(color);
        for y in y0..y1 {
            self.pixels[y * self.width + x0..y * self.width + x1].fill(c);
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Rect {
    fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    fn width(self) -> i32 {
        self.x1 - self.x0
    }

    fn height(self) -> i32 {
        self.y1 - self.y0
    }

    /// Shrinks the rect on every side by `fraction` of its width.
    fn inset(self, fraction: f32) -> Self {
        self.inset_xy(fraction, fraction)
    }

    /// Shrinks the rect by `fx` of its width on the left and right, and `fy` of its
    /// height on the top and bottom.
    fn inset_xy(self, fx: f32, fy: f32) -> Self {
        let dx = (self.width() as f32 * fx) as i32;
        let dy = (self.height() as f32 * fy) as i32;
        Self::new(self.x0 + dx, self.y0 + dy, self.x1 - dx, self.y1 - dy)
    }

    /// A horizontal slice from `top` to `bottom`, as fractions of the height.
    fn band(self, top: f32, bottom: f32) -> Self {
        let h = self.height() as f32;
        Self::new(
            self.x0,
            self.y0 + (h * top) as i32,
            self.x1,
            self.y0 + (h * bottom) as i32,
        )
    }
}

fn pack([r, g, b]: Rgb) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xFF00_0000
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

fn shade(c: Rgb, f: f32) -> Rgb {
    c.map(|v| (v as f32 * f).min(255.0) as u8)
}

/// What the time of year does to the map.
struct Season {
    month: u32,
    setting: Setting,
    harvest_month: u32,
}

impl Season {
    /// How much snow lies at this tile, with a ragged edge to the snow line.
    fn snow(&self, x: i32, y: i32, latitude: f32) -> f32 {
        let depth = era::winter(self.setting, self.month, latitude);
        if depth == 0.0 {
            return 0.0;
        }
        (depth * 1.3 - 0.25 + 0.3 * hash(11, x, y)).clamp(0.0, 1.0)
    }

    /// The color of cultivated ground through the year: bare before sowing, green as
    /// it grows, ripe at harvest, stubble after.
    fn crop(&self, ripe: Rgb) -> Rgb {
        let green = [92, 148, 60];
        if self.setting == Setting::India {
            // Two crops a year: rabi wheat through the cool season, harvested in March;
            // bare, baked fields in the hot months; kharif rice and millet with the
            // monsoon, harvested in October.
            return match self.month % 12 {
                10 | 11 | 0 => green,
                1 => mix(green, ripe, 0.5),
                2 | 9 => ripe,
                3 | 4 => [150, 118, 80],
                _ => [104, 156, 64],
            };
        }
        // The New World harvest is in September; shift other calendars to match.
        let month = (self.month + 20 - self.harvest_month) % 12;
        match month {
            0..=2 | 11 => [118, 92, 66],
            3 | 4 => [128, 140, 78],
            5 | 6 => [92, 148, 60],
            7 => mix([92, 148, 60], ripe, 0.6),
            8 => ripe,
            _ => [164, 138, 92],
        }
    }
}

fn terrain_color(t: Terrain, x: i32, y: i32, latitude: f32, season: &Season) -> Rgb {
    let base = match t {
        Terrain::DeepWater => [28, 58, 116],
        Terrain::Water => [46, 96, 168],
        Terrain::Sand => [222, 206, 148],
        Terrain::Grass => [110, 166, 82],
        Terrain::Forest => [52, 116, 62],
        Terrain::Hills => [148, 136, 92],
        Terrain::Mountain => [130, 126, 126],
    };
    // A little per-tile noise so large areas don't look flat.
    let mut c = shade(base, 0.95 + 0.1 * hash(7, x, y));
    let month = season.month % 12;
    if season.setting == Setting::India {
        return match t {
            // The Himalaya keep their snow all year.
            Terrain::Mountain => mix(c, SNOW, 0.35 + 0.4 * hash(11, x, y)),
            // The land dries out before the monsoon and turns lush after it.
            Terrain::Grass | Terrain::Hills => match month {
                3..=5 => mix(c, [184, 164, 96], 0.45),
                6..=9 => mix(c, [70, 160, 60], 0.25),
                _ => c,
            },
            _ => c,
        };
    }
    if t == Terrain::Forest && (month == 8 || month == 9) {
        // Autumn color, strongest in the north.
        let amount = if month == 9 { 0.55 } else { 0.25 } * (1.0 - latitude);
        let leaf = if hash(13, x, y) > 0.5 {
            [196, 96, 36]
        } else {
            [214, 160, 48]
        };
        c = mix(c, leaf, amount * (0.6 + 0.4 * hash(17, x, y)));
    }
    if t == Terrain::Grass && (6..=8).contains(&month) {
        // Late-summer grass dries out in the south.
        c = mix(c, [168, 168, 86], 0.3 * latitude);
    }
    let snow = season.snow(x, y, latitude);
    if snow > 0.0 {
        c = match t {
            Terrain::DeepWater => c,
            // Coastal water and lakes freeze in the deepest northern winters.
            Terrain::Water => mix(c, ICE, (snow - 0.65) * 2.0),
            // Snow shows less among the trees.
            Terrain::Forest => mix(c, SNOW, snow * 0.5),
            _ => mix(c, SNOW, snow * 0.85),
        };
    }
    c
}

pub struct Camera {
    /// The tile coordinate at the center of the screen.
    pub x: f32,
    pub y: f32,
    /// Screen pixels per tile.
    pub zoom: f32,
}

impl Camera {
    const MIN_ZOOM: f32 = 2.0;
    const MAX_ZOOM: f32 = 48.0;

    /// Centered on the map, zoomed so the whole map fits on screen.
    pub fn fit(map_width: usize, map_height: usize, fb: &Framebuffer) -> Self {
        let zoom = (fb.width as f32 / map_width as f32).min(fb.height as f32 / map_height as f32);
        Self {
            x: map_width as f32 / 2.0,
            y: map_height as f32 / 2.0,
            zoom: zoom.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM),
        }
    }

    /// Fits the whole map into the part of the screen from (`left`, `top`) to (`right`,
    /// `bottom`), in screen pixels: the part not covered by panels.
    pub fn fit_area(
        map_width: usize,
        map_height: usize,
        fb: &Framebuffer,
        (left, top, right, bottom): (f32, f32, f32, f32),
    ) -> Self {
        let (w, h) = ((right - left).max(50.0), (bottom - top).max(50.0));
        let zoom = (w / map_width as f32)
            .min(h / map_height as f32)
            .clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        // Put the map's middle at the middle of the area.
        let (ax, ay) = ((left + right) / 2.0, (top + bottom) / 2.0);
        Self {
            x: map_width as f32 / 2.0 - (ax - fb.width as f32 / 2.0) / zoom,
            y: map_height as f32 / 2.0 - (ay - fb.height as f32 / 2.0) / zoom,
            zoom,
        }
    }

    /// Centres the view on a tile, zooming in far enough to see its towns.
    pub fn focus(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.zoom = self.zoom.max(8.0);
    }

    pub fn to_screen(&self, tx: f32, ty: f32, fb: &Framebuffer) -> (f32, f32) {
        (
            (tx - self.x) * self.zoom + fb.width as f32 / 2.0,
            (ty - self.y) * self.zoom + fb.height as f32 / 2.0,
        )
    }

    pub fn to_tile(&self, sx: f32, sy: f32, fb: &Framebuffer) -> (f32, f32) {
        (
            (sx - fb.width as f32 / 2.0) / self.zoom + self.x,
            (sy - fb.height as f32 / 2.0) / self.zoom + self.y,
        )
    }

    /// Moves the view by a drag of `dx`, `dy` screen pixels.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.x -= dx / self.zoom;
        self.y -= dy / self.zoom;
    }

    /// Zooms while keeping the tile under the cursor in place.
    pub fn zoom_at(&mut self, factor: f32, sx: f32, sy: f32, fb: &Framebuffer) {
        let before = self.to_tile(sx, sy, fb);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let after = self.to_tile(sx, sy, fb);
        self.x += before.0 - after.0;
        self.y += before.1 - after.1;
    }

    fn tile_rect(&self, x: i32, y: i32, fb: &Framebuffer) -> Rect {
        let (x0, y0) = self.to_screen(x as f32, y as f32, fb);
        let (x1, y1) = self.to_screen((x + 1) as f32, (y + 1) as f32, fb);
        Rect::new(
            x0.floor() as i32,
            y0.floor() as i32,
            x1.floor() as i32,
            y1.floor() as i32,
        )
    }
}

/// The people a town belongs to: their id, color and culture, and whether the town
/// builds in brick.
#[derive(Clone, Copy)]
struct Folk {
    civ: Entity,
    color: Rgb,
    culture: Culture,
    brick: bool,
}

/// Extra information drawn over the map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlay {
    #[default]
    None,
    /// Town lots colored from red (cheap) through yellow to green (dear).
    LandValue,
}

/// What's drawn over the map besides the land, and the time for things that move.
#[derive(Clone, Copy, Debug)]
pub struct Layers {
    /// Trade routes, with goods moving along them.
    pub routes: bool,
    /// Wars: battle sites, advancing armies, and towns being fought over.
    pub wars: bool,
    /// Seconds, for animation.
    pub time: f32,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            routes: true,
            wars: true,
            time: 0.0,
        }
    }
}

/// How long after a war ends the map still marks it, in months.
const WAR_AFTERGLOW: u32 = 6;

/// The wars to draw now: those going on, and those just over.
fn wars_now(sim: &Simulation) -> Vec<War> {
    let month = sim.month();
    sim.history()
        .wars
        .iter()
        .filter(|w| w.start <= month && month <= w.end + WAR_AFTERGLOW)
        .cloned()
        .collect()
}

/// Points along a gentle curve from `a` to `b`, bowed to the left of the way it
/// goes, so goods going each way between two towns travel on separate lines.
/// Returns (x, y, distance along) every `step` pixels.
fn curve(a: (f32, f32), b: (f32, f32), step: f32) -> Vec<(f32, f32, f32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return Vec::new();
    }
    let bow = 0.16 * len;
    let c = (
        (a.0 + b.0) / 2.0 + dy / len * bow,
        (a.1 + b.1) / 2.0 - dx / len * bow,
    );
    let n = (len * 1.1 / step).ceil().max(2.0) as usize;
    let mut out = Vec::with_capacity(n + 1);
    let mut along = 0.0;
    let mut prev = a;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let p = (
            u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
            u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
        );
        along += ((p.0 - prev.0).powi(2) + (p.1 - prev.1).powi(2)).sqrt();
        prev = p;
        out.push((p.0, p.1, along));
    }
    out
}

/// How states are colored on the map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// Boldly when zoomed out, to see who rules what, fading close in to show the land.
    #[default]
    Auto,
    /// Boldly at every zoom.
    States,
    /// Hardly at all: the land as it looks, with states outlined.
    Land,
}

/// How much a tile takes on its state's color.
fn state_tint(style: Style, zoom: f32) -> f32 {
    match style {
        Style::States => 0.6,
        Style::Land => 0.08,
        Style::Auto => {
            let t = ((zoom - 4.0) / 10.0).clamp(0.0, 1.0);
            0.6 + (0.16 - 0.6) * t
        }
    }
}

/// The overlay color for a land value from 0 to 100. Most lots are worth between 30
/// and 75, so the colors span that.
fn value_color(value: u8) -> Rgb {
    let t = ((value as f32 - 30.0) / 45.0).clamp(0.0, 1.0);
    let (low, mid, high) = ([200, 56, 48], [236, 204, 72], [56, 172, 84]);
    if t < 0.5 {
        mix(low, mid, t * 2.0)
    } else {
        mix(mid, high, (t - 0.5) * 2.0)
    }
}

fn town_folk(sim: &Simulation) -> BTreeMap<Entity, Folk> {
    let civs = sim.world.read::<Civ>();
    let owners = sim.world.read::<Owner>();
    sim.world
        .read::<City>()
        .iter()
        .filter_map(|(e, city)| {
            let civ = owners.get(e)?.0;
            let c = civs.get(civ)?;
            let folk = Folk {
                civ,
                color: c.color,
                culture: c.culture,
                brick: city.brick_since.is_some(),
            };
            Some((e, folk))
        })
        .collect()
}

pub fn render(
    sim: &Simulation,
    cam: &Camera,
    fb: &mut Framebuffer,
    selected: Option<(i32, i32)>,
    unit: Option<Entity>,
    overlay: Overlay,
    style: Style,
    layers: Layers,
) {
    fb.clear(SEA);
    let map = sim.world.resource::<Map>();
    let tiles = sim.world.resource::<Tiles>();
    let land = sim.world.resource::<LandValue>();
    let burning = sim.world.resource::<Burning>();
    let rules = sim.rules();
    let season = Season {
        month: sim.month(),
        setting: rules.setting,
        harvest_month: rules.harvest_month,
    };
    let folk_of = town_folk(sim);
    let tile_folk = |x: i32, y: i32| tiles.owner(x, y).and_then(|c| folk_of.get(&c).copied());

    let (fx0, fy0) = cam.to_tile(0.0, 0.0, fb);
    let (fx1, fy1) = cam.to_tile(fb.width as f32, fb.height as f32, fb);
    let x0 = (fx0.floor() as i32).max(0);
    let y0 = (fy0.floor() as i32).max(0);
    let x1 = (fx1.ceil() as i32).min(map.width as i32);
    let y1 = (fy1.ceil() as i32).min(map.height as i32);
    let u = fb.scale;
    let border = ((cam.zoom / 8.0) as i32).max(u.round() as i32).max(1);
    let tint = state_tint(style, cam.zoom);
    let detail = cam.zoom >= DETAIL_ZOOM;
    // On bold colors a dark border reads best; on the land, a light one.
    let edge_of = |c: Rgb| {
        if tint >= 0.35 {
            shade(c, 0.45)
        } else {
            shade(c, 1.15)
        }
    };
    // A failed monsoon's area is washed in dry ochre, and ringed below.
    let drought = sim.world.resource::<Drought>().area;
    let parched =
        |x: i32, y: i32| drought.is_some_and(|(c, r2)| (x - c.x).pow(2) + (y - c.y).pow(2) <= r2);
    // Towns being fought over are hatched in the attacker's color.
    let wars = if layers.wars {
        wars_now(sim)
    } else {
        Vec::new()
    };
    let month = sim.month();
    let fought: BTreeMap<Entity, Rgb> = wars
        .iter()
        .filter(|w| w.active(month))
        .flat_map(|w| w.towns.iter().map(move |t| (*t, w.color)))
        .collect();
    let stripe = (4.0 * u).round().max(2.0) as i32;

    for y in y0..y1 {
        let latitude = era::latitude(y, map.height);
        for x in x0..x1 {
            let r = cam.tile_rect(x, y, fb);
            let folk = tile_folk(x, y);
            let terrain = map.get(x as usize, y as usize);
            let mut color = terrain_color(terrain, x, y, latitude, &season);
            if let Some(f) = folk {
                color = mix(color, f.color, tint);
            }
            if parched(x, y) {
                color = mix(color, [196, 138, 58], 0.3);
            }
            fb.fill(r, color);
            if let Some(c) = tiles.owner(x, y).and_then(|t| fought.get(&t)) {
                fb.hatch(r, shade(*c, 0.8), stripe * 2, stripe.max(2) / 2 + 1);
            }
            let snow = season.snow(x, y, latitude);
            let lit = tiles.building(x, y) == Building::Ruins && burning.is_burning(x, y);
            if detail {
                draw_building(fb, &tiles, &season, snow, (x, y), r, folk, lit);
            } else if lit {
                fb.fill(r, FIRE);
            }
            if overlay == Overlay::LandValue {
                match land.get(x, y) {
                    Some(value) => fb.tint(r, value_color(value), 0.55),
                    None => fb.tint(r, [24, 28, 40], 0.45),
                }
            }

            // Draw the border on edges where the neighbor belongs to someone else.
            if let Some(f) = folk {
                let other =
                    |dx: i32, dy: i32| tile_folk(x + dx, y + dy).map(|o| o.civ) != Some(f.civ);
                let edge = edge_of(f.color);
                if other(-1, 0) {
                    fb.fill(Rect::new(r.x0, r.y0, r.x0 + border, r.y1), edge);
                }
                if other(1, 0) {
                    fb.fill(Rect::new(r.x1 - border, r.y0, r.x1, r.y1), edge);
                }
                if other(0, -1) {
                    fb.fill(Rect::new(r.x0, r.y0, r.x1, r.y0 + border), edge);
                }
                if other(0, 1) {
                    fb.fill(Rect::new(r.x0, r.y1 - border, r.x1, r.y1), edge);
                }
            }
        }
    }

    let civs = sim.world.read::<Civ>();
    let owners = sim.world.read::<Owner>();
    let positions = sim.world.read::<Position>();

    // Trade routes: a dark track with goods moving along it toward the buyer, in the
    // good's color, as wide as the trade is large.
    if layers.routes {
        let trade = sim.world.resource::<Trade>();
        let mut links: Vec<_> = trade.links.values().collect();
        links.sort_by(|a, b| a.flow.total_cmp(&b.flow));
        let period = 12.0 * u;
        let shift = (layers.time * 16.0 * u) % period;
        // Zoomed out, only the larger routes, so the network stays readable.
        let least = if cam.zoom < OVERVIEW_ZOOM { 0.6 } else { 0.0 };
        for l in links.into_iter().filter(|l| l.flow >= least) {
            let (Some(a), Some(b)) = (positions.get(l.from), positions.get(l.to)) else {
                continue;
            };
            let a = cam.to_screen(a.x as f32 + 0.5, a.y as f32 + 0.5, fb);
            let b = cam.to_screen(b.x as f32 + 0.5, b.y as f32 + 0.5, fb);
            let off = |p: (f32, f32)| {
                p.0 < -50.0
                    || p.1 < -50.0
                    || p.0 > fb.width as f32 + 50.0
                    || p.1 > fb.height as f32 + 50.0
            };
            if off(a) && off(b) {
                continue;
            }
            let w = (0.8 + l.flow.sqrt() * 0.9).min(4.0) * u;
            let points = curve(a, b, w * 0.6);
            for &(x, y, _) in &points {
                fb.dot(x, y, w + 2.0 * u, INK);
            }
            let color = l.good.color();
            for &(x, y, along) in &points {
                let dash = (along - shift).rem_euclid(period) < period * 0.55;
                fb.dot(x, y, w, if dash { color } else { shade(color, 0.45) });
            }
        }
    }

    // A failed monsoon: a dashed ring round the area whose harvest will be poor.
    if let Some((c, r2)) = sim.world.resource::<Drought>().area {
        let (cx, cy) = cam.to_screen(c.x as f32 + 0.5, c.y as f32 + 0.5, fb);
        let r = (r2 as f32).sqrt() * cam.zoom;
        fb.ring(cx, cy, r, 3.0 * u, INK, true);
        fb.ring(cx, cy, r, 2.0 * u, HUNGRY, true);
    }

    // Towns: a marker sized by population when zoomed out, and rings round any town
    // with plague (red) or hunger (orange) at every zoom.
    for (e, city) in sim.world.read::<City>().iter() {
        let (Some(p), Some(f)) = (positions.get(e), folk_of.get(&e)) else {
            continue;
        };
        let (cx, cy) = cam.to_screen(p.x as f32 + 0.5, p.y as f32 + 0.5, fb);
        if cx < -40.0 || cy < -40.0 || cx > fb.width as f32 + 40.0 || cy > fb.height as f32 + 40.0 {
            continue;
        }
        let size = marker_size(city.population) * u;
        if cam.zoom < OVERVIEW_ZOOM {
            fb.disc(cx, cy, size + u, INK);
            fb.disc(cx, cy, size, shade(f.color, 1.25));
            if city.population >= 1500 {
                fb.disc(cx, cy, size * 0.4, WHITE);
            }
        }
        let alarm = if city.outbreak.is_some() {
            Some(SICK)
        } else if city.starving {
            Some(HUNGRY)
        } else {
            None
        };
        if let Some(c) = alarm {
            let r = if cam.zoom < OVERVIEW_ZOOM {
                size + 3.5 * u
            } else {
                cam.zoom * 1.6
            };
            fb.ring(cx, cy, r, 2.6 * u, INK, false);
            fb.ring(cx, cy, r, 1.6 * u, c, false);
        }
    }

    let color_of = |e: Entity| {
        owners
            .get(e)
            .and_then(|o| civs.get(o.0))
            .map_or(WHITE, |c| c.color)
    };
    for (e, _) in sim.world.read::<Settler>().iter() {
        let Some(p) = positions.get(e) else {
            continue;
        };
        let r = cam.tile_rect(p.x, p.y, fb);
        fb.fill(r.inset(0.12), WHITE);
        fb.fill(r.inset(0.26), color_of(e));
    }
    for (e, ship) in sim.world.read::<Ship>().iter() {
        if let Some(p) = positions.get(e) {
            let r = cam.tile_rect(p.x, p.y, fb);
            draw_ship(fb, r, color_of(e), ship.passengers > 0);
        }
    }

    // Wars: an arrow from where the attack comes from to the fighting, and crossed
    // swords on the battlefield, pulsing while the fighting goes on.
    let centers = if wars.is_empty() {
        BTreeMap::new()
    } else {
        civ_centers(sim)
    };
    for war in &wars {
        let active = war.active(month);
        let site = cam.to_screen(war.site.x as f32 + 0.5, war.site.y as f32 + 0.5, fb);
        let from = war
            .from
            .map(|p| (p.x as f32 + 0.5, p.y as f32 + 0.5))
            .or_else(|| war.attacker.and_then(|a| centers.get(&a).copied()));
        let color = if active {
            war.color
        } else {
            mix(war.color, [120, 120, 120], 0.5)
        };
        if let Some((fx, fy)) = from.filter(|_| active) {
            let a = cam.to_screen(fx, fy, fb);
            let (dx, dy) = (site.0 - a.0, site.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 30.0 * u {
                let (ux, uy) = (dx / len, dy / len);
                // Stop short of the swords, then a head.
                let tip = (site.0 - ux * 13.0 * u, site.1 - uy * 13.0 * u);
                let start = (a.0 + ux * 6.0 * u, a.1 + uy * 6.0 * u);
                let back = (tip.0 - ux * 9.0 * u, tip.1 - uy * 9.0 * u);
                let wings = [-1.0, 1.0]
                    .map(|side: f32| (back.0 - uy * 6.0 * u * side, back.1 + ux * 6.0 * u * side));
                fb.line(start, tip, 6.5 * u, INK);
                for wing in wings {
                    fb.line(wing, tip, 5.0 * u, INK);
                }
                fb.line(start, tip, 4.0 * u, color);
                for wing in wings {
                    fb.line(wing, tip, 3.0 * u, color);
                }
            }
        }
        if active {
            let pulse = 11.0 * u + 3.0 * u * (layers.time * 4.0).sin();
            fb.ring(site.0, site.1, pulse, 2.6 * u, INK, false);
            fb.ring(site.0, site.1, pulse, 1.6 * u, SICK, false);
        }
        let k = 6.5 * u;
        for (w, c) in [(4.0 * u, INK), (2.2 * u, WHITE)] {
            fb.line((site.0 - k, site.1 - k), (site.0 + k, site.1 + k), w, c);
            fb.line((site.0 + k, site.1 - k), (site.0 - k, site.1 + k), w, c);
        }
        fb.disc(site.0, site.1, 2.0 * u, color);
    }

    if let Some((x, y)) = selected {
        outline(fb, cam.tile_rect(x, y, fb), border + 1, WHITE);
    }
    // The selected unit, ringed in gold, and the way it's going.
    if let Some((pos, route)) = unit.and_then(|u| sim.unit_route(u)) {
        let dot = ((cam.zoom / 5.0) as i32).max(2);
        for p in &route {
            let r = cam.tile_rect(p.x, p.y, fb);
            let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
            fb.fill(
                Rect::new(
                    cx - dot / 2,
                    cy - dot / 2,
                    cx + dot - dot / 2,
                    cy + dot - dot / 2,
                ),
                GOLD,
            );
        }
        if let Some(last) = route.last() {
            outline(fb, cam.tile_rect(last.x, last.y, fb), border, GOLD);
        }
        let r = cam.tile_rect(pos.x, pos.y, fb);
        let pad = ((cam.zoom / 4.0) as i32).max(2);
        let ring = Rect::new(r.x0 - pad, r.y0 - pad, r.x1 + pad, r.y1 + pad);
        outline(fb, ring, border + 1, GOLD);
    }
}

/// A frame of thickness `t` just outside `r`.
fn outline(fb: &mut Framebuffer, r: Rect, t: i32, color: Rgb) {
    fb.fill(Rect::new(r.x0 - t, r.y0 - t, r.x1 + t, r.y0), color);
    fb.fill(Rect::new(r.x0 - t, r.y1, r.x1 + t, r.y1 + t), color);
    fb.fill(Rect::new(r.x0 - t, r.y0, r.x0, r.y1), color);
    fb.fill(Rect::new(r.x1, r.y0, r.x1 + t, r.y1), color);
}

/// A ship: hull, mast, sail and a flag in its power's colors. Ships sailing home
/// empty ride higher in the water.
fn draw_ship(fb: &mut Framebuffer, r: Rect, flag: Rgb, laden: bool) {
    if r.width() < 5 {
        fb.fill(r, SAIL);
        fb.fill(r.inset(0.3), flag);
        return;
    }
    let hull = if laden {
        r.band(0.62, 0.86)
    } else {
        r.band(0.56, 0.8)
    };
    fb.fill(hull.inset_xy(0.06, 0.0), WOOD);
    fb.fill(hull.inset_xy(0.2, 0.0).band(0.0, 0.3), shade(WOOD, 1.4));
    let mast = r.inset_xy(0.47, 0.0).band(0.08, 0.6);
    fb.fill(mast, WOOD);
    fb.fill(r.inset_xy(0.2, 0.0).band(0.2, 0.54), SAIL);
    let pennant = (r.width() / 5).max(1);
    fb.fill(
        Rect::new(mast.x1, mast.y0, mast.x1 + pennant, mast.y0 + pennant),
        flag,
    );
}

/// Draws what's built on a tile. `burning` is for ruins still in flames.
#[allow(clippy::too_many_arguments)]
fn draw_building(
    fb: &mut Framebuffer,
    tiles: &Tiles,
    season: &Season,
    snow: f32,
    (x, y): (i32, i32),
    r: Rect,
    folk: Option<Folk>,
    burning: bool,
) {
    let w = r.width();
    let civ = folk.map_or(WHITE, |f| f.color);
    // Roofs, roads and bare ground take on the snow.
    let wintry = |c: Rgb| mix(c, SNOW, snow * 0.7);
    match tiles.building(x, y) {
        Building::Empty => {}
        Building::Road => {
            let road = wintry(ROAD);
            if w < 5 {
                fb.fill(r, road);
                return;
            }
            // A square in the middle plus an arm toward each connected neighbor.
            let i = (w as f32 * 0.34) as i32;
            fb.fill(Rect::new(r.x0 + i, r.y0 + i, r.x1 - i, r.y1 - i), road);
            let joins = |dx: i32, dy: i32| {
                let b = tiles.building(x + dx, y + dy);
                matches!(b, Building::Road | Building::Fort | Building::Wharf)
            };
            if joins(-1, 0) {
                fb.fill(Rect::new(r.x0, r.y0 + i, r.x0 + i, r.y1 - i), road);
            }
            if joins(1, 0) {
                fb.fill(Rect::new(r.x1 - i, r.y0 + i, r.x1, r.y1 - i), road);
            }
            if joins(0, -1) {
                fb.fill(Rect::new(r.x0 + i, r.y0, r.x1 - i, r.y0 + i), road);
            }
            if joins(0, 1) {
                fb.fill(Rect::new(r.x0 + i, r.y1 - i, r.x1 - i, r.y1), road);
            }
        }
        Building::Fort => {
            // A palisade around a blockhouse flying the colony's flag.
            fb.fill(r, WOOD);
            fb.fill(r.inset(0.14), wintry(EARTH));
            fb.fill(r.inset(0.3), shade(WOOD, 1.25));
            fb.fill(r.inset(0.4), civ);
        }
        Building::House => {
            let brick = folk.is_some_and(|f| f.brick);
            let (walls, roof) = match folk.map(|f| f.culture) {
                // Rebuilt in brick after a great fire.
                Some(Culture::English | Culture::French) if brick => ([164, 74, 54], [90, 60, 50]),
                // Whitewashed brick and stone with flat roofs.
                Some(Culture::Indian) => ([232, 214, 178], [196, 160, 112]),
                Some(Culture::Portuguese) => ([236, 232, 222], [184, 84, 56]),
                Some(Culture::Danish) => ([176, 80, 60], [90, 60, 50]),
                // Adobe with tile roofs.
                Some(Culture::Spanish) => ([226, 210, 176], [184, 84, 56]),
                // Brick with steep gables.
                Some(Culture::Dutch) => ([168, 72, 52], [90, 60, 50]),
                // Whitewashed stone.
                Some(Culture::French) => ([214, 206, 190], [84, 80, 88]),
                // Timber frame and clapboard.
                _ => ([186, 140, 96], [120, 78, 58]),
            };
            let h = r.inset(0.16);
            fb.fill(h, walls);
            fb.fill(h.band(0.0, 0.5), wintry(roof));
        }
        Building::Farm => {
            let crop = wintry(season.crop([222, 190, 92]));
            fb.fill(r.inset(0.04), crop);
            if w >= 8 {
                // Furrows.
                let mut row = r.y0 + 2;
                while row < r.y1 - 1 {
                    let furrow = Rect::new(r.x0 + 1, row, r.x1 - 1, row + 1);
                    fb.fill(furrow, shade(crop, 0.82));
                    row += 3;
                }
            }
        }
        Building::Plantation => {
            // Tobacco: broad leaves in summer, the crop curing brown after.
            let leaf = shade(wintry(season.crop([176, 132, 64])), 0.85);
            fb.fill(r.inset(0.04), leaf);
            dots(fb, r, shade(leaf, 1.3), 2);
        }
        Building::Workshop => {
            let b = r.inset(0.14);
            fb.fill(b, WORKSHOP);
            fb.fill(b.band(0.0, 0.35), wintry(shade(WORKSHOP, 0.8)));
            if w >= 8 {
                let chimney = (w / 6).max(1);
                fb.fill(
                    Rect::new(b.x1 - 2 * chimney, r.y0, b.x1 - chimney, b.y0 + chimney),
                    shade(WORKSHOP, 0.6),
                );
            }
        }
        Building::Wharf => {
            fb.fill(r.inset(0.06), [146, 112, 74]);
            if w >= 6 {
                // Planks.
                let mut col = r.x0 + 2;
                while col < r.x1 - 1 {
                    fb.fill(Rect::new(col, r.y0 + 1, col + 1, r.y1 - 1), [110, 82, 54]);
                    col += 3;
                }
            }
        }
        Building::Church => {
            let body = r.inset_xy(0.18, 0.0).band(0.35, 0.9);
            fb.fill(body, CHURCH);
            fb.fill(body.band(0.0, 0.3), wintry([92, 84, 84]));
            fb.fill(r.inset_xy(0.4, 0.0).band(0.06, 0.4), CHURCH);
        }
        Building::TradingPost => {
            let cabin = r.inset(0.18);
            fb.fill(cabin, [118, 82, 50]);
            fb.fill(cabin.band(0.0, 0.4), wintry([84, 58, 38]));
            if w >= 8 {
                // Pelts stretched out to dry.
                fb.fill(r.inset_xy(0.3, 0.0).band(0.62, 0.8), [176, 128, 76]);
            }
        }
        Building::Village => {
            // A palisaded village around its central fire.
            fb.fill(r, shade(civ, 0.6));
            fb.fill(r.inset(0.14), wintry(EARTH));
            fb.fill(r.inset(0.38), FIRE);
        }
        Building::Longhouse => {
            // Long, bark-covered and rounded at the ends.
            let body = r.inset_xy(0.06, 0.26);
            let bark = [136, 106, 76];
            fb.fill(body.inset_xy(0.06, 0.0), bark);
            fb.fill(body.inset_xy(0.0, 0.2), bark);
            fb.fill(
                body.inset_xy(0.06, 0.0).band(0.0, 0.45),
                wintry(shade(bark, 0.8)),
            );
        }
        Building::Ruins if burning => {
            // Still burning: flames over charred timbers.
            fb.fill(r.inset(0.08), [70, 40, 30]);
            fb.fill(r.inset(0.18), [232, 92, 32]);
            fb.fill(r.inset_xy(0.3, 0.0).band(0.3, 0.85), FIRE);
            fb.fill(r.inset_xy(0.4, 0.0).band(0.5, 0.85), [255, 228, 132]);
        }
        Building::Ruins => {
            // Blackened walls and ash.
            let ash = wintry([88, 82, 78]);
            fb.fill(r.inset(0.14), [52, 46, 42]);
            fb.fill(r.inset(0.14).band(0.55, 1.0), ash);
            if w >= 8 {
                let post = (w / 8).max(1);
                fb.fill(
                    Rect::new(
                        r.x0 + w / 4,
                        r.y0 + w / 5,
                        r.x0 + w / 4 + post,
                        r.y1 - w / 5,
                    ),
                    [30, 26, 24],
                );
                fb.fill(
                    Rect::new(
                        r.x1 - w / 3,
                        r.y0 + w / 3,
                        r.x1 - w / 3 + post,
                        r.y1 - w / 5,
                    ),
                    [30, 26, 24],
                );
            }
        }
        Building::Field => {
            // Maize, beans and squash grown together on small mounds.
            let crop = wintry(season.crop([216, 184, 76]));
            fb.fill(r.inset(0.04), crop);
            dots(fb, r, shade(crop, 0.7), 1);
        }
    }
}

/// A grid of small squares across a tile: mounds of maize, rows of tobacco.
fn dots(fb: &mut Framebuffer, r: Rect, color: Rgb, size: i32) {
    let w = r.width();
    if w < 6 {
        return;
    }
    let step = (w / 3).max(2);
    let mut row = r.y0 + step / 2;
    while row + size <= r.y1 {
        let mut col = r.x0 + step / 2;
        while col + size <= r.x1 {
            fb.fill(Rect::new(col, row, col + size, row + size), color);
            col += step;
        }
        row += step;
    }
}

/// A town marker's radius in CSS pixels.
fn marker_size(population: u32) -> f32 {
    match population {
        0..150 => 2.2,
        150..600 => 3.0,
        600..1500 => 3.8,
        _ => 4.8,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    /// A state's name, over the middle of its towns.
    State,
    Town,
    /// A note on the map, such as a failed monsoon.
    Note,
}

pub struct Label {
    pub kind: LabelKind,
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    pub text: String,
    /// Labels that don't fit are dropped, lowest priority first.
    pub priority: u32,
    /// `plague` or `hunger` for a town in trouble, otherwise empty.
    pub flag: &'static str,
}

/// The middle of each state's towns, weighted by their people, as a tile position.
pub fn civ_centers(sim: &Simulation) -> BTreeMap<Entity, (f32, f32)> {
    civ_centers_of(&sim.world)
}

/// [`civ_centers`] for a bare world.
pub fn civ_centers_of(world: &World) -> BTreeMap<Entity, (f32, f32)> {
    let owners = world.read::<Owner>();
    let positions = world.read::<Position>();
    let mut sums: BTreeMap<Entity, (f32, f32, f32)> = BTreeMap::new();
    for (e, city) in world.read::<City>().iter() {
        let (Some(o), Some(p)) = (owners.get(e), positions.get(e)) else {
            continue;
        };
        let w = city.population.max(1) as f32;
        let s = sums.entry(o.0).or_default();
        s.0 += p.x as f32 * w;
        s.1 += p.y as f32 * w;
        s.2 += w;
    }
    sums.into_iter()
        .map(|(civ, (x, y, w))| (civ, (x / w + 0.5, y / w + 0.5)))
        .collect()
}

/// Text to draw over the map: state names when zoomed out, town names (with their
/// people when zoomed in), and notes. The page draws them, largest first, skipping any
/// that would overlap.
pub fn labels(sim: &Simulation, cam: &Camera, fb: &Framebuffer) -> Vec<Label> {
    let mut out = Vec::new();
    let on_screen = |x: f32, y: f32| {
        x > -100.0 && y > -20.0 && x < fb.width as f32 + 100.0 && y < fb.height as f32 + 20.0
    };
    if let Some((c, r2)) = sim.world.resource::<Drought>().area {
        let (x, y) = cam.to_screen(c.x as f32 + 0.5, c.y as f32 + 0.5, fb);
        let y = y - (r2 as f32).sqrt() * cam.zoom;
        if on_screen(x, y) {
            out.push(Label {
                kind: LabelKind::Note,
                x,
                y,
                color: HUNGRY,
                text: if sim.rules().setting == Setting::India {
                    "Monsoon failed".to_string()
                } else {
                    "Harvest failed".to_string()
                },
                priority: u32::MAX,
                flag: "",
            });
        }
    }
    let month = sim.month();
    for war in wars_now(sim).iter().filter(|w| w.active(month)) {
        let (x, y) = cam.to_screen(war.site.x as f32 + 0.5, war.site.y as f32 + 0.5, fb);
        if on_screen(x, y) {
            out.push(Label {
                kind: LabelKind::Note,
                x,
                y: y - 15.0 * fb.scale,
                color: shade(war.color, 1.35),
                text: format!("⚔ {}", war.name),
                priority: u32::MAX,
                flag: "",
            });
        }
    }
    let civs = sim.world.read::<Civ>();
    if cam.zoom < OVERVIEW_ZOOM {
        let towns = town_count(sim);
        for (civ, (tx, ty)) in civ_centers(sim) {
            let Some(c) = civs.get(civ) else { continue };
            if !c.is_local() || towns.get(&civ).copied().unwrap_or(0) < 2 {
                continue;
            }
            let (x, y) = cam.to_screen(tx, ty, fb);
            if on_screen(x, y) {
                out.push(Label {
                    kind: LabelKind::State,
                    x,
                    y,
                    color: shade(c.color, 1.3),
                    text: c.name.clone(),
                    priority: u32::MAX - 1,
                    flag: "",
                });
            }
        }
    }
    let folk_of = town_folk(sim);
    let positions = sim.world.read::<Position>();
    for (e, city) in sim.world.read::<City>().iter() {
        let Some(p) = positions.get(e) else { continue };
        let (x, y) = cam.to_screen(p.x as f32 + 0.5, p.y as f32 + 0.5, fb);
        if !on_screen(x, y) {
            continue;
        }
        let gap = if cam.zoom < OVERVIEW_ZOOM {
            (marker_size(city.population) + 2.0) * fb.scale
        } else {
            cam.zoom * 0.6
        };
        let flag = if city.outbreak.is_some() {
            "plague"
        } else if city.starving {
            "hunger"
        } else {
            ""
        };
        out.push(Label {
            kind: LabelKind::Town,
            x,
            y: y - gap,
            color: folk_of.get(&e).map_or(WHITE, |f| shade(f.color, 1.3)),
            text: if cam.zoom >= OVERVIEW_ZOOM {
                format!("{} {}", city.name, group(city.population))
            } else {
                city.name.clone()
            },
            // Towns in trouble are named first, then the biggest.
            priority: city.population + if flag.is_empty() { 0 } else { 1_000_000 },
            flag,
        });
    }
    out
}

fn town_count(sim: &Simulation) -> BTreeMap<Entity, u32> {
    let owners = sim.world.read::<Owner>();
    let mut count = BTreeMap::new();
    for (e, _) in sim.world.read::<City>().iter() {
        if let Some(o) = owners.get(e) {
            *count.entry(o.0).or_insert(0) += 1;
        }
    }
    count
}
