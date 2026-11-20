//! Runs the simulation in the terminal, without a browser:
//!
//!     cargo run --release -- [years] [seed] [newworld|india]

use a0::era::Setting;
use a0::sim::{format_date, group, Simulation};

fn main() {
    let mut args = std::env::args().skip(1);
    let years: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(50);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1);
    let setting = match args.next().as_deref() {
        Some("india") => Setting::India,
        _ => Setting::NewWorld,
    };

    let mut sim = Simulation::scenario(setting, seed);
    println!(
        "{}, seed {seed}, simulating {years} years from 1600\n",
        sim.rules().title
    );
    for year in 0..=years {
        if year % 10 == 0 || year == years {
            println!("== {} ==", format_date(sim.month()));
            for civ in sim.civ_summaries() {
                println!(
                    "  {:<27} {:>3} towns  {:>7} people  {}",
                    civ.name,
                    civ.cities,
                    group(civ.population),
                    civ.status
                );
            }
        }
        if year < years {
            for _ in 0..12 {
                sim.step_month();
            }
        }
    }

    println!("\nRecent events:");
    for event in sim.events().iter().take(16) {
        println!("  {event}");
    }
    println!("\n{}", sim.ascii_map(2));
}
