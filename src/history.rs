//! Real history, played against the simulation.
//!
//! The simulation grows the world on its own. This module holds what really happened
//! in India from 1600 as a timeline of events. Each event has conditions: when its date
//! comes, it happens only if the simulated world still allows it. The Battle of Plassey
//! needs a Company factory in Bengal, a Nawab of Bengal to fight, and Robert Clive,
//! who made his name at Arcot six years before. If it happens, it changes the world:
//! towns change hands, new states rise, harvests fail. Some events can still happen a
//! few years late, once their conditions come right.
//!
//! The player can change history with what-ifs: a strong heir for Aurangzeb, a French
//! government that backs Dupleix, a Mir Jafar who stays loyal. Each later event checks
//! its conditions against the world as it now is, so one change can stop a chain of
//! events from happening, or make them happen differently. The chronicle records what
//! happened as in history, what went differently, and what never happened, and why.
//!
//! Every December the economy is added up (see [`crate::economy`]), so the timeline's
//! population and GDP can be set against the real ones.

use std::collections::BTreeSet;

use crate::economy::{Economy, COMPANY_STATE};
use crate::ecs::{Commands, Entity, World};
use crate::era::{self, month, Setting, Town};
use crate::sim::{
    self, City, Civ, Clock, Drought, EventLog, Outbreak, Owner, Position, Rules, DROUGHT_KEEP,
};
use crate::world::Map;

/// How an event of the timeline came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It happened as it did in history.
    AsInHistory,
    /// It happened, but not as in history.
    Different,
    /// The world had changed too much for it to happen.
    DidNotHappen,
    /// A change the player made.
    Yours,
}

/// A line of the chronicle.
#[derive(Clone, Debug)]
pub struct Entry {
    pub month: u32,
    pub title: &'static str,
    /// What happened in this timeline, or why it didn't.
    pub text: String,
    /// What happened in real history.
    pub real: &'static str,
    pub outcome: Outcome,
}

/// Things that have happened (or been changed) that later events depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    StrongHeir,
    NaderTurnsBack,
    FranceBacksDupleix,
    MirJafarLoyal,
    CompanyBankrupt,
    FrenchHoldMadras,
    ArcotHeld,
    PlasseyWon,
    Diwani,
}

#[derive(Default)]
pub struct History {
    on: bool,
    /// How firmly the Mughal emperor holds his provinces, from 0 to 1.
    pub cohesion: f32,
    marks: BTreeSet<Mark>,
    /// Oldest first.
    pub chronicle: Vec<Entry>,
    /// Which events of the timeline have come and gone.
    settled: Vec<bool>,
    /// Failed monsoons waiting for their year's harvest.
    droughts: Vec<Failure>,
    /// The English town the French took in 1746, to hand back.
    madras: Option<Entity>,
    /// Wars and battles, oldest first, for the map.
    pub wars: Vec<War>,
    pub economy: Economy,
}

/// A war, campaign or battle, as the map shows it.
#[derive(Clone, Debug)]
pub struct War {
    pub name: &'static str,
    /// Where the fighting is: a battlefield or a besieged town.
    pub site: Position,
    /// Who attacks. Invaders from beyond the map, such as Nader Shah's Persians, have
    /// no civ of their own, only a color.
    pub attacker: Option<Entity>,
    pub color: [u8; 3],
    /// Where the attack comes from, if not the middle of the attacker's towns.
    pub from: Option<Position>,
    pub start: u32,
    /// The last month the fighting goes on.
    pub end: u32,
    /// Towns fought over, hatched on the map while the war goes on.
    pub towns: Vec<Entity>,
    /// Towns still to fall to the attacker, nearest first, one every `every` months,
    /// so long as `defender` still holds them.
    queue: Vec<Entity>,
    /// How many towns the attacker set out to take.
    planned: usize,
    defender: Option<Entity>,
    every: u32,
    next: u32,
}

impl War {
    pub fn active(&self, month: u32) -> bool {
        (self.start..=self.end).contains(&month)
    }

    /// How many of its towns the attacker has taken, and how many it set out to.
    pub fn progress(&self) -> (usize, usize) {
        (
            self.planned - self.queue.len().min(self.planned),
            self.planned,
        )
    }
}

impl History {
    pub fn has(&self, mark: Mark) -> bool {
        self.marks.contains(&mark)
    }
}

/// Starts history in January 1600, once the world has been built.
pub fn start(world: &mut World) {
    let setting = world.resource::<Rules>().0.setting;
    let mut economy = Economy::default();
    economy.start(world);
    let mut h = world.resource_mut::<History>();
    *h = History {
        on: true,
        cohesion: 1.0,
        settled: vec![false; timeline(setting).len()],
        economy,
        ..History::default()
    };
}

/// A failed harvest waiting for its year.
#[derive(Clone, Copy, Debug)]
struct Failure {
    year: u32,
    center: Position,
    radius: i32,
    /// The share of a usual harvest that still comes in.
    keep: f32,
}

/// An event of real history.
pub struct Event {
    /// When it happened.
    pub date: u32,
    /// It can still happen this many years late, if its conditions come right.
    late: u32,
    pub title: &'static str,
    pub real: &'static str,
    /// Whether the world allows it now, and if not, why not.
    needs: fn(&World) -> Result<(), &'static str>,
    /// Makes it happen and says what happened.
    apply: fn(&mut World) -> (Outcome, String),
}

/// A change to history the player can make.
pub struct WhatIf {
    pub title: &'static str,
    pub text: &'static str,
    /// Available from this month, up to and including `until`.
    from: u32,
    until: u32,
    /// Changes that can only be made once leave this mark.
    once: Option<Mark>,
    /// Acts on the tile the player picked, or the nearest town to it.
    pub needs_place: bool,
    available: fn(&World) -> bool,
    apply: fn(&mut World, Option<Position>) -> String,
}

pub fn timeline(setting: Setting) -> &'static [Event] {
    match setting {
        Setting::India => &INDIA,
        Setting::NewWorld => &[],
    }
}

pub fn what_ifs(setting: Setting) -> &'static [WhatIf] {
    match setting {
        Setting::India => &INDIA_WHAT_IFS,
        Setting::NewWorld => &NEW_WORLD_WHAT_IFS,
    }
}

/// Runs every month, after everything else: plays the timeline, fails the monsoons
/// that history or the player have set, and adds up the economy each December.
pub fn play(world: &World, commands: &mut Commands) {
    if !world.resource::<History>().on {
        return;
    }
    let now = world.resource::<Clock>().month;
    let rules = world.resource::<Rules>().0;
    let events = timeline(rules.setting);

    // Failed monsoons are set the month before the harvest, so the year's random
    // monsoon can't undo them. Outside India nothing else clears a failed harvest.
    if now % 12 + 1 == rules.harvest_month {
        let year = era::year(now);
        let due: Vec<Failure> = {
            let mut h = world.resource_mut::<History>();
            let due = h
                .droughts
                .iter()
                .filter(|d| d.year == year)
                .copied()
                .collect();
            h.droughts.retain(|d| d.year > year);
            due
        };
        for f in due {
            commands.add(move |w| set_drought(w, f));
        }
    }
    if rules.setting != Setting::India && now % 12 == (rules.harvest_month + 1) % 12 {
        world.resource_mut::<Drought>().area = None;
    }

    let mut ready = Vec::new();
    let mut missed = Vec::new();
    {
        let h = world.resource::<History>();
        for (i, event) in events.iter().enumerate() {
            if h.settled[i] || now < event.date {
                continue;
            }
            match (event.needs)(world) {
                Ok(()) => ready.push(i),
                Err(why) if now >= event.date + 12 * event.late => missed.push((i, why)),
                Err(_) => {}
            }
        }
    }
    for &(i, why) in &missed {
        let event = &events[i];
        world.resource_mut::<History>().settled[i] = true;
        note(
            world,
            event.title,
            why.to_string(),
            event.real,
            Outcome::DidNotHappen,
        );
        // In the chronicle, an event that never happened goes at its real date.
        let mut h = world.resource_mut::<History>();
        if let Some(mut entry) = h.chronicle.pop() {
            entry.month = event.date;
            let at = h.chronicle.partition_point(|e| e.month <= entry.month);
            h.chronicle.insert(at, entry);
        }
    }
    for i in ready {
        world.resource_mut::<History>().settled[i] = true;
        commands.add(move |w| {
            let event = &timeline(w.resource::<Rules>().0.setting)[i];
            let (outcome, text) = (event.apply)(w);
            note(w, event.title, text, event.real, outcome);
        });
    }

    // Campaigns: the next town falls.
    let falls: Vec<(usize, Entity)> = {
        let h = world.resource::<History>();
        h.wars
            .iter()
            .enumerate()
            .filter(|(_, w)| !w.queue.is_empty() && now >= w.next)
            .map(|(i, w)| (i, w.queue[0]))
            .collect()
    };
    for (i, town) in falls {
        commands.add(move |w| fall(w, i, town));
    }

    if now % 12 == 11 && rules.setting == Setting::India {
        commands.add(move |w| {
            let mut economy = std::mem::take(&mut w.resource_mut::<History>().economy);
            economy.close_year(w, era::year(now));
            w.resource_mut::<History>().economy = economy;
        });
    }
}

/// The next town of a campaign falls to the attacker, if its defender still holds it.
fn fall(world: &mut World, war: usize, town: Entity) {
    let now = now(world);
    let (attacker, defender, name) = {
        let mut h = world.resource_mut::<History>();
        let w = &mut h.wars[war];
        w.queue.retain(|&t| t != town);
        w.next = now + w.every;
        if w.queue.is_empty() {
            w.end = w.end.max(now + 2);
        }
        (w.attacker, w.defender, w.name)
    };
    let Some(attacker) = attacker else { return };
    let owner = world.read::<Owner>().get(town).map(|o| o.0);
    if owner.is_none() || (defender.is_some() && owner != defender) {
        return;
    }
    let Some(held) = town_held(world, town) else {
        return;
    };
    let town_name = held.name.clone();
    transfer(world, &[held], attacker);
    let who = world
        .read::<Civ>()
        .get(attacker)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    world
        .resource_mut::<EventLog>()
        .push(now, format!("{name}: {town_name} falls to the {who}"));
}

fn town_held(world: &World, town: Entity) -> Option<Held> {
    let c = world.read::<City>();
    let city = c.get(town)?;
    Some(Held {
        town,
        pos: *world.read::<Position>().get(town)?,
        name: city.name.clone(),
        population: city.population,
    })
}

/// How a war is fought: a battle (no towns change hands), a siege of some towns, or
/// a campaign that takes towns one by one.
struct Fight {
    name: &'static str,
    /// Where it is, in longitude and latitude.
    site: (f32, f32),
    attacker: Option<Entity>,
    /// The color of attackers with no civ.
    color: [u8; 3],
    /// Where an attack from off the map comes from, in longitude and latitude.
    from: Option<(f32, f32)>,
    months: u32,
    /// Towns fought over but not taken.
    contested: Vec<Held>,
    /// Towns to take, one every `every` months, from `defender`.
    take: Vec<Held>,
    defender: Option<Entity>,
    every: u32,
}

impl Fight {
    fn battle(name: &'static str, site: (f32, f32), attacker: Option<Entity>) -> Self {
        Self {
            name,
            site,
            attacker,
            color: [220, 60, 50],
            from: None,
            months: 3,
            contested: Vec::new(),
            take: Vec::new(),
            defender: None,
            every: 1,
        }
    }
}

/// Puts a war on the map. Towns to take are ordered nearest the attacker first, so
/// the front moves across the country.
fn wage(world: &World, f: Fight) {
    let now = now(world);
    let color = f
        .attacker
        .and_then(|a| world.read::<Civ>().get(a).map(|c| c.color))
        .unwrap_or(f.color);
    let base = f
        .attacker
        .and_then(|a| crate::render::civ_centers_of(world).get(&a).copied());
    let mut take = f.take;
    if let Some((bx, by)) = base {
        take.sort_by(|a, b| {
            let d = |t: &Held| (t.pos.x as f32 - bx).powi(2) + (t.pos.y as f32 - by).powi(2);
            d(a).total_cmp(&d(b))
        });
    }
    let mut towns: Vec<Entity> = f.contested.iter().map(|t| t.town).collect();
    towns.extend(take.iter().map(|t| t.town));
    let queue: Vec<Entity> = take.iter().map(|t| t.town).collect();
    let months = f.months.max(queue.len() as u32 * f.every + 2);
    let war = War {
        name: f.name,
        site: clamped(world, f.site.0, f.site.1),
        attacker: f.attacker,
        color,
        from: f.from.map(|(lon, lat)| clamped(world, lon, lat)),
        start: now,
        end: now + months,
        towns,
        planned: queue.len(),
        queue,
        defender: f.defender,
        every: f.every,
        next: now + 1,
    };
    world.resource_mut::<History>().wars.push(war);
}

/// [`wage`], at a town's tile rather than a longitude and latitude.
fn wage_at(world: &World, f: Fight, (x, y): (i32, i32)) {
    wage(world, f);
    if let Some(war) = world.resource_mut::<History>().wars.last_mut() {
        war.site = Position { x, y };
    }
}

/// A tile for a longitude and latitude, kept on the map.
fn clamped(world: &World, lon: f32, lat: f32) -> Position {
    let map = world.resource::<Map>();
    let (x, y) = era::project(lon, lat, map.width, map.height);
    Position {
        x: x.clamp(0, map.width as i32 - 1),
        y: y.clamp(0, map.height as i32 - 1),
    }
}

fn note(world: &World, title: &'static str, text: String, real: &'static str, outcome: Outcome) {
    let month = world.resource::<Clock>().month;
    let line = match outcome {
        Outcome::DidNotHappen => format!("{title} did not happen: {text}"),
        Outcome::Yours => format!("You changed history: {title}"),
        _ => format!("{title}: {text}"),
    };
    world.resource_mut::<EventLog>().push(month, line);
    world.resource_mut::<History>().chronicle.push(Entry {
        month,
        title,
        text,
        real,
        outcome,
    });
}

/// The what-ifs the player can choose now.
pub fn available(world: &World) -> Vec<usize> {
    let now = world.resource::<Clock>().month;
    let h = world.resource::<History>();
    what_ifs(world.resource::<Rules>().0.setting)
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            (w.from..=w.until).contains(&now)
                && !w.once.is_some_and(|m| h.has(m))
                && (w.available)(world)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Makes the player's change, if it's still available, and says what it did.
pub fn change(world: &mut World, i: usize, at: Option<Position>) -> String {
    if !available(world).contains(&i) {
        return "That can't be changed any more.".to_string();
    }
    let w = &what_ifs(world.resource::<Rules>().0.setting)[i];
    if let Some(m) = w.once {
        world.resource_mut::<History>().marks.insert(m);
    }
    let text = (w.apply)(world, at);
    note(world, w.title, text.clone(), "", Outcome::Yours);
    text
}

/// The next few events of the timeline still to come.
pub fn coming(world: &World, count: usize) -> Vec<&'static Event> {
    let h = world.resource::<History>();
    let mut list: Vec<&'static Event> = timeline(world.resource::<Rules>().0.setting)
        .iter()
        .enumerate()
        .filter(|(i, _)| !h.settled.get(*i).copied().unwrap_or(true))
        .map(|(_, e)| e)
        .collect();
    list.sort_by_key(|e| e.date);
    list.truncate(count);
    list
}

// ---------------------------------------------------------------------------
// Looking at the world
// ---------------------------------------------------------------------------

const MUGHALS: &str = "Mughal Empire";
const DECCAN: &str = "Deccan Sultanates";
const SOUTH: &str = "Vijayanagara";
const PORTUGUESE: &str = "Estado da Índia";
const ENGLISH: &str = "English East India Company";
const FRENCH: &str = "French East India Company";
const MARATHAS: &str = "Maratha Empire";
const BENGAL_NAWABS: &str = "Nawabs of Bengal";
const HYDERABAD: &str = "Hyderabad";

/// A box of longitude and latitude on the India map.
#[derive(Clone, Copy)]
struct Region {
    west: f32,
    east: f32,
    south: f32,
    north: f32,
}

const fn region(west: f32, east: f32, south: f32, north: f32) -> Region {
    Region {
        west,
        east,
        south,
        north,
    }
}

/// Bengal, Bihar and Orissa.
const BENGAL: Region = region(84.0, 92.6, 20.0, 27.0);
/// The western Deccan and the Konkan, where the Marathas rose.
const WESTERN_DECCAN: Region = region(72.8, 75.6, 15.8, 19.8);
/// The eastern Deccan and Telangana, the Nizam's lands.
const EASTERN_DECCAN: Region = region(75.6, 82.8, 15.0, 21.5);
/// Malwa and Khandesh.
const MALWA: Region = region(74.5, 79.0, 21.0, 25.0);
/// The Coromandel coast, where the English and French fought for the Carnatic.
const COROMANDEL: Region = region(78.5, 81.5, 10.0, 15.0);
const MYSORE: Region = region(75.0, 78.6, 11.0, 14.2);
const DELHI_AGRA: Region = region(75.8, 79.5, 26.5, 29.6);
/// The plains north of the Narmada, where the rebellion of 1857 was fought.
const NORTH: Region = region(74.0, 88.0, 23.0, 31.0);

impl Region {
    fn contains(&self, world: &World, p: Position) -> bool {
        let map = world.resource::<Map>();
        let (x0, y0) = era::project(self.west, self.north, map.width, map.height);
        let (x1, y1) = era::project(self.east, self.south, map.width, map.height);
        (x0..=x1).contains(&p.x) && (y0..=y1).contains(&p.y)
    }

    fn center(&self) -> (f32, f32) {
        (
            (self.west + self.east) / 2.0,
            (self.south + self.north) / 2.0,
        )
    }
}

fn civ(world: &World, name: &str) -> Option<Entity> {
    world
        .read::<Civ>()
        .iter()
        .find(|(_, c)| c.name == name)
        .map(|(e, _)| e)
}

struct Held {
    town: Entity,
    pos: Position,
    name: String,
    population: u32,
}

/// The towns of the civ called `name`, within `area` if given, biggest first.
fn held(world: &World, name: &str, area: Option<Region>) -> Vec<Held> {
    let Some(owner) = civ(world, name) else {
        return Vec::new();
    };
    let owners = world.read::<Owner>();
    let positions = world.read::<Position>();
    let mut list: Vec<Held> = world
        .read::<City>()
        .iter()
        .filter(|(e, _)| owners.get(*e).is_some_and(|o| o.0 == owner))
        .filter_map(|(e, c)| {
            Some(Held {
                town: e,
                pos: *positions.get(e)?,
                name: c.name.clone(),
                population: c.population,
            })
        })
        .filter(|t| area.is_none_or(|a| a.contains(world, t.pos)))
        .collect();
    list.sort_by_key(|t| std::cmp::Reverse(t.population));
    list
}

fn holds(world: &World, name: &str, area: Option<Region>) -> bool {
    !held(world, name, area).is_empty()
}

fn mark(world: &World, m: Mark) -> bool {
    world.resource::<History>().has(m)
}

fn set_mark(world: &World, m: Mark) {
    world.resource_mut::<History>().marks.insert(m);
}

fn cohesion(world: &World) -> f32 {
    world.resource::<History>().cohesion
}

fn now(world: &World) -> u32 {
    world.resource::<Clock>().month
}

/// "Poona, Satara and 3 more".
fn names(towns: &[Held]) -> String {
    let shown: Vec<&str> = towns.iter().take(3).map(|t| t.name.as_str()).collect();
    match (shown.len(), towns.len()) {
        (0, _) => "no towns".to_string(),
        (1, _) => shown[0].to_string(),
        (n, total) if total > n => format!("{} and {} more", shown.join(", "), total - n),
        _ => {
            let (last, rest) = shown.split_last().unwrap();
            format!("{} and {last}", rest.join(", "))
        }
    }
}

// ---------------------------------------------------------------------------
// Changing the world
// ---------------------------------------------------------------------------

/// A state that rises during the period.
struct Rising {
    name: &'static str,
    people: &'static str,
    color: [u8; 3],
    shrine: &'static str,
}

const MARATHA_STATE: Rising = Rising {
    name: MARATHAS,
    people: "Maratha",
    color: [196, 92, 36],
    shrine: "Temple",
};
const BENGAL_STATE: Rising = Rising {
    name: BENGAL_NAWABS,
    people: "Bengali",
    color: [150, 204, 92],
    shrine: "Mosque",
};
const HYDERABAD_STATE: Rising = Rising {
    name: HYDERABAD,
    people: "Hyderabadi",
    color: [214, 190, 120],
    shrine: "Mosque",
};
const COMPANY_RULE: Rising = Rising {
    name: COMPANY_STATE,
    people: "Company",
    color: [236, 96, 96],
    shrine: "Temple",
};

fn rise(world: &mut World, r: &Rising) -> Entity {
    if let Some(e) = civ(world, r.name) {
        return e;
    }
    world.spawn((Civ::successor(r.name, r.people, r.color, r.shrine),))
}

/// Hands towns to another civ, with their land and people.
fn transfer(world: &mut World, towns: &[Held], to: Entity) {
    {
        let mut owners = world.write::<Owner>();
        for t in towns {
            if let Some(o) = owners.get_mut(t.town) {
                o.0 = to;
            }
        }
    }
    if let Some(c) = world.write::<Civ>().get_mut(to) {
        c.home.extend(towns.iter().map(|t| t.pos));
    }
}

fn scale_people(world: &World, towns: &[Held], share: f32) {
    let mut cities = world.write::<City>();
    for t in towns {
        if let Some(c) = cities.get_mut(t.town) {
            c.population = (c.population as f32 * share) as u32;
        }
    }
}

fn scale_food(world: &World, towns: &[Held], share: f32) {
    let mut cities = world.write::<City>();
    for t in towns {
        if let Some(c) = cities.get_mut(t.town) {
            c.food *= share;
        }
    }
}

fn tile(world: &World, lon: f32, lat: f32) -> Position {
    let map = world.resource::<Map>();
    let (x, y) = era::project(lon, lat, map.width, map.height);
    Position { x, y }
}

fn nearest_town(world: &World, at: Position) -> Option<(Entity, Position, String)> {
    let positions = world.read::<Position>();
    world
        .read::<City>()
        .iter()
        .filter_map(|(e, c)| positions.get(e).map(|p| (e, *p, c.name.clone())))
        .min_by_key(|(_, p, _)| p.dist2(at))
}

/// Fails the monsoon (or the harvest) within `radius` of `center` at the coming
/// harvest, leaving `keep` of a usual crop. Returns the year it fails.
fn fail_harvest(world: &World, center: Position, radius: i32, keep: f32) -> u32 {
    let now = now(world);
    let harvest = world.resource::<Rules>().0.harvest_month;
    let year = era::year(now) + if now % 12 + 1 > harvest { 1 } else { 0 };
    let f = Failure {
        year,
        center,
        radius,
        keep,
    };
    if now % 12 + 1 == harvest {
        set_drought(world, f);
    } else {
        world.resource_mut::<History>().droughts.push(f);
    }
    year
}

fn set_drought(world: &World, f: Failure) {
    let center = f.center;
    {
        let mut d = world.resource_mut::<Drought>();
        d.area = Some((center, f.radius * f.radius));
        d.keep = f.keep;
    }
    let india = world.resource::<Rules>().0.setting == Setting::India;
    if let Some((_, _, name)) = nearest_town(world, center) {
        let text = if india {
            format!("The monsoon has failed around {name}: the crops are withering")
        } else {
            format!("The harvest has failed around {name}")
        };
        world.resource_mut::<EventLog>().push(now(world), text);
    }
}

fn outbreak(world: &World, at: Position, disease: &'static str) -> Option<String> {
    let (town, _, name) = nearest_town(world, at)?;
    let month = now(world);
    let mut cities = world.write::<City>();
    let city = cities.get_mut(town)?;
    city.outbreak = Some(Outbreak {
        disease,
        months_left: 6,
        rate: 0.03,
        deaths: 0.0,
    });
    city.immune_until = month + 12 * 15;
    Some(name)
}

/// Gives the English company a town at a real place, unless it already has one of
/// these names.
fn english_town(
    w: &mut World,
    names: &[&'static str],
    lon: f32,
    lat: f32,
    done: &str,
) -> (Outcome, String) {
    if let Some(t) = held(w, ENGLISH, None)
        .iter()
        .find(|t| names.contains(&t.name.as_str()))
    {
        return (
            Outcome::AsInHistory,
            format!("The English already held {}.", t.name),
        );
    }
    let english = civ(w, ENGLISH).unwrap();
    let town = Town {
        name: names[0],
        lon,
        lat,
        population: 100,
    };
    sim::place_towns(w, english, std::slice::from_ref(&town));
    (Outcome::AsInHistory, done.to_string())
}

// ---------------------------------------------------------------------------
// India, 1600–1858
// ---------------------------------------------------------------------------

/// What the great famines left of the harvest. Set so that, over a handful of seeds,
/// the simulation's excess deaths (against the same world with a good harvest) come
/// out near the usual estimates: some 7.4 million in Gujarat and the Deccan in
/// 1630–32, and seven to ten million in Bengal in 1770. A random failed monsoon is
/// harsher on the towns it hits, but hits far fewer of them.
const DECCAN_KEEP: f32 = 0.5;
const BENGAL_KEEP: f32 = 0.5;

fn always(_: &World) -> Result<(), &'static str> {
    Ok(())
}

fn english_trade(world: &World) -> Result<(), &'static str> {
    if mark(world, Mark::CompanyBankrupt) {
        return Err("the East India Company had gone bankrupt");
    }
    if !holds(world, ENGLISH, None) {
        return Err("the English had no factory in India yet");
    }
    Ok(())
}

static INDIA: [Event; 30] = [
    Event {
        date: month(1600, 12),
        late: 0,
        title: "The East India Company is chartered",
        real: "Elizabeth I gives a company of London merchants the monopoly of English trade east of the Cape of Good Hope.",
        needs: always,
        apply: |_| (Outcome::AsInHistory, "London merchants get their charter for the East Indies trade.".into()),
    },
    Event {
        date: month(1602, 3),
        late: 0,
        title: "The Dutch East India Company is founded",
        real: "Rival Dutch merchants merge into the United East India Company (VOC), the first company to sell shares to the public.",
        needs: always,
        apply: |_| (Outcome::AsInHistory, "The Dutch merchants combine into one company.".into()),
    },
    Event {
        date: month(1612, 11),
        late: 3,
        title: "The Battle of Swally",
        real: "Four English ships beat a Portuguese squadron off Surat, and the Mughal court starts to take the English seriously.",
        needs: english_trade,
        apply: |w| {
            if let Some(p) = civ(w, PORTUGUESE) {
                if let Some(c) = w.write::<Civ>().get_mut(p) {
                    c.emigrants_per_year *= 0.7;
                }
            }
            wage(w, Fight::battle("Battle of Swally", (72.65, 21.08), civ(w, ENGLISH)));
            (Outcome::AsInHistory, "English ships beat the Portuguese off Surat; Portugal's hold on the sea lanes slips.".into())
        },
    },
    Event {
        date: month(1615, 9),
        late: 5,
        title: "Sir Thomas Roe at the Mughal court",
        real: "James I's ambassador spends three years at Jahangir's court and wins the English the right to trade and keep factories.",
        needs: |w| {
            english_trade(w)?;
            if !holds(w, MUGHALS, None) {
                return Err("there was no Mughal court left to win over");
            }
            Ok(())
        },
        apply: |w| {
            if let Some(e) = civ(w, ENGLISH) {
                if let Some(c) = w.write::<Civ>().get_mut(e) {
                    c.emigrants_per_year *= 1.25;
                }
            }
            (Outcome::AsInHistory, "The Emperor lets the English trade and build factories in his empire.".into())
        },
    },
    Event {
        date: month(1630, 9),
        late: 0,
        title: "The Deccan famine",
        real: "The rains fail over Gujarat and the Deccan, and millions die in the famine of 1630–32.",
        needs: always,
        apply: |w| {
            let center = tile(w, 75.0, 20.5);
            fail_harvest(w, center, 26, DECCAN_KEEP);
            w.resource_mut::<History>().droughts.push(Failure {
                year: 1631,
                center,
                radius: 22,
                keep: DECCAN_KEEP,
            });
            (Outcome::AsInHistory, "The monsoon fails over Gujarat and the Deccan, two years running.".into())
        },
    },
    Event {
        date: month(1639, 8),
        late: 5,
        title: "Fort St. George is founded",
        real: "Francis Day gets a strip of coast at Madraspatnam from the local Nayak, and the Company builds Fort St. George: the start of Madras.",
        needs: english_trade,
        apply: |w| english_town(w, &["Fort St. George"], 80.28, 13.08, "The English build Fort St. George on the Coromandel coast."),
    },
    Event {
        date: month(1658, 7),
        late: 0,
        title: "Aurangzeb takes the throne",
        real: "Aurangzeb wins the war of succession, locks up his father Shah Jahan, and starts fifty years of conquest in the south.",
        needs: |w| {
            if holds(w, MUGHALS, None) {
                Ok(())
            } else {
                Err("the Mughal Empire was gone")
            }
        },
        apply: |_| (Outcome::AsInHistory, "Aurangzeb wins the war between Shah Jahan's sons.".into()),
    },
    Event {
        date: month(1661, 6),
        late: 2,
        title: "Bombay comes to the English",
        real: "Portugal gives Bombay to Charles II as part of Catherine of Braganza's dowry; in 1668 he leases it to the Company.",
        needs: |w| {
            english_trade(w)?;
            if !holds(w, PORTUGUESE, None) {
                return Err("Portugal had no Indian lands left to give away");
            }
            Ok(())
        },
        apply: |w| english_town(w, &["Bombay"], 72.88, 18.93, "Bombay's islands pass to the English, who fortify them."),
    },
    Event {
        date: month(1674, 6),
        late: 0,
        title: "Shivaji is crowned",
        real: "Shivaji, who has carved a kingdom out of Bijapur's and the Mughals' lands, is crowned Chhatrapati at Raigad.",
        needs: always,
        apply: |w| {
            let mut towns = held(w, DECCAN, Some(WESTERN_DECCAN));
            towns.extend(held(w, MUGHALS, Some(WESTERN_DECCAN)));
            let marathas = rise(w, &MARATHA_STATE);
            transfer(w, &towns, marathas);
            let raigad = Town { name: "Raigad", lon: 73.44, lat: 18.23, population: 300 };
            sim::place_towns(w, marathas, std::slice::from_ref(&raigad));
            let text = if towns.is_empty() {
                "Shivaji is crowned king at his hill fort of Raigad.".to_string()
            } else {
                format!("Shivaji is crowned at Raigad; the Marathas hold {}.", names(&towns))
            };
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1687, 9),
        late: 12,
        title: "Aurangzeb conquers the Deccan",
        real: "Bijapur falls in 1686 and Golconda in 1687; the Mughal Empire reaches its greatest extent.",
        needs: |w| {
            if !holds(w, MUGHALS, None) {
                return Err("the Mughal Empire was gone");
            }
            if !holds(w, DECCAN, None) {
                return Err("the Deccan sultanates had already fallen");
            }
            if cohesion(w) < 0.6 {
                return Err("the empire was too divided to march south");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, DECCAN, None);
            let text = format!("Aurangzeb's armies march south against {}.", names(&towns));
            wage(w, Fight {
                take: towns,
                defender: civ(w, DECCAN),
                months: 12,
                ..Fight::battle("Aurangzeb's Deccan wars", (78.4, 17.38), civ(w, MUGHALS))
            });
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1690, 8),
        late: 5,
        title: "Job Charnock founds Calcutta",
        real: "Job Charnock sets up the Company's factory at Sutanuti on the Hooghly; it grows into Calcutta, guarded by Fort William.",
        needs: english_trade,
        apply: |w| english_town(w, &["Calcutta", "Fort William"], 88.35, 22.57, "The English settle on the Hooghly at Calcutta."),
    },
    Event {
        date: month(1707, 3),
        late: 0,
        title: "Aurangzeb dies",
        real: "Aurangzeb dies at 88; his sons fight over the throne, and the provinces start to go their own way.",
        needs: |w| {
            if holds(w, MUGHALS, None) {
                Ok(())
            } else {
                Err("the Mughal Empire was gone")
            }
        },
        apply: |w| {
            if mark(w, Mark::StrongHeir) {
                w.resource_mut::<History>().cohesion -= 0.1;
                (Outcome::Different, "Aurangzeb dies, and his chosen heir takes the throne unopposed. The provinces stay loyal.".into())
            } else {
                w.resource_mut::<History>().cohesion -= 0.45;
                (Outcome::AsInHistory, "Aurangzeb dies; his sons go to war over the throne.".into())
            }
        },
    },
    Event {
        date: month(1717, 1),
        late: 30,
        title: "Bengal goes its own way",
        real: "Murshid Quli Khan, governor of Bengal, becomes its hereditary Nawab, sending Delhi only tribute.",
        needs: |w| {
            if !holds(w, MUGHALS, Some(BENGAL)) {
                return Err("the Mughals held nothing in Bengal");
            }
            if cohesion(w) >= 0.6 {
                return Err("the emperor kept a firm hold on his governors");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, MUGHALS, Some(BENGAL));
            let nawabs = rise(w, &BENGAL_STATE);
            transfer(w, &towns, nawabs);
            (Outcome::AsInHistory, format!("The Nawabs of Bengal rule {} in all but name.", names(&towns)))
        },
    },
    Event {
        date: month(1724, 10),
        late: 30,
        title: "The Nizam founds Hyderabad",
        real: "Asaf Jah, the Mughal viceroy of the Deccan, beats a rival at Shakar Kheda and rules the Deccan as Nizam.",
        needs: |w| {
            if !holds(w, MUGHALS, Some(EASTERN_DECCAN)) {
                return Err("the Mughals held nothing in the eastern Deccan");
            }
            if cohesion(w) >= 0.6 {
                return Err("the emperor kept a firm hold on his governors");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, MUGHALS, Some(EASTERN_DECCAN));
            let nizam = rise(w, &HYDERABAD_STATE);
            transfer(w, &towns, nizam);
            (Outcome::AsInHistory, format!("The Nizam rules {} from Hyderabad.", names(&towns)))
        },
    },
    Event {
        date: month(1738, 1),
        late: 20,
        title: "Malwa goes to the Marathas",
        real: "After his defeat at Bhopal, the Nizam cedes Malwa to the Maratha Peshwa Baji Rao.",
        needs: |w| {
            if !holds(w, MARATHAS, None) {
                return Err("there was no Maratha state to take it");
            }
            if !holds(w, MUGHALS, Some(MALWA)) {
                return Err("the Mughals held nothing in Malwa");
            }
            if cohesion(w) >= 0.6 {
                return Err("the Mughal armies were still too strong");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, MUGHALS, Some(MALWA));
            let text = format!("Maratha armies ride north into Malwa, against {}.", names(&towns));
            wage(w, Fight {
                take: towns,
                defender: civ(w, MUGHALS),
                ..Fight::battle("Maratha conquest of Malwa", (77.4, 23.26), civ(w, MARATHAS))
            });
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1739, 3),
        late: 0,
        title: "Nader Shah sacks Delhi",
        real: "Nader Shah of Persia routs the Mughal army at Karnal, sacks Delhi and carries off the Peacock Throne.",
        needs: |w| {
            if mark(w, Mark::NaderTurnsBack) {
                return Err("Nader Shah turned back at the passes");
            }
            if !held(w, MUGHALS, None).iter().any(|t| t.name == "Delhi") {
                return Err("Delhi was no longer the Mughals' to lose");
            }
            Ok(())
        },
        apply: |w| {
            let delhi: Vec<Held> = held(w, MUGHALS, None).into_iter().filter(|t| t.name == "Delhi").collect();
            scale_people(w, &delhi, 0.97);
            scale_food(w, &delhi, 0.2);
            w.resource_mut::<History>().cohesion -= 0.2;
            wage(w, Fight {
                color: [80, 200, 210],
                from: Some((66.0, 34.0)),
                months: 4,
                contested: delhi,
                ..Fight::battle("Nader Shah's invasion", (76.98, 29.69), None)
            });
            (Outcome::AsInHistory, "Persian troops sack Delhi and carry off the imperial treasury.".into())
        },
    },
    Event {
        date: month(1746, 9),
        late: 0,
        title: "The French take Madras",
        real: "In the War of the Austrian Succession, La Bourdonnais's squadron captures Fort St. George.",
        needs: |w| {
            english_trade(w)?;
            if !holds(w, FRENCH, None) {
                return Err("there was no French company in India");
            }
            if !holds(w, ENGLISH, Some(COROMANDEL)) {
                return Err("the English had nothing on the Coromandel coast to take");
            }
            Ok(())
        },
        apply: |w| {
            let mut towns = held(w, ENGLISH, Some(COROMANDEL));
            let i = towns.iter().position(|t| t.name == "Fort St. George").unwrap_or(0);
            let town = towns.swap_remove(i);
            let french = civ(w, FRENCH).unwrap();
            let name = town.name.clone();
            w.resource_mut::<History>().madras = Some(town.town);
            let site = (town.pos.x, town.pos.y);
            wage_at(w, Fight {
                contested: vec![town_held(w, town.town).unwrap()],
                ..Fight::battle("Siege of Madras", (0.0, 0.0), Some(french))
            }, site);
            transfer(w, &[town], french);
            set_mark(w, Mark::FrenchHoldMadras);
            (Outcome::AsInHistory, format!("A French squadron takes {name} from the English."))
        },
    },
    Event {
        date: month(1749, 8),
        late: 0,
        title: "Madras is handed back",
        real: "The Treaty of Aix-la-Chapelle gives Madras back to the English, in exchange for Louisbourg in North America.",
        needs: |w| {
            if !mark(w, Mark::FrenchHoldMadras) {
                return Err("the French had never taken it");
            }
            if mark(w, Mark::FranceBacksDupleix) {
                return Err("with Paris behind him, Dupleix kept it");
            }
            Ok(())
        },
        apply: |w| {
            let town = w.resource::<History>().madras;
            let (Some(town), Some(english)) = (town, civ(w, ENGLISH)) else {
                return (Outcome::Different, "There was nothing left to hand back.".into());
            };
            let back: Vec<Held> = held(w, FRENCH, None).into_iter().filter(|t| t.town == town).collect();
            let name = back.first().map(|t| t.name.clone()).unwrap_or_default();
            transfer(w, &back, english);
            (Outcome::AsInHistory, format!("The French hand {name} back to the English."))
        },
    },
    Event {
        date: month(1751, 9),
        late: 2,
        title: "Clive holds Arcot",
        real: "A young Company clerk, Robert Clive, seizes Arcot and holds it through a seven-week siege, turning the war against the French.",
        needs: |w| {
            english_trade(w)?;
            if mark(w, Mark::FranceBacksDupleix) {
                return Err("Dupleix, well supplied from France, took Arcot first, and Clive made no name for himself");
            }
            if !holds(w, ENGLISH, Some(COROMANDEL)) {
                return Err("the English had no foothold on the Coromandel coast");
            }
            Ok(())
        },
        apply: |w| {
            set_mark(w, Mark::ArcotHeld);
            wage(w, Fight { months: 4, ..Fight::battle("Siege of Arcot", (79.37, 12.9), civ(w, ENGLISH)) });
            (Outcome::AsInHistory, "Clive holds Arcot, and the English gain the upper hand in the Carnatic.".into())
        },
    },
    Event {
        date: month(1756, 6),
        late: 0,
        title: "Siraj ud-Daulah takes Calcutta",
        real: "The new Nawab of Bengal storms Fort William, angered by the Company's new fortifications.",
        needs: |w| {
            english_trade(w)?;
            if !holds(w, BENGAL_NAWABS, None) {
                return Err("there was no Nawab of Bengal");
            }
            if !holds(w, ENGLISH, Some(BENGAL)) {
                return Err("the English had no factory in Bengal");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, ENGLISH, Some(BENGAL));
            let town = towns
                .iter()
                .find(|t| t.name == "Calcutta" || t.name == "Fort William")
                .unwrap_or(&towns[0]);
            let text = format!("The Nawab's army storms the English factory at {}.", town.name);
            let site = (town.pos.x, town.pos.y);
            let contested = vec![town_held(w, town.town).unwrap()];
            wage_at(w, Fight {
                contested,
                ..Fight::battle("Fall of Calcutta", (0.0, 0.0), civ(w, BENGAL_NAWABS))
            }, site);
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1757, 6),
        late: 0,
        title: "The Battle of Plassey",
        real: "Clive beats Siraj ud-Daulah when Mir Jafar's troops stand aside, and the Company makes Mir Jafar Nawab.",
        needs: |w| {
            english_trade(w)?;
            if !holds(w, BENGAL_NAWABS, None) {
                return Err("there was no Nawab of Bengal to fight: the province was still the emperor's");
            }
            if !holds(w, ENGLISH, Some(BENGAL)) {
                return Err("the Company had no factory in Bengal to fight from");
            }
            if !mark(w, Mark::ArcotHeld) {
                return Err("without Arcot, Clive never rose to command, and no army sailed for Bengal");
            }
            Ok(())
        },
        apply: |w| {
            let from = held(w, ENGLISH, Some(BENGAL)).first().map(|t| t.pos);
            wage(w, Fight { months: 2, ..Fight::battle("Battle of Plassey", (88.25, 23.8), civ(w, ENGLISH)) });
            if let (Some(from), Some(war)) = (from, w.resource_mut::<History>().wars.last_mut()) {
                war.from = Some(from);
            }
            if mark(w, Mark::MirJafarLoyal) {
                let towns = held(w, ENGLISH, Some(BENGAL));
                let nawabs = civ(w, BENGAL_NAWABS).unwrap();
                transfer(w, &towns, nawabs);
                return (
                    Outcome::Different,
                    format!("Mir Jafar's cavalry charges, Clive's army breaks, and the Nawab takes {}.", names(&towns)),
                );
            }
            set_mark(w, Mark::PlasseyWon);
            (Outcome::AsInHistory, "Mir Jafar stands aside, Clive wins, and the Company makes him Nawab.".into())
        },
    },
    Event {
        date: month(1761, 1),
        late: 0,
        title: "The Third Battle of Panipat",
        real: "Ahmad Shah Durrani's Afghans crush the Maratha army north of Delhi, in one of the bloodiest battles of the century.",
        needs: |w| {
            if held(w, MARATHAS, None).len() < 4 {
                return Err("the Marathas never grew strong enough to march north");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, MARATHAS, None);
            scale_people(w, &towns, 0.97);
            wage(w, Fight {
                color: [170, 130, 80],
                from: Some((68.5, 33.0)),
                ..Fight::battle("Third Battle of Panipat", (76.97, 29.39), None)
            });
            (Outcome::AsInHistory, "The Afghans crush the Maratha army, and a generation of its leaders dies.".into())
        },
    },
    Event {
        date: month(1764, 10),
        late: 0,
        title: "The Battle of Buxar",
        real: "Hector Munro's Company army beats the combined forces of the Nawabs of Bengal and Awadh and the Mughal emperor.",
        needs: |w| {
            if !mark(w, Mark::PlasseyWon) {
                return Err("the Company had never won at Plassey, and had no army in Bengal");
            }
            Ok(())
        },
        apply: |w| {
            wage(w, Fight { months: 2, ..Fight::battle("Battle of Buxar", (83.98, 25.57), civ(w, ENGLISH)) });
            (Outcome::AsInHistory, "The Company's army beats the Nawabs and the emperor at Buxar.".into())
        },
    },
    Event {
        date: month(1765, 8),
        late: 5,
        title: "The Company takes the Diwani of Bengal",
        real: "After winning at Buxar, the Company gets the right to collect Bengal's revenues from the emperor: a trading company becomes a ruler.",
        needs: |w| {
            if !mark(w, Mark::PlasseyWon) {
                return Err("the Company had never won at Plassey");
            }
            if !holds(w, BENGAL_NAWABS, None) {
                return Err("there was no Nawab of Bengal left");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, BENGAL_NAWABS, None);
            let company = rise(w, &COMPANY_RULE);
            transfer(w, &towns, company);
            set_mark(w, Mark::Diwani);
            (Outcome::AsInHistory, format!("The Company rules {} and collects their land revenue.", names(&towns)))
        },
    },
    Event {
        date: month(1770, 9),
        late: 0,
        title: "The great Bengal famine",
        real: "Drought, and revenue collection that went on regardless, kill perhaps a third of Bengal's people: around ten million.",
        needs: always,
        apply: |w| {
            let (lon, lat) = BENGAL.center();
            let center = tile(w, lon, lat);
            fail_harvest(w, center, 24, BENGAL_KEEP);
            let ruled = held(w, COMPANY_STATE, Some(BENGAL));
            if ruled.is_empty() {
                (Outcome::Different, "The rains fail in Bengal, but with no Company collecting the revenue, the granaries are fuller.".into())
            } else {
                scale_food(w, &ruled, 0.25);
                (Outcome::AsInHistory, "The rains fail in Bengal, and the Company keeps collecting its revenue from the empty granaries.".into())
            }
        },
    },
    Event {
        date: month(1799, 5),
        late: 10,
        title: "The fall of Seringapatam",
        real: "Tipu Sultan dies defending his capital, and the Company takes the lands of Mysore.",
        needs: |w| {
            if !holds(w, COMPANY_STATE, None) {
                return Err("the Company ruled no land in India");
            }
            if !holds(w, SOUTH, Some(MYSORE)) {
                return Err("there was no southern kingdom in Mysore to fight");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, SOUTH, Some(MYSORE));
            let text = format!("The Company's armies march on Seringapatam and {}.", names(&towns));
            wage(w, Fight {
                take: towns,
                defender: civ(w, SOUTH),
                ..Fight::battle("Fourth Anglo-Mysore War", (76.69, 12.42), civ(w, COMPANY_STATE))
            });
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1803, 9),
        late: 10,
        title: "The Company takes Delhi",
        real: "General Lake beats the Marathas outside Delhi; the Mughal emperor lives on under Company protection.",
        needs: |w| {
            if !holds(w, COMPANY_STATE, None) {
                return Err("the Company ruled no land in India");
            }
            if !holds(w, MUGHALS, Some(DELHI_AGRA)) && !holds(w, MARATHAS, Some(DELHI_AGRA)) {
                return Err("there was no one at Delhi for the Company to beat");
            }
            Ok(())
        },
        apply: |w| {
            let mut towns = held(w, MUGHALS, Some(DELHI_AGRA));
            towns.extend(held(w, MARATHAS, Some(DELHI_AGRA)));
            let text = format!("Lake's army marches on {}.", names(&towns));
            wage(w, Fight {
                take: towns,
                ..Fight::battle("Second Anglo-Maratha War", (77.3, 28.6), civ(w, COMPANY_STATE))
            });
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1817, 11),
        late: 10,
        title: "The end of the Maratha Empire",
        real: "The Peshwa surrenders, ending the Third Anglo-Maratha War; the Company is master of most of India.",
        needs: |w| {
            if !holds(w, COMPANY_STATE, None) {
                return Err("the Company ruled no land in India");
            }
            if !holds(w, MARATHAS, None) {
                return Err("there was no Maratha state left");
            }
            Ok(())
        },
        apply: |w| {
            let towns = held(w, MARATHAS, None);
            let text = format!("The Third Anglo-Maratha War: the Company takes {} one by one.", names(&towns));
            wage(w, Fight {
                take: towns,
                defender: civ(w, MARATHAS),
                ..Fight::battle("Third Anglo-Maratha War", (73.86, 18.52), civ(w, COMPANY_STATE))
            });
            (Outcome::AsInHistory, text)
        },
    },
    Event {
        date: month(1857, 5),
        late: 0,
        title: "The rebellion of 1857",
        real: "Sepoys of the Bengal Army mutiny at Meerut, and the revolt spreads across the north before it is put down.",
        needs: |w| {
            if holds(w, COMPANY_STATE, Some(NORTH)) {
                Ok(())
            } else {
                Err("the Company ruled nothing in the north to rise against")
            }
        },
        apply: |w| {
            let towns = held(w, COMPANY_STATE, Some(NORTH));
            scale_people(w, &towns, 0.98);
            scale_food(w, &towns, 0.6);
            wage(w, Fight {
                color: [235, 80, 60],
                months: 14,
                contested: towns.iter().map(|t| town_held(w, t.town).unwrap()).collect(),
                ..Fight::battle("Rebellion of 1857", (77.7, 28.98), None)
            });
            (Outcome::AsInHistory, format!("Revolt spreads through {} before it is put down.", names(&towns)))
        },
    },
    Event {
        date: month(1858, 11),
        late: 0,
        title: "The Crown takes over from the Company",
        real: "Parliament ends the Company's rule, and India is governed from London: the British Raj.",
        needs: |w| {
            if holds(w, COMPANY_STATE, None) {
                Ok(())
            } else {
                Err("the Company ruled no land in India")
            }
        },
        apply: |w| {
            if let Some(e) = civ(w, COMPANY_STATE) {
                if let Some(c) = w.write::<Civ>().get_mut(e) {
                    c.name = "British Raj".into();
                }
            }
            (Outcome::AsInHistory, "The Company's lands pass to the Crown.".into())
        },
    },
];

static INDIA_WHAT_IFS: [WhatIf; 7] = [
    WhatIf {
        title: "A strong heir for Aurangzeb",
        text: "Aurangzeb names one heir and his sons accept it, so the empire holds together after him.",
        from: month(1680, 1),
        until: month(1707, 2),
        once: Some(Mark::StrongHeir),
        needs_place: false,
        available: |w| holds(w, MUGHALS, None),
        apply: |w, _| {
            w.resource_mut::<History>().cohesion = 1.0;
            "Aurangzeb has named his heir.".into()
        },
    },
    WhatIf {
        title: "Nader Shah turns back",
        text: "The Persian army never comes through the passes to Delhi.",
        from: month(1730, 1),
        until: month(1739, 2),
        once: Some(Mark::NaderTurnsBack),
        needs_place: false,
        available: |_| true,
        apply: |_, _| "Nader Shah stays west of the Khyber.".into(),
    },
    WhatIf {
        title: "France backs Dupleix",
        text: "Paris sends Dupleix the money, ships and soldiers he asks for in the Carnatic.",
        from: month(1742, 1),
        until: month(1751, 8),
        once: Some(Mark::FranceBacksDupleix),
        needs_place: false,
        available: |w| holds(w, FRENCH, None),
        apply: |w, _| {
            if let Some(e) = civ(w, FRENCH) {
                if let Some(c) = w.write::<Civ>().get_mut(e) {
                    c.emigrants_per_year *= 3.0;
                }
            }
            "French ships and soldiers sail for Pondichéry.".into()
        },
    },
    WhatIf {
        title: "Mir Jafar stays loyal",
        text: "At Plassey, the Nawab's general charges with his cavalry instead of standing aside.",
        from: month(1750, 1),
        until: month(1757, 5),
        once: Some(Mark::MirJafarLoyal),
        needs_place: false,
        available: |_| true,
        apply: |_, _| "Mir Jafar has turned down the Company's offer.".into(),
    },
    WhatIf {
        title: "The Company goes bankrupt",
        text: "The East India Company's shareholders lose everything and it stops sending ships.",
        from: month(1609, 1),
        until: month(1856, 12),
        once: Some(Mark::CompanyBankrupt),
        needs_place: false,
        available: |w| civ(w, ENGLISH).is_some(),
        apply: |w, _| {
            if let Some(e) = civ(w, ENGLISH) {
                if let Some(c) = w.write::<Civ>().get_mut(e) {
                    c.emigrants_per_year = 0.0;
                }
            }
            "No more English ships will come.".into()
        },
    },
    WhatIf {
        title: "The monsoon fails here",
        text: "The rains fail around the tile you picked, and this year's crop with them.",
        from: 0,
        until: u32::MAX,
        once: None,
        needs_place: true,
        available: |_| true,
        apply: |w, at| {
            let center = at.unwrap_or_else(|| tile(w, 78.0, 22.0));
            let year = fail_harvest(w, center, 22, DROUGHT_KEEP);
            format!("The rains will fail here in {year}.")
        },
    },
    WhatIf {
        title: "Plague breaks out here",
        text: "An epidemic of plague breaks out in the town nearest the tile you picked.",
        from: 0,
        until: u32::MAX,
        once: None,
        needs_place: true,
        available: |_| true,
        apply: |w, at| {
            let at = at.unwrap_or_else(|| tile(w, 77.2, 28.6));
            match outbreak(w, at, "Plague") {
                Some(name) => format!("Plague has broken out in {name}."),
                None => "There was no town to strike.".into(),
            }
        },
    },
];

static NEW_WORLD_WHAT_IFS: [WhatIf; 2] = [
    WhatIf {
        title: "The harvest fails here",
        text: "Drought or early frost ruins this year's crop around the tile you picked.",
        from: 0,
        until: u32::MAX,
        once: None,
        needs_place: true,
        available: |_| true,
        apply: |w, at| {
            let center = at.unwrap_or_else(|| {
                let map = w.resource::<Map>();
                Position {
                    x: map.width as i32 / 2,
                    y: map.height as i32 / 2,
                }
            });
            let year = fail_harvest(w, center, 18, DROUGHT_KEEP);
            format!("The harvest will fail here in {year}.")
        },
    },
    WhatIf {
        title: "Smallpox breaks out here",
        text: "An epidemic of smallpox breaks out in the town nearest the tile you picked.",
        from: 0,
        until: u32::MAX,
        once: None,
        needs_place: true,
        available: |_| true,
        apply: |w, at| {
            let at = at.unwrap_or(Position { x: 0, y: 0 });
            match outbreak(w, at, "Smallpox") {
                Some(name) => format!("Smallpox has broken out in {name}."),
                None => "There was no town to strike.".into(),
            }
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Simulation;

    fn run_to(sim: &mut Simulation, year: u32) {
        while era::year(sim.month()) < year {
            sim.step_month();
        }
    }

    fn outcome(sim: &Simulation, title: &str) -> Option<Outcome> {
        sim.history()
            .chronicle
            .iter()
            .find(|e| e.title == title)
            .map(|e| e.outcome)
    }

    fn what_if(sim: &Simulation, title: &str) -> usize {
        what_ifs(Setting::India)
            .iter()
            .position(|w| w.title == title)
            .unwrap()
    }

    #[test]
    fn the_timeline_is_in_order() {
        for pair in INDIA.windows(2) {
            assert!(
                pair[0].date <= pair[1].date,
                "{} is out of order",
                pair[1].title
            );
        }
    }

    #[test]
    fn history_plays_out_and_the_economy_is_counted() {
        let mut sim = Simulation::scenario(Setting::India, 7);
        run_to(&mut sim, 1712);
        assert_eq!(outcome(&sim, "Aurangzeb dies"), Some(Outcome::AsInHistory));
        assert_eq!(
            outcome(&sim, "Shivaji is crowned"),
            Some(Outcome::AsInHistory)
        );
        assert!(sim
            .civ_summaries()
            .iter()
            .any(|c| c.name == MARATHAS && c.cities > 0));
        let years = &sim.history().economy.years;
        assert_eq!(years.len(), 112);
        let first = &years[0];
        let real_per_head = first.real_gdp / first.real_population;
        assert!(
            (first.per_head() / real_per_head - 1.0).abs() < 0.05,
            "{first:?}"
        );
        assert!(years.iter().all(|y| y.gdp > 0.0 && y.population > 0.0));
    }

    #[test]
    fn campaigns_take_towns_one_by_one_and_battles_mark_the_map() {
        let mut sim = Simulation::scenario(Setting::India, 7);
        run_to(&mut sim, 1687);
        while sim.month() <= month(1687, 9) {
            sim.step_month();
        }
        let deccan = |sim: &Simulation| held(&sim.world, DECCAN, None).len();
        let before = deccan(&sim);
        let war = sim
            .history()
            .wars
            .iter()
            .find(|w| w.name == "Aurangzeb's Deccan wars")
            .cloned();
        let war = war.expect("the Deccan campaign should be on the map");
        assert!(war.active(sim.month()) && war.progress().1 == before + war.progress().0);
        sim.step_month();
        sim.step_month();
        let after = deccan(&sim);
        assert!(
            after < before && after > 0,
            "towns should fall one at a time: {before} -> {after}"
        );
        run_to(&mut sim, 1691);
        assert_eq!(deccan(&sim), 0);
        let h = sim.history();
        let done = h
            .wars
            .iter()
            .find(|w| w.name == "Aurangzeb's Deccan wars")
            .unwrap();
        assert!(!done.active(sim.month()));
        assert_eq!(done.progress().0, done.progress().1);
    }

    #[test]
    fn a_strong_heir_keeps_bengal_in_the_empire_and_stops_plassey() {
        let mut sim = Simulation::scenario(Setting::India, 7);
        run_to(&mut sim, 1690);
        let heir = what_if(&sim, "A strong heir for Aurangzeb");
        assert!(sim.what_ifs_now().contains(&heir));
        sim.change_history(heir, None);
        assert!(
            !sim.what_ifs_now().contains(&heir),
            "it can only be made once"
        );
        run_to(&mut sim, 1760);
        assert_eq!(outcome(&sim, "Aurangzeb dies"), Some(Outcome::Different));
        assert_eq!(
            outcome(&sim, "Bengal goes its own way"),
            Some(Outcome::DidNotHappen)
        );
        assert_eq!(
            outcome(&sim, "The Battle of Plassey"),
            Some(Outcome::DidNotHappen)
        );
        assert!(sim
            .history()
            .chronicle
            .iter()
            .any(|e| e.outcome == Outcome::Yours));
    }

    #[test]
    fn a_failed_monsoon_shows_in_the_harvest_accounts() {
        let mut sim = Simulation::scenario(Setting::India, 7);
        run_to(&mut sim, 1602);
        let fail = what_if(&sim, "The monsoon fails here");
        let agra = {
            let map = sim.world.resource::<Map>();
            let (x, y) = era::project(78.0, 27.2, map.width, map.height);
            Position { x, y }
        };
        sim.change_history(fail, Some(agra));
        run_to(&mut sim, 1603);
        let years = &sim.history().economy.years;
        let (before, after) = (&years[years.len() - 2], &years[years.len() - 1]);
        let mughal = |y: &crate::economy::Year| {
            y.states.iter().find(|s| s.name == MUGHALS).unwrap().output[1]
        };
        assert!(
            mughal(after) < mughal(before),
            "weaving should fall with the harvest"
        );
    }
}
