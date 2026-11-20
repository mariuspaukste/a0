//! Terrain and procedural map generation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    DeepWater,
    Water,
    Sand,
    Grass,
    Forest,
    Hills,
    Mountain,
}

impl Terrain {
    pub fn is_water(self) -> bool {
        matches!(self, Terrain::DeepWater | Terrain::Water)
    }

    pub fn buildable(self) -> bool {
        matches!(
            self,
            Terrain::Sand | Terrain::Grass | Terrain::Forest | Terrain::Hills
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Terrain::DeepWater => "Deep water",
            Terrain::Water => "Water",
            Terrain::Sand => "Sand",
            Terrain::Grass => "Grassland",
            Terrain::Forest => "Forest",
            Terrain::Hills => "Hills",
            Terrain::Mountain => "Mountains",
        }
    }
}

pub struct Map {
    pub width: usize,
    pub height: usize,
    tiles: Vec<Terrain>,
}

impl Map {
    /// A map covered entirely by one terrain type. Mostly useful for tests.
    pub fn flat(width: usize, height: usize, terrain: Terrain) -> Self {
        Self {
            width,
            height,
            tiles: vec![terrain; width * height],
        }
    }

    /// Generates an island-like continent from fractal noise. The same seed always
    /// produces the same map.
    pub fn generate(width: usize, height: usize, seed: u64) -> Self {
        let mut tiles = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32, y as f32);
                let noise = fbm(seed, fx / 28.0, fy / 28.0, 5);
                let moisture = fbm(seed ^ 0xA5A5_5A5A, fx / 18.0, fy / 18.0, 3);

                // Sink the edges of the map so land forms a continent surrounded by sea.
                let dx = fx / width as f32 * 2.0 - 1.0;
                let dy = fy / height as f32 * 2.0 - 1.0;
                let elevation = noise + 0.28 - 0.75 * (dx * dx + dy * dy);

                tiles.push(match elevation {
                    e if e < 0.30 => Terrain::DeepWater,
                    e if e < 0.42 => Terrain::Water,
                    e if e < 0.45 => Terrain::Sand,
                    e if e < 0.68 && moisture > 0.56 => Terrain::Forest,
                    e if e < 0.68 => Terrain::Grass,
                    e if e < 0.78 => Terrain::Hills,
                    _ => Terrain::Mountain,
                });
            }
        }
        Self {
            width,
            height,
            tiles,
        }
    }

    /// The Indian subcontinent between 60–100°E and 38–5°N, from a traced coastline and
    /// the main landforms: the Himalaya and Hindu Kush, the Thar desert, the Indus and
    /// Ganges plains, the Deccan plateau between the Western and Eastern Ghats, the
    /// forests of central India, Bengal and Burma, and Ceylon. Noise roughens the edges.
    pub fn india(width: usize, height: usize, seed: u64) -> Self {
        let mut tiles = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32, y as f32);
                let n1 = fbm(seed, fx / 6.0, fy / 6.0, 4);
                let n2 = fbm(seed ^ 0x5151, fx / 4.0, fy / 4.0, 3);
                let lon = 60.0 + (fx + 0.5) / width as f32 * 40.0;
                let lat = 38.0 - (fy + 0.5) / height as f32 * 33.0;
                // A ragged coastline: test a point nudged by noise.
                let (jx, jy) = (lon + (n1 - 0.5) * 0.5, lat + (n2 - 0.5) * 0.5);
                let land = inside(&INDIA_COAST, jx, jy) || inside(&CEYLON_COAST, jx, jy);
                tiles.push(if land {
                    india_land(lon, lat, n1, n2)
                } else {
                    Terrain::Water
                });
            }
        }
        let mut map = Self {
            width,
            height,
            tiles,
        };
        map.finish_coasts(seed);
        map
    }

    /// Deepens open sea away from the shore and puts beaches along parts of it.
    fn finish_coasts(&mut self, seed: u64) {
        let near = |map: &Map, x: usize, y: usize, r: i32, water: bool| {
            (-r..=r).any(|dy| {
                (-r..=r).any(|dx| {
                    map.terrain_at(x as i32 + dx, y as i32 + dy)
                        .is_some_and(|t| t.is_water() == water)
                })
            })
        };
        let mut next = self.tiles.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                let t = self.get(x, y);
                next[y * self.width + x] = match t {
                    Terrain::Water if !near(self, x, y, 3, false) => Terrain::DeepWater,
                    Terrain::Grass | Terrain::Forest
                        if near(self, x, y, 1, true) && hash(seed, x as i32, y as i32) > 0.55 =>
                    {
                        Terrain::Sand
                    }
                    t => t,
                };
            }
        }
        self.tiles = next;
    }

    pub fn get(&self, x: usize, y: usize) -> Terrain {
        self.tiles[y * self.width + x]
    }

    /// Terrain at signed coordinates, or `None` off the edge of the map.
    pub fn terrain_at(&self, x: i32, y: i32) -> Option<Terrain> {
        let inside = x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height;
        inside.then(|| self.get(x as usize, y as usize))
    }

    pub fn set(&mut self, x: usize, y: usize, terrain: Terrain) {
        self.tiles[y * self.width + x] = terrain;
    }
}

/// The mainland coast from the Makran to the Malay peninsula, as (longitude, latitude),
/// closed along the north, east and west edges of the map, which are all land.
const INDIA_COAST: [(f32, f32); 63] = [
    (60.0, 25.4),
    (62.5, 25.2),
    (64.5, 25.3),
    (66.6, 25.4),
    (67.2, 24.8),
    (67.6, 24.0),
    (68.4, 23.5),
    (68.8, 22.9),
    (69.6, 22.9),
    (70.3, 22.7),
    (69.6, 22.4),
    (69.0, 22.3),
    (69.6, 21.6),
    (70.4, 21.0),
    (71.0, 20.7),
    (71.8, 21.0),
    (72.2, 21.6),
    (72.6, 22.2),
    (72.8, 21.5),
    (72.8, 20.6),
    (72.8, 19.5),
    (72.9, 18.8),
    (73.1, 17.6),
    (73.4, 16.4),
    (73.8, 15.4),
    (74.3, 14.4),
    (74.7, 13.2),
    (75.1, 12.2),
    (75.7, 11.3),
    (76.1, 10.2),
    (76.4, 9.2),
    (76.9, 8.5),
    (77.5, 8.05),
    (78.1, 8.7),
    (78.6, 9.2),
    (79.3, 9.3),
    (78.9, 9.9),
    (79.85, 10.3),
    (79.85, 11.6),
    (80.2, 12.8),
    (80.3, 13.6),
    (80.1, 14.6),
    (80.2, 15.5),
    (81.2, 16.0),
    (82.3, 16.6),
    (83.4, 17.6),
    (85.0, 19.4),
    (86.4, 20.1),
    (87.0, 21.0),
    (88.1, 21.6),
    (89.2, 21.7),
    (90.4, 22.0),
    (91.6, 22.5),
    (92.2, 21.0),
    (93.2, 19.4),
    (94.4, 16.1),
    (96.6, 16.6),
    (98.4, 12.0),
    // Down the Malay peninsula to the east edge, then round the map's land edges.
    (98.6, 8.5),
    (100.5, 6.8),
    (100.5, 38.5),
    (59.5, 38.5),
    (59.5, 25.4),
];

const CEYLON_COAST: [(f32, f32); 10] = [
    (79.8, 9.85),
    (80.3, 9.7),
    (80.9, 8.9),
    (81.4, 8.3),
    (81.9, 7.3),
    (81.6, 6.3),
    (80.6, 5.95),
    (80.0, 6.4),
    (79.8, 7.4),
    (79.75, 8.6),
];

/// Whether a point is inside a closed outline (even-odd rule).
fn inside(points: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut odd = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let ((xi, yi), (xj, yj)) = (points[i], points[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Linear interpolation through `points`, sorted by their first coordinate.
fn through(points: &[(f32, f32)], x: f32) -> f32 {
    let i = points
        .iter()
        .position(|p| p.0 >= x)
        .unwrap_or(points.len() - 1);
    if i == 0 {
        return points[0].1;
    }
    let ((x0, y0), (x1, y1)) = (points[i - 1], points[i]);
    if x >= x1 {
        return y1;
    }
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}

/// What covers the land at a longitude and latitude.
fn india_land(lon: f32, lat: f32, n1: f32, n2: f32) -> Terrain {
    let wobble = (n1 - 0.5) * 1.2;
    // The foot of the Himalaya and Hindu Kush, by longitude.
    let foothills = through(
        &[
            (60.0, 34.2),
            (69.0, 34.0),
            (71.0, 34.4),
            (73.0, 33.6),
            (75.0, 32.7),
            (77.0, 31.2),
            (79.0, 30.1),
            (81.0, 29.1),
            (84.0, 27.9),
            (86.0, 27.3),
            (88.0, 26.9),
            (90.0, 26.8),
            (92.0, 27.1),
            (95.0, 27.5),
            (100.0, 26.5),
        ],
        lon,
    );
    let kashmir = (73.8..75.6).contains(&lon) && (33.4..34.5).contains(&lat);
    if kashmir {
        return Terrain::Grass;
    }
    if lat > foothills + 1.4 + wobble {
        return Terrain::Mountain;
    }
    if lat > foothills + wobble * 0.5 {
        return if n2 > 0.55 {
            Terrain::Forest
        } else {
            Terrain::Hills
        };
    }
    // The Sulaiman and Kirthar ranges west of the Indus, and Baluchistan beyond.
    let ranges = through(
        &[
            (24.0, 67.0),
            (26.0, 67.6),
            (28.0, 68.0),
            (31.0, 69.8),
            (34.0, 70.8),
        ],
        lat,
    );
    if lon < ranges - 1.5 + wobble {
        return if n2 > 0.6 {
            Terrain::Mountain
        } else if n1 > 0.5 {
            Terrain::Sand
        } else {
            Terrain::Hills
        };
    }
    if lon < ranges + wobble * 0.5 {
        return Terrain::Hills;
    }
    // The Thar desert and the Rann of Kutch.
    if (69.8..74.2).contains(&(lon + wobble)) && (24.4..29.8).contains(&(lat + wobble * 0.5)) {
        return Terrain::Sand;
    }
    if (68.8..71.4).contains(&lon) && (23.3..24.3).contains(&lat) {
        return Terrain::Sand;
    }
    // The Western Ghats rise just behind the west coast, wooded on the seaward side.
    let west_coast = through(
        &[
            (8.0, 77.3),
            (9.0, 76.6),
            (11.0, 75.9),
            (13.0, 74.8),
            (15.5, 73.9),
            (17.0, 73.4),
            (19.0, 73.0),
            (21.5, 72.9),
        ],
        lat,
    );
    if (8.0..21.0).contains(&lat) && lon < west_coast + 1.1 + wobble * 0.3 {
        return if lon > west_coast + 0.6 {
            Terrain::Hills
        } else {
            Terrain::Forest
        };
    }
    // The Nilgiris and Cardamom hills.
    if (lon - 76.8).hypot(lat - 11.4) < 0.6 || (lon - 77.2).hypot(lat - 9.6) < 0.6 {
        return Terrain::Hills;
    }
    // Ceylon: hill country in the middle, wet forest in the south-west.
    if lat < 10.0 && lon > 79.6 {
        let d = (lon - 80.75).hypot(lat - 7.1);
        return match d {
            d if d < 0.35 => Terrain::Mountain,
            d if d < 0.75 => Terrain::Hills,
            _ if lat < 7.6 && lon < 80.6 => Terrain::Forest,
            _ if n2 > 0.6 => Terrain::Forest,
            _ => Terrain::Grass,
        };
    }
    // Burma, Arakan and the hills of the north-east: forest.
    if lon > 92.2 || (lon > 89.8 && lat > 24.4) {
        let irrawaddy = (94.2..96.6).contains(&lon) && (17.0..24.0).contains(&lat);
        return if irrawaddy && n2 < 0.6 {
            Terrain::Grass
        } else if n1 > 0.62 {
            Terrain::Hills
        } else {
            Terrain::Forest
        };
    }
    // The Sundarbans mangroves at the mouths of the Ganges.
    if (88.0..90.2).contains(&lon) && lat < 22.5 {
        return Terrain::Forest;
    }
    // The forests of central India (Gondwana) and the Vindhya and Satpura hills.
    if (78.0..87.0).contains(&lon) && (19.0..24.3).contains(&lat) && n2 > 0.38 {
        return if n1 > 0.62 {
            Terrain::Hills
        } else {
            Terrain::Forest
        };
    }
    if (74.0..82.0).contains(&lon) && (21.3..23.6).contains(&lat) && n1 > 0.52 {
        return Terrain::Hills;
    }
    // The Eastern Ghats: broken hills a little inland of the east coast.
    if (13.0..20.0).contains(&lat) {
        let east_coast = through(
            &[
                (13.0, 80.2),
                (15.5, 80.2),
                (16.5, 81.8),
                (18.0, 83.8),
                (20.0, 86.0),
            ],
            lat,
        );
        let inland = east_coast - lon;
        if (0.4..1.6).contains(&inland) && n2 > 0.48 {
            return Terrain::Hills;
        }
    }
    // The Deccan plateau: open country broken by hills.
    if (12.0..21.5).contains(&lat) && n1 > 0.64 {
        return Terrain::Hills;
    }
    // Everything else: the great plains of the Indus and Ganges, Gujarat, the Tamil
    // country, with patches of woodland.
    if n2 > 0.7 {
        Terrain::Forest
    } else {
        Terrain::Grass
    }
}

/// A pseudo-random value in [0, 1) for an integer lattice point.
pub fn hash(seed: u64, x: i32, y: i32) -> f32 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn value_noise(seed: u64, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (ix, iy) = (x0 as i32, y0 as i32);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(x - x0), smooth(y - y0));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = lerp(hash(seed, ix, iy), hash(seed, ix + 1, iy), tx);
    let bottom = lerp(hash(seed, ix, iy + 1), hash(seed, ix + 1, iy + 1), tx);
    lerp(top, bottom, ty)
}

/// Fractal noise: several octaves of value noise, each at double the frequency and
/// half the amplitude of the last. Returns a value in [0, 1).
fn fbm(seed: u64, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut norm, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0);
    for octave in 0..octaves {
        let octave_seed = seed.wrapping_add(octave as u64 * 1013);
        sum += amp * value_noise(octave_seed, x * freq, y * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_map() {
        let a = Map::generate(64, 48, 42);
        let b = Map::generate(64, 48, 42);
        assert_eq!(a.tiles, b.tiles);
    }

    #[test]
    fn different_seeds_give_different_maps() {
        let a = Map::generate(64, 48, 1);
        let b = Map::generate(64, 48, 2);
        assert_ne!(a.tiles, b.tiles);
    }

    #[test]
    fn maps_have_land_and_sea_with_water_at_the_edges() {
        for seed in 0..20 {
            let map = Map::generate(160, 120, seed);
            let land = map.tiles.iter().filter(|t| !t.is_water()).count();
            let share = land as f32 / map.tiles.len() as f32;
            assert!(
                (0.2..0.8).contains(&share),
                "seed {seed}: land share {share}"
            );
            assert!(
                map.get(0, 0).is_water(),
                "seed {seed}: corner should be sea"
            );
        }
    }
}
