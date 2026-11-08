//! Runs the simulation in the terminal, without a browser:
//!
//!     cargo run --release -- [years] [seed]

use a0::sim::{format_date, Simulation, MAP_HEIGHT, MAP_WIDTH};

fn main() {
    let mut args = std::env::args().skip(1);
    let years: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(50);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1);

    let mut sim = Simulation::new(MAP_WIDTH, MAP_HEIGHT, seed);
    println!("Seed {seed}, simulating {years} years\n");
    for year in 1..=years {
        for _ in 0..12 {
            sim.step_month();
        }
        if year % 10 == 0 || year == years {
            println!("== {} ==", format_date(sim.month().saturating_sub(1)));
            for civ in sim.civ_summaries() {
                println!(
                    "  {:<8} {:>3} cities  {:>6} people  {} settlers",
                    civ.name, civ.cities, civ.population, civ.settlers
                );
            }
        }
    }

    println!("\nRecent events:");
    for event in sim.events().iter().take(12) {
        println!("  {event}");
    }
    println!("\n{}", sim.ascii_map(2));
}
