//! When a learning word comes back: FSRS, the scheduler Anki uses (FSRS-5,
//! its published default weights; owner, 2026-11-11: "is it using the anki
//! algorithm anywhere?"). Each word keeps a stability (days until recall
//! falls to 90%) and a difficulty, updated by every answer; the next review
//! is set for 90% recall. Again sends a word to a short relearning step
//! (2 min); Good on a new word to a 10 min learning step, then it
//! graduates.
//!
//! One copy for every app (moved here from ccez-keys, 2026-11-12): the
//! keyboard's screen words answer it directly, and ccez-words grades what
//! the owner watches ([watched]), so the videos and the screen words share
//! one schedule. A card travels as JSON (`{"due", "at", "s", "d", "state",
//! "reps", "lapses", "last"}`, seconds); the later `at` wins a sync.

/// FSRS-5's default weights (as Anki ships them).
const W: [f64; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925, 1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898,
    0.51655, 0.6621,
];
const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
/// Reviews are set for this chance of still knowing the word.
const RETENTION: f64 = 0.9;
pub const DAY: u64 = 86_400;
/// Good on a new word: back once more after this, then it graduates.
pub const LEARN_STEP: u64 = 600;
/// Again: back after this.
pub const RELEARN: u64 = 120;
/// Never put off longer than this, in days.
const MAX_DAYS: f64 = 365.0;

/// An answer. Hard isn't asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Good,
    Easy,
    Again,
}

impl Grade {
    /// FSRS's rating: 1 Again, 3 Good, 4 Easy.
    fn rating(self) -> f64 {
        match self {
            Grade::Again => 1.0,
            Grade::Good => 3.0,
            Grade::Easy => 4.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    New,
    Learning,
    Review,
    Relearning,
}

impl State {
    fn code(self) -> u64 {
        match self {
            State::New => 0,
            State::Learning => 1,
            State::Review => 2,
            State::Relearning => 3,
        }
    }
    fn of(n: u64) -> State {
        match n {
            1 => State::Learning,
            2 => State::Review,
            3 => State::Relearning,
            _ => State::New,
        }
    }
}

/// One word's schedule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Card {
    /// Unix seconds it may show again.
    pub due: u64,
    /// Unix seconds it was last answered (the newest answer wins a sync).
    pub at: u64,
    /// Days until recall falls to 90%.
    pub s: f64,
    /// 1 (easy) to 10 (hard).
    pub d: f64,
    pub state: State,
    pub reps: u32,
    pub lapses: u32,
    /// Unix seconds of the last answer, for how long it's been.
    pub last: u64,
}

fn init_s(g: f64) -> f64 {
    W[(g as usize) - 1]
}
fn init_d(g: f64) -> f64 {
    (W[4] - (W[5] * (g - 1.0)).exp() + 1.0).clamp(1.0, 10.0)
}
fn recall(elapsed_days: f64, s: f64) -> f64 {
    (1.0 + FACTOR * elapsed_days / s.max(0.01)).powf(DECAY)
}
fn next_d(d: f64, g: f64) -> f64 {
    let d1 = d - W[6] * (g - 3.0) * (10.0 - d) / 9.0;
    (W[7] * init_d(4.0) + (1.0 - W[7]) * d1).clamp(1.0, 10.0)
}
fn recall_s(d: f64, s: f64, r: f64, g: f64) -> f64 {
    let easy = if g == 4.0 { W[16] } else { 1.0 };
    s * (1.0 + W[8].exp() * (11.0 - d) * s.powf(-W[9]) * ((W[10] * (1.0 - r)).exp() - 1.0) * easy)
}
fn forget_s(d: f64, s: f64, r: f64) -> f64 {
    // FSRS-5's cap: no higher than the same-day step down from S.
    let long = W[11] * d.powf(-W[12]) * ((s + 1.0).powf(W[13]) - 1.0) * (W[14] * (1.0 - r)).exp();
    long.min(s / (W[17] * W[18]).exp())
}
/// Answered again within a day (learning steps).
fn short_s(s: f64, g: f64) -> f64 {
    s * (W[17] * (g - 3.0 + W[18])).exp()
}
/// Days until recall falls to [RETENTION] (whole days, 1 to [MAX_DAYS]).
fn interval(s: f64) -> u64 {
    let days = s / FACTOR * (RETENTION.powf(1.0 / DECAY) - 1.0);
    (days.round().clamp(1.0, MAX_DAYS) as u64) * DAY
}

/// The step ladder FSRS replaced (Good steps, in seconds): an old card's
/// step becomes a stability of that many days.
const OLD_STEPS: [u64; 7] = [600, 3_600, 86_400, 3 * 86_400, 7 * 86_400, 21 * 86_400, 60 * 86_400];

impl Card {
    /// FSRS on one answer at `now`.
    pub fn answer(&mut self, g: Grade, now: u64) {
        let r = g.rating();
        match self.state {
            State::New => {
                self.s = init_s(r);
                self.d = init_d(r);
                (self.state, self.due) = match g {
                    Grade::Again => (State::Learning, now + RELEARN),
                    Grade::Good => (State::Learning, now + LEARN_STEP),
                    Grade::Easy => (State::Review, now + interval(self.s)),
                };
            }
            State::Learning | State::Relearning => {
                // Within a day: the short-term step. After a longer gap
                // (watching leaves days between passes) the usual recall
                // and lapse formulas, as FSRS-5 does (review, 2026-11-13).
                let elapsed = now.saturating_sub(self.last) as f64 / DAY as f64;
                let old_d = self.d;
                self.s = if elapsed < 1.0 {
                    short_s(self.s.max(0.01), r)
                } else {
                    let rec = recall(elapsed, self.s);
                    if g == Grade::Again { forget_s(old_d, self.s, rec) } else { recall_s(old_d, self.s, rec, r) }
                };
                self.d = next_d(old_d, r);
                (self.state, self.due) = match g {
                    Grade::Again => (self.state, now + RELEARN),
                    _ => (State::Review, now + interval(self.s)),
                };
            }
            State::Review => {
                let elapsed = now.saturating_sub(self.last) as f64 / DAY as f64;
                let rec = recall(elapsed, self.s);
                // Stability from the difficulty as it was, then the new
                // difficulty (FSRS-5's order; the other way round made a
                // lapse's stability too low and Easy's too long).
                let old_d = self.d;
                if g == Grade::Again {
                    self.s = forget_s(old_d, self.s, rec);
                    self.lapses += 1;
                    self.state = State::Relearning;
                    self.due = now + RELEARN;
                } else {
                    self.s = if elapsed < 1.0 { short_s(self.s, r) } else { recall_s(old_d, self.s, rec, r) };
                    self.due = now + interval(self.s);
                }
                self.d = next_d(old_d, r);
            }
        }
        self.reps += 1;
        self.last = now;
        self.at = now;
    }

    /// A card from its JSON; a card from the old step ladder (no "s")
    /// carried over as it stood.
    pub fn from_json(c: &serde_json::Value) -> Card {
        let due = c["due"].as_u64().unwrap_or(0);
        let at = c["at"].as_u64().unwrap_or(0);
        if let Some(s) = c["s"].as_f64() {
            return Card {
                due,
                at,
                s,
                d: c["d"].as_f64().unwrap_or(5.0),
                state: State::of(c["state"].as_u64().unwrap_or(2)),
                reps: c["reps"].as_u64().unwrap_or(1) as u32,
                lapses: c["lapses"].as_u64().unwrap_or(0) as u32,
                last: c["last"].as_u64().unwrap_or(at),
            };
        }
        let step = c["step"].as_u64().unwrap_or(0) as usize;
        let s = if step == 0 { init_s(1.0) } else { (OLD_STEPS[(step - 1).min(OLD_STEPS.len() - 1)] as f64 / DAY as f64).max(0.1) };
        let state = if step == 0 { State::Learning } else { State::Review };
        Card { due, at, s, d: 5.0, state, reps: step as u32, lapses: 0, last: at }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "due": self.due, "at": self.at, "s": self.s, "d": self.d, "state": self.state.code(),
            "reps": self.reps, "lapses": self.lapses, "last": self.last,
            // For apps still reading the old ladder.
            "step": self.reps.min(255),
        })
    }
}

/// A Learning word met while watching (owner, 2026-11-12: watching feeds
/// the schedule, no review screen): `tapped` when its meaning was looked
/// up (a miss, Again), else it went by on screen untapped (a pass, Good).
/// A pass counts only when the word is due (never answered, or its time
/// has come), so seeing a word often doesn't push it off for months; a
/// miss counts unless the last answer was a miss in the last [RELEARN]
/// (one lookup, not every tap; a lookup right after a pass still counts).
/// The new card, or None: nothing changes.
pub fn watched(card: Option<Card>, tapped: bool, at: u64) -> Option<Card> {
    let mut c = card.unwrap_or_default();
    if tapped {
        let just_missed = c.due == c.last + RELEARN && c.last + RELEARN > at;
        if card.is_some() && just_missed {
            return None;
        }
        c.answer(Grade::Again, at);
    } else {
        if card.is_some() && c.due > at {
            return None;
        }
        c.answer(Grade::Good, at);
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graded(steps: &[(Grade, u64)]) -> Card {
        let mut c = Card::default();
        for (g, at) in steps {
            c.answer(*g, *at);
        }
        c
    }

    #[test]
    fn a_new_word_learns_graduates_then_waits_longer_each_time() {
        let mut c = graded(&[(Grade::Good, 0)]);
        assert_eq!(c.due, 600);
        c.answer(Grade::Good, 600);
        let first = c.due - 600;
        assert!(first >= DAY, "{first}");
        let at = 600 + first;
        c.answer(Grade::Good, at);
        assert!(c.due - at > first);
    }

    #[test]
    fn again_comes_back_in_two_minutes_and_shortens_what_follows() {
        let mut steady = graded(&[(Grade::Good, 0), (Grade::Good, 600)]);
        let mut slipped = steady;
        let due = steady.due;
        slipped.answer(Grade::Again, due);
        assert_eq!(slipped.due, due + RELEARN);
        slipped.answer(Grade::Good, due + RELEARN);
        steady.answer(Grade::Good, due + RELEARN);
        assert!(slipped.due < steady.due, "a lapse shortens the next wait");
    }

    #[test]
    fn stability_uses_the_difficulty_before_the_answer() {
        // A card in review, a lapse: stability from the old difficulty.
        let mut c = Card::default();
        c.answer(Grade::Good, 0);
        c.answer(Grade::Good, LEARN_STEP);
        let (d, s, last) = (c.d, c.s, c.last);
        let now = last + 10 * DAY;
        let rec = recall(10.0, s);
        c.answer(Grade::Again, now);
        assert!((c.s - forget_s(d, s, rec)).abs() < 1e-9);
        assert!((c.d - next_d(d, 1.0)).abs() < 1e-9);
        assert!(c.s <= s / (W[17] * W[18]).exp() + 1e-9);
    }

    #[test]
    fn a_learning_word_seen_again_weeks_later_grows_like_a_review() {
        // Good once, then a pass two weeks later (watching): the recall
        // formula, which rewards remembering after a long gap.
        let mut c = Card::default();
        c.answer(Grade::Good, 0);
        assert_eq!(c.state, State::Learning);
        let (d, s) = (c.d, c.s);
        c.answer(Grade::Good, 14 * DAY);
        assert!((c.s - recall_s(d, s, recall(14.0, s), 3.0)).abs() < 1e-9);
        assert!(c.s > short_s(s, 3.0));
    }

    #[test]
    fn easy_on_a_new_word_skips_ahead() {
        assert!(graded(&[(Grade::Easy, 0)]).due >= 7 * DAY);
    }

    #[test]
    fn a_card_round_trips_through_its_json_and_the_old_ladder_reads() {
        let c = graded(&[(Grade::Good, 0), (Grade::Good, 600)]);
        assert_eq!(Card::from_json(&c.to_json()), c);
        let old = Card::from_json(&serde_json::json!({"due": 5, "step": 2}));
        assert_eq!((old.due, old.state), (5, State::Review));
    }

    #[test]
    fn watching_passes_a_word_only_when_due_and_a_lookup_is_a_miss() {
        // Seen untapped, never answered: Good, a learning step.
        let c = watched(None, false, 1_000).unwrap();
        assert_eq!(c.due, 1_000 + LEARN_STEP);
        // Seen again before it's due: nothing.
        assert_eq!(watched(Some(c), false, 1_300), None);
        // Seen once due: it graduates.
        let c = watched(Some(c), false, 1_600).unwrap();
        assert!(c.due >= 1_600 + DAY);
        // Looked up later: Again, back in two minutes...
        let at = c.due - 3_600;
        let missed = watched(Some(c), true, at).unwrap();
        assert_eq!((missed.due, missed.lapses), (at + RELEARN, 1));
        // ...once: looking it up again a minute on changes nothing.
        assert_eq!(watched(Some(missed), true, at + 60), None);
        // A word never answered, looked up: Again.
        assert_eq!(watched(None, true, 50).unwrap().due, 50 + RELEARN);
        // Gone by, then looked up a moment later: the lookup still counts.
        let passed = watched(None, false, 100).unwrap();
        assert_eq!(watched(Some(passed), true, 101).unwrap().due, 101 + RELEARN);
    }
}
