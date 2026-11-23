//! The economy: a Leontief input–output model, added up once a year for every Indian
//! state.
//!
//! The economy has four sectors. To make one unit of its output each sector buys
//! inputs from the others (weavers buy raw cotton from farmers, farmers buy ploughs
//! from smiths, everyone pays carters). These are the input coefficients `A`. Final
//! demand `d` is what people eat and wear, plus the cloth the European companies buy
//! for export. The gross output every sector must produce to meet that demand is
//!
//! ```text
//! x = (I − A)⁻¹ d
//! ```
//!
//! where `(I − A)⁻¹` is the Leontief inverse: its column `j` says how much every
//! sector has to produce, directly and through its suppliers, for one more unit of
//! sector `j`'s goods. GDP is the value added, `Σ vⱼ xⱼ`, with `vⱼ = 1 − Σᵢ aᵢⱼ`.
//!
//! Farming is held to what the simulation's fields, hunting and fishing actually
//! brought in that year. When the harvest falls short, people go on eating as much as
//! they can and stop buying cloth and goods first. So a failed monsoon reaches the
//! weavers too, through the matrix.
//!
//! The coefficients and the household basket are round, illustrative numbers for a
//! farming economy that weaves its own cotton. They are not estimates for any real
//! year. The totals are scaled so that the simulation starts out at Angus Maddison's
//! estimates of India's population and GDP for 1600, and those estimates are kept
//! for comparison.

use std::collections::BTreeMap;

use crate::ecs::{Entity, World};
use crate::era;
use crate::sim::{tally, City, Civ, Owner, Position, Tiles};
use crate::world::Map;

pub const SECTORS: [&str; 4] = ["Farming", "Textiles", "Crafts and mining", "Trade"];

const FARMING: usize = 0;
const TEXTILES: usize = 1;

/// `INPUTS[i][j]`: how much of sector `i`'s output sector `j` uses up to make one unit
/// of its own. Columns are the buyers.
pub const INPUTS: [[f64; 4]; 4] = [
    // Farming Textiles Crafts Trade
    [0.10, 0.35, 0.10, 0.05], // Farming: seed, fodder, raw cotton, indigo
    [0.02, 0.05, 0.02, 0.02], // Textiles: sacking, sailcloth
    [0.05, 0.05, 0.15, 0.05], // Crafts and mining: ploughs, looms, tools, dyes
    [0.05, 0.10, 0.08, 0.05], // Trade: carting, boats, markets, credit
];

/// What one person buys in a year from each sector, in the model's units.
pub const BASKET: [f64; 4] = [0.62, 0.12, 0.08, 0.06];

/// Food per person-month people want from a harvest: a month's eating, and some to
/// store against a bad year.
const WANTED: f64 = 1.15;

/// Cloth bought for export per cargo a company ships home.
const CLOTH_PER_CARGO: f64 = 0.5;

/// The share of household spending a Company-ruled state pays as land revenue. The
/// Company spent much of it on its "investment": cloth bought for export, which left
/// the country with nothing coming back.
pub const TRIBUTE: f64 = 0.15;

/// The state the Company rules directly once it holds the Diwani.
pub const COMPANY_STATE: &str = "Company Raj";

/// Maddison's estimates for India: (year, population in millions, GDP in millions of
/// 1990 international dollars). From *The World Economy: A Millennial Perspective*
/// (OECD, 2001).
pub const MADDISON: [(u32, f64, f64); 4] = [
    (1600, 135.0, 74_250.0),
    (1700, 165.0, 90_750.0),
    (1820, 209.0, 111_417.0),
    (1870, 253.0, 134_882.0),
];

/// Maddison's population (people) and GDP (1990 dollars) for a year, read off a
/// straight line between his estimates.
pub fn maddison(year: f64) -> (f64, f64) {
    let first = MADDISON[0];
    let last = MADDISON[MADDISON.len() - 1];
    let (p, g) = if year <= first.0 as f64 {
        (first.1, first.2)
    } else if year >= last.0 as f64 {
        (last.1, last.2)
    } else {
        let i = MADDISON
            .windows(2)
            .position(|w| year < w[1].0 as f64)
            .unwrap();
        let (a, b) = (MADDISON[i], MADDISON[i + 1]);
        let t = (year - a.0 as f64) / (b.0 - a.0) as f64;
        (a.1 + t * (b.1 - a.1), a.2 + t * (b.2 - a.2))
    };
    (p * 1e6, g * 1e6)
}

/// `(I − A)⁻¹`, by Gauss–Jordan elimination.
pub fn leontief_inverse(a: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = if i == j { 1.0 } else { 0.0 } - a[i][j];
        }
        m[i][4 + i] = 1.0;
    }
    for col in 0..4 {
        let pivot = (col..4)
            .max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs()))
            .unwrap();
        m.swap(col, pivot);
        let p = m[col][col];
        for v in m[col].iter_mut() {
            *v /= p;
        }
        for row in 0..4 {
            if row != col {
                let f = m[row][col];
                for k in 0..8 {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    let mut inv = [[0.0; 4]; 4];
    for i in 0..4 {
        inv[i].copy_from_slice(&m[i][4..]);
    }
    inv
}

/// Value added per unit of each sector's output: what is left once inputs are paid.
pub fn value_added() -> [f64; 4] {
    let mut v = [1.0; 4];
    for (j, vj) in v.iter_mut().enumerate() {
        for row in &INPUTS {
            *vj -= row[j];
        }
    }
    v
}

fn times(m: &[[f64; 4]; 4], d: &[f64; 4]) -> [f64; 4] {
    let mut x = [0.0; 4];
    for i in 0..4 {
        x[i] = (0..4).map(|j| m[i][j] * d[j]).sum();
    }
    x
}

/// How much harder households cut cloth and goods than food when the harvest is
/// short: if they keep a share `s` of their other spending, they keep `s^FOOD_CUT`
/// of their food.
const FOOD_CUT: f64 = 0.25;

/// Gross output for final demand `households + exports`, with farming held to at
/// most `farm_cap`. When farming can't keep up, food prices rise: households cut
/// back on cloth and goods much more than on food, and exports fall with them.
/// Returns the output and the share of their food households got.
pub fn produce(
    inverse: &[[f64; 4]; 4],
    households: [f64; 4],
    exports: [f64; 4],
    farm_cap: f64,
) -> ([f64; 4], f64) {
    let demand = |food: f64, rest: f64| {
        let mut d = [0.0; 4];
        for j in 0..4 {
            let share = if j == FARMING { food } else { rest };
            d[j] = share * (households[j] + exports[j]);
        }
        d
    };
    let full = times(inverse, &demand(1.0, 1.0));
    if full[FARMING] <= farm_cap {
        return (full, 1.0);
    }
    // Farming output rises with `s`, so find the `s` that uses up the harvest.
    let farming = |s: f64| times(inverse, &demand(s.powf(FOOD_CUT), s))[FARMING];
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        if farming(mid) > farm_cap {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (
        times(inverse, &demand(lo.powf(FOOD_CUT), lo)),
        lo.powf(FOOD_CUT),
    )
}

/// One state's year, in real people and 1990 dollars.
#[derive(Clone, Debug)]
pub struct Accounts {
    pub name: String,
    pub color: [u8; 3],
    pub population: f64,
    pub gdp: f64,
    /// Gross output of each sector, in 1990 dollars.
    pub output: [f64; 4],
    /// What households bought, in 1990 dollars.
    pub consumption: f64,
    /// Cloth bought for export, in 1990 dollars.
    pub exports: f64,
    /// Land revenue paid to the Company, in 1990 dollars.
    pub tribute: f64,
    /// Share of their food people got.
    pub fed: f64,
    /// Resources: tiles of fields, weavers' quarters, and grain in store (months).
    pub fields: u32,
    pub workshops: u32,
    pub stores: f64,
}

/// India in one year.
#[derive(Clone, Debug)]
pub struct Year {
    pub year: u32,
    pub states: Vec<Accounts>,
    pub population: f64,
    pub gdp: f64,
    pub output: [f64; 4],
    /// What households bought.
    pub consumption: f64,
    pub exports: f64,
    pub tribute: f64,
    /// Maddison's estimates for the year, for comparison.
    pub real_population: f64,
    pub real_gdp: f64,
}

impl Year {
    pub fn per_head(&self) -> f64 {
        self.gdp / self.population.max(1.0)
    }

    /// What households bought per head: GDP less exports and the Company's revenue.
    pub fn consumption_per_head(&self) -> f64 {
        self.consumption / self.population.max(1.0)
    }

    /// Share of their food people got, across India.
    pub fn fed(&self) -> f64 {
        self.states
            .iter()
            .map(|s| s.fed * s.population)
            .sum::<f64>()
            / self.population.max(1.0)
    }
}

/// What the economy remembers from one year to the next.
#[derive(Default)]
pub struct Economy {
    /// Real people per simulated person, fixed in January 1600.
    pub people_scale: f64,
    /// 1990 dollars per model unit, fixed in the first year.
    gdp_scale: f64,
    /// Cargoes each company had shipped home by the end of last year.
    shipped: BTreeMap<Entity, f32>,
    /// Each town's usual harvest, as food per person-month, averaged over recent
    /// years. A year is good or bad against this.
    usual: BTreeMap<Entity, f64>,
    pub years: Vec<Year>,
}

/// Indian states' simulated people now.
pub fn local_population(world: &World) -> u32 {
    let civs = world.read::<Civ>();
    let owners = world.read::<Owner>();
    world
        .read::<City>()
        .iter()
        .filter(|(e, _)| {
            owners
                .get(*e)
                .and_then(|o| civs.get(o.0))
                .is_some_and(|c| c.is_local())
        })
        .map(|(_, c)| c.population)
        .sum()
}

impl Economy {
    pub fn start(&mut self, world: &World) {
        let year = era::START_YEAR as f64;
        self.people_scale = maddison(year).0 / local_population(world).max(1) as f64;
        self.gdp_scale = 0.0;
        self.years.clear();
        self.usual.clear();
        self.shipped = world
            .read::<Civ>()
            .iter()
            .map(|(e, c)| (e, c.exports))
            .collect();
    }

    /// Adds up the year that ends this month, and starts counting the next.
    pub fn close_year(&mut self, world: &World, year: u32) {
        let inverse = leontief_inverse(&INPUTS);
        let v = value_added();
        let map = world.resource::<Map>();
        let tallies = tally(&map, &world.resource::<Tiles>());
        let civs = world.read::<Civ>();
        let owners = world.read::<Owner>();
        let positions = world.read::<Position>();
        let mut cities = world.write::<City>();

        // Each company's cargoes this year, shared out among its factories, are cloth
        // bought from the state of the nearest Indian town.
        let local_towns: Vec<(Position, Entity)> = cities
            .iter()
            .filter_map(|(e, _)| {
                let civ = owners.get(e)?.0;
                civs.get(civ)
                    .filter(|c| c.is_local())
                    .and_then(|_| positions.get(e).map(|p| (*p, civ)))
            })
            .collect();
        let mut exports: BTreeMap<Entity, f64> = BTreeMap::new();
        for (company, c) in civs.iter().filter(|(_, c)| !c.is_local()) {
            let before = self.shipped.insert(company, c.exports).unwrap_or(0.0);
            let cargoes = (c.exports - before).max(0.0) as f64;
            let factories: Vec<Position> = cities
                .iter()
                .filter(|(e, _)| owners.get(*e).is_some_and(|o| o.0 == company))
                .filter_map(|(e, _)| positions.get(e).copied())
                .collect();
            for f in &factories {
                if let Some((_, state)) = local_towns.iter().min_by_key(|(p, _)| p.dist2(*f)) {
                    *exports.entry(*state).or_default() +=
                        cargoes * CLOTH_PER_CARGO / factories.len() as f64;
                }
            }
        }

        // Each state's people, harvests and resources.
        #[derive(Default)]
        struct Sums {
            people: f64,
            /// People weighted by how much of a good harvest their town brought in.
            harvested: f64,
            stores: f64,
            fields: u32,
            workshops: u32,
        }
        let mut sums: BTreeMap<Entity, Sums> = BTreeMap::new();
        self.usual.retain(|e, _| cities.get(*e).is_some());
        for (e, city) in cities.iter_mut() {
            let Some(civ) = owners.get(e).map(|o| o.0) else {
                continue;
            };
            if !civs.get(civ).is_some_and(|c| c.is_local()) {
                city.food_income = 0.0;
                continue;
            }
            let s = sums.entry(civ).or_default();
            let t = tallies.get(&e).copied().unwrap_or_default();
            // Farming produces what the land gave this year. People eat one unit of
            // food a month and want to put some by, so a harvest short of that, or of
            // a usual year where the land never gives that much, is a bad one. A
            // bumper crop beyond it rots once the granaries are full. Grain didn't
            // travel far, so each town's harvest is reckoned on its own.
            let people = city.population as f64;
            if people > 0.0 {
                let per_head = city.food_income as f64 / (12.0 * people);
                let usual = *self.usual.entry(e).or_insert(per_head);
                let enough = usual.min(WANTED).max(1e-9);
                s.harvested += people * (per_head / enough).min(1.0);
                self.usual.insert(e, 0.9 * usual + 0.1 * per_head);
            }
            s.people += people;
            s.stores += city.food as f64;
            s.fields += t.farms + t.fields;
            s.workshops += t.workshops;
            city.food_income = 0.0;
        }

        let mut states = Vec::new();
        for (civ, s) in &sums {
            let Some(c) = civs.get(*civ) else { continue };
            if s.people <= 0.0 {
                continue;
            }
            let mut households = BASKET.map(|b| b * s.people);
            let mut export = [0.0; 4];
            export[TEXTILES] = exports.get(civ).copied().unwrap_or(0.0);
            let mut tribute = 0.0;
            if c.name == COMPANY_STATE {
                tribute = TRIBUTE * households.iter().sum::<f64>();
                households = households.map(|h| h * (1.0 - TRIBUTE));
                export[TEXTILES] += tribute;
            }
            let harvest = s.harvested / s.people;
            let full = times(&inverse, &{
                let mut d = households;
                for j in 0..4 {
                    d[j] += export[j];
                }
                d
            })[FARMING];
            let (x, fed) = produce(&inverse, households, export, harvest * full);
            let gdp: f64 = (0..4).map(|j| v[j] * x[j]).sum();
            let consumption: f64 = (0..4)
                .map(|j| households[j] * if j == FARMING { fed } else { 1.0 })
                .sum::<f64>()
                .min(gdp);
            states.push((
                c,
                s,
                x,
                gdp,
                consumption,
                export[TEXTILES] - tribute,
                tribute,
                fed,
            ));
        }

        let raw_gdp: f64 = states.iter().map(|s| s.3).sum();
        if self.gdp_scale == 0.0 && raw_gdp > 0.0 {
            self.gdp_scale = maddison(year as f64 + 0.5).1 / raw_gdp;
        }
        let k = self.gdp_scale;
        let ps = self.people_scale;
        let states: Vec<Accounts> = states
            .into_iter()
            .map(
                |(c, s, x, gdp, consumption, exports, tribute, fed)| Accounts {
                    name: c.name.clone(),
                    color: c.color,
                    population: s.people * ps,
                    gdp: gdp * k,
                    output: x.map(|o| o * k),
                    consumption: consumption * k,
                    exports: exports * k,
                    tribute: tribute * k,
                    fed,
                    fields: s.fields,
                    workshops: s.workshops,
                    stores: s.stores / s.people,
                },
            )
            .collect();
        let (real_population, real_gdp) = maddison(year as f64 + 0.5);
        let mut output = [0.0; 4];
        for s in &states {
            for j in 0..4 {
                output[j] += s.output[j];
            }
        }
        self.years.push(Year {
            year,
            population: states.iter().map(|s| s.population).sum(),
            gdp: states.iter().map(|s| s.gdp).sum(),
            consumption: states.iter().map(|s| s.consumption).sum(),
            exports: states.iter().map(|s| s.exports).sum(),
            tribute: states.iter().map(|s| s.tribute).sum(),
            output,
            states,
            real_population,
            real_gdp,
        });
    }
}

/// A sum of money or people, short: "74.3 bn", "135 m", "12,000".
pub fn short(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1} bn", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0} m", n / 1e6)
    } else if n >= 1e4 {
        format!("{:.0} k", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leontief_inverse_undoes_i_minus_a() {
        let inv = leontief_inverse(&INPUTS);
        for i in 0..4 {
            for j in 0..4 {
                let ij: f64 = (0..4)
                    .map(|k| inv[i][k] * (if k == j { 1.0 } else { 0.0 } - INPUTS[k][j]))
                    .sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((ij - want).abs() < 1e-9, "({i},{j}) = {ij}");
            }
        }
        // Every sector needs at least its own unit, and some of the others'.
        for i in 0..4 {
            assert!(inv[i][i] >= 1.0);
        }
    }

    #[test]
    fn gdp_equals_final_demand() {
        let inv = leontief_inverse(&INPUTS);
        let v = value_added();
        let d = BASKET.map(|b| b * 1000.0);
        let (x, fed) = produce(&inv, d, [0.0; 4], f64::MAX);
        let gdp: f64 = (0..4).map(|j| v[j] * x[j]).sum();
        assert_eq!(fed, 1.0);
        assert!((gdp - d.iter().sum::<f64>()).abs() < 1e-6);
    }

    #[test]
    fn a_bad_harvest_hits_weavers_before_bellies() {
        let inv = leontief_inverse(&INPUTS);
        let d = BASKET.map(|b| b * 1000.0);
        let (full, _) = produce(&inv, d, [0.0; 4], f64::MAX);
        let (lean, fed) = produce(&inv, d, [0.0; 4], 0.95 * full[FARMING]);
        assert!((lean[FARMING] - 0.95 * full[FARMING]).abs() < 1e-6);
        let cloth = lean[TEXTILES] / full[TEXTILES];
        assert!(
            fed > 0.9 && cloth < 0.9 && 1.0 - cloth > 2.0 * (1.0 - fed),
            "{fed} {cloth}"
        );
        let (famine, fed) = produce(&inv, d, [0.0; 4], 0.5 * full[FARMING]);
        assert!(fed < 0.6 && famine[TEXTILES] < 0.2 * full[TEXTILES]);
    }

    #[test]
    fn maddison_figures_are_read_between_the_estimates() {
        let (p, g) = maddison(1650.0);
        assert!((p - 150e6).abs() < 1.0);
        assert!((g - 82_500e6).abs() < 1.0);
        assert_eq!(maddison(1500.0).0, 135e6);
    }
}
