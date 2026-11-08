//! The simulation: the components, resources and systems that make the world develop
//! on its own. Every month the [`Schedule`] runs each system once, in order.

use std::collections::{BTreeMap, VecDeque};

use crate::ecs::{Commands, Entity, Schedule, World};
use crate::world::{Map, Terrain};

pub const MAP_WIDTH: usize = 160;
pub const MAP_HEIGHT: usize = 120;

const CIVS: [(&str, [u8; 3]); 5] = [
    ("Ostari", [220, 64, 64]),
    ("Velmar", [70, 120, 235]),
    ("Kethi", [245, 205, 50]),
    ("Aurelan", [175, 85, 210]),
    ("Dunmori", [245, 145, 45]),
];

pub const MIN_CITY_DISTANCE: i32 = 7;
const MAX_TERRITORY_RADIUS: i32 = 9;
const SETTLER_SEARCH_RADIUS: i32 = 20;
const SETTLER_LIFESPAN: u32 = 48;
const SETTLER_POPULATION: u32 = 40;
const SETTLER_PRODUCTION: u32 = 40;
const SETTLER_MIN_POPULATION: u32 = 150;
const MAX_CITIES_PER_CIV: u32 = 16;
const MAX_FOOD: u32 = 100;
const BUILDS_PER_MONTH: u32 = 1;
/// Cities lay their roads out on a grid with this spacing, centered on the city.
const STREET_SPACING: i32 = 3;
const LOG_LENGTH: usize = 40;

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Position {
    pub fn dist2(self, other: Position) -> i32 {
        (self.x - other.x).pow(2) + (self.y - other.y).pow(2)
    }
}

/// A civilization. Its cities and settlers point back to it with [`Owner`].
pub struct Civ {
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner(pub Entity);

pub struct City {
    pub name: String,
    pub population: u32,
    pub food: u32,
    pub production: u32,
    pub founded: u32,
}

/// A unit walking to a spot where it will found a new city.
pub struct Settler {
    /// The city that trained it.
    pub home: Entity,
    pub target: Option<Position>,
    pub age: u32,
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Building {
    Empty,
    Road,
    CityCenter,
    House,
    Farm,
    Workshop,
}

impl Building {
    /// Production a city spends to build this.
    pub fn cost(self) -> u32 {
        match self {
            Building::Road => 1,
            Building::House | Building::Farm => 3,
            Building::Workshop => 5,
            Building::Empty | Building::CityCenter => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Building::Empty => "Empty",
            Building::Road => "Road",
            Building::CityCenter => "City center",
            Building::House => "Houses",
            Building::Farm => "Farm",
            Building::Workshop => "Workshop",
        }
    }
}

/// What's built on each tile and which city's territory it belongs to.
pub struct Tiles {
    width: usize,
    height: usize,
    buildings: Vec<Building>,
    owners: Vec<Option<Entity>>,
}

impl Tiles {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            buildings: vec![Building::Empty; width * height],
            owners: vec![None; width * height],
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let inside = x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height;
        inside.then(|| y as usize * self.width + x as usize)
    }

    pub fn building(&self, x: i32, y: i32) -> Building {
        self.index(x, y)
            .map_or(Building::Empty, |i| self.buildings[i])
    }

    /// The city whose territory this tile is in.
    pub fn owner(&self, x: i32, y: i32) -> Option<Entity> {
        self.index(x, y).and_then(|i| self.owners[i])
    }

    fn set_building(&mut self, x: i32, y: i32, building: Building) {
        if let Some(i) = self.index(x, y) {
            self.buildings[i] = building;
        }
    }

    /// Gives an unclaimed tile to `city`. Claimed tiles never change hands (yet).
    fn claim(&mut self, x: i32, y: i32, city: Entity) {
        if let Some(i) = self.index(x, y) {
            self.owners[i].get_or_insert(city);
        }
    }

    fn touches(&self, x: i32, y: i32, pred: impl Fn(Building) -> bool) -> bool {
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|&(dx, dy)| pred(self.building(x + dx, y + dy)))
    }
}

pub struct Clock {
    pub month: u32,
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn format_date(month: u32) -> String {
    format!(
        "{}, year {}",
        MONTH_NAMES[(month % 12) as usize],
        month / 12 + 1
    )
}

/// A small xorshift random number generator. The whole simulation draws from this one
/// resource, so a seed always plays out the same way.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % n as u64) as u32
    }

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u32) as usize]
    }
}

/// Notable things that happened, newest first.
#[derive(Default)]
pub struct EventLog {
    pub entries: VecDeque<String>,
}

impl EventLog {
    fn push(&mut self, month: u32, text: String) {
        self.entries
            .push_front(format!("{}: {}", format_date(month), text));
        self.entries.truncate(LOG_LENGTH);
    }
}

// ---------------------------------------------------------------------------
// City economy
// ---------------------------------------------------------------------------

/// How many of each building a city has.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub roads: u32,
    pub houses: u32,
    pub farms: u32,
    pub workshops: u32,
}

impl Tally {
    fn add(&mut self, building: Building) {
        match building {
            Building::Road => self.roads += 1,
            Building::House => self.houses += 1,
            Building::Farm => self.farms += 1,
            Building::Workshop => self.workshops += 1,
            Building::Empty | Building::CityCenter => {}
        }
    }

    pub fn housing(&self) -> u32 {
        20 + 25 * self.houses
    }

    pub fn food_income(&self) -> u32 {
        3 + 6 * self.farms
    }

    pub fn production_income(&self, population: u32) -> u32 {
        1 + 3 * self.workshops + population / 50
    }
}

/// Food a population eats per month.
pub fn food_need(population: u32) -> u32 {
    population.div_ceil(10)
}

fn territory_radius(population: u32) -> i32 {
    (2 + ((population / 8) as f32).sqrt() as i32).min(MAX_TERRITORY_RADIUS)
}

pub fn tally(tiles: &Tiles) -> BTreeMap<Entity, Tally> {
    let mut tallies: BTreeMap<Entity, Tally> = BTreeMap::new();
    for (building, owner) in tiles.buildings.iter().zip(&tiles.owners) {
        if let Some(city) = owner {
            tallies.entry(*city).or_default().add(*building);
        }
    }
    tallies
}

/// What a city should build next, based on what's holding it back.
fn choose_building(city: &City, tally: &Tally) -> Option<Building> {
    if tally.food_income() < food_need(city.population) + 2 {
        Some(Building::Farm)
    } else if city.population + 10 >= tally.housing() {
        Some(Building::House)
    } else if tally.workshops * 3 < tally.houses {
        Some(Building::Workshop)
    } else {
        None
    }
}

fn is_street(center: Position, x: i32, y: i32) -> bool {
    (x - center.x).rem_euclid(STREET_SPACING) == 0 || (y - center.y).rem_euclid(STREET_SPACING) == 0
}

/// The best tile in `city`'s territory for a new `kind` of building. Roads go on the
/// street grid next to existing roads; everything else goes between streets, facing a road.
fn find_site(
    map: &Map,
    tiles: &Tiles,
    rng: &mut Rng,
    city: Entity,
    center: Position,
    kind: Building,
) -> Option<Position> {
    let r = MAX_TERRITORY_RADIUS;
    let mut best: Option<(i32, Position)> = None;
    for y in center.y - r..=center.y + r {
        for x in center.x - r..=center.x + r {
            if tiles.owner(x, y) != Some(city) || tiles.building(x, y) != Building::Empty {
                continue;
            }
            let Some(terrain) = map.terrain_at(x, y).filter(|t| t.buildable()) else {
                continue;
            };
            let street = is_street(center, x, y);
            let d = center.dist2(Position { x, y });
            let score = match kind {
                Building::Road => {
                    let connected =
                        tiles.touches(x, y, |b| matches!(b, Building::Road | Building::CityCenter));
                    if !street || !connected {
                        continue;
                    }
                    -4 * d
                }
                _ => {
                    if street || !tiles.touches(x, y, |b| b == Building::Road) {
                        continue;
                    }
                    match kind {
                        Building::House => -4 * d,
                        // Farms spread outward, on open land.
                        Building::Farm if matches!(terrain, Terrain::Grass | Terrain::Sand) => d,
                        Building::Farm => continue,
                        // Workshops like forests and hills, for timber and stone.
                        _ if matches!(terrain, Terrain::Forest | Terrain::Hills) => 40 - 2 * d,
                        _ => -2 * d,
                    }
                }
            } + rng.below(4) as i32;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, Position { x, y }));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Whether a new city can be founded at `p`, given the existing (and about to be
/// founded) city sites.
fn is_valid_site(map: &Map, tiles: &Tiles, cities: &[Position], p: Position) -> bool {
    map.terrain_at(p.x, p.y).is_some_and(|t| t.buildable())
        && tiles.owner(p.x, p.y).is_none()
        && tiles.building(p.x, p.y) == Building::Empty
        && cities
            .iter()
            .all(|c| c.dist2(p) >= MIN_CITY_DISTANCE * MIN_CITY_DISTANCE)
}

/// How attractive a spot is for a city: lots of open land around, ideally by water.
fn site_score(map: &Map, p: Position) -> i32 {
    let mut score = 0;
    let mut water = false;
    for dy in -3..=3 {
        for dx in -3..=3 {
            match map.terrain_at(p.x + dx, p.y + dy) {
                Some(Terrain::Grass) => score += 2,
                Some(Terrain::Sand | Terrain::Forest | Terrain::Hills) => score += 1,
                Some(Terrain::Water) => water = true,
                _ => {}
            }
        }
    }
    score + if water { 8 } else { 0 }
}

/// The best place for a new city that can be reached over land from `from`.
fn find_city_site(
    map: &Map,
    tiles: &Tiles,
    cities: &[Position],
    from: Position,
) -> Option<Position> {
    let reach = Reach::new(map, from, SETTLER_SEARCH_RADIUS);
    reach
        .tiles()
        .filter(|&(p, _)| is_valid_site(map, tiles, cities, p))
        .max_by_key(|&(p, steps)| site_score(map, p) - steps as i32 / 2)
        .map(|(p, _)| p)
}

/// Walking distances over land (in any of 8 directions) from `origin` to every tile
/// within `radius` of it, found with a breadth-first search.
struct Reach {
    origin: Position,
    radius: i32,
    steps: Vec<Option<u16>>,
}

impl Reach {
    fn new(map: &Map, origin: Position, radius: i32) -> Self {
        let side = (2 * radius + 1) as usize;
        let mut reach = Self {
            origin,
            radius,
            steps: vec![None; side * side],
        };
        let walkable = |p: Position| map.terrain_at(p.x, p.y).is_some_and(|t| !t.is_water());
        if !walkable(origin) {
            return reach;
        }
        let mut queue = VecDeque::from([origin]);
        reach.set(origin, 0);
        while let Some(p) = queue.pop_front() {
            let next = reach.get(p).unwrap() + 1;
            for (dx, dy) in NEIGHBORS_8 {
                let n = Position {
                    x: p.x + dx,
                    y: p.y + dy,
                };
                if reach.inside(n) && reach.get(n).is_none() && walkable(n) {
                    reach.set(n, next);
                    queue.push_back(n);
                }
            }
        }
        reach
    }

    fn inside(&self, p: Position) -> bool {
        (p.x - self.origin.x).abs() <= self.radius && (p.y - self.origin.y).abs() <= self.radius
    }

    fn index(&self, p: Position) -> usize {
        let side = 2 * self.radius + 1;
        ((p.y - self.origin.y + self.radius) * side + (p.x - self.origin.x + self.radius)) as usize
    }

    fn get(&self, p: Position) -> Option<u16> {
        if self.inside(p) {
            self.steps[self.index(p)]
        } else {
            None
        }
    }

    fn set(&mut self, p: Position, steps: u16) {
        let i = self.index(p);
        self.steps[i] = Some(steps);
    }

    fn tiles(&self) -> impl Iterator<Item = (Position, u16)> + '_ {
        let side = 2 * self.radius + 1;
        self.steps.iter().enumerate().filter_map(move |(i, steps)| {
            let p = Position {
                x: self.origin.x - self.radius + i as i32 % side,
                y: self.origin.y - self.radius + i as i32 / side,
            };
            steps.map(|s| (p, s))
        })
    }
}

const NEIGHBORS_8: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

fn city_positions(world: &World) -> Vec<Position> {
    let cities = world.read::<City>();
    let positions = world.read::<Position>();
    cities
        .iter()
        .filter_map(|(e, _)| positions.get(e).copied())
        .collect()
}

const NAME_STARTS: &[&str] = &[
    "Ash", "Bel", "Cor", "Dun", "El", "Fen", "Gar", "Hal", "Is", "Kar", "Lin", "Mor", "Nor", "Ost",
    "Pel", "Quen", "Ros", "Sil", "Tor", "Ul", "Var", "Wen", "Yar", "Zel",
];
const NAME_ENDS: &[&str] = &[
    "ford", "mere", "wick", "holm", "dale", "gate", "haven", "moor", "stead", "ton", "bury",
    "fell", "port", "crest",
];

fn city_name(rng: &mut Rng) -> String {
    format!("{}{}", rng.pick(NAME_STARTS), rng.pick(NAME_ENDS))
}

fn found_city(world: &mut World, civ: Entity, at: Position) {
    // Re-check: the spot may have been claimed since the settler decided to stop here.
    let sites = city_positions(world);
    let valid = is_valid_site(
        &world.resource::<Map>(),
        &world.resource::<Tiles>(),
        &sites,
        at,
    );
    let civ_name = world
        .read::<Civ>()
        .get(civ)
        .map_or(String::new(), |c| c.name.clone());
    let month = world.resource::<Clock>().month;
    if !valid {
        let text = format!("{civ_name} settlers found their chosen land already taken");
        world.resource_mut::<EventLog>().push(month, text);
        return;
    }

    let name = city_name(&mut world.resource_mut::<Rng>());
    let city = world.spawn((
        at,
        Owner(civ),
        City {
            name: name.clone(),
            population: 10,
            food: 10,
            production: 0,
            founded: month,
        },
    ));
    let mut tiles = world.resource_mut::<Tiles>();
    tiles.set_building(at.x, at.y, Building::CityCenter);
    claim_radius(&mut tiles, at, territory_radius(10), city);
    drop(tiles);
    world
        .resource_mut::<EventLog>()
        .push(month, format!("The {civ_name} founded {name}"));
}

fn claim_radius(tiles: &mut Tiles, center: Position, r: i32, city: Entity) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                tiles.claim(center.x + dx, center.y + dy, city);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Systems, in the order they run each month
// ---------------------------------------------------------------------------

/// Farms feed people, houses hold them, workshops produce.
fn city_economy(world: &World, _: &mut Commands) {
    let tallies = tally(&world.resource::<Tiles>());
    let mut cities = world.write::<City>();
    for (e, city) in cities.iter_mut() {
        let t = tallies.get(&e).copied().unwrap_or_default();
        let need = food_need(city.population);
        let income = t.food_income();
        city.food = (city.food + income).saturating_sub(need).min(MAX_FOOD);

        let step = (city.population / 40).max(1);
        if city.food > 0 && city.population < t.housing() {
            city.population = (city.population + step).min(t.housing());
        } else if city.food == 0 && income < need {
            city.population = city.population.saturating_sub(step).max(1);
        }
        city.production += t.production_income(city.population);
    }
}

/// Big, productive cities send settlers to found new cities.
fn train_settlers(world: &World, commands: &mut Commands) {
    let map = world.resource::<Map>();
    let tiles = world.resource::<Tiles>();
    let sites = city_positions(world);
    let positions = world.read::<Position>();
    let owners = world.read::<Owner>();
    let settlers = world.read::<Settler>();
    let mut cities = world.write::<City>();

    let mut per_civ: BTreeMap<Entity, u32> = BTreeMap::new();
    for (e, _) in cities.iter() {
        *per_civ.entry(owners.get(e).unwrap().0).or_default() += 1;
    }
    // Each civilization sends out one band of settlers at a time.
    let mut traveling: Vec<Entity> = settlers
        .iter()
        .map(|(e, _)| owners.get(e).unwrap().0)
        .collect();

    for (e, city) in cities.iter_mut() {
        let civ = owners.get(e).unwrap().0;
        if city.population < SETTLER_MIN_POPULATION
            || city.production < SETTLER_PRODUCTION
            || traveling.contains(&civ)
        {
            continue;
        }
        let count = per_civ.entry(civ).or_default();
        if *count >= MAX_CITIES_PER_CIV {
            continue;
        }
        let from = *positions.get(e).unwrap();
        let Some(target) = find_city_site(&map, &tiles, &sites, from) else {
            continue;
        };
        city.population -= SETTLER_POPULATION;
        city.production -= SETTLER_PRODUCTION;
        *count += 1;
        traveling.push(civ);
        let settler = Settler {
            home: e,
            target: Some(target),
            age: 0,
        };
        commands.spawn((from, Owner(civ), settler));
    }
}

/// Cities spend production on whatever is holding them back.
fn city_build(world: &World, _: &mut Commands) {
    let map = world.resource::<Map>();
    let mut tiles = world.resource_mut::<Tiles>();
    let mut rng = world.resource_mut::<Rng>();
    let positions = world.read::<Position>();
    let mut cities = world.write::<City>();
    let mut tallies = tally(&tiles);

    for (e, city) in cities.iter_mut() {
        let Some(&center) = positions.get(e) else {
            continue;
        };
        let t = tallies.entry(e).or_default();
        for _ in 0..BUILDS_PER_MONTH {
            let Some(want) = choose_building(city, t) else {
                break;
            };
            // No free lot along a road: extend the streets instead.
            let (kind, site) = match find_site(&map, &tiles, &mut rng, e, center, want) {
                Some(site) => (want, site),
                None => match find_site(&map, &tiles, &mut rng, e, center, Building::Road) {
                    Some(site) => (Building::Road, site),
                    None => break,
                },
            };
            if city.production < kind.cost() {
                break;
            }
            city.production -= kind.cost();
            tiles.set_building(site.x, site.y, kind);
            t.add(kind);
        }
    }
}

/// Borders grow with population.
fn expand_territory(world: &World, _: &mut Commands) {
    let mut tiles = world.resource_mut::<Tiles>();
    let positions = world.read::<Position>();
    let cities = world.read::<City>();
    for (e, city) in cities.iter() {
        if let Some(&p) = positions.get(e) {
            claim_radius(&mut tiles, p, territory_radius(city.population), e);
        }
    }
}

/// Settlers walk toward their target and found a city when they get there.
fn move_settlers(world: &World, commands: &mut Commands) {
    let map = world.resource::<Map>();
    let tiles = world.resource::<Tiles>();
    let mut sites = city_positions(world);
    let owners = world.read::<Owner>();
    let civs = world.read::<Civ>();
    let month = world.resource::<Clock>().month;
    let mut positions = world.write::<Position>();
    let mut settlers = world.write::<Settler>();

    for (e, settler) in settlers.iter_mut() {
        let Some(pos) = positions.get_mut(e) else {
            continue;
        };
        let civ = owners.get(e).unwrap().0;
        settler.age += 1;
        if settler.age > SETTLER_LIFESPAN {
            let name = civs.get(civ).map_or(String::new(), |c| c.name.clone());
            commands.despawn(e);
            commands.add(move |w| {
                let text = format!("A band of {name} settlers was lost in the wilderness");
                w.resource_mut::<EventLog>().push(month, text);
            });
            continue;
        }

        if !settler
            .target
            .is_some_and(|t| is_valid_site(&map, &tiles, &sites, t))
        {
            settler.target = find_city_site(&map, &tiles, &sites, *pos);
        }
        let Some(target) = settler.target else {
            continue;
        };

        if *pos == target {
            sites.push(target);
            commands.despawn(e);
            commands.add(move |w| found_city(w, civ, target));
            continue;
        }

        // Walk one tile along the shortest land path, found by searching back from the target.
        let paths = Reach::new(&map, target, SETTLER_SEARCH_RADIUS + 4);
        let step = NEIGHBORS_8
            .iter()
            .map(|&(dx, dy)| Position {
                x: pos.x + dx,
                y: pos.y + dy,
            })
            .filter_map(|n| paths.get(n).map(|steps| (steps, n)))
            .min_by_key(|&(steps, _)| steps);
        match step {
            Some((_, next)) => *pos = next,
            // No way there any more: pick somewhere else next month.
            None => settler.target = None,
        }
    }
}

fn advance_clock(world: &World, _: &mut Commands) {
    world.resource_mut::<Clock>().month += 1;
}

// ---------------------------------------------------------------------------
// The simulation as a whole
// ---------------------------------------------------------------------------

pub struct CivSummary {
    pub name: String,
    pub color: [u8; 3],
    pub cities: u32,
    pub population: u32,
    pub settlers: u32,
}

pub struct Simulation {
    pub world: World,
    schedule: Schedule,
}

impl Simulation {
    pub fn new(width: usize, height: usize, seed: u64) -> Self {
        Self::with_map(Map::generate(width, height, seed), seed)
    }

    pub fn with_map(map: Map, seed: u64) -> Self {
        let mut world = World::new();
        world.register::<Position>();
        world.register::<Civ>();
        world.register::<Owner>();
        world.register::<City>();
        world.register::<Settler>();

        let mut rng = Rng::new(seed);
        let starts = pick_starts(&map, &mut rng, CIVS.len());
        for (&(name, color), &start) in CIVS.iter().zip(&starts) {
            let civ = world.spawn((Civ {
                name: name.to_string(),
                color,
            },));
            let settler = Settler {
                home: civ,
                target: Some(start),
                age: 0,
            };
            world.spawn((start, Owner(civ), settler));
        }

        world.insert_resource(Tiles::new(map.width, map.height));
        world.insert_resource(map);
        world.insert_resource(rng);
        world.insert_resource(Clock { month: 0 });
        world.insert_resource(EventLog::default());

        let schedule = Schedule::new()
            .with("city_economy", city_economy)
            .with("train_settlers", train_settlers)
            .with("city_build", city_build)
            .with("expand_territory", expand_territory)
            .with("move_settlers", move_settlers)
            .with("advance_clock", advance_clock);
        Self { world, schedule }
    }

    pub fn step_month(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub fn month(&self) -> u32 {
        self.world.resource::<Clock>().month
    }

    pub fn events(&self) -> Vec<String> {
        self.world
            .resource::<EventLog>()
            .entries
            .iter()
            .cloned()
            .collect()
    }

    pub fn civ_summaries(&self) -> Vec<CivSummary> {
        let civs = self.world.read::<Civ>();
        let owners = self.world.read::<Owner>();
        let mut summaries: BTreeMap<Entity, CivSummary> = civs
            .iter()
            .map(|(e, c)| {
                let summary = CivSummary {
                    name: c.name.clone(),
                    color: c.color,
                    cities: 0,
                    population: 0,
                    settlers: 0,
                };
                (e, summary)
            })
            .collect();
        for (e, city) in self.world.read::<City>().iter() {
            if let Some(s) = owners.get(e).and_then(|o| summaries.get_mut(&o.0)) {
                s.cities += 1;
                s.population += city.population;
            }
        }
        for (e, _) in self.world.read::<Settler>().iter() {
            if let Some(s) = owners.get(e).and_then(|o| summaries.get_mut(&o.0)) {
                s.settlers += 1;
            }
        }
        summaries.into_values().collect()
    }

    /// A human-readable description of a tile, for the inspect panel.
    pub fn describe_tile(&self, x: i32, y: i32) -> String {
        let map = self.world.resource::<Map>();
        let Some(terrain) = map.terrain_at(x, y) else {
            return String::new();
        };
        let tiles = self.world.resource::<Tiles>();
        let mut out = format!("Tile {x}, {y}\n{}", terrain.name());
        let building = tiles.building(x, y);
        if building != Building::Empty {
            out += &format!(" · {}", building.name());
        }

        let Some(city_entity) = tiles.owner(x, y) else {
            out += "\nUnclaimed land";
            return out;
        };
        let cities = self.world.read::<City>();
        let Some(city) = cities.get(city_entity) else {
            return out;
        };
        let civ = self
            .world
            .read::<Owner>()
            .get(city_entity)
            .and_then(|o| self.world.read::<Civ>().get(o.0).map(|c| c.name.clone()))
            .unwrap_or_default();
        let t = tally(&tiles).get(&city_entity).copied().unwrap_or_default();
        out += &format!(
            "\n\n{} ({civ})\nFounded {}\nPopulation {} / {} housing\nFood {} stored, +{} / -{} a month\nProduction {} stored, +{} a month\n{} houses · {} farms · {} workshops · {} roads",
            city.name,
            format_date(city.founded),
            city.population,
            t.housing(),
            city.food,
            t.food_income(),
            food_need(city.population),
            city.production,
            t.production_income(city.population),
            t.houses,
            t.farms,
            t.workshops,
            t.roads,
        );
        out
    }

    /// The map as text, one character per `scale`×`scale` block. Used by the terminal runner.
    pub fn ascii_map(&self, scale: usize) -> String {
        let map = self.world.resource::<Map>();
        let tiles = self.world.resource::<Tiles>();
        let mut out = String::new();
        for by in (0..map.height).step_by(scale) {
            for bx in (0..map.width).step_by(scale) {
                // Show the most important thing in the block: buildings over terrain.
                let mut best = (0, ' ');
                for y in by..(by + scale).min(map.height) {
                    for x in bx..(bx + scale).min(map.width) {
                        let candidate = match tiles.building(x as i32, y as i32) {
                            Building::CityCenter => (9, '@'),
                            Building::Workshop => (6, 'w'),
                            Building::House => (5, 'h'),
                            Building::Farm => (4, 'f'),
                            Building::Road => (3, '#'),
                            Building::Empty => match map.get(x, y) {
                                Terrain::DeepWater | Terrain::Water => (0, '~'),
                                Terrain::Sand => (1, ','),
                                Terrain::Grass => (1, '.'),
                                Terrain::Forest => (1, 'T'),
                                Terrain::Hills => (1, 'n'),
                                Terrain::Mountain => (2, 'A'),
                            },
                        };
                        if candidate.0 >= best.0 {
                            best = candidate;
                        }
                    }
                }
                out.push(best.1);
            }
            out.push('\n');
        }
        out
    }
}

/// Spread-out grassland starting spots, one per civilization.
fn pick_starts(map: &Map, rng: &mut Rng, count: usize) -> Vec<Position> {
    let mut starts: Vec<Position> = Vec::new();
    let mut min_distance = 40;
    let mut attempts = 0;
    while starts.len() < count && min_distance > 0 {
        attempts += 1;
        if attempts % 400 == 0 {
            min_distance -= 4;
        }
        let p = Position {
            x: rng.below(map.width as u32) as i32,
            y: rng.below(map.height as u32) as i32,
        };
        if map.terrain_at(p.x, p.y) == Some(Terrain::Grass)
            && starts
                .iter()
                .all(|s| s.dist2(p) >= min_distance * min_distance)
        {
            starts.push(p);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sim: &mut Simulation, months: u32) {
        for _ in 0..months {
            sim.step_month();
        }
    }

    fn total_population(sim: &Simulation) -> u32 {
        sim.civ_summaries().iter().map(|c| c.population).sum()
    }

    #[test]
    fn every_civ_founds_its_capital_in_the_first_month() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        run(&mut sim, 1);
        for civ in sim.civ_summaries() {
            assert_eq!(civ.cities, 1, "{} has no capital", civ.name);
        }
    }

    #[test]
    fn the_world_develops_on_its_own() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        run(&mut sim, 1);
        let population_at_start = total_population(&sim);
        run(&mut sim, 12 * 40);
        let cities: u32 = sim.civ_summaries().iter().map(|c| c.cities).sum();
        assert!(cities > CIVS.len() as u32, "no new cities after 40 years");
        assert!(total_population(&sim) > population_at_start * 10);
    }

    #[test]
    fn same_seed_plays_out_the_same() {
        let mut a = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 3);
        let mut b = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 3);
        run(&mut a, 12 * 20);
        run(&mut b, 12 * 20);
        assert_eq!(a.ascii_map(1), b.ascii_map(1));
        assert_eq!(total_population(&a), total_population(&b));
    }

    #[test]
    fn cities_keep_their_distance() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 11);
        run(&mut sim, 12 * 40);
        let sites = city_positions(&sim.world);
        for (i, a) in sites.iter().enumerate() {
            for b in &sites[i + 1..] {
                assert!(a.dist2(*b) >= MIN_CITY_DISTANCE * MIN_CITY_DISTANCE);
            }
        }
    }

    #[test]
    fn buildings_face_roads_and_stay_inside_their_city() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 5);
        run(&mut sim, 12 * 30);
        let tiles = sim.world.resource::<Tiles>();
        let map = sim.world.resource::<Map>();
        for y in 0..map.height as i32 {
            for x in 0..map.width as i32 {
                let b = tiles.building(x, y);
                if b == Building::Empty {
                    continue;
                }
                assert!(tiles.owner(x, y).is_some(), "{b:?} at {x},{y} has no city");
                assert!(map.get(x as usize, y as usize).buildable());
                if matches!(b, Building::House | Building::Farm | Building::Workshop) {
                    assert!(
                        tiles.touches(x, y, |n| n == Building::Road),
                        "{b:?} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_city_on_its_own_grows_houses_and_farms() {
        let mut sim = Simulation::with_map(Map::flat(60, 60, Terrain::Grass), 1);
        run(&mut sim, 12 * 15);
        let tallies = tally(&sim.world.resource::<Tiles>());
        let t = tallies.values().next().expect("a city was founded");
        assert!(t.houses > 0 && t.farms > 0 && t.roads > 0, "{t:?}");
    }
}
