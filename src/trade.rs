//! Trade: where goods are grown, where they are worked up, and where they are sold
//! abroad.
//!
//! Each month, every Indian town brings in the goods its land is known for: cotton
//! in Gujarat and the Deccan, raw silk in Bengal, pepper on the Malabar coast, indigo
//! around Ahmedabad and Agra, saltpetre in Bihar, and a little cotton everywhere.
//! Raw cotton is carted to the nearest weaving centre (a town with several weavers'
//! quarters) and woven into cloth, up to what its looms can take. Half of the cloth, silk, pepper, indigo and
//! saltpetre is then sold to the two nearest European factories within reach, the
//! nearer taking more. The rest is sold at home. What a factory buys is its company's
//! cargo for the ships home.
//!
//! Each link between two towns remembers how much moves along it in a year, so the
//! map can draw the network, and the economy can count each state's exports by the
//! sector that made them.

use std::collections::BTreeMap;

use crate::ecs::{Commands, Entity, World};
use crate::era::{self, Setting};
use crate::sim::{tally, City, Civ, Clock, Drought, Owner, Position, Rules, Tiles};
use crate::world::Map;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Good {
    Cotton,
    Cloth,
    Silk,
    Pepper,
    Indigo,
    Saltpetre,
}

pub const GOODS: [Good; 6] = [
    Good::Cotton,
    Good::Cloth,
    Good::Silk,
    Good::Pepper,
    Good::Indigo,
    Good::Saltpetre,
];

impl Good {
    pub fn name(self) -> &'static str {
        match self {
            Good::Cotton => "Raw cotton",
            Good::Cloth => "Cloth",
            Good::Silk => "Raw silk",
            Good::Pepper => "Pepper",
            Good::Indigo => "Indigo",
            Good::Saltpetre => "Saltpetre",
        }
    }

    /// The color its routes are drawn in.
    pub fn color(self) -> [u8; 3] {
        match self {
            Good::Cotton => [214, 214, 206],
            Good::Cloth => [255, 228, 160],
            Good::Silk => [250, 190, 60],
            Good::Pepper => [236, 92, 60],
            Good::Indigo => [92, 120, 255],
            Good::Saltpetre => [180, 255, 240],
        }
    }

    /// What a load is worth, against a load of raw cotton.
    pub fn price(self) -> f32 {
        match self {
            Good::Cotton => 1.0,
            Good::Cloth => 3.0,
            Good::Silk => 5.0,
            Good::Pepper => 4.0,
            Good::Indigo => 4.0,
            Good::Saltpetre => 2.0,
        }
    }

    /// The economy's sector that makes it (see [`crate::economy::SECTORS`]).
    pub fn sector(self) -> usize {
        match self {
            Good::Cloth => 1,
            Good::Saltpetre => 2,
            _ => 0,
        }
    }
}

/// Where a good grows, and how many loads a year a tile of fields there yields.
struct Source {
    good: Good,
    west: f32,
    east: f32,
    south: f32,
    north: f32,
    per_field: f32,
}

const fn source(
    good: Good,
    west: f32,
    east: f32,
    south: f32,
    north: f32,
    per_field: f32,
) -> Source {
    Source {
        good,
        west,
        east,
        south,
        north,
        per_field,
    }
}

const SOURCES: [Source; 8] = [
    // Gujarat and the black soils of Khandesh and the Deccan.
    source(Good::Cotton, 68.5, 74.8, 20.0, 24.8, 0.12),
    source(Good::Cotton, 73.5, 80.5, 16.0, 22.5, 0.12),
    // Kasimbazar and the silk districts of Bengal.
    source(Good::Silk, 86.5, 90.5, 22.0, 25.5, 0.06),
    // The Malabar coast.
    source(Good::Pepper, 74.5, 77.4, 8.0, 13.2, 0.1),
    // Sarkhej by Ahmedabad, and Bayana by Agra.
    source(Good::Indigo, 71.5, 73.8, 22.0, 24.0, 0.07),
    source(Good::Indigo, 76.5, 78.6, 26.3, 28.0, 0.07),
    // The saltpetre grounds of Bihar.
    source(Good::Saltpetre, 83.5, 87.5, 24.5, 27.0, 0.08),
    // A little cotton grows almost everywhere.
    source(Good::Cotton, 60.0, 100.0, 0.0, 40.0, 0.03),
];

/// How far raw cotton is carted to be woven, in tiles.
const WEAVE_REACH: i32 = 18;
/// A town with this many weavers' quarters is a weaving centre; smaller towns send
/// their cotton to the nearest one.
const WEAVING_CENTRE: u32 = 3;
/// Loads of cloth a weavers' quarter can weave in a year.
const LOOMS: f32 = 1.5;
/// How far a factory's buyers go, in tiles (the same reach as a factory's market).
const BUY_REACH: i32 = 25;
/// How many factories compete for a town's goods.
const BUYERS: usize = 2;
/// The share of a town's cloth, silk, pepper, indigo and saltpetre sold abroad.
pub const EXPORT_SHARE: f32 = 0.5;
/// Cargoes home per unit of value bought, set so the companies ship about as much as
/// they did before trade was traced town by town.
pub const CARGO_PER_VALUE: f32 = 4.0;

/// Goods moving between two towns.
#[derive(Clone, Debug)]
pub struct Link {
    pub from: Entity,
    pub to: Entity,
    pub good: Good,
    /// Loads a year, averaged over about a year.
    pub flow: f32,
    /// Loads moved this month.
    month: f32,
}

#[derive(Default)]
pub struct Trade {
    pub links: BTreeMap<(Entity, Entity, Good), Link>,
    /// Loads of each good brought in, made and sold abroad so far this year.
    pub produced: BTreeMap<Good, f32>,
    pub exported: BTreeMap<Good, f32>,
    /// Value sold abroad this year from each state's towns, by economic sector.
    pub state_exports: BTreeMap<Entity, [f32; 4]>,
    /// Last year's figures, kept for the panel while this year's add up.
    pub last_produced: BTreeMap<Good, f32>,
    pub last_exported: BTreeMap<Good, f32>,
}

/// Runs each month.
pub fn flow(world: &World, commands: &mut Commands) {
    let rules = world.resource::<Rules>().0;
    if rules.setting != Setting::India {
        return;
    }
    let now = world.resource::<Clock>().month;
    let map = world.resource::<Map>();
    let tallies = tally(&map, &world.resource::<Tiles>());
    let drought = world.resource::<Drought>().area;
    let civs = world.read::<Civ>();
    let owners = world.read::<Owner>();
    let positions = world.read::<Position>();
    let cities = world.read::<City>();

    struct Town {
        e: Entity,
        civ: Entity,
        pos: Position,
        fields: u32,
        workshops: u32,
        local: bool,
    }
    let towns: Vec<Town> = cities
        .iter()
        .filter_map(|(e, c)| {
            let civ = owners.get(e)?.0;
            let t = tallies.get(&e).copied().unwrap_or_default();
            Some(Town {
                e,
                civ,
                pos: *positions.get(e)?,
                fields: t.fields + t.farms,
                workshops: t.workshops,
                local: civs.get(civ)?.is_local() && c.population > 0,
            })
        })
        .collect();
    let (w, h) = (map.width, map.height);
    let inside = |s: &Source, p: Position| {
        let (x0, y0) = era::project(s.west, s.north, w, h);
        let (x1, y1) = era::project(s.east, s.south, w, h);
        (x0..=x1).contains(&p.x) && (y0..=y1).contains(&p.y)
    };

    // What moves this month: (from, to, good, loads), and each factory's purchases.
    let mut moves: Vec<(Entity, Entity, Good, f32)> = Vec::new();
    let mut produced: BTreeMap<Good, f32> = BTreeMap::new();
    let mut bought: BTreeMap<Entity, f32> = BTreeMap::new();
    let mut state_value: BTreeMap<Entity, [f32; 4]> = BTreeMap::new();
    let mut cotton_in: BTreeMap<Entity, f32> = BTreeMap::new();
    let mut goods: BTreeMap<Entity, Vec<(Good, f32)>> = BTreeMap::new();

    for t in towns.iter().filter(|t| t.local) {
        // A failed monsoon leaves little to bring in.
        let dry = drought.is_some_and(|(c, r2)| t.pos.dist2(c) <= r2);
        let season = if dry { 0.3 } else { 1.0 };
        for s in SOURCES.iter().filter(|s| inside(s, t.pos)) {
            let loads = s.per_field * t.fields as f32 * season / 12.0;
            if loads <= 0.0 {
                continue;
            }
            *produced.entry(s.good).or_default() += loads;
            if s.good == Good::Cotton {
                // Carted to the nearest town with looms, or woven at home.
                let weaver = if t.workshops >= WEAVING_CENTRE {
                    Some(t)
                } else {
                    towns
                        .iter()
                        .filter(|o| o.local && o.workshops >= WEAVING_CENTRE)
                        .filter(|o| o.pos.dist2(t.pos) <= WEAVE_REACH * WEAVE_REACH)
                        .min_by_key(|o| o.pos.dist2(t.pos))
                };
                if let Some(weaver) = weaver {
                    *cotton_in.entry(weaver.e).or_default() += loads;
                    if weaver.e != t.e {
                        moves.push((t.e, weaver.e, Good::Cotton, loads));
                    }
                }
            } else {
                goods.entry(t.e).or_default().push((s.good, loads));
            }
        }
    }
    // Weaving.
    for t in towns.iter().filter(|t| t.local) {
        let cotton = cotton_in.get(&t.e).copied().unwrap_or(0.0);
        let cloth = cotton.min(t.workshops as f32 * LOOMS / 12.0);
        if cloth > 0.0 {
            *produced.entry(Good::Cloth).or_default() += cloth;
            goods.entry(t.e).or_default().push((Good::Cloth, cloth));
        }
    }
    // Sales abroad: the two nearest factories in reach compete for a town's goods,
    // and the nearer gets more.
    for t in towns.iter().filter(|t| t.local) {
        let Some(list) = goods.get(&t.e) else {
            continue;
        };
        let mut buyers: Vec<&Town> = towns
            .iter()
            .filter(|o| !o.local && o.pos.dist2(t.pos) <= BUY_REACH * BUY_REACH)
            .collect();
        buyers.sort_by_key(|o| o.pos.dist2(t.pos));
        buyers.truncate(BUYERS);
        let pull = |o: &Town| 1.0 / (o.pos.dist2(t.pos) as f32 + 9.0);
        let total: f32 = buyers.iter().map(|o| pull(o)).sum();
        for factory in buyers {
            let share = pull(factory) / total;
            for &(good, loads) in list {
                let sold = loads * EXPORT_SHARE * share;
                moves.push((t.e, factory.e, good, sold));
                *bought.entry(factory.civ).or_default() += sold * good.price();
                state_value.entry(t.civ).or_default()[good.sector()] += sold * good.price();
            }
        }
    }

    drop(cities);
    drop(positions);
    drop(owners);
    drop(civs);
    drop(map);
    commands.add(move |w| {
        {
            let mut civs = w.write::<Civ>();
            for (civ, value) in &bought {
                if let Some(c) = civs.get_mut(*civ) {
                    c.goods += value * CARGO_PER_VALUE;
                }
            }
        }
        let alive = |e: Entity| w.read::<City>().get(e).is_some();
        let mut trade = w.resource_mut::<Trade>();
        if now % 12 == 0 {
            trade.last_produced = std::mem::take(&mut trade.produced);
            trade.last_exported = std::mem::take(&mut trade.exported);
            trade.state_exports.clear();
        }
        for link in trade.links.values_mut() {
            link.month = 0.0;
        }
        for (from, to, good, loads) in moves {
            let link = trade.links.entry((from, to, good)).or_insert(Link {
                from,
                to,
                good,
                flow: 0.0,
                month: 0.0,
            });
            link.month += loads;
            if to != from && good != Good::Cotton {
                *trade.exported.entry(good).or_default() += loads;
            }
        }
        for (good, loads) in produced {
            *trade.produced.entry(good).or_default() += loads;
        }
        for (state, value) in state_value {
            let s = trade.state_exports.entry(state).or_default();
            for j in 0..4 {
                s[j] += value[j];
            }
        }
        // A year's flow, smoothed: each month adds its loads and lets a twelfth go.
        for link in trade.links.values_mut() {
            link.flow += link.month - link.flow / 12.0;
        }
        trade
            .links
            .retain(|_, l| l.flow > 0.02 && alive(l.from) && alive(l.to));
    });
}

/// What a town sends where, for its description: "Sends cloth to Surat Factory".
pub fn describe(world: &World, town: Entity) -> Vec<String> {
    let trade = world.resource::<Trade>();
    let cities = world.read::<City>();
    let name = |e: Entity| cities.get(e).map_or("?".to_string(), |c| c.name.clone());
    let mut lines = Vec::new();
    let mut out: Vec<&Link> = trade.links.values().filter(|l| l.from == town).collect();
    out.sort_by(|a, b| b.flow.total_cmp(&a.flow));
    for l in out.iter().take(4) {
        lines.push(format!(
            "Sends {:.0} loads of {} a year to {}",
            l.flow.max(1.0),
            l.good.name().to_lowercase(),
            name(l.to)
        ));
    }
    let mut incoming: BTreeMap<Good, (f32, usize)> = BTreeMap::new();
    for l in trade.links.values().filter(|l| l.to == town) {
        let e = incoming.entry(l.good).or_default();
        e.0 += l.flow;
        e.1 += 1;
    }
    for (good, (flow, n)) in incoming {
        lines.push(format!(
            "Takes in {:.0} loads of {} a year from {n} town{}",
            flow.max(1.0),
            good.name().to_lowercase(),
            if n == 1 { "" } else { "s" }
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Simulation;

    #[test]
    fn cotton_is_woven_and_goods_reach_the_factories() {
        let mut sim = Simulation::scenario(Setting::India, 7);
        for _ in 0..12 * 30 {
            sim.step_month();
        }
        let trade = sim.world.resource::<Trade>();
        let kinds = |good: Good| trade.links.values().filter(|l| l.good == good).count();
        assert!(
            kinds(Good::Cotton) > 5,
            "cotton should be carted to weavers"
        );
        assert!(kinds(Good::Cloth) > 3, "cloth should go to the factories");
        assert!(kinds(Good::Pepper) > 0, "Malabar pepper should be sold");
        let civs = sim.world.read::<Civ>();
        let cities = sim.world.read::<City>();
        for l in trade.links.values().filter(|l| l.good != Good::Cotton) {
            let owner = sim.world.read::<Owner>().get(l.to).unwrap().0;
            assert!(
                !civs.get(owner).unwrap().is_local(),
                "exports go to factories"
            );
            assert!(cities.get(l.from).is_some());
        }
        assert!(trade.last_produced[&Good::Cloth] > 0.0);
        assert!(trade.last_exported[&Good::Cloth] > 0.0);
    }

    #[test]
    fn the_new_world_has_no_traced_trade() {
        let mut sim = Simulation::scenario(Setting::NewWorld, 7);
        for _ in 0..24 {
            sim.step_month();
        }
        assert!(sim.world.resource::<Trade>().links.is_empty());
    }
}
