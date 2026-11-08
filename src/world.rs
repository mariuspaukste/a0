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
