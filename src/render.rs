//! Software renderer: draws the world into a buffer of RGBA pixels that the browser
//! copies onto a canvas. No graphics library involved.
//!
//! The map follows the seasons: snow lies in the north in winter, forests turn in the
//! autumn, and fields go from bare earth to green to gold at the September harvest.

use std::collections::BTreeMap;

use crate::ecs::Entity;
use crate::era::{self, Culture, Setting};
use crate::sim::{
    group, Building, Burning, City, Civ, LandValue, Owner, Position, Settler, Ship, Simulation,
    Tiles,
};
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

pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    /// One `u32` per pixel. On little-endian wasm its bytes are R, G, B, A, which is
    /// exactly the layout the canvas `ImageData` expects.
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        *self = Self::new(width, height);
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
    let border = ((cam.zoom / 8.0) as i32).max(1);

    for y in y0..y1 {
        let latitude = era::latitude(y, map.height);
        for x in x0..x1 {
            let r = cam.tile_rect(x, y, fb);
            let folk = tile_folk(x, y);
            let terrain = map.get(x as usize, y as usize);
            let mut color = terrain_color(terrain, x, y, latitude, &season);
            if let Some(f) = folk {
                color = mix(color, f.color, 0.16);
            }
            fb.fill(r, color);
            let snow = season.snow(x, y, latitude);
            let lit = tiles.building(x, y) == Building::Ruins && burning.is_burning(x, y);
            draw_building(fb, &tiles, &season, snow, (x, y), r, folk, lit);
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
                let edge = shade(f.color, 1.15);
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

pub struct Label {
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    pub text: String,
}

/// Names of the towns on screen, positioned just above each town center. The page
/// draws these as text on top of the pixels.
pub fn city_labels(sim: &Simulation, cam: &Camera, fb: &Framebuffer) -> Vec<Label> {
    if cam.zoom < 3.0 {
        return Vec::new();
    }
    let folk_of = town_folk(sim);
    let positions = sim.world.read::<Position>();
    sim.world
        .read::<City>()
        .iter()
        .filter_map(|(e, city)| {
            let p = positions.get(e)?;
            let (x, y) = cam.to_screen(p.x as f32 + 0.5, p.y as f32, fb);
            let on_screen = x > -100.0
                && y > -20.0
                && x < fb.width as f32 + 100.0
                && y < fb.height as f32 + 20.0;
            on_screen.then(|| Label {
                x,
                y,
                color: folk_of.get(&e).map_or(WHITE, |f| f.color),
                text: format!("{} {}", city.name, group(city.population)),
            })
        })
        .collect()
}
