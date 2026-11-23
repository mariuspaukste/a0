//! The historical settings, from 1600 on.
//!
//! - **New World**: a generated continent across the Atlantic. Native nations (fictional,
//!   like the continent) already live there; Spain, England, France and the Dutch arrive
//!   by ship at their historical dates and found colonies of settlers.
//! - **India**: the subcontinent as it was, with the Mughal Empire, the Deccan
//!   sultanates, the southern kingdoms and Kandy holding the land and its great cities.
//!   The Portuguese are already there; the East India Companies of England, the Dutch
//!   Republic, Denmark and France arrive to trade, not to settle, and build fortified
//!   factories on the coast.
//!
//! Numbers of people are scaled far down to fit the map, but keep their proportions.

use crate::sim::Rng;

/// Month 0 of the simulation is January of this year.
pub const START_YEAR: u32 = 1600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    NewWorld,
    India,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Culture {
    /// The peoples of the New World: villages of longhouses and fields, no immunity
    /// to European diseases.
    Native,
    Indian,
    English,
    French,
    Spanish,
    Dutch,
    Portuguese,
    Danish,
}

impl Culture {
    /// Whether this is one of the land's own peoples rather than Europeans.
    pub fn is_local(self) -> bool {
        matches!(self, Culture::Native | Culture::Indian)
    }

    /// Native villages: longhouses and fields with no streets.
    pub fn lives_in_villages(self) -> bool {
        self == Culture::Native
    }
}

/// Where a power's first ship makes for.
#[derive(Clone, Copy, Debug)]
pub enum Landing {
    /// A stretch of coast at this latitude: 0 is the north edge of the map, 1 the south.
    Latitude(f32),
    /// A spot near this longitude and latitude, in degrees.
    Near(f32, f32),
}

/// A town that already stands when the simulation starts, at a real place.
pub struct Town {
    pub name: &'static str,
    pub lon: f32,
    pub lat: f32,
    pub population: u32,
}

const fn town(name: &'static str, lon: f32, lat: f32, population: u32) -> Town {
    Town {
        name,
        lon,
        lat,
        population,
    }
}

/// A European power that sends ships.
pub struct Power {
    pub name: &'static str,
    /// The adjective for event messages: "English settlers".
    pub people: &'static str,
    pub color: [u8; 3],
    pub culture: Culture,
    /// When the first ship arrives, in months since January 1600.
    pub arrival: u32,
    /// People a year who sail out. Trade makes the colonies more attractive later on.
    pub emigrants_per_year: f32,
    /// Average people on a ship.
    pub ship_passengers: u32,
    pub landing: Landing,
    /// Trading companies build fortified factories in other peoples' lands, buy their
    /// food and draw local merchants and weavers, instead of farming new land.
    pub trade_only: bool,
    /// Real places where the power built factories, with their locations. A new
    /// factory takes the name of the one nearest to it, and companies are drawn toward
    /// them.
    pub places: &'static [Town],
    /// Towns the power already holds in 1600.
    pub towns: &'static [Town],
}

/// One of the land's own peoples.
pub struct Nation {
    pub name: &'static str,
    pub people: &'static str,
    pub color: [u8; 3],
    pub culture: Culture,
    /// What their main house of worship is called.
    pub shrine: &'static str,
    /// Real places for new towns, with their locations: a new town takes the name of
    /// the nearest one. Empty for the New World, whose nations make up names.
    pub places: &'static [Town],
    /// Towns that stand in 1600. Empty for the New World, where villages are placed
    /// on the generated map.
    pub towns: &'static [Town],
}

pub struct Scenario {
    pub setting: Setting,
    pub title: &'static str,
    pub width: usize,
    pub height: usize,
    /// Headings for the two groups of peoples in the side panel.
    pub locals_heading: &'static str,
    pub powers_heading: &'static str,
    pub nations: &'static [Nation],
    pub powers: &'static [Power],
    /// The month (0 = January) when the main crop is brought in.
    pub harvest_month: u32,
    /// How long the land's own peoples grow before the clock starts in 1600.
    pub prehistory_years: u32,
    /// Which map edges ships arrive from: west, east, south.
    pub sea_lanes: [bool; 3],
}

/// Month `month` (1 to 12) of `year`, counted from January 1600.
pub const fn month(year: u32, month: u32) -> u32 {
    (year - START_YEAR) * 12 + month - 1
}

pub fn scenario(setting: Setting) -> &'static Scenario {
    match setting {
        Setting::NewWorld => &NEW_WORLD,
        Setting::India => &INDIA,
    }
}

// ---------------------------------------------------------------------------
// The New World
// ---------------------------------------------------------------------------

/// England sent far more people than anyone else; New France grew from a few thousand
/// immigrants in 150 years. Spain's mainland colonies were mostly forts and missions.
const NEW_WORLD_POWERS: [Power; 4] = [
    Power {
        name: "Spain",
        people: "Spanish",
        color: [236, 190, 40],
        culture: Culture::Spanish,
        arrival: month(1601, 3),
        emigrants_per_year: 25.0,
        ship_passengers: 90,
        landing: Landing::Latitude(0.88),
        trade_only: false,
        places: &[],
        towns: &[],
    },
    Power {
        name: "England",
        people: "English",
        color: [205, 38, 46],
        culture: Culture::English,
        arrival: month(1607, 5),
        emigrants_per_year: 160.0,
        ship_passengers: 90,
        landing: Landing::Latitude(0.62),
        trade_only: false,
        places: &[],
        towns: &[],
    },
    Power {
        name: "France",
        people: "French",
        color: [58, 98, 225],
        culture: Culture::French,
        arrival: month(1608, 7),
        emigrants_per_year: 25.0,
        ship_passengers: 90,
        landing: Landing::Latitude(0.12),
        trade_only: false,
        places: &[],
        towns: &[],
    },
    Power {
        name: "Dutch Republic",
        people: "Dutch",
        color: [245, 128, 28],
        culture: Culture::Dutch,
        arrival: month(1614, 4),
        emigrants_per_year: 60.0,
        ship_passengers: 90,
        landing: Landing::Latitude(0.38),
        trade_only: false,
        places: &[],
        towns: &[],
    },
];

const fn native(name: &'static str, color: [u8; 3]) -> Nation {
    Nation {
        name,
        people: name,
        color,
        culture: Culture::Native,
        shrine: "Council house",
        places: &[],
        towns: &[],
    }
}

const NEW_WORLD_NATIONS: [Nation; 4] = [
    native("Ostari", [40, 178, 160]),
    native("Velmar", [150, 96, 210]),
    native("Kethi", [226, 108, 162]),
    native("Aurelan", [120, 200, 240]),
];

static NEW_WORLD: Scenario = Scenario {
    setting: Setting::NewWorld,
    title: "New World",
    width: 160,
    height: 120,
    locals_heading: "Native nations",
    powers_heading: "Colonial powers",
    nations: &NEW_WORLD_NATIONS,
    powers: &NEW_WORLD_POWERS,
    harvest_month: 8,
    prehistory_years: 60,
    sea_lanes: [false, true, false],
};

// ---------------------------------------------------------------------------
// India
// ---------------------------------------------------------------------------

/// The Mughal Empire under Akbar held the whole north, from Kabul to Bengal. Its cities
/// were among the largest in the world: Agra and Lahore rivalled London and Paris.
const MUGHAL_TOWNS: [Town; 16] = [
    town("Agra", 78.0, 27.2, 2400),
    town("Lahore", 74.3, 31.5, 2000),
    town("Delhi", 77.2, 28.6, 1600),
    town("Ahmedabad", 72.6, 23.0, 1600),
    town("Patna", 85.1, 25.6, 1200),
    town("Dhaka", 90.4, 23.7, 1000),
    town("Surat", 72.8, 21.3, 900),
    town("Benares", 83.0, 25.3, 900),
    town("Burhanpur", 76.2, 21.3, 900),
    town("Multan", 71.5, 30.2, 800),
    town("Lucknow", 80.9, 26.8, 700),
    town("Ajmer", 74.6, 26.4, 600),
    town("Thatta", 67.9, 24.7, 600),
    town("Rajmahal", 87.8, 25.0, 600),
    town("Srinagar", 74.8, 34.0, 500),
    town("Ujjain", 75.8, 23.2, 600),
];

/// Bijapur, Golconda and Ahmadnagar: wealthy sultanates on the Deccan plateau.
/// Golconda's diamond mines and Masulipatnam's cloth made it famous.
const DECCAN_TOWNS: [Town; 7] = [
    town("Golconda", 78.4, 17.4, 1500),
    town("Bijapur", 75.7, 16.8, 1400),
    town("Ahmadnagar", 74.7, 19.1, 900),
    town("Bidar", 77.5, 17.9, 600),
    town("Masulipatnam", 81.1, 16.2, 500),
    town("Gulbarga", 76.8, 17.3, 500),
    town("Dabhol", 73.2, 17.6, 300),
];

/// The successors of the Vijayanagara empire: its emperor at Chandragiri, the Nayaks of
/// Madurai, Thanjavur and Gingee, and the Zamorin of Calicut on the pepper coast.
const SOUTHERN_TOWNS: [Town; 9] = [
    town("Madurai", 78.1, 9.9, 1000),
    town("Thanjavur", 79.1, 10.8, 900),
    town("Chandragiri", 79.3, 13.6, 700),
    town("Calicut", 75.8, 11.3, 700),
    town("Vellore", 79.1, 12.9, 600),
    town("Gingee", 79.4, 12.25, 500),
    town("Mysore", 76.6, 12.3, 500),
    town("Penukonda", 77.6, 14.1, 500),
    town("Quilon", 76.6, 8.9, 300),
];

const KANDY_TOWNS: [Town; 3] = [
    town("Kandy", 80.6, 7.3, 500),
    town("Jaffna", 80.0, 9.7, 400),
    town("Batticaloa", 81.7, 7.7, 200),
];

const INDIAN_NATIONS: [Nation; 4] = [
    Nation {
        name: "Mughal Empire",
        people: "Mughal",
        color: [46, 152, 74],
        culture: Culture::Indian,
        shrine: "Mosque",
        places: &[
            town("Sirhind", 76.4, 30.6, 0),
            town("Panipat", 76.97, 29.39, 0),
            town("Mathura", 77.67, 27.49, 0),
            town("Gwalior", 78.18, 26.22, 0),
            town("Jaunpur", 82.68, 25.75, 0),
            town("Kanauj", 79.92, 27.05, 0),
            town("Bareilly", 79.43, 28.37, 0),
            town("Sonargaon", 90.6, 23.65, 0),
            town("Mandu", 75.4, 22.35, 0),
            town("Bhakkar", 71.06, 31.63, 0),
            town("Sialkot", 74.53, 32.49, 0),
            town("Jalandhar", 75.58, 31.33, 0),
            town("Hissar", 75.72, 29.15, 0),
            town("Sambhal", 78.57, 28.58, 0),
            town("Chunar", 82.88, 25.13, 0),
            town("Rohtas", 84.03, 24.62, 0),
            town("Fatehpur Sikri", 77.66, 27.09, 0),
            town("Kalpi", 79.75, 26.12, 0),
            town("Saharanpur", 77.55, 29.96, 0),
            town("Peshawar", 71.57, 34.0, 0),
            town("Jodhpur", 73.02, 26.24, 0),
            town("Udaipur", 73.71, 24.58, 0),
            town("Chittor", 74.63, 24.88, 0),
            town("Cuttack", 85.88, 20.46, 0),
            town("Sylhet", 91.87, 24.9, 0),
            town("Murshidabad", 88.27, 24.18, 0),
            town("Hajipur", 85.22, 25.69, 0),
            town("Gorakhpur", 83.37, 26.76, 0),
            town("Bikaner", 73.31, 28.02, 0),
            town("Jaisalmer", 70.91, 26.92, 0),
            town("Kota", 75.86, 25.18, 0),
            town("Sironj", 77.69, 24.1, 0),
            town("Broach", 72.98, 21.7, 0),
            town("Cambay", 72.62, 22.31, 0),
            town("Baroda", 73.18, 22.3, 0),
            town("Junagadh", 70.46, 21.52, 0),
            town("Sehwan", 67.86, 26.43, 0),
            town("Nagaur", 73.73, 27.2, 0),
            town("Bahraich", 81.6, 27.57, 0),
            town("Midnapore", 87.32, 22.42, 0),
            town("Chittagong", 91.83, 22.36, 0),
        ],
        towns: &MUGHAL_TOWNS,
    },
    Nation {
        name: "Deccan Sultanates",
        people: "Deccani",
        color: [116, 196, 214],
        culture: Culture::Indian,
        shrine: "Mosque",
        places: &[
            town("Raichur", 77.36, 16.2, 0),
            town("Khadki", 75.34, 19.88, 0),
            town("Daulatabad", 75.21, 19.94, 0),
            town("Kalyani", 76.93, 17.88, 0),
            town("Sholapur", 75.91, 17.66, 0),
            town("Adoni", 77.27, 15.63, 0),
            town("Warangal", 79.59, 17.97, 0),
            town("Nanded", 77.32, 19.15, 0),
            town("Paranda", 75.45, 18.27, 0),
            town("Kondapalli", 80.54, 16.62, 0),
            town("Rajahmundry", 81.8, 17.0, 0),
            town("Medak", 78.27, 18.04, 0),
            town("Gandikota", 78.28, 14.81, 0),
            town("Kurnool", 78.04, 15.83, 0),
            town("Junnar", 73.88, 19.2, 0),
            town("Poona", 73.86, 18.52, 0),
            town("Satara", 74.0, 17.68, 0),
            town("Belgaum", 74.5, 15.85, 0),
            town("Kolhapur", 74.24, 16.7, 0),
            town("Ellichpur", 77.51, 21.26, 0),
        ],
        towns: &DECCAN_TOWNS,
    },
    Nation {
        name: "Vijayanagara",
        people: "Vijayanagara",
        color: [204, 72, 140],
        culture: Culture::Indian,
        shrine: "Temple",
        places: &[
            town("Tirupati", 79.42, 13.63, 0),
            town("Kanchipuram", 79.7, 12.83, 0),
            town("Tiruchirappalli", 78.7, 10.8, 0),
            town("Ramnad", 78.83, 9.37, 0),
            town("Tirunelveli", 77.7, 8.73, 0),
            town("Kumbakonam", 79.39, 10.96, 0),
            town("Ikkeri", 75.06, 14.13, 0),
            town("Srirangapatna", 76.68, 12.42, 0),
            town("Kolar", 78.13, 13.14, 0),
            town("Dindigul", 77.98, 10.36, 0),
            town("Chidambaram", 79.69, 11.4, 0),
            town("Salem", 78.15, 11.66, 0),
            town("Coimbatore", 76.96, 11.0, 0),
            town("Hampi", 76.46, 15.33, 0),
            town("Bangalore", 77.59, 12.97, 0),
            town("Tiruvannamalai", 79.07, 12.23, 0),
            town("Nellore", 79.99, 14.44, 0),
            town("Cuddapah", 78.82, 14.47, 0),
        ],
        towns: &SOUTHERN_TOWNS,
    },
    Nation {
        name: "Kingdom of Kandy",
        people: "Kandyan",
        color: [156, 104, 214],
        culture: Culture::Indian,
        shrine: "Temple",
        places: &[
            town("Matale", 80.62, 7.47, 0),
            town("Badulla", 81.05, 6.99, 0),
            town("Kurunegala", 80.36, 7.49, 0),
            town("Anuradhapura", 80.4, 8.31, 0),
            town("Ratnapura", 80.4, 6.68, 0),
            town("Trincomalee", 81.23, 8.57, 0),
        ],
        towns: &KANDY_TOWNS,
    },
];

/// The Portuguese had ruled the sea lanes and held Goa since 1510.
const PORTUGUESE_TOWNS: [Town; 6] = [
    town("Goa", 73.8, 15.5, 800),
    town("Cochin", 76.25, 9.95, 400),
    town("Diu", 70.98, 20.71, 250),
    town("Bassein", 72.83, 19.33, 300),
    town("Colombo", 79.85, 6.93, 300),
    town("Hooghly", 88.4, 22.9, 300),
];

/// The East India Companies sent few people: merchants ("factors"), clerks, soldiers
/// and sailors. Their factories grew because Indian merchants, weavers and laborers
/// moved in: Madras had some 40,000 people within forty years of its founding.
const INDIA_POWERS: [Power; 5] = [
    Power {
        name: "Estado da Índia",
        people: "Portuguese",
        color: [18, 120, 110],
        culture: Culture::Portuguese,
        arrival: 0,
        emigrants_per_year: 30.0,
        ship_passengers: 40,
        landing: Landing::Near(73.8, 15.5),
        trade_only: true,
        places: &[
            town("Chaul", 72.93, 18.55, 0),
            town("Daman", 72.83, 20.4, 0),
            town("Mangalore", 74.86, 12.87, 0),
            town("Cannanore", 75.37, 11.87, 0),
            town("Negapatão", 79.84, 10.77, 0),
            town("São Tomé", 80.27, 13.03, 0),
            town("Tuticorin", 78.13, 8.8, 0),
        ],
        towns: &PORTUGUESE_TOWNS,
    },
    Power {
        name: "Dutch East India Company",
        people: "Dutch",
        color: [245, 128, 28],
        culture: Culture::Dutch,
        arrival: month(1605, 4),
        emigrants_per_year: 45.0,
        ship_passengers: 40,
        landing: Landing::Near(80.3, 13.4),
        trade_only: true,
        places: &[
            town("Fort Geldria", 80.32, 13.42, 0),
            town("Negapatnam", 79.85, 10.77, 0),
            town("Chinsurah", 88.4, 22.9, 0),
            town("Bimlipatam", 83.45, 17.9, 0),
            town("Sadras", 80.17, 12.53, 0),
            town("Palakol", 81.73, 16.52, 0),
            town("Galle", 80.22, 6.03, 0),
            town("Jaffnapatam", 80.0, 9.66, 0),
            town("Vengurla", 73.63, 15.86, 0),
            town("Dutch Surat", 72.8, 21.2, 0),
        ],
        towns: &[],
    },
    Power {
        name: "English East India Company",
        people: "English",
        color: [205, 38, 46],
        culture: Culture::English,
        arrival: month(1608, 8),
        emigrants_per_year: 40.0,
        ship_passengers: 40,
        landing: Landing::Near(72.8, 21.1),
        trade_only: true,
        places: &[
            town("Surat Factory", 72.83, 21.17, 0),
            town("Fort St. George", 80.28, 13.08, 0),
            town("Bombay", 72.88, 18.93, 0),
            town("Fort William", 88.35, 22.57, 0),
            town("Fort St. David", 79.77, 11.75, 0),
            town("Balasore", 86.93, 21.49, 0),
            town("Masulipatnam Factory", 81.13, 16.17, 0),
            town("Vizagapatam", 83.3, 17.7, 0),
            town("Tellicherry", 75.49, 11.75, 0),
            town("Anjengo", 76.77, 8.66, 0),
            town("Karwar", 74.13, 14.8, 0),
        ],
        towns: &[],
    },
    Power {
        name: "Danish East India Company",
        people: "Danish",
        color: [236, 206, 52],
        culture: Culture::Danish,
        arrival: month(1620, 11),
        emigrants_per_year: 8.0,
        ship_passengers: 30,
        landing: Landing::Near(79.85, 11.0),
        trade_only: true,
        places: &[
            town("Tranquebar", 79.85, 11.03, 0),
            town("Serampore", 88.34, 22.75, 0),
            town("Danish Balasore", 86.93, 21.49, 0),
        ],
        towns: &[],
    },
    Power {
        name: "French East India Company",
        people: "French",
        color: [58, 98, 225],
        culture: Culture::French,
        arrival: month(1668, 2),
        emigrants_per_year: 25.0,
        ship_passengers: 40,
        landing: Landing::Near(79.8, 11.9),
        trade_only: true,
        places: &[
            town("Pondichéry", 79.83, 11.93, 0),
            town("Chandernagor", 88.37, 22.87, 0),
            town("Karikal", 79.83, 10.92, 0),
            town("Mahé", 75.53, 11.7, 0),
            town("Yanaon", 82.21, 16.73, 0),
            town("French Surat", 72.83, 21.17, 0),
        ],
        towns: &[],
    },
];

static INDIA: Scenario = Scenario {
    setting: Setting::India,
    title: "India",
    width: 200,
    height: 165,
    locals_heading: "Indian states",
    powers_heading: "European companies",
    nations: &INDIAN_NATIONS,
    powers: &INDIA_POWERS,
    // The kharif crop, sown with the monsoon rains, comes in around October.
    harvest_month: 9,
    prehistory_years: 25,
    sea_lanes: [true, false, true],
};

/// The tile at a longitude and latitude on the India map, which covers 60–100°E and
/// 38–5°N.
pub fn project(lon: f32, lat: f32, width: usize, height: usize) -> (i32, i32) {
    let x = (lon - 60.0) / 40.0 * width as f32;
    let y = (38.0 - lat) / 33.0 * height as f32;
    (x as i32, y as i32)
}

// ---------------------------------------------------------------------------
// Names, dates and climate
// ---------------------------------------------------------------------------

/// A name for a new town, made up in the founders' language.
pub fn town_name(culture: Culture, rng: &mut Rng, taken: &[String]) -> String {
    for _ in 0..20 {
        let name = made_up_name(culture, rng);
        if !taken.contains(&name) {
            return name;
        }
    }
    made_up_name(culture, rng)
}

fn made_up_name(culture: Culture, rng: &mut Rng) -> String {
    let pick = |rng: &mut Rng, items: &[&str]| rng.pick(items).to_string();
    match culture {
        Culture::English => match rng.below(3) {
            0 => format!(
                "New {}",
                pick(
                    rng,
                    &[
                        "Bristol",
                        "Plymouth",
                        "Hampton",
                        "Kent",
                        "York",
                        "Exeter",
                        "Dover",
                        "Norwich",
                        "Lincoln",
                        "Salisbury",
                        "Durham",
                        "Bedford",
                        "Windsor",
                        "Haven"
                    ]
                )
            ),
            1 => format!(
                "{}{}",
                pick(
                    rng,
                    &[
                        "James",
                        "Charles",
                        "Elizabeth",
                        "Anne",
                        "Mary",
                        "George",
                        "Henry",
                        "Williams",
                        "Fred",
                        "Hamp"
                    ]
                ),
                pick(rng, &["town", "burg", "ton", "field"])
            ),
            _ => pick(
                rng,
                &[
                    "Salem",
                    "Providence",
                    "Concord",
                    "Hartford",
                    "Greenwich",
                    "Dorchester",
                    "Cambridge",
                    "Springfield",
                    "Newport",
                    "Portsmouth",
                    "Ipswich",
                    "Middleton",
                    "Milford",
                    "Stamford",
                    "Guilford",
                    "Fairfield",
                    "Wethersfield",
                ],
            ),
        },
        Culture::French => match rng.below(3) {
            0 => format!(
                "{}{}",
                pick(rng, &["Saint-", "Sainte-", "Port-", "Fort "]),
                pick(
                    rng,
                    &[
                        "Laurent",
                        "Louis",
                        "Denis",
                        "Croix",
                        "Pierre",
                        "Malo",
                        "Charles",
                        "Ignace",
                        "Michel",
                        "Joseph",
                        "Anne",
                        "Royal",
                        "Frontenac",
                        "Dauphin"
                    ]
                )
            ),
            1 => format!(
                "Nouvelle-{}",
                pick(
                    rng,
                    &["Rochelle", "Rouen", "Orléans", "Bretagne", "Lorient", "Brest"]
                )
            ),
            _ => pick(
                rng,
                &[
                    "Trois-Rivières",
                    "Beaupré",
                    "Beauport",
                    "Bonsecours",
                    "Charlesbourg",
                    "Bellefontaine",
                    "Montmagny",
                    "Cap-Rouge",
                    "Lachine",
                    "Boucherville",
                    "Sorel",
                    "Repentigny",
                    "Chambly",
                ],
            ),
        },
        Culture::Spanish | Culture::Portuguese => match rng.below(3) {
            0 => format!(
                "San {}",
                pick(
                    rng,
                    &[
                        "Agustín", "Mateo", "Miguel", "Felipe", "Marcos", "Luis", "Juan", "Pedro",
                        "Diego", "José", "Lorenzo", "Antonio"
                    ]
                )
            ),
            1 => format!(
                "Santa {}",
                pick(
                    rng,
                    &[
                        "Lucía", "Elena", "Fe", "Cruz", "Catalina", "Bárbara", "Isabel", "Rosa",
                        "Clara"
                    ]
                )
            ),
            _ => pick(
                rng,
                &[
                    "Nueva Sevilla",
                    "Nueva Granada",
                    "Nuevo Toledo",
                    "Nueva Cádiz",
                    "Puerto Real",
                    "Asunción",
                    "Concepción",
                    "Trinidad",
                    "Guadalupe",
                    "Los Remedios",
                    "Pensacola",
                    "Soledad",
                ],
            ),
        },
        Culture::Dutch | Culture::Danish => match rng.below(3) {
            0 => format!(
                "Nieuw {}",
                pick(
                    rng,
                    &[
                        "Amsterdam",
                        "Haarlem",
                        "Utrecht",
                        "Leiden",
                        "Delft",
                        "Gouda",
                        "Zeeland",
                        "Amstel",
                        "Rotterdam",
                        "Dordrecht"
                    ]
                )
            ),
            1 => format!(
                "Fort {}",
                pick(
                    rng,
                    &["Oranje", "Nassau", "Casimir", "Goede Hoop", "Amstel"]
                )
            ),
            _ => pick(
                rng,
                &[
                    "Beverwijck",
                    "Breuckelen",
                    "Vlissingen",
                    "Wiltwijck",
                    "Bergen",
                    "Midwout",
                    "Vlacke Bos",
                    "Rensselaerswijck",
                    "Swaanendael",
                    "Esopus",
                    "Schenectady",
                    "Haerlem",
                ],
            ),
        },
        Culture::Indian => {
            let root = pick(
                rng,
                &[
                    "Ram", "Sultan", "Fateh", "Shah", "Krishna", "Hari", "Chandra", "Moti", "Sher",
                    "Islam", "Shiv", "Bhavani", "Nur", "Jahan", "Raj", "Dev",
                ],
            );
            let suffix = pick(rng, &["pur", "abad", "nagar", "garh", "kot", "ganj"]);
            // Krishna + abad = Krishnabad.
            match (root.ends_with('a'), suffix.strip_prefix('a')) {
                (true, Some(rest)) => format!("{root}{rest}"),
                _ => format!("{root}{suffix}"),
            }
        }
        Culture::Native => {
            // Two or three syllables from a small, invented sound set.
            const SYLLABLES: &[&str] = &[
                "ka", "ta", "no", "wa", "shi", "ke", "to", "mi", "sa", "ne", "ho", "ra", "lo",
                "pe", "qua", "chi", "na", "ma", "te", "go", "su", "yo", "an", "ok",
            ];
            let count = 2 + rng.below(2);
            let mut name = String::new();
            for _ in 0..count {
                name += rng.pick(SYLLABLES);
            }
            let mut chars = name.chars();
            let first = chars.next().unwrap().to_ascii_uppercase();
            std::iter::once(first).chain(chars).collect()
        }
    }
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

/// "May 1607".
pub fn format_date(month: u32) -> String {
    format!("{} {}", MONTH_NAMES[(month % 12) as usize], year(month))
}

pub fn year(month: u32) -> u32 {
    START_YEAR + month / 12
}

/// How far south a row of the map is: 0 at the top (north) edge, 1 at the bottom.
pub fn latitude(y: i32, height: usize) -> f32 {
    (y as f32 / height.max(1) as f32).clamp(0.0, 1.0)
}

/// How well crops grow: in the New World the north has a short season and the south a
/// long one. India's crops depend on the monsoon instead (see the famine rules).
pub fn growing(setting: Setting, latitude: f32) -> f32 {
    match setting {
        Setting::NewWorld => 0.8 + 0.45 * latitude,
        Setting::India => 1.0,
    }
}

/// How deep the winter is, from 0 (none) to 1 (snow on the ground, little grows,
/// little outdoor work). Northern New World winters are long; the far south barely
/// has one; India's plains have none.
pub fn winter(setting: Setting, month: u32, latitude: f32) -> f32 {
    if setting == Setting::India {
        return 0.0;
    }
    let depth: f32 = match month % 12 {
        0 => 1.0,
        1 => 0.9,
        2 => 0.55,
        3 => 0.15,
        10 => 0.35,
        11 => 0.8,
        _ => 0.0,
    };
    (depth * (1.25 - 1.45 * latitude)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_start_in_1600() {
        assert_eq!(format_date(0), "January 1600");
        assert_eq!(format_date(NEW_WORLD_POWERS[1].arrival), "May 1607");
        assert_eq!(format_date(INDIA_POWERS[2].arrival), "August 1608");
    }

    #[test]
    fn the_north_has_harder_winters() {
        let nw = Setting::NewWorld;
        assert!(winter(nw, 0, 0.1) > winter(nw, 0, 0.6));
        assert_eq!(winter(nw, 0, 0.95), 0.0);
        assert_eq!(winter(nw, 6, 0.0), 0.0);
        assert_eq!(winter(Setting::India, 0, 0.0), 0.0);
        assert!(growing(nw, 0.9) > growing(nw, 0.1));
    }

    #[test]
    fn made_up_town_names_are_new_and_never_empty() {
        let mut rng = Rng::new(5);
        let mut taken = Vec::new();
        for _ in 0..10 {
            let name = town_name(Culture::Dutch, &mut rng, &taken);
            assert!(!taken.contains(&name), "{name} used twice");
            taken.push(name);
        }
        for culture in [
            Culture::Native,
            Culture::Indian,
            Culture::English,
            Culture::French,
        ] {
            for _ in 0..50 {
                let name = town_name(culture, &mut rng, &[]);
                assert!(!name.is_empty(), "{culture:?}");
                // Krishna + abad makes Krishnabad, not Krishnaabad.
                assert!(culture != Culture::Indian || !name.contains("aa"), "{name}");
            }
        }
    }

    #[test]
    fn indian_cities_land_on_the_map() {
        let s = scenario(Setting::India);
        for nation in s.nations {
            for t in nation.towns {
                let (x, y) = project(t.lon, t.lat, s.width, s.height);
                assert!(x >= 0 && y >= 0 && (x as usize) < s.width && (y as usize) < s.height);
            }
        }
    }
}
