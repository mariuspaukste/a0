//! The simulation: the components, resources and systems that make the world develop
//! on its own. Every month the [`Schedule`] runs each system once, in order.
//!
//! A [`Scenario`] (see [`crate::era`]) says who lives on the map in 1600 and which
//! European powers sail there, and when. Everyone has to get through the year on what
//! they harvested. Ships and settlers run themselves, but any of them can be given
//! orders (see [`Simulation::order`]).

use std::collections::{BTreeMap, VecDeque};

use crate::ecs::{Commands, Entity, Schedule, World};
use crate::era::{self, Culture, Landing, Scenario, Setting};
use crate::world::{Map, Terrain};

pub use crate::era::format_date;

/// The size of the New World map.
pub const MAP_WIDTH: usize = 160;
pub const MAP_HEIGHT: usize = 120;

pub const MIN_CITY_DISTANCE: i32 = 7;
/// Trading factories can sit right next to an existing city.
const MIN_FACTORY_DISTANCE: i32 = 3;
const MAX_TERRITORY_RADIUS: i32 = 11;
const MAX_VILLAGE_RADIUS: i32 = 9;
const MAX_FACTORY_RADIUS: i32 = 5;
const SETTLER_SEARCH_RADIUS: i32 = 22;
const SETTLER_LIFESPAN: u32 = 24;
/// Tiles a band of settlers walks in a month.
const SETTLER_SPEED: u32 = 3;
/// Tiles a ship sails in a month (the last stretch of the voyage).
const SHIP_SPEED: u32 = 10;
const MAX_TOWNS: u32 = 32;
const MAX_VILLAGES: u32 = 12;
/// Villages each New World nation has when the simulation starts.
const START_VILLAGES: usize = 6;
const MAX_BUILDS_PER_MONTH: u32 = 8;
/// Cities lay their roads out on a grid with this spacing, centered on the city.
const STREET_SPACING: i32 = 3;
const LOG_LENGTH: usize = 40;
/// How far beyond its 1600 towns a historic state founds new ones.
const HEARTLAND: i32 = 30;
/// How far a trading factory draws merchants, weavers and goods from.
const TRADE_RADIUS: i32 = 25;

// Demography, per person per month. Colonists had very large families and, once past
// the first years, lower mortality than in Europe: colonial America grew about 2.5% a
// year from births alone. Native populations grew slowly before contact, and India's
// huge population barely grew at all: high births, high deaths.
const COLONIAL_BIRTHS: f32 = 0.0040;
const COLONIAL_DEATHS: f32 = 0.0019;
const NATIVE_BIRTHS: f32 = 0.0034;
const NATIVE_DEATHS: f32 = 0.0022;
const INDIAN_BIRTHS: f32 = 0.0032;
const INDIAN_DEATHS: f32 = 0.0025;
/// Extra deaths in a colony's first two years, from disease and an unfamiliar climate:
/// "seasoning". Jamestown lost more than half its first settlers; in India the saying
/// went that two monsoons were the age of a man.
const SEASONING: f32 = 0.012;
/// Share of the people who go unfed in a month who die.
const STARVATION: f32 = 0.12;

/// Food a farm tile (several family farms) or a field tile (a village's planting
/// ground) brings in at harvest, before climate. A person eats 1 a month.
const FARM_YIELD: f32 = 360.0;
const FIELD_YIELD: f32 = 300.0;

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

    fn offset(self, dx: i32, dy: i32) -> Position {
        Position {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

/// A people: one of the land's own nations, or a European power. Its towns, settlers
/// and ships point back to it with [`Owner`].
pub struct Civ {
    pub name: String,
    /// The adjective for event messages: "English", "Mughal".
    pub people: String,
    pub color: [u8; 3],
    pub culture: Culture,
    pub trade_only: bool,
    /// When the first ship arrives (0 for peoples already there).
    pub arrival: u32,
    emigrants_per_year: f32,
    ship_passengers: u32,
    landing: Landing,
    /// Real places for new towns, nearest first (see [`era::Power::places`]).
    places: &'static [era::Town],
    /// The land a nation's towns stood on in 1600: it expands only within reach of it.
    home: Vec<Position>,
    shrine: &'static str,
    /// Builds up to 1 as people gather for the next ship.
    ship_progress: f32,
    /// Goods waiting in port for the next ship home.
    pub goods: f32,
    /// Everything shipped home so far.
    pub exports: f32,
    /// Recent exports, fading over a few years. Prosperity draws emigrants.
    recent_exports: f32,
    /// Everyone who has come over by ship.
    pub immigrants: u32,
    arrived_this_year: u32,
    shipped_this_year: f32,
}

impl Civ {
    fn nation(n: &era::Nation) -> Self {
        Self {
            name: n.name.to_string(),
            people: n.people.to_string(),
            color: n.color,
            culture: n.culture,
            trade_only: false,
            arrival: 0,
            emigrants_per_year: 0.0,
            ship_passengers: 0,
            landing: Landing::Latitude(0.5),
            places: n.places,
            home: Vec::new(),
            shrine: n.shrine,
            ship_progress: 0.0,
            goods: 0.0,
            exports: 0.0,
            recent_exports: 0.0,
            immigrants: 0,
            arrived_this_year: 0,
            shipped_this_year: 0.0,
        }
    }

    fn power(p: &era::Power) -> Self {
        Self {
            name: p.name.to_string(),
            people: p.people.to_string(),
            color: p.color,
            culture: p.culture,
            trade_only: p.trade_only,
            arrival: p.arrival,
            emigrants_per_year: p.emigrants_per_year,
            ship_passengers: p.ship_passengers,
            landing: p.landing,
            places: p.places,
            home: Vec::new(),
            shrine: "Church",
            ship_progress: 0.0,
            goods: 0.0,
            exports: 0.0,
            recent_exports: 0.0,
            immigrants: 0,
            arrived_this_year: 0,
            shipped_this_year: 0.0,
        }
    }

    /// One of the land's own peoples, not Europeans.
    pub fn is_local(&self) -> bool {
        self.culture.is_local()
    }

    fn folk(&self, civ: Entity) -> Folk {
        Folk {
            civ,
            culture: self.culture,
            trade_only: self.trade_only,
        }
    }

    /// Emigrants a month. Colonies that ship a lot home attract more of them.
    fn emigrants_per_month(&self) -> f32 {
        let prosperity = (1.0 + (self.recent_exports / 500.0).sqrt()).min(2.5);
        self.emigrants_per_year * prosperity / 12.0
    }

    /// What a building is called among these people.
    pub fn building_name(&self, b: Building) -> &'static str {
        match (b, self.culture, self.trade_only) {
            (Building::Church, ..) => self.shrine,
            (Building::Workshop, Culture::Indian, _) => "Weavers' quarter",
            (Building::Workshop, _, true) => "Warehouses",
            (Building::Fort, _, true) => "Factory",
            (Building::Fort, Culture::Indian, _) => "Fort and palace",
            (Building::Farm, Culture::Indian, _) => "Fields",
            _ => b.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner(pub Entity);

/// Who an entity (town, ship or settler band) belongs to, and how they live.
#[derive(Clone, Copy, Debug)]
pub struct Folk {
    pub civ: Entity,
    pub culture: Culture,
    pub trade_only: bool,
}

impl Folk {
    pub fn villages(self) -> bool {
        self.culture.lives_in_villages()
    }

    pub fn local(self) -> bool {
        self.culture.is_local()
    }
}

/// The folk of every owned entity: towns, settlers and ships.
fn folks(world: &World) -> BTreeMap<Entity, Folk> {
    let civs = world.read::<Civ>();
    world
        .read::<Owner>()
        .iter()
        .filter_map(|(e, o)| civs.get(o.0).map(|c| (e, c.folk(o.0))))
        .collect()
}

/// A town, village, city or factory.
pub struct City {
    pub name: String,
    pub population: u32,
    pub food: f32,
    /// Work available for building, accumulated month by month.
    pub labor: f32,
    /// `None` for towns that already stood in 1600.
    pub founded: Option<u32>,
    /// Births minus deaths not yet counted as whole people.
    growth: f32,
    pub starving: bool,
    pub outbreak: Option<Outbreak>,
    immune_until: u32,
    /// A colony fresh from Europe suffers extra deaths until this month.
    seasoning_until: u32,
    /// When hunger here last made the news.
    hunger_reported: u32,
    milestone: u32,
}

impl City {
    fn new(name: String, population: u32, food: f32, founded: Option<u32>) -> Self {
        Self {
            name,
            population,
            food,
            labor: 20.0,
            founded,
            growth: 0.0,
            starving: false,
            outbreak: None,
            immune_until: 0,
            seasoning_until: 0,
            hunger_reported: 0,
            milestone: 0,
        }
    }
}

/// An epidemic running through a town.
pub struct Outbreak {
    pub disease: &'static str,
    months_left: u32,
    /// Share of the people who die each month it lasts.
    rate: f32,
    pub deaths: f32,
}

/// A band of families walking to a spot where they will found a new town.
pub struct Settler {
    /// The town they left.
    pub home: Entity,
    pub target: Option<Position>,
    pub age: u32,
    pub people: u32,
    /// Following the player's orders rather than choosing for themselves.
    pub manual: bool,
}

/// A ship from Europe. It sails along `path` to its destination, lands its passengers,
/// loads goods and sails home off the edge of the map.
pub struct Ship {
    pub passengers: u32,
    pub voyage: Voyage,
    path: VecDeque<Position>,
    /// Following the player's orders rather than choosing for itself.
    pub manual: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voyage {
    /// Carrying people to found a new colony or factory at this spot.
    Found(Position),
    /// Carrying people to an existing port.
    Supply(Entity),
    /// Sailing to this stretch of water and waiting there for orders.
    Hold(Position),
    /// Sailing back to Europe.
    Home,
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// The scenario being played.
pub struct Rules(pub &'static Scenario);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Building {
    Empty,
    Road,
    /// The fort every colony starts as; a city's citadel; a company's factory.
    Fort,
    House,
    Farm,
    /// Sawmill, smithy, weavers, warehouses.
    Workshop,
    Wharf,
    Church,
    /// Tobacco, grown for export in the warm south of the New World.
    Plantation,
    /// Where furs are bought from native hunters, in the forested north.
    TradingPost,
    /// The center of a native village.
    Village,
    Longhouse,
    /// Maize, beans and squash grown together.
    Field,
}

impl Building {
    /// Labor a town spends to build this.
    pub fn cost(self) -> f32 {
        match self {
            Building::Road => 1.0,
            Building::Field => 8.0,
            Building::Longhouse | Building::TradingPost => 12.0,
            Building::Farm | Building::Plantation => 14.0,
            Building::House | Building::Workshop => 16.0,
            Building::Wharf => 20.0,
            Building::Church => 40.0,
            Building::Empty | Building::Fort | Building::Village => 0.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Building::Empty => "Empty",
            Building::Road => "Road",
            Building::Fort => "Fort",
            Building::House => "Houses",
            Building::Farm => "Farm",
            Building::Workshop => "Workshop",
            Building::Wharf => "Wharf",
            Building::Church => "Church",
            Building::Plantation => "Tobacco plantation",
            Building::TradingPost => "Trading post",
            Building::Village => "Village",
            Building::Longhouse => "Longhouse",
            Building::Field => "Maize fields",
        }
    }

    pub fn is_center(self) -> bool {
        matches!(self, Building::Fort | Building::Village)
    }

    /// Crops, which have to clear the forest first if planted there.
    fn is_crop(self) -> bool {
        matches!(
            self,
            Building::Farm | Building::Field | Building::Plantation
        )
    }

    fn is_village(self) -> bool {
        matches!(
            self,
            Building::Village | Building::Longhouse | Building::Field
        )
    }
}

/// What's built on each tile and which town's territory it belongs to.
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

    /// The town whose territory this tile is in.
    pub fn owner(&self, x: i32, y: i32) -> Option<Entity> {
        self.index(x, y).and_then(|i| self.owners[i])
    }

    fn set_building(&mut self, x: i32, y: i32, building: Building) {
        if let Some(i) = self.index(x, y) {
            self.buildings[i] = building;
        }
    }

    /// Makes `city` the owner of a tile, whoever held it.
    fn take(&mut self, x: i32, y: i32, city: Entity) {
        if let Some(i) = self.index(x, y) {
            self.owners[i] = Some(city);
        }
    }

    /// Gives an unclaimed tile to `city`.
    fn claim(&mut self, x: i32, y: i32, city: Entity) {
        if let Some(i) = self.index(x, y) {
            self.owners[i].get_or_insert(city);
        }
    }

    /// Gives a tile to `city` even if someone holds it, as long as `may_take` agrees and
    /// nothing but fields stands there: land granted to a trading company.
    fn grant(&mut self, x: i32, y: i32, city: Entity, may_take: impl Fn(Entity) -> bool) {
        if let Some(i) = self.index(x, y) {
            let open = matches!(
                self.buildings[i],
                Building::Empty | Building::Farm | Building::Field
            );
            match self.owners[i] {
                None => self.owners[i] = Some(city),
                Some(owner) if owner != city && open && may_take(owner) => {
                    self.owners[i] = Some(city);
                    if self.buildings[i] == Building::Field {
                        self.buildings[i] = Building::Empty;
                    }
                }
                _ => {}
            }
        }
    }

    /// Frees all of `city`'s land. Its fields go back to the wild.
    fn release(&mut self, city: Entity) {
        for (owner, building) in self.owners.iter_mut().zip(&mut self.buildings) {
            if *owner == Some(city) {
                *owner = None;
                *building = Building::Empty;
            }
        }
    }

    pub fn touches(&self, x: i32, y: i32, pred: impl Fn(Building) -> bool) -> bool {
        NEIGHBORS_4
            .iter()
            .any(|&(dx, dy)| pred(self.building(x + dx, y + dy)))
    }
}

pub struct Clock {
    pub month: u32,
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

    /// A value in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
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

/// A failed monsoon: this year's harvest within the area will be poor.
#[derive(Default)]
pub struct Drought {
    pub area: Option<(Position, i32)>,
}

/// Sailing distances from the edges of the map that ships arrive from, to every
/// stretch of sea they can reach. Lakes inland have no distance.
pub struct Ocean {
    width: usize,
    height: usize,
    dist: Vec<Option<u16>>,
}

impl Ocean {
    /// `lanes` says which edges ships come in from: west, east, south.
    fn new(map: &Map, lanes: [bool; 3]) -> Self {
        let (width, height) = (map.width, map.height);
        let mut ocean = Self {
            width,
            height,
            dist: vec![None; width * height],
        };
        let (w, h) = (width as i32, height as i32);
        let mut edge = Vec::new();
        if lanes[0] {
            edge.extend((0..h).map(|y| Position { x: 0, y }));
        }
        if lanes[1] {
            edge.extend((0..h).map(|y| Position { x: w - 1, y }));
        }
        if lanes[2] {
            edge.extend((0..w).map(|x| Position { x, y: h - 1 }));
        }
        let mut queue = VecDeque::new();
        for p in edge {
            if map.terrain_at(p.x, p.y).is_some_and(|t| t.is_water()) {
                let i = ocean.index(p).unwrap();
                if ocean.dist[i].is_none() {
                    ocean.dist[i] = Some(0);
                    queue.push_back(p);
                }
            }
        }
        while let Some(p) = queue.pop_front() {
            let next = ocean.dist(p).unwrap() + 1;
            for (dx, dy) in NEIGHBORS_8 {
                let n = p.offset(dx, dy);
                if let Some(i) = ocean.index(n) {
                    if ocean.dist[i].is_none() && map.get(n.x as usize, n.y as usize).is_water() {
                        ocean.dist[i] = Some(next);
                        queue.push_back(n);
                    }
                }
            }
        }
        ocean
    }

    fn index(&self, p: Position) -> Option<usize> {
        let inside =
            p.x >= 0 && p.y >= 0 && (p.x as usize) < self.width && (p.y as usize) < self.height;
        inside.then(|| p.y as usize * self.width + p.x as usize)
    }

    fn dist(&self, p: Position) -> Option<u16> {
        self.index(p).and_then(|i| self.dist[i])
    }

    pub fn is_sea(&self, p: Position) -> bool {
        self.dist(p).is_some()
    }

    /// Whether a ship can reach the water next to this (land) tile.
    pub fn is_coast(&self, p: Position) -> bool {
        NEIGHBORS_8
            .iter()
            .any(|&(dx, dy)| self.dist(p.offset(dx, dy)).is_some())
    }

    /// How far out to sea the nearest water to a coast tile is from the sea lanes.
    fn coast_dist(&self, p: Position) -> u16 {
        NEIGHBORS_8
            .iter()
            .filter_map(|&(dx, dy)| self.dist(p.offset(dx, dy)))
            .min()
            .unwrap_or(u16::MAX)
    }

    /// Where a ship coming from Europe enters the map on its way to `to`.
    fn entry(&self, to: Position) -> Option<Position> {
        (0..self.width * self.height)
            .filter(|&i| self.dist[i] == Some(0))
            .map(|i| Position {
                x: (i % self.width) as i32,
                y: (i / self.width) as i32,
            })
            .min_by_key(|p| p.dist2(to))
    }

    /// The shortest sea route from `from` to `to` if it's open sea, or to the water
    /// next to it if it's land.
    fn route(&self, from: Position, to: Position) -> Option<VecDeque<Position>> {
        let start = self.index(from)?;
        let mut parent = vec![u32::MAX; self.width * self.height];
        parent[start] = start as u32;
        let mut queue = VecDeque::from([from]);
        let exact = self.is_sea(to);
        while let Some(p) = queue.pop_front() {
            let arrived = if exact {
                p == to
            } else {
                (p.x - to.x).abs() <= 1 && (p.y - to.y).abs() <= 1
            };
            if arrived {
                let mut path = VecDeque::new();
                let mut i = self.index(p).unwrap();
                while i != start {
                    path.push_front(Position {
                        x: (i % self.width) as i32,
                        y: (i / self.width) as i32,
                    });
                    i = parent[i] as usize;
                }
                return Some(path);
            }
            for (dx, dy) in NEIGHBORS_8 {
                let n = p.offset(dx, dy);
                if let Some(i) = self.index(n) {
                    if parent[i] == u32::MAX && self.dist[i].is_some() {
                        parent[i] = self.index(p).unwrap() as u32;
                        queue.push_back(n);
                    }
                }
            }
        }
        None
    }

    /// One step from `p` toward Europe.
    fn homeward(&self, p: Position) -> Option<Position> {
        let here = self.dist(p)?;
        NEIGHBORS_8
            .iter()
            .map(|&(dx, dy)| p.offset(dx, dy))
            .filter_map(|n| self.dist(n).map(|d| (d, n)))
            .filter(|&(d, _)| d < here)
            .min_by_key(|&(d, _)| d)
            .map(|(_, n)| n)
    }
}

// ---------------------------------------------------------------------------
// Town economy
// ---------------------------------------------------------------------------

/// How many of each building a town has, and what land its territory covers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub roads: u32,
    pub houses: u32,
    pub farms: u32,
    pub workshops: u32,
    pub wharves: u32,
    pub churches: u32,
    pub plantations: u32,
    pub trading_posts: u32,
    pub longhouses: u32,
    pub fields: u32,
    /// Untouched forest in the territory: game, furs, timber.
    pub forest: u32,
    /// Untouched open land in the territory.
    pub open: u32,
}

impl Tally {
    fn add(&mut self, building: Building, terrain: Terrain) {
        match building {
            Building::Road => self.roads += 1,
            Building::House => self.houses += 1,
            Building::Farm => self.farms += 1,
            Building::Workshop => self.workshops += 1,
            Building::Wharf => self.wharves += 1,
            Building::Church => self.churches += 1,
            Building::Plantation => self.plantations += 1,
            Building::TradingPost => self.trading_posts += 1,
            Building::Longhouse => self.longhouses += 1,
            Building::Field => self.fields += 1,
            Building::Empty => match terrain {
                Terrain::Forest => self.forest += 1,
                Terrain::Grass | Terrain::Sand | Terrain::Hills => self.open += 1,
                _ => {}
            },
            Building::Fort | Building::Village => {}
        }
    }

    /// People the town can house. A tile of houses holds four households of about six;
    /// a longhouse holds several related families.
    pub fn housing(&self, villages: bool) -> u32 {
        if villages {
            30 + 25 * self.longhouses
        } else {
            40 + 24 * self.houses
        }
    }

    /// Food brought in at harvest.
    pub fn harvest(&self, growing: f32) -> f32 {
        (FARM_YIELD * self.farms as f32 + FIELD_YIELD * self.fields as f32) * growing
    }

    /// Food a month from hunting and gathering, before winter. There's only so much
    /// game, and only so many hunters.
    pub fn forage(&self, villages: bool, population: u32) -> f32 {
        let found = if villages {
            0.4 * self.forest as f32 + 0.08 * self.open as f32
        } else {
            0.1 * self.forest as f32
        };
        found.min(population as f32 * 0.6)
    }

    /// Food a month from the sea or a lake, before winter.
    pub fn fishing(&self, by_water: bool, population: u32) -> f32 {
        let shore = if by_water {
            (population as f32 * 0.25).min(40.0)
        } else {
            0.0
        };
        shore + 40.0 * self.wharves as f32
    }

    /// Roughly what a town can count on eating each month over a year.
    fn food_per_month(&self, folk: Folk, population: u32, growing: f32, water: bool) -> f32 {
        if folk.trade_only {
            // Factories buy their food in the local markets.
            return f32::MAX;
        }
        let villages = folk.villages();
        self.harvest(growing) / 12.0
            + 0.8 * self.forage(villages, population)
            + 0.8 * self.fishing(water, population)
    }
}

pub fn tally(map: &Map, tiles: &Tiles) -> BTreeMap<Entity, Tally> {
    let mut tallies: BTreeMap<Entity, Tally> = BTreeMap::new();
    for (i, (building, owner)) in tiles.buildings.iter().zip(&tiles.owners).enumerate() {
        if let Some(city) = owner {
            let terrain = map.get(i % map.width, i / map.width);
            tallies.entry(*city).or_default().add(*building, terrain);
        }
    }
    tallies
}

/// Whether there's water close enough to fish.
pub fn by_water(map: &Map, p: Position) -> bool {
    (-2..=2).any(|dy| {
        (-2..=2).any(|dx| {
            map.terrain_at(p.x + dx, p.y + dy)
                .is_some_and(|t| t.is_water())
        })
    })
}

fn territory_radius(folk: Folk, population: u32) -> i32 {
    if folk.villages() {
        // Villages claim wide hunting grounds.
        (3 + ((population / 25) as f32).sqrt() as i32).min(MAX_VILLAGE_RADIUS)
    } else if folk.trade_only {
        // A factory holds only what the local ruler grants it.
        (3 + ((population / 200) as f32).sqrt() as i32).min(MAX_FACTORY_RADIUS)
    } else {
        // Colonists claimed land generously: Virginia granted 50 acres a head.
        (4 + ((population / 12) as f32).sqrt() as i32).min(MAX_TERRITORY_RADIUS)
    }
}

/// What a town wants to build, most pressing first.
fn wants(
    folk: Folk,
    setting: Setting,
    city: &City,
    t: &Tally,
    latitude: f32,
    water: bool,
) -> Vec<Building> {
    let pop = city.population;
    let growing = era::growing(setting, latitude);
    let food = t.food_per_month(folk, pop, growing, water);
    // Plant with a margin: the harvest can be poor, and the town will grow.
    let hungry = food < pop as f32 * 1.2 + 8.0;
    let mut wants = Vec::new();
    if folk.villages() {
        if hungry {
            wants.push(Building::Field);
        }
        if pop + 10 >= t.housing(true) {
            wants.push(Building::Longhouse);
        }
        return wants;
    }
    if hungry {
        wants.push(Building::Farm);
    }
    if pop + 12 >= t.housing(false) {
        wants.push(Building::House);
    }
    let port_size = if folk.trade_only { 30 } else { 60 };
    if water && t.wharves == 0 && pop >= port_size {
        wants.push(Building::Wharf);
    }
    if t.churches == 0 && pop >= 250 {
        wants.push(Building::Church);
    }
    let per_workshop = if folk.trade_only { 3 } else { 5 };
    if t.workshops * per_workshop < t.houses + 1 {
        wants.push(Building::Workshop);
    }
    if folk.local() || folk.trade_only || setting != Setting::NewWorld {
        return wants;
    }
    if latitude >= 0.55 && t.plantations * 3 < t.houses {
        wants.push(Building::Plantation);
    }
    if latitude < 0.45 && t.forest >= 10 && t.trading_posts < 1 + pop / 500 {
        wants.push(Building::TradingPost);
    }
    wants
}

fn is_street(center: Position, x: i32, y: i32) -> bool {
    (x - center.x).rem_euclid(STREET_SPACING) == 0 || (y - center.y).rem_euclid(STREET_SPACING) == 0
}

/// The best tile in `city`'s territory for a new `kind` of building.
///
/// A town has a compact core within `core2` (squared distance) of its center: streets
/// on a grid, and houses, workshops and the church between them, each facing a road.
/// Farms, plantations and trading posts spread over the countryside beyond it, and a
/// growing town spreads over the farms at its edge. Native villages grow outward from
/// the village, with no streets.
#[allow(clippy::too_many_arguments)]
fn find_site(
    map: &Map,
    tiles: &Tiles,
    rng: &mut Rng,
    city: Entity,
    center: Position,
    core2: i32,
    kind: Building,
) -> Option<Position> {
    let r = MAX_TERRITORY_RADIUS;
    let in_town = !kind.is_village() && !kind.is_crop() && kind != Building::TradingPost;
    let mut best: Option<(i32, Position)> = None;
    for y in center.y - r..=center.y + r {
        for x in center.x - r..=center.x + r {
            if tiles.owner(x, y) != Some(city) {
                continue;
            }
            let Some(terrain) = map.terrain_at(x, y).filter(|t| t.buildable()) else {
                continue;
            };
            let d = center.dist2(Position { x, y });
            let over_farm = match tiles.building(x, y) {
                Building::Empty => 0,
                Building::Farm if in_town && d <= core2 + 6 => -10,
                _ => continue,
            };
            let wooded = matches!(terrain, Terrain::Forest | Terrain::Hills);
            // Clearing forest for crops is hard work; hills are poor soil.
            let soil = match terrain {
                Terrain::Forest => -30,
                Terrain::Hills => -15,
                _ => 0,
            };
            let score = match kind {
                Building::Longhouse | Building::Field => {
                    let next_to_village = NEIGHBORS_8.iter().any(|&(dx, dy)| {
                        tiles.owner(x + dx, y + dy) == Some(city)
                            && tiles.building(x + dx, y + dy) != Building::Empty
                    });
                    if !next_to_village {
                        continue;
                    }
                    if kind == Building::Longhouse {
                        -4 * d
                    } else {
                        -d + soil
                    }
                }
                Building::Farm | Building::Plantation | Building::TradingPost => {
                    if d <= core2 {
                        continue;
                    }
                    match kind {
                        // Trading posts go out on the forest frontier.
                        Building::TradingPost if terrain == Terrain::Forest => d,
                        Building::TradingPost => d - 60,
                        // Farms ring the town, as close in as they can.
                        _ => -d + soil,
                    }
                }
                Building::Road => {
                    let connected = tiles.touches(x, y, |b| b == Building::Road || b.is_center());
                    if !is_street(center, x, y) || !connected {
                        continue;
                    }
                    -4 * d
                }
                _ => {
                    let faces_road = tiles.touches(x, y, |b| b == Building::Road || b.is_center());
                    if is_street(center, x, y) || !faces_road {
                        continue;
                    }
                    match kind {
                        Building::House => -4 * d,
                        Building::Church => -6 * d,
                        Building::Wharf => {
                            let shore = NEIGHBORS_4.iter().any(|&(dx, dy)| {
                                map.terrain_at(x + dx, y + dy).is_some_and(|t| t.is_water())
                            });
                            if !shore {
                                continue;
                            }
                            -d
                        }
                        // Workshops like forests and hills, for timber and stone.
                        _ if wooded => 40 - 2 * d,
                        _ => -2 * d,
                    }
                }
            } + over_farm
                + rng.below(4) as i32;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, Position { x, y }));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The squared radius of a town's core, which grows with its buildings.
fn core2(t: &Tally) -> i32 {
    8 + (t.houses + t.workshops + t.churches + t.wharves) as i32
}

/// Labor to build `kind` on `terrain`: crops planted in a forest have to clear it first.
fn build_cost(kind: Building, terrain: Option<Terrain>) -> f32 {
    let clearing = kind.is_crop() && terrain == Some(Terrain::Forest);
    kind.cost() + if clearing { 10.0 } else { 0.0 }
}

/// Whether a new town can be founded at `p`, given the existing (and about to be
/// founded) town sites.
fn is_valid_site(map: &Map, tiles: &Tiles, cities: &[Position], p: Position) -> bool {
    map.terrain_at(p.x, p.y).is_some_and(|t| t.buildable())
        && tiles.owner(p.x, p.y).is_none()
        && tiles.building(p.x, p.y) == Building::Empty
        && cities
            .iter()
            .all(|c| c.dist2(p) >= MIN_CITY_DISTANCE * MIN_CITY_DISTANCE)
}

/// Whether a trading company can put a factory at `p`: on the coast, on open ground
/// that's unclaimed or held by one of the land's own peoples, and not right on top of
/// another town.
fn is_valid_factory_site(
    map: &Map,
    tiles: &Tiles,
    ocean: &Ocean,
    folks: &BTreeMap<Entity, Folk>,
    cities: &[Position],
    p: Position,
) -> bool {
    // Rulers granted companies land, fields and all.
    let open = matches!(
        tiles.building(p.x, p.y),
        Building::Empty | Building::Farm | Building::Field
    );
    map.terrain_at(p.x, p.y).is_some_and(|t| t.buildable())
        && open
        && ocean.is_coast(p)
        && tiles
            .owner(p.x, p.y)
            .is_none_or(|o| folks.get(&o).is_some_and(|f| f.local()))
        && cities
            .iter()
            .all(|c| c.dist2(p) >= MIN_FACTORY_DISTANCE * MIN_FACTORY_DISTANCE)
}

/// How attractive a spot is for a town: lots of open land around, ideally by water.
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

/// The best place for a new town that can be reached over land from `from`.
fn find_city_site(
    map: &Map,
    tiles: &Tiles,
    cities: &[Position],
    from: Position,
    home: &[Position],
) -> Option<Position> {
    let reach = Reach::new(map, from, SETTLER_SEARCH_RADIUS);
    let at_home =
        |p: Position| home.is_empty() || home.iter().any(|h| h.dist2(p) <= HEARTLAND * HEARTLAND);
    reach
        .tiles()
        .filter(|&(p, _)| is_valid_site(map, tiles, cities, p) && at_home(p))
        .max_by_key(|&(p, steps)| site_score(map, p) - steps as i32 / 2)
        .map(|(p, _)| p)
}

/// Everything needed to pick a spot on the coast for a new colony or factory.
struct Coast<'a> {
    map: &'a Map,
    tiles: &'a Tiles,
    ocean: &'a Ocean,
    folks: &'a BTreeMap<Entity, Folk>,
    cities: &'a [Position],
    /// The land's own towns and their populations: trading partners.
    locals: &'a [(Position, u32)],
    /// Names of all towns, so a company knows which historic sites are still free.
    taken: &'a [String],
}

impl Coast<'_> {
    fn valid(&self, civ: &Civ, p: Position) -> bool {
        if civ.trade_only {
            is_valid_factory_site(self.map, self.tiles, self.ocean, self.folks, self.cities, p)
        } else {
            self.ocean.is_coast(p) && is_valid_site(self.map, self.tiles, self.cities, p)
        }
    }

    /// The best stretch of coast for a power's next colony or factory. Colonists look
    /// for good land near the latitude their power historically settled; companies make
    /// first for their historical landfall, then for coasts near rich markets.
    fn landing_site(&self, civ: &Civ, first: bool) -> Option<Position> {
        let (w, h) = (self.map.width, self.map.height);
        let mut best: Option<(i32, Position)> = None;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let p = Position { x, y };
                if !self.ocean.is_coast(p) || !self.valid(civ, p) {
                    continue;
                }
                let sailing = self.ocean.coast_dist(p) as i32 / 10;
                let score = match civ.landing {
                    Landing::Near(lon, lat) if first => {
                        let (tx, ty) = era::project(lon, lat, w, h);
                        -((p.dist2(Position { x: tx, y: ty }) as f32).sqrt() as i32) * 4
                    }
                    Landing::Latitude(lat) => {
                        let off_course = (60.0 * (era::latitude(y, h) - lat).abs()) as i32;
                        site_score(self.map, p) - off_course - sailing
                    }
                    Landing::Near(..) => {
                        // Companies settled where history put their factories.
                        let history = nearest_place(civ.places, self.taken, p, w, h, 15)
                            .map_or(0, |(_, d)| 60 - 4 * d);
                        let market: u32 = self
                            .locals
                            .iter()
                            .filter(|(l, _)| l.dist2(p) <= TRADE_RADIUS * TRADE_RADIUS)
                            .map(|(_, n)| n)
                            .sum();
                        (market / 100).min(80) as i32 + history + site_score(self.map, p) / 4
                            - sailing
                    }
                };
                if best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }
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
                let n = p.offset(dx, dy);
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

    /// The next step from `from` toward the origin.
    fn step_toward_origin(&self, from: Position) -> Option<Position> {
        NEIGHBORS_8
            .iter()
            .map(|&(dx, dy)| from.offset(dx, dy))
            .filter_map(|n| self.get(n).map(|steps| (steps, n)))
            .min_by_key(|&(steps, _)| steps)
            .map(|(_, n)| n)
    }
}

const NEIGHBORS_4: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

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

/// The land's own towns and their populations.
fn local_towns(world: &World, folks: &BTreeMap<Entity, Folk>) -> Vec<(Position, u32)> {
    let positions = world.read::<Position>();
    world
        .read::<City>()
        .iter()
        .filter(|(e, _)| folks.get(e).is_some_and(|f| f.local()))
        .filter_map(|(e, c)| positions.get(e).map(|p| (*p, c.population)))
        .collect()
}

fn town_names(world: &World) -> Vec<String> {
    world
        .read::<City>()
        .iter()
        .map(|(_, c)| c.name.clone())
        .collect()
}

/// The land's own towns and their names.
fn local_towns_named(world: &World, folks: &BTreeMap<Entity, Folk>) -> Vec<(Position, String)> {
    let positions = world.read::<Position>();
    world
        .read::<City>()
        .iter()
        .filter(|(e, _)| folks.get(e).is_some_and(|f| f.local()))
        .filter_map(|(e, c)| positions.get(e).map(|p| (*p, c.name.clone())))
        .collect()
}

/// The historic place not yet used nearest to `p`, within `within` tiles, and how far
/// it is.
fn nearest_place<'a>(
    places: &'a [era::Town],
    taken: &[String],
    p: Position,
    width: usize,
    height: usize,
    within: i32,
) -> Option<(&'a str, i32)> {
    places
        .iter()
        .filter(|t| !taken.iter().any(|n| n == t.name))
        .map(|t| {
            let (x, y) = era::project(t.lon, t.lat, width, height);
            let d = (p.dist2(Position { x, y }) as f32).sqrt() as i32;
            (t.name, d)
        })
        .filter(|&(_, d)| d <= within)
        .min_by_key(|&(_, d)| d)
}

/// How a town came to be founded, for the event log.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Founding {
    /// Before 1600, on a spot chosen by the simulation.
    Prehistory,
    /// A real town that stood in 1600, at its real place.
    Historic,
    Settlers,
    Ship,
}

/// Founds a town of `people` for `civ` at `at`. If the spot was taken in the meantime,
/// the people join the nearest town of their own instead.
fn found_city(
    world: &mut World,
    civ: Entity,
    at: Position,
    people: u32,
    how: Founding,
    name: Option<&str>,
) {
    let all = folks(world);
    let sites = city_positions(world);
    let (culture, folk_name, trade_only, places) = {
        let civs = world.read::<Civ>();
        let c = civs.get(civ).unwrap();
        (c.culture, c.people.clone(), c.trade_only, c.places)
    };
    let valid = how == Founding::Historic || {
        let map = world.resource::<Map>();
        let tiles = world.resource::<Tiles>();
        if trade_only {
            let ocean = world.resource::<Ocean>();
            is_valid_factory_site(&map, &tiles, &ocean, &all, &sites, at)
        } else {
            is_valid_site(&map, &tiles, &sites, at)
        }
    };
    let month = world.resource::<Clock>().month;
    if !valid {
        let joined = rejoin(world, civ, at, people);
        let text = match joined {
            Some(town) => {
                format!("{folk_name} settlers found their chosen land taken and went on to {town}")
            }
            None => {
                format!("{folk_name} settlers found their chosen land taken and were scattered")
            }
        };
        world.resource_mut::<EventLog>().push(month, text);
        return;
    }

    let name = match name {
        Some(name) => name.to_string(),
        None => {
            let taken = town_names(world);
            let (w, h) = {
                let map = world.resource::<Map>();
                (map.width, map.height)
            };
            let historic = nearest_place(places, &taken, at, w, h, 10).map(|(n, _)| n.to_string());
            // Otherwise a company names its factory after the town it trades with:
            // "the English factory at Patna".
            let after_town = || {
                let mut locals = local_towns_named(world, &all);
                locals.sort_by_key(|(p, _)| p.dist2(at));
                locals
                    .into_iter()
                    .map(|(_, town)| format!("{town} Factory"))
                    .find(|n| !taken.contains(n))
            };
            historic
                .or_else(|| trade_only.then(after_town).flatten())
                .unwrap_or_else(|| {
                    era::town_name(culture, &mut world.resource_mut::<Rng>(), &taken)
                })
        }
    };
    // Ships bring six months of provisions. Settlers bring a year's worth: seed, stores
    // and livestock, and the help of the town they came from.
    let food = people as f32 * if how == Founding::Ship { 6.0 } else { 12.0 };
    let founded = matches!(how, Founding::Settlers | Founding::Ship).then_some(month);
    let mut town = City::new(name.clone(), people, food, founded);
    if how == Founding::Ship {
        // Newcomers from Europe die in large numbers in their first two years.
        town.seasoning_until = month + 24;
    }
    let city = world.spawn((at, Owner(civ), town));
    let folk = world.read::<Civ>().get(civ).unwrap().folk(civ);
    let mut tiles = world.resource_mut::<Tiles>();
    let center = if folk.villages() {
        Building::Village
    } else {
        Building::Fort
    };
    tiles.set_building(at.x, at.y, center);
    tiles.take(at.x, at.y, city);
    // Neighboring historic cities start small and grow toward each other.
    let mut r = territory_radius(folk, people);
    if how == Founding::Historic {
        r = r.min(3);
    }
    claim_territory(&mut tiles, &all, folk, at, r, city);
    drop(tiles);
    let text = match (how, folk.villages(), trade_only) {
        (Founding::Prehistory | Founding::Historic, ..) => return,
        (Founding::Ship, _, true) => format!(
            "The {folk_name} landed {people} factors and soldiers and built a factory at {name}"
        ),
        (Founding::Ship, ..) => {
            format!("The {folk_name} landed {people} colonists and founded {name}")
        }
        (Founding::Settlers, true, _) => format!("The {folk_name} founded the village of {name}"),
        (Founding::Settlers, false, _) if culture == Culture::Indian => {
            format!("The {folk_name} founded the town of {name}")
        }
        (Founding::Settlers, ..) => format!("{folk_name} frontier families founded {name}"),
    };
    world.resource_mut::<EventLog>().push(month, text);
}

/// Adds `people` to the town of `civ` nearest to `near`, returning its name.
fn rejoin(world: &mut World, civ: Entity, near: Position, people: u32) -> Option<String> {
    let positions = world.read::<Position>();
    let owners = world.read::<Owner>();
    let mut cities = world.write::<City>();
    let (_, city) = cities
        .iter_mut()
        .filter(|(e, _)| owners.get(*e).is_some_and(|o| o.0 == civ))
        .filter_map(|(e, c)| positions.get(e).map(|p| (p.dist2(near), c)))
        .min_by_key(|(d, _)| *d)?;
    city.population += people;
    Some(city.name.clone())
}

/// Claims the land within `r` of `center` for `city`. Trading companies are granted
/// land by the local rulers, even land that was theirs.
fn claim_territory(
    tiles: &mut Tiles,
    folks: &BTreeMap<Entity, Folk>,
    folk: Folk,
    center: Position,
    r: i32,
    city: Entity,
) {
    let may_take = |owner: Entity| folks.get(&owner).is_some_and(|f| f.local());
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                let (x, y) = (center.x + dx, center.y + dy);
                if folk.trade_only {
                    tiles.grant(x, y, city, may_take);
                } else {
                    tiles.claim(x, y, city);
                }
            }
        }
    }
}

/// The town of `civ` best placed to receive ships, the tile they tie up at, and how
/// much room it has for newcomers: the biggest town with room whose fort or wharf is on
/// the open sea.
fn best_port(world: &World, civ: Entity) -> Option<(Entity, Position, i64)> {
    let tallies = tally(&world.resource::<Map>(), &world.resource::<Tiles>());
    let owners = world.read::<Owner>();
    let villages = world
        .read::<Civ>()
        .get(civ)
        .is_some_and(|c| c.culture.lives_in_villages());
    world
        .read::<City>()
        .iter()
        .filter(|(e, _)| owners.get(*e).is_some_and(|o| o.0 == civ))
        .filter_map(|(e, city)| {
            let dock = dock_of(world, e)?;
            let housing = tallies.get(&e).map_or(0, |t| t.housing(villages));
            Some((
                city.population,
                e,
                dock,
                housing as i64 - city.population as i64,
            ))
        })
        .max_by_key(|&(population, _, _, room)| (room > 0, population))
        .map(|(_, e, dock, room)| (e, dock, room))
}

/// Where ships tie up at `city`: its fort or a wharf on the open sea.
fn dock_of(world: &World, city: Entity) -> Option<Position> {
    let tiles = world.resource::<Tiles>();
    let ocean = world.resource::<Ocean>();
    let center = *world.read::<Position>().get(city)?;
    let r = MAX_TERRITORY_RADIUS;
    (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| center.offset(dx, dy)))
        .filter(|p| {
            let b = tiles.building(p.x, p.y);
            tiles.owner(p.x, p.y) == Some(city) && (b == Building::Wharf || b.is_center())
        })
        .filter(|&p| ocean.is_coast(p))
        .min_by_key(|p| p.dist2(center))
}

// ---------------------------------------------------------------------------
// Systems, in the order they run each month
// ---------------------------------------------------------------------------

/// Harvests, hunting and fishing feed people; houses hold them; births and deaths,
/// hunger and disease change their number; work goes into building; colonies and
/// factories produce goods for export.
fn city_economy(world: &World, _: &mut Commands) {
    let setting = world.resource::<Rules>().0.setting;
    let harvest_month = world.resource::<Rules>().0.harvest_month;
    let map = world.resource::<Map>();
    let tallies = tally(&map, &world.resource::<Tiles>());
    let drought = world.resource::<Drought>().area;
    let month = world.resource::<Clock>().month;
    let all = folks(world);
    let locals = local_towns(world, &all);
    let mut log = world.resource_mut::<EventLog>();
    let positions = world.read::<Position>();
    let mut civs = world.write::<Civ>();
    let mut cities = world.write::<City>();

    for (e, city) in cities.iter_mut() {
        let (Some(&pos), Some(&folk)) = (positions.get(e), all.get(&e)) else {
            continue;
        };
        let Some(civ) = civs.get_mut(folk.civ) else {
            continue;
        };
        let villages = folk.villages();
        let t = tallies.get(&e).copied().unwrap_or_default();
        let latitude = era::latitude(pos.y, map.height);
        let winter = era::winter(setting, month, latitude);
        let growing = era::growing(setting, latitude);
        let pop = city.population;
        let people = pop as f32;

        // Food.
        let mut income = t.forage(villages, pop) * (1.0 - 0.6 * winter)
            + t.fishing(by_water(&map, pos), pop) * (1.0 - 0.4 * winter);
        if month % 12 == harvest_month {
            let failed = drought.is_some_and(|(c, r2)| c.dist2(pos) <= r2);
            income += t.harvest(growing) * if failed { 0.2 } else { 1.0 };
        }
        if folk.trade_only {
            income += people;
        }
        city.food += income - people;
        let unfed = (-city.food).max(0.0);
        city.food = city.food.clamp(0.0, people * 15.0 + 100.0);
        let starving = unfed > people * 0.05;
        let news = month >= city.hunger_reported + 60 || city.hunger_reported == 0;
        if starving && !city.starving && pop >= 30 && news {
            city.hunger_reported = month.max(1);
            let text = if setting == Setting::India {
                format!("Famine in {}: the granaries are empty", city.name)
            } else {
                format!("Hunger in {}: the food stores have run out", city.name)
            };
            log.push(month, text);
        }
        city.starving = starving;

        // Births and deaths.
        let (birth_rate, death_rate) = match folk.culture {
            Culture::Native => (NATIVE_BIRTHS, NATIVE_DEATHS),
            Culture::Indian => (INDIAN_BIRTHS, INDIAN_DEATHS),
            _ => (COLONIAL_BIRTHS, COLONIAL_DEATHS),
        };
        let mut births = people * birth_rate;
        if pop >= t.housing(villages) {
            births *= 0.15;
        }
        if starving {
            births *= 0.3;
        }
        let mut deaths = people * death_rate + unfed * STARVATION;
        if month < city.seasoning_until {
            deaths += people * SEASONING;
        }
        if let Some(outbreak) = &mut city.outbreak {
            let dead = people * outbreak.rate;
            outbreak.deaths += dead;
            deaths += dead;
        }
        city.growth += births - deaths;
        let whole = city.growth.trunc();
        city.growth -= whole;
        city.population = (people + whole).max(0.0) as u32;

        // Work: less gets done outdoors in winter.
        let workers = 1.0 + people / 12.0;
        city.labor =
            (city.labor + workers * (1.0 - 0.5 * winter) + 2.0 * t.workshops as f32).min(150.0);

        if folk.local() {
            continue;
        }
        // Exports. Companies buy cloth, pepper, indigo and saltpetre from the markets
        // around their factories; colonists grow tobacco, trade for furs, cut timber
        // and salt fish.
        let market: u32 = locals
            .iter()
            .filter(|(p, _)| p.dist2(pos) <= TRADE_RADIUS * TRADE_RADIUS)
            .map(|(_, n)| n)
            .sum();
        civ.goods += if folk.trade_only {
            market as f32 / 3000.0 * (1.0 + 0.3 * t.workshops as f32) + 0.05 * t.wharves as f32
        } else {
            let furs = t.trading_posts as f32 * (0.05 + market as f32 / 8000.0);
            0.2 * growing * t.plantations as f32
                + furs
                + 0.03 * t.workshops as f32
                + 0.04 * t.wharves as f32
        };

        for milestone in [1_000, 2_500, 5_000, 10_000] {
            if city.population >= milestone && city.milestone < milestone {
                city.milestone = milestone;
                log.push(
                    month,
                    format!("{} has grown to {} people", city.name, group(milestone)),
                );
            }
        }
    }
}

/// In India the harvest depends on the monsoon. When the rains fail over a region, the
/// crop fails with them, and famine follows: the Deccan famine of 1630–32 killed
/// millions.
fn monsoon(world: &World, _: &mut Commands) {
    let rules = world.resource::<Rules>().0;
    if rules.setting != Setting::India {
        return;
    }
    let month = world.resource::<Clock>().month;
    let mut drought = world.resource_mut::<Drought>();
    if month % 12 == (rules.harvest_month + 1) % 12 {
        drought.area = None;
    }
    if month % 12 != (rules.harvest_month + 10) % 12 {
        return;
    }
    let mut rng = world.resource_mut::<Rng>();
    if !rng.chance(0.08) {
        return;
    }
    let all = folks(world);
    let towns: Vec<(Position, String)> = {
        let positions = world.read::<Position>();
        world
            .read::<City>()
            .iter()
            .filter(|(e, _)| all.get(e).is_some_and(|f| f.local()))
            .filter_map(|(e, c)| positions.get(e).map(|p| (*p, c.name.clone())))
            .collect()
    };
    if towns.is_empty() {
        return;
    }
    let (center, name) = rng.pick(&towns).clone();
    let r = rng.range(20, 38);
    drought.area = Some((center, r * r));
    world.resource_mut::<EventLog>().push(
        month,
        format!("The monsoon has failed around {name}: the crops are withering"),
    );
}

const NATIVE_DISEASES: [&str; 5] = ["Smallpox", "Measles", "Influenza", "Typhus", "Diphtheria"];
const COLONIAL_DISEASES: [&str; 4] = ["Smallpox", "Yellow fever", "Measles", "Diphtheria"];
const INDIAN_DISEASES: [&str; 4] = ["Plague", "Cholera", "Smallpox", "Malaria"];

/// The New World's peoples had no immunity to the diseases Europeans brought. Contact
/// with colonists, traders or sailors set off epidemics that spread from village to
/// village and killed a large part of the population, often before any colonist was
/// nearby. Elsewhere, epidemics strike crowded towns now and then.
fn epidemics(world: &World, _: &mut Commands) {
    let setting = world.resource::<Rules>().0.setting;
    let month = world.resource::<Clock>().month;
    let all = folks(world);
    let mut rng = world.resource_mut::<Rng>();
    let mut log = world.resource_mut::<EventLog>();
    let positions = world.read::<Position>();
    let mut cities = world.write::<City>();
    let vulnerable = |e: &Entity| all.get(e).is_some_and(|f| f.villages());

    // Everywhere Europeans are: their towns, settlers and ships.
    let europeans: Vec<Position> = positions
        .iter()
        .filter(|(e, _)| all.get(e).is_some_and(|f| !f.local()))
        .map(|(_, p)| *p)
        .collect();
    let sick: Vec<Position> = cities
        .iter()
        .filter(|(e, c)| c.outbreak.is_some() && vulnerable(e))
        .filter_map(|(e, _)| positions.get(e).copied())
        .collect();

    for (e, city) in cities.iter_mut() {
        let Some(&pos) = positions.get(e) else {
            continue;
        };
        if let Some(outbreak) = &mut city.outbreak {
            outbreak.months_left -= 1;
            if outbreak.months_left == 0 {
                let text = format!(
                    "The {} in {} has passed; {} died",
                    outbreak.disease.to_lowercase(),
                    city.name,
                    group(outbreak.deaths.round() as u32)
                );
                log.push(month, text);
                city.outbreak = None;
            }
            continue;
        }
        if month < city.immune_until {
            continue;
        }
        let near = |list: &[Position], r: i32| list.iter().any(|p| p.dist2(pos) <= r * r);
        let native = vulnerable(&e);
        let chance = if native {
            let contact = if near(&europeans, 25) { 0.015 } else { 0.0 };
            let spread = if near(&sick, 14) { 0.10 } else { 0.0 };
            contact + spread
        } else if city.population >= 600 {
            if setting == Setting::India {
                0.0012
            } else {
                0.0008
            }
        } else {
            0.0
        };
        if chance == 0.0 || !rng.chance(chance) {
            continue;
        }
        let (disease, rate) = if native {
            (*rng.pick(&NATIVE_DISEASES), 0.07 + 0.1 * rng.unit())
        } else if setting == Setting::India {
            (*rng.pick(&INDIAN_DISEASES), 0.01 + 0.02 * rng.unit())
        } else {
            (*rng.pick(&COLONIAL_DISEASES), 0.008 + 0.01 * rng.unit())
        };
        city.outbreak = Some(Outbreak {
            disease,
            months_left: 3 + rng.below(3),
            rate,
            deaths: 0.0,
        });
        city.immune_until = month + 12 * (15 + rng.below(10));
        log.push(month, format!("{disease} broke out in {}", city.name));
    }
}

/// Towns that dwindle away are abandoned. Survivors move to the nearest town of their
/// own people, and the land is free again.
fn abandon_towns(world: &World, commands: &mut Commands) {
    let all = folks(world);
    for (e, city) in world.read::<City>().iter() {
        let villages = all.get(&e).is_some_and(|f| f.villages());
        if city.population < if villages { 20 } else { 6 } {
            commands.add(move |w| abandon(w, e));
        }
    }
}

fn abandon(world: &mut World, city: Entity) {
    let month = world.resource::<Clock>().month;
    let (Some(&pos), Some(&Owner(civ))) = (
        world.read::<Position>().get(city),
        world.read::<Owner>().get(city),
    ) else {
        return;
    };
    let Some((name, people)) = world
        .read::<City>()
        .get(city)
        .map(|c| (c.name.clone(), c.population))
    else {
        return;
    };
    world.despawn(city);
    world.resource_mut::<Tiles>().release(city);
    let text = match (people, rejoin(world, civ, pos, people)) {
        (1.., Some(other)) => {
            format!("{name} was abandoned; its last {people} people moved to {other}")
        }
        _ => format!("{name} was abandoned"),
    };
    world.resource_mut::<EventLog>().push(month, text);
}

/// People leave crowded or hungry towns for towns of their own people that have room:
/// new arrivals move on from the ports, and families move out to the frontier. Trading
/// factories draw local merchants, weavers and laborers from the towns around them.
fn migrate(world: &World, _: &mut Commands) {
    let setting = world.resource::<Rules>().0.setting;
    let map = world.resource::<Map>();
    let tallies = tally(&map, &world.resource::<Tiles>());
    let all = folks(world);
    let positions = world.read::<Position>();
    let mut cities = world.write::<City>();
    // Room for more people in each town, by housing and by food; negative when crowded.
    let mut room: Vec<(Entity, Folk, Position, i64)> = cities
        .iter()
        .filter_map(|(e, city)| {
            let folk = *all.get(&e)?;
            let p = *positions.get(e)?;
            let t = tallies.get(&e).copied().unwrap_or_default();
            let latitude = era::latitude(p.y, map.height);
            let pop = city.population;
            let growing = era::growing(setting, latitude);
            let food = t.food_per_month(folk, pop, growing, by_water(&map, p));
            // Newcomers only move where there's food to spare.
            let capacity = (t.housing(folk.villages()) as f32).min(food / 1.2) as i64;
            Some((e, folk, p, capacity - pop as i64))
        })
        .collect();
    let mut moves = Vec::new();
    for i in 0..room.len() {
        let (from, folk, _, spare) = room[i];
        if spare >= 0 {
            continue;
        }
        let p = room[i].2;
        let target = (0..room.len())
            .filter(|&j| room[j].1.civ == folk.civ && room[j].3 > 0)
            .max_by_key(|&j| room[j].3)
            // A crowded factory's hangers-on drift back into the country around it.
            .or_else(|| {
                (0..room.len())
                    .filter(|&j| folk.trade_only && room[j].1.local())
                    .min_by_key(|&j| room[j].2.dist2(p))
            });
        if let Some(j) = target {
            let people = if room[j].1.civ == folk.civ {
                (-spare / 3 + 1).min(room[j].3)
            } else {
                -spare / 3 + 1
            };
            room[j].3 -= people;
            room[i].3 += people;
            moves.push((from, room[j].0, people as u32));
        }
    }
    // Factories with room draw people from the biggest town nearby.
    for i in 0..room.len() {
        let (factory, folk, p, spare) = room[i];
        if !folk.trade_only || spare <= 0 {
            continue;
        }
        let near =
            |j: &usize| room[*j].1.local() && room[*j].2.dist2(p) <= TRADE_RADIUS * TRADE_RADIUS;
        let source = (0..room.len())
            .filter(near)
            .max_by_key(|&j| cities.get(room[j].0).map_or(0, |c| c.population));
        if let Some(j) = source {
            let available = cities.get(room[j].0).map_or(0, |c| c.population);
            let people = (spare as u32).min(2 + available / 600).min(10);
            if available > people + 100 {
                room[i].3 -= people as i64;
                moves.push((room[j].0, factory, people));
            }
        }
    }
    for (from, to, people) in moves {
        let Some(source) = cities.get_mut(from) else {
            continue;
        };
        let people = people.min(source.population);
        source.population -= people;
        if let Some(dest) = cities.get_mut(to) {
            dest.population += people;
        }
    }
}

/// Crowded towns send families out to found new towns on the frontier.
fn train_settlers(world: &World, commands: &mut Commands) {
    let map = world.resource::<Map>();
    let tiles = world.resource::<Tiles>();
    let tallies = tally(&map, &tiles);
    let sites = city_positions(world);
    let all = folks(world);
    let civs = world.read::<Civ>();
    let positions = world.read::<Position>();
    let settlers = world.read::<Settler>();
    let mut cities = world.write::<City>();

    let mut per_civ: BTreeMap<Entity, u32> = BTreeMap::new();
    for (e, _) in cities.iter() {
        if let Some(f) = all.get(&e) {
            *per_civ.entry(f.civ).or_default() += 1;
        }
    }
    // Each people sends out one band of settlers at a time.
    let mut traveling: Vec<Entity> = settlers
        .iter()
        .filter_map(|(e, _)| all.get(&e).map(|f| f.civ))
        .collect();

    for (e, city) in cities.iter_mut() {
        let Some(&folk) = all.get(&e) else {
            continue;
        };
        if folk.trade_only {
            continue;
        }
        let housing = tallies.get(&e).map_or(0, |t| t.housing(folk.villages()));
        let (min_population, people, max_towns) = match folk.culture {
            Culture::Native => (280, 70, MAX_VILLAGES),
            // The great Indian states ruled hundreds of towns.
            Culture::Indian => (100, 30, 2 * MAX_TOWNS),
            _ => (100, 30, MAX_TOWNS),
        };
        if city.population < min_population
            || city.population + 12 < housing
            || city.labor < 30.0
            || city.starving
            || traveling.contains(&folk.civ)
        {
            continue;
        }
        let count = per_civ.entry(folk.civ).or_default();
        if *count >= max_towns {
            continue;
        }
        let from = *positions.get(e).unwrap();
        let home = civs.get(folk.civ).map_or(&[][..], |c| &c.home[..]);
        let Some(target) = find_city_site(&map, &tiles, &sites, from, home) else {
            continue;
        };
        city.population -= people;
        city.labor -= 30.0;
        *count += 1;
        traveling.push(folk.civ);
        let settler = Settler {
            home: e,
            target: Some(target),
            age: 0,
            people,
            manual: false,
        };
        commands.spawn((from, Owner(folk.civ), settler));
    }
}

/// Towns spend their labor on whatever is holding them back.
fn city_build(world: &World, _: &mut Commands) {
    let setting = world.resource::<Rules>().0.setting;
    let map = world.resource::<Map>();
    let mut tiles = world.resource_mut::<Tiles>();
    let mut rng = world.resource_mut::<Rng>();
    let all = folks(world);
    let positions = world.read::<Position>();
    let mut cities = world.write::<City>();
    let mut tallies = tally(&map, &tiles);

    for (e, city) in cities.iter_mut() {
        let (Some(&center), Some(&folk)) = (positions.get(e), all.get(&e)) else {
            continue;
        };
        let latitude = era::latitude(center.y, map.height);
        let water = by_water(&map, center);
        let t = tallies.entry(e).or_default();
        // Work through everything the town needs, one of each per round, most pressing
        // first, until the labor runs out. When there's no free lot along a road for
        // something, extend the streets instead.
        let mut built = 0;
        'rounds: loop {
            let mut progress = false;
            for want in wants(folk, setting, city, t, latitude, water) {
                if built >= MAX_BUILDS_PER_MONTH {
                    break 'rounds;
                }
                let core = core2(t);
                let site = find_site(&map, &tiles, &mut rng, e, center, core, want)
                    .map(|site| (want, site))
                    .or_else(|| {
                        if folk.villages() {
                            return None;
                        }
                        find_site(&map, &tiles, &mut rng, e, center, core, Building::Road)
                            .map(|site| (Building::Road, site))
                    });
                let Some((kind, site)) = site else {
                    continue;
                };
                let terrain = map.terrain_at(site.x, site.y);
                let cost = build_cost(kind, terrain);
                if city.labor < cost {
                    break 'rounds;
                }
                city.labor -= cost;
                if tiles.building(site.x, site.y) == Building::Farm {
                    t.farms -= 1;
                }
                tiles.set_building(site.x, site.y, kind);
                t.add(kind, terrain.unwrap_or(Terrain::Grass));
                built += 1;
                progress = true;
            }
            if !progress {
                break;
            }
        }
    }
}

/// Borders grow with population.
fn expand_territory(world: &World, _: &mut Commands) {
    let all = folks(world);
    let mut tiles = world.resource_mut::<Tiles>();
    let positions = world.read::<Position>();
    for (e, city) in world.read::<City>().iter() {
        if let (Some(&p), Some(&folk)) = (positions.get(e), all.get(&e)) {
            let r = territory_radius(folk, city.population);
            claim_territory(&mut tiles, &all, folk, p, r, e);
        }
    }
}

/// Settlers walk toward their target and found a town when they get there. Settlers
/// under orders go where they're told and wait if they can't settle there.
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
        if settler.age > SETTLER_LIFESPAN && !settler.manual {
            let folk = civs.get(civ).map_or(String::new(), |c| c.people.clone());
            let (at, people) = (*pos, settler.people);
            commands.despawn(e);
            commands.add(move |w| {
                let text = match rejoin(w, civ, at, people) {
                    Some(town) => format!(
                        "{folk} settlers found no land to their liking and returned to {town}"
                    ),
                    None => format!("A band of {folk} settlers was lost in the wilderness"),
                };
                w.resource_mut::<EventLog>().push(month, text);
            });
            continue;
        }

        if !settler.manual
            && !settler
                .target
                .is_some_and(|t| is_valid_site(&map, &tiles, &sites, t))
        {
            let home = civs.get(civ).map_or(&[][..], |c| &c.home[..]);
            settler.target = find_city_site(&map, &tiles, &sites, *pos, home);
        }
        let Some(target) = settler.target else {
            continue;
        };

        // Walk along the shortest land path, found by searching back from the target.
        let distance = (pos.x - target.x).abs().max((pos.y - target.y).abs());
        let paths = Reach::new(&map, target, distance.max(SETTLER_SEARCH_RADIUS) + 4);
        for _ in 0..SETTLER_SPEED {
            if *pos == target {
                break;
            }
            match paths.step_toward_origin(*pos) {
                Some(next) => *pos = next,
                // No way there any more: pick somewhere else next month.
                None => {
                    if !settler.manual {
                        settler.target = None;
                    }
                    break;
                }
            }
        }

        if *pos == target && is_valid_site(&map, &tiles, &sites, target) {
            sites.push(target);
            let people = settler.people;
            commands.despawn(e);
            commands.add(move |w| found_city(w, civ, target, people, Founding::Settlers, None));
        }
    }
}

/// Where a ship of `civ` should go: to found a new colony or factory, or to its main
/// port. `None` if there's nowhere to go, or no room for newcomers.
fn plan_voyage(world: &World, civ_e: Entity, founding: bool) -> Option<(Voyage, Position)> {
    let civs = world.read::<Civ>();
    let civ = civs.get(civ_e)?;
    let towns = {
        let cities = world.read::<City>();
        world
            .read::<Owner>()
            .iter()
            .filter(|(e, o)| o.0 == civ_e && cities.contains(*e))
            .count()
    };
    let expand = {
        let mut rng = world.resource_mut::<Rng>();
        // Small companies opened fewer factories: the Danes had two.
        let (max_towns, chance) = if civ.trade_only {
            (7, 0.2 * (civ.emigrants_per_year / 40.0).min(1.0))
        } else {
            (3, 0.25)
        };
        founding || (towns < max_towns && rng.chance(chance))
    };
    let at_most = if civ.trade_only {
        12
    } else {
        MAX_TOWNS as usize
    };
    if !expand {
        if let Some((city, dock, room)) = best_port(world, civ_e) {
            // When every port is full, the emigrants found a new colony instead.
            if room >= 0 || civ.trade_only {
                return Some((Voyage::Supply(city), dock));
            }
        }
    }
    let all = folks(world);
    let sites = city_positions(world);
    let locals = local_towns(world, &all);
    let taken = town_names(world);
    let map = world.resource::<Map>();
    let tiles = world.resource::<Tiles>();
    let ocean = world.resource::<Ocean>();
    let coast = Coast {
        map: &map,
        tiles: &tiles,
        ocean: &ocean,
        folks: &all,
        cities: &sites,
        locals: &locals,
        taken: &taken,
    };
    if towns >= at_most {
        return None;
    }
    let site = coast.landing_site(civ, towns == 0)?;
    Some((Voyage::Found(site), site))
}

/// People gather in Europe and sail in the sailing season: first a ship to found each
/// power's first colony or factory, then a steady stream to its main port, or now and
/// then to found another along the coast.
fn send_ships(world: &World, commands: &mut Commands) {
    let setting = world.resource::<Rules>().0.setting;
    let month = world.resource::<Clock>().month;
    // Ships left Europe in spring to arrive in summer. Fleets for India left in spring
    // too, and arrived months later with the monsoon winds.
    let sailing = match setting {
        Setting::NewWorld => (2..=8).contains(&(month % 12)),
        Setting::India => (7..=11).contains(&(month % 12)),
    };
    let at_sea: Vec<Entity> = {
        let owners = world.read::<Owner>();
        world
            .read::<Ship>()
            .iter()
            .filter_map(|(e, _)| owners.get(e).map(|o| o.0))
            .collect()
    };
    let mut towns: BTreeMap<Entity, u32> = BTreeMap::new();
    {
        let owners = world.read::<Owner>();
        for (e, _) in world.read::<City>().iter() {
            *towns.entry(owners.get(e).unwrap().0).or_default() += 1;
        }
    }
    let powers: Vec<Entity> = world
        .read::<Civ>()
        .iter()
        .filter(|(_, c)| !c.is_local() && month >= c.arrival)
        .map(|(e, _)| e)
        .collect();

    for civ_e in powers {
        let founding = towns.get(&civ_e).copied().unwrap_or(0) == 0 && !at_sea.contains(&civ_e);
        let (ready, average) = {
            let mut civs = world.write::<Civ>();
            let civ = civs.get_mut(civ_e).unwrap();
            let average = civ.ship_passengers.max(1);
            civ.ship_progress =
                (civ.ship_progress + civ.emigrants_per_month() / average as f32).min(3.0);
            (civ.ship_progress >= 1.0, average)
        };
        if !sailing || !(founding || ready) {
            continue;
        }
        let Some((voyage, dock)) = plan_voyage(world, civ_e, founding) else {
            continue;
        };
        let path = {
            let ocean = world.resource::<Ocean>();
            ocean
                .entry(dock)
                .and_then(|entry| ocean.route(entry, dock).map(|path| (entry, path)))
        };
        let Some((entry, path)) = path else {
            continue;
        };
        let passengers = {
            let mut rng = world.resource_mut::<Rng>();
            let spread = average / 3;
            if founding {
                average + 15 + rng.below(spread + 1)
            } else {
                average - spread + rng.below(2 * spread + 1)
            }
        };
        if !founding {
            world.write::<Civ>().get_mut(civ_e).unwrap().ship_progress -= 1.0;
        }
        let ship = Ship {
            passengers,
            voyage,
            path,
            manual: false,
        };
        commands.spawn((entry, Owner(civ_e), ship));
    }
}

/// Ships sail along their route, land their passengers and take the goods home.
fn sail_ships(world: &World, commands: &mut Commands) {
    let map = world.resource::<Map>();
    let tiles = world.resource::<Tiles>();
    let ocean = world.resource::<Ocean>();
    let all = folks(world);
    let towns = city_positions(world);
    let mut claimed: Vec<Position> = Vec::new();
    let locals = local_towns(world, &all);
    let taken = town_names(world);
    let owners = world.read::<Owner>();
    let civs = world.read::<Civ>();
    let cities = world.read::<City>();
    let mut positions = world.write::<Position>();
    let mut ships = world.write::<Ship>();
    let coast = Coast {
        map: &map,
        tiles: &tiles,
        ocean: &ocean,
        folks: &all,
        cities: &towns,
        locals: &locals,
        taken: &taken,
    };

    for (e, ship) in ships.iter_mut() {
        let Some(pos) = positions.get_mut(e) else {
            continue;
        };
        let civ_e = owners.get(e).unwrap().0;
        let Some(civ) = civs.get(civ_e) else {
            continue;
        };

        if ship.voyage == Voyage::Home {
            for _ in 0..SHIP_SPEED {
                match ocean.homeward(*pos) {
                    Some(next) => *pos = next,
                    None => break,
                }
            }
            if ocean.dist(*pos).is_none_or(|d| d == 0) {
                commands.despawn(e);
            }
            continue;
        }

        // Someone else got to the spot first, or the port is gone.
        let lost = match ship.voyage {
            Voyage::Found(site) => !coast.valid(civ, site) || claimed.contains(&site),
            Voyage::Supply(city) => !cities.contains(city),
            Voyage::Hold(_) | Voyage::Home => false,
        };
        if lost {
            if ship.manual {
                // Wait here for new orders.
                ship.voyage = Voyage::Hold(*pos);
                ship.path.clear();
            } else {
                let site = coast.landing_site(civ, false);
                match site.and_then(|s| ocean.route(*pos, s).map(|path| (s, path))) {
                    Some((site, path)) => {
                        ship.voyage = Voyage::Found(site);
                        ship.path = path;
                    }
                    None => {
                        ship.voyage = Voyage::Home;
                        continue;
                    }
                }
            }
        }

        for _ in 0..SHIP_SPEED {
            match ship.path.pop_front() {
                Some(next) => *pos = next,
                None => break,
            }
        }
        if !ship.path.is_empty() {
            continue;
        }
        let passengers = ship.passengers;
        match ship.voyage {
            Voyage::Found(site) => {
                claimed.push(site);
                commands.add(move |w| {
                    found_city(w, civ_e, site, passengers, Founding::Ship, None);
                    unload(w, civ_e, None, passengers);
                });
            }
            Voyage::Supply(city) => commands.add(move |w| unload(w, civ_e, Some(city), passengers)),
            // Waiting for orders.
            Voyage::Hold(_) | Voyage::Home => continue,
        }
        ship.passengers = 0;
        ship.voyage = Voyage::Home;
        ship.manual = false;
    }
}

/// Lands a ship's passengers (into `city`, if they aren't founding one) and loads the
/// goods waiting in port for the voyage home.
fn unload(world: &mut World, civ: Entity, city: Option<Entity>, passengers: u32) {
    if let Some(city) = city {
        if let Some(c) = world.write::<City>().get_mut(city) {
            c.population += passengers;
            // What's left of the ship's stores comes ashore with them.
            c.food += passengers as f32 * 4.0;
        }
    }
    if let Some(c) = world.write::<Civ>().get_mut(civ) {
        c.immigrants += passengers;
        c.arrived_this_year += passengers;
        c.exports += c.goods;
        c.recent_exports += c.goods;
        c.shipped_this_year += c.goods;
        c.goods = 0.0;
    }
}

/// Each December, a line per European power on the year's arrivals and trade.
fn year_end(world: &World, _: &mut Commands) {
    let month = world.resource::<Clock>().month;
    let mut civs = world.write::<Civ>();
    for (_, civ) in civs.iter_mut() {
        civ.recent_exports *= 0.98;
    }
    if month % 12 != 11 {
        return;
    }
    let mut log = world.resource_mut::<EventLog>();
    for (_, civ) in civs.iter_mut() {
        if civ.arrived_this_year > 0 {
            let who = if civ.trade_only {
                "company men"
            } else {
                "colonists"
            };
            let mut text = format!(
                "{} {} {who} arrived this year",
                group(civ.arrived_this_year),
                civ.people
            );
            if civ.shipped_this_year >= 1.0 {
                text += &format!(
                    ", and {} cargoes went home",
                    group(civ.shipped_this_year as u32)
                );
            }
            log.push(month, text);
        }
        civ.arrived_this_year = 0;
        civ.shipped_this_year = 0.0;
    }
}

fn advance_clock(world: &World, _: &mut Commands) {
    world.resource_mut::<Clock>().month += 1;
}

/// A number with thousands separators: 12,345.
pub fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// The simulation as a whole
// ---------------------------------------------------------------------------

pub struct CivSummary {
    pub name: String,
    pub color: [u8; 3],
    /// One of the land's own peoples rather than a European power.
    pub local: bool,
    pub cities: u32,
    pub population: u32,
    pub settlers: u32,
    pub ships: u32,
    /// A short line about a European power: when it arrives, or how many people it has
    /// brought over and what it has shipped home.
    pub status: String,
}

pub struct Simulation {
    pub world: World,
    schedule: Schedule,
}

impl Simulation {
    /// The New World on a generated map of the given size.
    pub fn new(width: usize, height: usize, seed: u64) -> Self {
        Self::with_map(Map::generate(width, height, seed), seed)
    }

    /// The New World on a given map.
    pub fn with_map(map: Map, seed: u64) -> Self {
        Self::build(era::scenario(Setting::NewWorld), map, seed)
    }

    /// A scenario on its own map.
    pub fn scenario(setting: Setting, seed: u64) -> Self {
        let s = era::scenario(setting);
        let map = match setting {
            Setting::NewWorld => Map::generate(s.width, s.height, seed),
            Setting::India => Map::india(s.width, s.height, seed),
        };
        Self::build(s, map, seed)
    }

    fn build(scenario: &'static Scenario, map: Map, seed: u64) -> Self {
        let mut world = World::new();
        world.register::<Position>();
        world.register::<Civ>();
        world.register::<Owner>();
        world.register::<City>();
        world.register::<Settler>();
        world.register::<Ship>();

        let mut rng = Rng::new(seed);
        let starts = pick_starts(&map, &mut rng, scenario.nations.len());
        world.insert_resource(Tiles::new(map.width, map.height));
        world.insert_resource(Ocean::new(&map, scenario.sea_lanes));
        world.insert_resource(map);
        world.insert_resource(rng);
        world.insert_resource(Clock { month: 0 });
        world.insert_resource(EventLog::default());
        world.insert_resource(Drought::default());
        world.insert_resource(Rules(scenario));

        let schedule = Schedule::new()
            .with("monsoon", monsoon)
            .with("city_economy", city_economy)
            .with("epidemics", epidemics)
            .with("abandon_towns", abandon_towns)
            .with("migrate", migrate)
            .with("train_settlers", train_settlers)
            .with("city_build", city_build)
            .with("expand_territory", expand_territory)
            .with("move_settlers", move_settlers)
            .with("send_ships", send_ships)
            .with("sail_ships", sail_ships)
            .with("year_end", year_end)
            .with("advance_clock", advance_clock);

        // The land's own peoples have lived here for a long time. Place their towns
        // (and any European ones already standing), let them grow for a few
        // generations, then start the clock in January 1600.
        for (i, nation) in scenario.nations.iter().enumerate() {
            let civ = world.spawn((Civ::nation(nation),));
            if nation.towns.is_empty() {
                if let Some(&start) = starts.get(i) {
                    place_villages(&mut world, civ, start);
                }
            }
            place_towns(&mut world, civ, nation.towns);
        }
        for power in scenario.powers.iter().filter(|p| !p.towns.is_empty()) {
            let civ = world.spawn((Civ::power(power),));
            place_towns(&mut world, civ, power.towns);
        }
        for (_, city) in world.write::<City>().iter_mut() {
            city.food = city.population as f32 * 18.0;
            city.labor = 150.0;
        }
        for _ in 0..scenario.prehistory_years * 12 {
            schedule.run(&mut world);
        }
        world.resource_mut::<Clock>().month = 0;
        world.resource_mut::<EventLog>().entries.clear();
        world.resource_mut::<Drought>().area = None;
        for (_, city) in world.write::<City>().iter_mut() {
            city.founded = None;
            city.outbreak = None;
            city.immune_until = 0;
            city.seasoning_until = 0;
            city.hunger_reported = 0;
        }
        for (_, civ) in world.write::<Civ>().iter_mut() {
            civ.immigrants = 0;
            civ.exports = 0.0;
            civ.goods = 0.0;
            civ.recent_exports = 0.0;
            civ.arrived_this_year = 0;
            civ.shipped_this_year = 0.0;
        }
        let ships: Vec<Entity> = world.read::<Ship>().iter().map(|(e, _)| e).collect();
        for ship in ships {
            world.despawn(ship);
        }

        for power in scenario.powers.iter().filter(|p| p.towns.is_empty()) {
            world.spawn((Civ::power(power),));
        }
        Self { world, schedule }
    }

    pub fn step_month(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub fn month(&self) -> u32 {
        self.world.resource::<Clock>().month
    }

    pub fn rules(&self) -> &'static Scenario {
        self.world.resource::<Rules>().0
    }

    pub fn width(&self) -> usize {
        self.world.resource::<Map>().width
    }

    pub fn height(&self) -> usize {
        self.world.resource::<Map>().height
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
        let month = self.month();
        let civs = self.world.read::<Civ>();
        let owners = self.world.read::<Owner>();
        let mut summaries: BTreeMap<Entity, CivSummary> = civs
            .iter()
            .map(|(e, c)| {
                let status = if c.is_local() {
                    String::new()
                } else if month < c.arrival {
                    format!("Arrives {}", era::year(c.arrival))
                } else {
                    let who = if c.trade_only {
                        "arrived"
                    } else {
                        "immigrants"
                    };
                    format!(
                        "{} {who} · {} cargoes home",
                        group(c.immigrants),
                        group(c.exports as u32)
                    )
                };
                let summary = CivSummary {
                    name: c.name.clone(),
                    color: c.color,
                    local: c.is_local(),
                    cities: 0,
                    population: 0,
                    settlers: 0,
                    ships: 0,
                    status,
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
        for (e, _) in self.world.read::<Ship>().iter() {
            if let Some(s) = owners.get(e).and_then(|o| summaries.get_mut(&o.0)) {
                s.ships += 1;
            }
        }
        // The land's own peoples first, then the Europeans in order of arrival.
        let mut list: Vec<(u32, CivSummary)> = summaries
            .into_iter()
            .map(|(e, s)| (civs.get(e).map_or(0, |c| c.arrival), s))
            .collect();
        list.sort_by_key(|(arrival, s)| (!s.local, *arrival));
        list.into_iter().map(|(_, s)| s).collect()
    }

    /// A human-readable description of a tile, for the inspect panel.
    pub fn describe_tile(&self, x: i32, y: i32) -> String {
        let setting = self.rules().setting;
        let map = self.world.resource::<Map>();
        let Some(terrain) = map.terrain_at(x, y) else {
            return String::new();
        };
        let tiles = self.world.resource::<Tiles>();
        let civs = self.world.read::<Civ>();
        let owners = self.world.read::<Owner>();
        let owner_civ = tiles
            .owner(x, y)
            .and_then(|c| owners.get(c))
            .and_then(|o| civs.get(o.0));
        let mut out = format!("Tile {x}, {y}\n{}", terrain.name());
        let building = tiles.building(x, y);
        if building != Building::Empty {
            let name = owner_civ.map_or(building.name(), |c| c.building_name(building));
            out += &format!(" · {name}");
        }

        let Some(city_entity) = tiles.owner(x, y) else {
            out += "\nUnclaimed land";
            return out;
        };
        let cities = self.world.read::<City>();
        let (Some(city), Some(civ)) = (cities.get(city_entity), owner_civ) else {
            return out;
        };
        let Some(&center) = self.world.read::<Position>().get(city_entity) else {
            return out;
        };
        let villages = civ.culture.lives_in_villages();
        let latitude = era::latitude(center.y, map.height);
        let t = tally(&map, &tiles)
            .get(&city_entity)
            .copied()
            .unwrap_or_default();
        let founded = city.founded.map_or("before 1600".to_string(), format_date);
        let kind = match civ.culture {
            _ if civ.trade_only => "trading factory",
            Culture::Native => "village",
            Culture::Indian if city.population >= 1000 => "city",
            Culture::Indian => "town",
            _ => "colony",
        };
        out += &format!(
            "\n\n{} ({} {kind})\nFounded {founded}\nPopulation {} / {} housing\nFood {} stored, eats {} a month",
            city.name,
            civ.people,
            group(city.population),
            group(t.housing(villages)),
            group(city.food as u32),
            group(city.population),
        );
        if civ.trade_only {
            out += "\nBuys its food in the local markets";
        } else {
            let caught = t.forage(villages, city.population)
                + t.fishing(by_water(&map, center), city.population);
            out += &format!(
                "\nHarvest {} a year · hunting & fishing {} a month",
                group(t.harvest(era::growing(setting, latitude)) as u32),
                group(caught as u32),
            );
        }
        out += &format!("\nLabor {}", city.labor as u32);
        out += &if villages {
            format!("\n{} longhouses · {} fields", t.longhouses, t.fields)
        } else {
            let mut parts = vec![format!("{} houses", t.houses)];
            for (n, b) in [
                (t.farms, Building::Farm),
                (t.workshops, Building::Workshop),
                (t.plantations, Building::Plantation),
                (t.trading_posts, Building::TradingPost),
                (t.wharves, Building::Wharf),
                (t.churches, Building::Church),
            ] {
                if n > 0 {
                    parts.push(format!("{n} × {}", civ.building_name(b).to_lowercase()));
                }
            }
            format!("\n{}", parts.join(" · "))
        };
        if city.starving {
            out += "\nStarving: the food stores are empty";
        }
        if let Some(outbreak) = &city.outbreak {
            out += &format!(
                "\n{} epidemic: {} dead so far",
                outbreak.disease,
                group(outbreak.deaths as u32)
            );
        }
        out
    }

    // -----------------------------------------------------------------------
    // Units: ships and settlers run themselves, but the player can take over.
    // -----------------------------------------------------------------------

    /// The ship or band of settlers closest to the tile point (`x`, `y`), within
    /// `radius` tiles.
    pub fn unit_near(&self, x: f32, y: f32, radius: f32) -> Option<Entity> {
        let positions = self.world.read::<Position>();
        let ships = self.world.read::<Ship>();
        let settlers = self.world.read::<Settler>();
        positions
            .iter()
            .filter(|(e, _)| ships.contains(*e) || settlers.contains(*e))
            .map(|(e, p)| {
                let (dx, dy) = (p.x as f32 + 0.5 - x, p.y as f32 + 0.5 - y);
                (dx * dx + dy * dy, e)
            })
            .filter(|&(d2, _)| d2 <= radius * radius)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, e)| e)
    }

    pub fn unit_exists(&self, unit: Entity) -> bool {
        self.world.read::<Ship>().contains(unit) || self.world.read::<Settler>().contains(unit)
    }

    /// Where a unit is and the tiles it's headed through, for drawing.
    pub fn unit_route(&self, unit: Entity) -> Option<(Position, Vec<Position>)> {
        let pos = *self.world.read::<Position>().get(unit)?;
        if let Some(ship) = self.world.read::<Ship>().get(unit) {
            return Some((pos, ship.path.iter().copied().collect()));
        }
        let target = self.world.read::<Settler>().get(unit)?.target;
        Some((pos, target.into_iter().collect()))
    }

    /// A description of a unit for the side panel.
    pub fn describe_unit(&self, unit: Entity) -> Option<String> {
        let civs = self.world.read::<Civ>();
        let civ = civs.get(self.world.read::<Owner>().get(unit)?.0)?;
        let pos = *self.world.read::<Position>().get(unit)?;
        let control = |manual: bool| {
            if manual {
                "Under your orders"
            } else {
                "Choosing its own course"
            }
        };
        if let Some(ship) = self.world.read::<Ship>().get(unit) {
            let cities = self.world.read::<City>();
            let heading = match ship.voyage {
                Voyage::Found(p) => {
                    let what = if civ.trade_only {
                        "build a factory"
                    } else {
                        "found a colony"
                    };
                    format!("Sailing to {what} at {}, {}", p.x, p.y)
                }
                Voyage::Supply(c) => format!(
                    "Sailing to {}",
                    cities.get(c).map_or("port".to_string(), |c| c.name.clone())
                ),
                Voyage::Hold(_) if ship.path.is_empty() => "Waiting for orders".to_string(),
                Voyage::Hold(p) => format!("Sailing to {}, {}", p.x, p.y),
                Voyage::Home => "Sailing home with cargo".to_string(),
            };
            let who = if civ.trade_only {
                "company men"
            } else {
                "emigrants"
            };
            return Some(format!(
                "{} ship at {}, {}\n{} {who} aboard\n{heading}\n{}",
                civ.people,
                pos.x,
                pos.y,
                ship.passengers,
                control(ship.manual)
            ));
        }
        let settlers = self.world.read::<Settler>();
        let s = settlers.get(unit)?;
        let heading = match s.target {
            Some(t) if t == pos => "Waiting: can't settle here".to_string(),
            Some(t) => format!("Walking to {}, {}", t.x, t.y),
            None => "Looking for land".to_string(),
        };
        Some(format!(
            "{} settlers at {}, {}\n{} people\n{heading}\n{}",
            civ.people,
            pos.x,
            pos.y,
            s.people,
            control(s.manual)
        ))
    }

    /// Orders a unit to the tile (`x`, `y`) and says what it will do there.
    ///
    /// A ship sent to one of its own towns lands its passengers there; sent to a
    /// stretch of coast, it founds a colony or factory; sent to open sea, it sails
    /// there and waits. Settlers walk to the tile and found a town if they can.
    pub fn order(&mut self, unit: Entity, x: i32, y: i32) -> String {
        let target = Position { x, y };
        let (Some(&Owner(civ_e)), Some(&pos)) = (
            self.world.read::<Owner>().get(unit),
            self.world.read::<Position>().get(unit),
        ) else {
            return "That unit is gone.".to_string();
        };
        let land = self
            .world
            .resource::<Map>()
            .terrain_at(x, y)
            .is_some_and(|t| !t.is_water());

        if self.world.read::<Settler>().contains(unit) {
            if !land {
                return "Settlers can't walk on water.".to_string();
            }
            let valid = {
                let sites = city_positions(&self.world);
                let map = self.world.resource::<Map>();
                let tiles = self.world.resource::<Tiles>();
                is_valid_site(&map, &tiles, &sites, target)
            };
            let mut settlers = self.world.write::<Settler>();
            let s = settlers.get_mut(unit).unwrap();
            s.target = Some(target);
            s.manual = true;
            return if valid {
                "They'll walk there and found a town.".to_string()
            } else {
                "They'll walk there, but can't settle on claimed land or so close to a town."
                    .to_string()
            };
        }

        let plan = self.plan_order(unit, civ_e, target, land);
        let (voyage, goal, message) = match plan {
            Ok(plan) => plan,
            Err(message) => return message,
        };
        let Some(path) = self.world.resource::<Ocean>().route(pos, goal) else {
            return "There's no sea route there.".to_string();
        };
        let mut ships = self.world.write::<Ship>();
        let ship = ships.get_mut(unit).unwrap();
        ship.voyage = voyage;
        ship.path = path;
        ship.manual = true;
        message
    }

    /// What a ship ordered to `target` will do: its voyage, where it sails to, and a
    /// message for the player. An error message if it can't.
    fn plan_order(
        &self,
        unit: Entity,
        civ_e: Entity,
        target: Position,
        land: bool,
    ) -> Result<(Voyage, Position, String), String> {
        let all = folks(&self.world);
        let passengers = self
            .world
            .read::<Ship>()
            .get(unit)
            .map_or(0, |s| s.passengers);
        let own_town = self
            .world
            .resource::<Tiles>()
            .owner(target.x, target.y)
            .filter(|c| all.get(c).is_some_and(|f| f.civ == civ_e));
        if let Some(city) = own_town {
            let name = self
                .world
                .read::<City>()
                .get(city)
                .map_or(String::new(), |c| c.name.clone());
            let dock = dock_of(&self.world, city)
                .ok_or_else(|| format!("{name} has no harbor on the open sea."))?;
            let what = if passengers > 0 {
                "land its passengers"
            } else {
                "call"
            };
            return Ok((
                Voyage::Supply(city),
                dock,
                format!("Sailing to {name} to {what}."),
            ));
        }
        let ocean = self.world.resource::<Ocean>();
        if !land {
            if !ocean.is_sea(target) {
                return Err("Ships can't reach that water.".to_string());
            }
            let message = "Sailing there to wait for orders.".to_string();
            return Ok((Voyage::Hold(target), target, message));
        }
        if passengers == 0 {
            return Err("This ship has no one aboard to settle.".to_string());
        }
        if !ocean.is_coast(target) {
            return Err("Pick a spot on the coast that ships can reach.".to_string());
        }
        let sites = city_positions(&self.world);
        let locals = local_towns(&self.world, &all);
        let taken = town_names(&self.world);
        let civs = self.world.read::<Civ>();
        let civ = civs.get(civ_e).ok_or("That ship's people are gone.")?;
        let map = self.world.resource::<Map>();
        let tiles = self.world.resource::<Tiles>();
        let coast = Coast {
            map: &map,
            tiles: &tiles,
            ocean: &ocean,
            folks: &all,
            cities: &sites,
            locals: &locals,
            taken: &taken,
        };
        if !coast.valid(civ, target) {
            return Err(if civ.trade_only {
                "Too close to another town, or held by another European power.".to_string()
            } else {
                "That land is claimed, or too close to another town.".to_string()
            });
        }
        let what = if civ.trade_only {
            "build a factory"
        } else {
            "found a colony"
        };
        Ok((
            Voyage::Found(target),
            target,
            format!("Sailing there to {what}."),
        ))
    }

    /// Hands a unit back to the simulation to run on its own.
    pub fn automate(&mut self, unit: Entity) {
        if let Some(s) = self.world.write::<Settler>().get_mut(unit) {
            s.manual = false;
            s.target = None;
            return;
        }
        let (Some(&Owner(civ)), Some(&pos)) = (
            self.world.read::<Owner>().get(unit),
            self.world.read::<Position>().get(unit),
        ) else {
            return;
        };
        let passengers = self
            .world
            .read::<Ship>()
            .get(unit)
            .map_or(0, |s| s.passengers);
        let plan = if passengers > 0 {
            plan_voyage(&self.world, civ, false).and_then(|(voyage, dock)| {
                let path = self.world.resource::<Ocean>().route(pos, dock)?;
                Some((voyage, path))
            })
        } else {
            None
        };
        if let Some(ship) = self.world.write::<Ship>().get_mut(unit) {
            ship.manual = false;
            match plan {
                Some((voyage, path)) => {
                    ship.voyage = voyage;
                    ship.path = path;
                }
                None => {
                    ship.voyage = Voyage::Home;
                    ship.path.clear();
                }
            }
        }
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
                            Building::Fort => (9, '@'),
                            Building::Village => (9, 'O'),
                            Building::Church => (7, '+'),
                            Building::Wharf => (7, '='),
                            Building::Workshop => (6, 'w'),
                            Building::TradingPost => (6, 't'),
                            Building::House => (5, 'h'),
                            Building::Longhouse => (5, 'L'),
                            Building::Plantation => (4, 'p'),
                            Building::Farm => (4, 'f'),
                            Building::Field => (4, ':'),
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

/// Founds a New World nation's villages around its starting spot.
fn place_villages(world: &mut World, civ: Entity, start: Position) {
    found_city(world, civ, start, 260, Founding::Prehistory, None);
    for _ in 1..START_VILLAGES {
        let sites = city_positions(world);
        let site = find_city_site(
            &world.resource::<Map>(),
            &world.resource::<Tiles>(),
            &sites,
            start,
            &[],
        );
        match site {
            Some(site) => found_city(world, civ, site, 160, Founding::Prehistory, None),
            None => break,
        }
    }
}

/// Places towns that stood in 1600 at their real places, or the nearest open land.
fn place_towns(world: &mut World, civ: Entity, towns: &[era::Town]) {
    for town in towns {
        let spot = {
            let map = world.resource::<Map>();
            let tiles = world.resource::<Tiles>();
            let (x, y) = era::project(town.lon, town.lat, map.width, map.height);
            let free = |p: Position| {
                map.terrain_at(p.x, p.y).is_some_and(|t| t.buildable())
                    && tiles.building(p.x, p.y) == Building::Empty
            };
            (0..=6).find_map(|r| {
                (-r..=r)
                    .flat_map(|dy| {
                        (-r..=r).map(move |dx| Position {
                            x: x + dx,
                            y: y + dy,
                        })
                    })
                    .find(|&p| free(p))
            })
        };
        if let Some(at) = spot {
            found_city(
                world,
                civ,
                at,
                town.population,
                Founding::Historic,
                Some(town.name),
            );
            if let Some(c) = world.write::<Civ>().get_mut(civ) {
                c.home.push(at);
            }
        }
    }
}

/// Spread-out grassland starting spots, one per nation.
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

    fn summary<'a>(civs: &'a [CivSummary], name: &str) -> &'a CivSummary {
        civs.iter().find(|c| c.name == name).unwrap()
    }

    fn arrival(name: &str) -> u32 {
        era::scenario(Setting::NewWorld)
            .powers
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .arrival
    }

    #[test]
    fn native_nations_are_settled_in_1600_and_europeans_are_not() {
        let sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        for civ in sim.civ_summaries() {
            if civ.local {
                assert!(civ.cities >= 2, "{} has {} villages", civ.name, civ.cities);
            } else {
                assert_eq!(civ.cities, 0, "{} is already here", civ.name);
            }
        }
        assert_eq!(format_date(sim.month()), "January 1600");
    }

    #[test]
    fn colonial_powers_arrive_by_ship() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        run(&mut sim, arrival("England"));
        assert_eq!(summary(&sim.civ_summaries(), "England").cities, 0);
        run(&mut sim, 12 * 12);
        let civs = sim.civ_summaries();
        let colonies: u32 = civs.iter().filter(|c| !c.local).map(|c| c.cities).sum();
        assert!(colonies >= 4, "only {colonies} colonies by 1619");
        assert!(summary(&civs, "England").cities >= 1);
    }

    #[test]
    fn colonies_grow_from_immigration_and_births() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        run(&mut sim, 12 * 60);
        let civs = sim.civ_summaries();
        let england = summary(&civs, "England");
        assert!(england.population > 3000, "England: {}", england.population);
        assert!(england.cities > 2, "England: {} towns", england.cities);
        // England sent far more people than France.
        let france = summary(&civs, "France");
        assert!(england.population > france.population);
    }

    #[test]
    fn same_seed_plays_out_the_same() {
        let mut a = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 3);
        let mut b = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 3);
        run(&mut a, 12 * 20);
        run(&mut b, 12 * 20);
        assert_eq!(a.ascii_map(1), b.ascii_map(1));
        assert_eq!(a.events(), b.events());
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
    fn buildings_stay_inside_their_town_and_town_buildings_face_a_road() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 5);
        run(&mut sim, 12 * 50);
        let tiles = sim.world.resource::<Tiles>();
        let map = sim.world.resource::<Map>();
        let mut in_town = 0;
        for y in 0..map.height as i32 {
            for x in 0..map.width as i32 {
                let b = tiles.building(x, y);
                if b == Building::Empty {
                    continue;
                }
                assert!(tiles.owner(x, y).is_some(), "{b:?} at {x},{y} has no town");
                assert!(map.get(x as usize, y as usize).buildable());
                let town_building = matches!(
                    b,
                    Building::House | Building::Workshop | Building::Church | Building::Wharf
                );
                if town_building {
                    in_town += 1;
                    assert!(
                        tiles.touches(x, y, |n| n == Building::Road || n.is_center()),
                        "{b:?} at {x},{y}"
                    );
                }
            }
        }
        assert!(in_town > 0, "no colonial buildings after 50 years");
    }

    #[test]
    fn a_village_on_its_own_grows_longhouses_and_fields() {
        let sim = Simulation::with_map(Map::flat(60, 60, Terrain::Grass), 1);
        let tallies = tally(&sim.world.resource::<Map>(), &sim.world.resource::<Tiles>());
        let t = tallies.values().next().expect("a village exists");
        assert!(t.longhouses > 0 && t.fields > 0, "{t:?}");
        // No sea, so no Europeans.
        assert!(sim.civ_summaries().iter().all(|c| c.local || c.cities == 0));
    }

    #[test]
    fn harvest_comes_in_september() {
        let mut sim = Simulation::with_map(Map::flat(60, 60, Terrain::Grass), 2);
        let food = |sim: &Simulation| -> f32 {
            sim.world.read::<City>().iter().map(|(_, c)| c.food).sum()
        };
        run(&mut sim, 8);
        let before = food(&sim);
        run(&mut sim, 1);
        assert!(food(&sim) > before, "{before} -> {}", food(&sim));
    }

    #[test]
    fn contact_brings_epidemics_to_native_villages() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 2);
        let mut outbreaks = 0;
        for _ in 0..12 * 60 {
            sim.step_month();
            outbreaks += sim
                .events()
                .iter()
                .take(3)
                .filter(|e| e.contains("broke out"))
                .count();
        }
        assert!(outbreaks > 0);
    }

    #[test]
    fn india_starts_with_its_real_cities_and_the_portuguese() {
        let sim = Simulation::scenario(Setting::India, 1);
        let names: Vec<String> = sim
            .world
            .read::<City>()
            .iter()
            .map(|(_, c)| c.name.clone())
            .collect();
        for city in ["Agra", "Lahore", "Golconda", "Madurai", "Kandy", "Goa"] {
            assert!(names.iter().any(|n| n == city), "{city} is missing");
        }
        let civs = sim.civ_summaries();
        assert!(summary(&civs, "Mughal Empire").population > 10_000);
        assert_eq!(summary(&civs, "English East India Company").cities, 0);
        assert!(summary(&civs, "Estado da Índia").cities >= 4);
    }

    #[test]
    fn the_companies_build_factories_that_draw_people_in() {
        let mut sim = Simulation::scenario(Setting::India, 1);
        run(&mut sim, 12 * 40);
        let civs = sim.civ_summaries();
        let eic = summary(&civs, "English East India Company");
        assert!(eic.cities >= 1, "no English factory by 1640");
        assert!(
            eic.population > 200,
            "English factories: {}",
            eic.population
        );
        let mughals = summary(&civs, "Mughal Empire");
        assert!(mughals.population > 10 * eic.population);
    }

    #[test]
    fn units_can_be_given_orders_and_handed_back() {
        let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, 7);
        let ship = (0..12 * 12).find_map(|_| {
            sim.step_month();
            let ships = sim.world.read::<Ship>();
            let first = ships.iter().map(|(e, _)| e).next();
            first
        });
        let ship = ship.expect("a ship by 1612");
        let ocean_tile = {
            let ocean = sim.world.resource::<Ocean>();
            let (w, h) = (sim.width() as i32, sim.height() as i32);
            (0..h)
                .flat_map(|y| (0..w).map(move |x| Position { x, y }))
                .find(|&p| ocean.dist(p) == Some(5))
                .unwrap()
        };
        let reply = sim.order(ship, ocean_tile.x, ocean_tile.y);
        assert!(reply.contains("wait"), "{reply}");
        assert!(sim.world.read::<Ship>().get(ship).unwrap().manual);
        run(&mut sim, 12);
        assert_eq!(*sim.world.read::<Position>().get(ship).unwrap(), ocean_tile);
        sim.automate(ship);
        assert!(!sim.world.read::<Ship>().get(ship).unwrap().manual);
    }

    #[test]
    fn numbers_are_grouped() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1,000");
        assert_eq!(group(1234567), "1,234,567");
    }
}
