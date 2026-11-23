//! Your timeline, and the one you didn't change.
//!
//! The simulation is deterministic: the same scenario and seed play out the same
//! world. So when the player first changes history, a second simulation is started
//! from the same seed and left alone. It replays the months already played (a few
//! at a time, so the page stays responsive) and then keeps pace with the player's.
//! Setting the two side by side shows what the player's changes did: which events
//! went differently, and how population and GDP diverged.

use crate::economy::Year;
use crate::era::Setting;
use crate::history::{Entry, Outcome};
use crate::sim::{Position, Simulation};

pub struct Timelines {
    /// The world the player is watching and changing.
    pub yours: Simulation,
    /// The same world, left as history had it. Starts when the player first changes
    /// history.
    pub unchanged: Option<Simulation>,
    /// The month of the player's first change.
    pub changed_at: Option<u32>,
    setting: Setting,
    seed: u64,
}

impl Timelines {
    pub fn new(setting: Setting, seed: u64) -> Self {
        Self {
            yours: Simulation::scenario(setting, seed),
            unchanged: None,
            changed_at: None,
            setting,
            seed,
        }
    }

    /// Plays `months` more months of your timeline. The unchanged one keeps pace once
    /// it has caught up (see [`Timelines::catch_up`]).
    pub fn step(&mut self, months: u32) {
        for _ in 0..months {
            let behind = self.behind();
            self.yours.step_month();
            if behind == 0 {
                if let Some(u) = &mut self.unchanged {
                    u.step_month();
                }
            }
        }
    }

    /// How many months the unchanged timeline still has to replay.
    pub fn behind(&self) -> u32 {
        self.unchanged
            .as_ref()
            .map_or(0, |u| self.yours.month().saturating_sub(u.month()))
    }

    /// Replays up to `months` months of the unchanged timeline. Returns how many it
    /// still has to go.
    pub fn catch_up(&mut self, months: u32) -> u32 {
        let n = months.min(self.behind());
        if let Some(u) = &mut self.unchanged {
            for _ in 0..n {
                u.step_month();
            }
        }
        self.behind()
    }

    /// Changes history in your timeline, starting the unchanged one first if this is
    /// your first change.
    pub fn change_history(&mut self, i: usize, at: Option<Position>) -> String {
        if !self.yours.what_ifs_now().contains(&i) {
            return "That can't be changed any more.".to_string();
        }
        if self.unchanged.is_none() {
            self.unchanged = Some(Simulation::scenario(self.setting, self.seed));
            self.changed_at = Some(self.yours.month());
        }
        self.yours.change_history(i, at)
    }

    /// The unchanged timeline's accounts for `year`, once it has got that far.
    pub fn unchanged_year(&self, year: u32) -> Option<Year> {
        let u = self.unchanged.as_ref()?;
        let h = u.history();
        h.economy.years.iter().find(|y| y.year == year).cloned()
    }

    /// What became of an event of your chronicle in the unchanged timeline, if it came
    /// out differently there. `None` until the unchanged timeline has reached it.
    pub fn unchanged_entry(&self, entry: &Entry) -> Option<Entry> {
        if entry.outcome == Outcome::Yours || self.changed_at.is_none_or(|m| entry.month < m) {
            return None;
        }
        let u = self.unchanged.as_ref()?;
        let h = u.history();
        let theirs = h.chronicle.iter().find(|e| e.title == entry.title)?;
        (theirs.outcome != entry.outcome || theirs.text != entry.text).then(|| theirs.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::era;
    use crate::history::what_ifs;

    fn run_to(t: &mut Timelines, year: u32) {
        while era::year(t.yours.month()) < year {
            t.step(1);
            t.catch_up(1000);
        }
    }

    #[test]
    fn the_unchanged_timeline_starts_with_your_first_change_and_catches_up() {
        let mut t = Timelines::new(Setting::India, 7);
        run_to(&mut t, 1700);
        assert!(t.unchanged.is_none());
        let heir = what_ifs(Setting::India)
            .iter()
            .position(|w| w.title == "A strong heir for Aurangzeb")
            .unwrap();
        t.change_history(heir, None);
        assert_eq!(t.changed_at, Some(t.yours.month()));
        assert_eq!(t.behind(), t.yours.month());
        t.catch_up(500);
        assert_eq!(t.behind(), t.yours.month() - 500);
        assert_eq!(t.catch_up(10_000), 0);

        // Up to the change, the two timelines are the same world.
        let a = t.unchanged_year(1699).unwrap();
        let b = {
            let h = t.yours.history();
            h.economy
                .years
                .iter()
                .find(|y| y.year == 1699)
                .cloned()
                .unwrap()
        };
        assert_eq!(a.population, b.population);
        assert_eq!(a.gdp, b.gdp);

        run_to(&mut t, 1760);
        let h = t.yours.history();
        let plassey = h
            .chronicle
            .iter()
            .find(|e| e.title == "The Battle of Plassey")
            .unwrap();
        assert_eq!(plassey.outcome, Outcome::DidNotHappen);
        let there = t
            .unchanged_entry(plassey)
            .expect("Plassey went differently there");
        assert_eq!(there.outcome, Outcome::AsInHistory);
        drop(h);
        assert!(t.unchanged_year(1759).is_some());
    }
}
