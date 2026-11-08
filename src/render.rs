//! Software renderer: draws the world into a buffer of RGBA pixels that the browser
//! copies onto a canvas. No graphics library involved.

use std::collections::BTreeMap;

use crate::ecs::Entity;
use crate::sim::{Building, City, Civ, Owner, Position, Settler, Simulation, Tiles};
use crate::world::{hash, Map, Terrain};

type Rgb = [u8; 3];

const SEA: Rgb = [20, 44, 92];
const ROAD: Rgb = [96, 94, 100];
const HOUSE: Rgb = [196, 112, 82];
const FARM: Rgb = [214, 188, 92];
const WORKSHOP: Rgb = [116, 104, 146];
const WHITE: Rgb = [250, 250, 250];

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

    /// Shrinks the rect on every side by `fraction` of its width.
    fn inset(self, fraction: f32) -> Self {
        let d = (self.width() as f32 * fraction) as i32;
        Self::new(self.x0 + d, self.y0 + d, self.x1 - d, self.y1 - d)
    }
}

fn pack([r, g, b]: Rgb) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xFF00_0000
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

fn shade(c: Rgb, f: f32) -> Rgb {
    c.map(|v| (v as f32 * f).min(255.0) as u8)
}

fn terrain_color(t: Terrain, x: i32, y: i32) -> Rgb {
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
    shade(base, 0.95 + 0.1 * hash(7, x, y))
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

/// Civilization and color for each city.
fn city_civs(sim: &Simulation) -> BTreeMap<Entity, (Entity, Rgb)> {
    let civs = sim.world.read::<Civ>();
    let owners = sim.world.read::<Owner>();
    sim.world
        .read::<City>()
        .iter()
        .filter_map(|(e, _)| {
            let civ = owners.get(e)?.0;
            Some((e, (civ, civs.get(civ)?.color)))
        })
        .collect()
}

pub fn render(sim: &Simulation, cam: &Camera, fb: &mut Framebuffer, selected: Option<(i32, i32)>) {
    fb.clear(SEA);
    let map = sim.world.resource::<Map>();
    let tiles = sim.world.resource::<Tiles>();
    let civ_of = city_civs(sim);
    let tile_civ = |x: i32, y: i32| tiles.owner(x, y).and_then(|c| civ_of.get(&c).copied());

    let (fx0, fy0) = cam.to_tile(0.0, 0.0, fb);
    let (fx1, fy1) = cam.to_tile(fb.width as f32, fb.height as f32, fb);
    let x0 = (fx0.floor() as i32).max(0);
    let y0 = (fy0.floor() as i32).max(0);
    let x1 = (fx1.ceil() as i32).min(map.width as i32);
    let y1 = (fy1.ceil() as i32).min(map.height as i32);
    let border = ((cam.zoom / 8.0) as i32).max(1);

    for y in y0..y1 {
        for x in x0..x1 {
            let r = cam.tile_rect(x, y, fb);
            let civ = tile_civ(x, y);
            let mut color = terrain_color(map.get(x as usize, y as usize), x, y);
            if let Some((_, civ_color)) = civ {
                color = mix(color, civ_color, 0.18);
            }
            fb.fill(r, color);
            draw_building(fb, &tiles, x, y, r, civ.map(|(_, c)| c));

            // Draw the border on edges where the neighbor belongs to someone else.
            if let Some((id, civ_color)) = civ {
                let other =
                    |dx: i32, dy: i32| tile_civ(x + dx, y + dy).map(|(id, _)| id) != Some(id);
                let edge = shade(civ_color, 1.15);
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
    for (e, _) in sim.world.read::<Settler>().iter() {
        let (Some(p), Some(owner)) = (positions.get(e), owners.get(e)) else {
            continue;
        };
        let color = civs.get(owner.0).map_or(WHITE, |c| c.color);
        let r = cam.tile_rect(p.x, p.y, fb);
        fb.fill(r.inset(0.12), WHITE);
        fb.fill(r.inset(0.26), color);
    }

    if let Some((x, y)) = selected {
        let r = cam.tile_rect(x, y, fb);
        let t = border + 1;
        fb.fill(Rect::new(r.x0 - t, r.y0 - t, r.x1 + t, r.y0), WHITE);
        fb.fill(Rect::new(r.x0 - t, r.y1, r.x1 + t, r.y1 + t), WHITE);
        fb.fill(Rect::new(r.x0 - t, r.y0, r.x0, r.y1), WHITE);
        fb.fill(Rect::new(r.x1, r.y0, r.x1 + t, r.y1), WHITE);
    }
}

fn draw_building(fb: &mut Framebuffer, tiles: &Tiles, x: i32, y: i32, r: Rect, civ: Option<Rgb>) {
    let w = r.width();
    match tiles.building(x, y) {
        Building::Empty => {}
        Building::Road => {
            if w < 5 {
                fb.fill(r, ROAD);
                return;
            }
            // A square in the middle plus an arm toward each connected neighbor.
            let i = (w as f32 * 0.32) as i32;
            fb.fill(Rect::new(r.x0 + i, r.y0 + i, r.x1 - i, r.y1 - i), ROAD);
            let joins = |dx: i32, dy: i32| {
                matches!(
                    tiles.building(x + dx, y + dy),
                    Building::Road | Building::CityCenter
                )
            };
            if joins(-1, 0) {
                fb.fill(Rect::new(r.x0, r.y0 + i, r.x0 + i, r.y1 - i), ROAD);
            }
            if joins(1, 0) {
                fb.fill(Rect::new(r.x1 - i, r.y0 + i, r.x1, r.y1 - i), ROAD);
            }
            if joins(0, -1) {
                fb.fill(Rect::new(r.x0 + i, r.y0, r.x1 - i, r.y0 + i), ROAD);
            }
            if joins(0, 1) {
                fb.fill(Rect::new(r.x0 + i, r.y1 - i, r.x1 - i, r.y1), ROAD);
            }
        }
        Building::CityCenter => {
            let c = civ.unwrap_or(WHITE);
            fb.fill(r, shade(c, 0.55));
            fb.fill(r.inset(0.12), c);
            fb.fill(r.inset(0.34), WHITE);
        }
        Building::House => {
            let h = r.inset(0.16);
            fb.fill(h, HOUSE);
            let roof = Rect::new(h.x0, h.y0, h.x1, h.y0 + (h.y1 - h.y0) / 2);
            fb.fill(roof, shade(HOUSE, 0.75));
        }
        Building::Farm => {
            fb.fill(r.inset(0.04), FARM);
            if w >= 8 {
                // Furrows.
                let mut row = r.y0 + 2;
                while row < r.y1 - 1 {
                    fb.fill(
                        Rect::new(r.x0 + 1, row, r.x1 - 1, row + 1),
                        shade(FARM, 0.8),
                    );
                    row += 3;
                }
            }
        }
        Building::Workshop => {
            let b = r.inset(0.14);
            fb.fill(b, WORKSHOP);
            if w >= 8 {
                let chimney = (w / 6).max(1);
                fb.fill(
                    Rect::new(b.x1 - 2 * chimney, r.y0, b.x1 - chimney, b.y0 + chimney),
                    shade(WORKSHOP, 0.6),
                );
            }
        }
    }
}

pub struct Label {
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    pub text: String,
}

/// Names of the cities on screen, positioned just above each city center. The page
/// draws these as text on top of the pixels.
pub fn city_labels(sim: &Simulation, cam: &Camera, fb: &Framebuffer) -> Vec<Label> {
    if cam.zoom < 3.0 {
        return Vec::new();
    }
    let civ_of = city_civs(sim);
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
                color: civ_of.get(&e).map_or(WHITE, |(_, c)| *c),
                text: format!("{} {}", city.name, city.population),
            })
        })
        .collect()
}
