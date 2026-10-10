// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The rules of the game, with no DOM in sight.
//!
//! Every decision here is a pure function or a small state machine driven by
//! timestamps, which is what makes the app testable without a browser. The
//! clock is a parameter rather than a call to [`std::time::Instant`], so a test
//! can step time forward exactly as far as it wants to and assert on the
//! boundary: 499 ms of press is still a tap, 500 ms is already a long press, a
//! change 2999 ms after the last one joins the running total and one at 3000 ms
//! starts a new one.

/// Lower bound of [`PLAYER_COUNT_OPTIONS`], and what [`parse_player_count`]
/// clamps to below.
pub const MIN_PLAYERS: u8 = 1;
/// Upper bound of [`PLAYER_COUNT_OPTIONS`], and what [`parse_player_count`]
/// clamps to above.
pub const MAX_PLAYERS: u8 = 12;

/// Starting life [`parse_starting_life`] clamps to below.
pub const MIN_STARTING_LIFE: i64 = 1;
/// Starting life [`parse_starting_life`] clamps to above, and the `max` on the
/// setup screen's number field.
pub const MAX_STARTING_LIFE: i64 = 9999;

/// Players the setup screen offers, in order.
///
/// Kept as a range rather than a hand-written list of twelve options because
/// the `<option>` elements are generated from it, so the two cannot disagree.
pub const PLAYER_COUNT_OPTIONS: std::ops::RangeInclusive<u8> = MIN_PLAYERS..=MAX_PLAYERS;

/// A tap on a control area is worth this much life.
pub const TAP_STEP: i64 = 1;

/// A long press on a control area is worth this much, times [`TAP_STEP`].
pub const LONG_PRESS_MULTIPLIER: i64 = 10;

/// How long a press must be held before it becomes a long press, in ms.
pub const LONG_PRESS: i64 = 500;

/// How close together two changes must fall to share one snackbar total, in ms.
pub const SNACKBAR_WINDOW_MS: i64 = 3000;

/// How long the snackbar stays up after the last change, in ms.
pub const SNACKBAR_HIDE_DELAY_MS: i64 = 2500;

/// The nine panel colours, in the order players are given them.
///
/// These are the Tailwind `-500` values the original JavaScript used, written
/// out as hex. The original reached them through a generated `tw.css`, a 13 KiB
/// committed file that carried a whole utility framework to paint eleven
/// panels; the rewrite keeps the colours and drops the framework. Player 9
/// wraps back to the first.
pub const COLORS: [&str; 9] = [
    "#ef4444", // red
    "#3b82f6", // blue
    "#22c55e", // green
    "#eab308", // yellow
    "#a855f7", // purple
    "#ec4899", // pink
    "#06b6d4", // cyan
    "#f97316", // orange
    "#84cc16", // lime
];

/// The grid the game screen is built on, decided once from the player count.
///
/// The original assigned two CSS template properties inline and appended the
/// panels in order. That is the same decision, with the grid track sizes left
/// to CSS: what matters here is how many columns and how many rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerLayout {
    /// Number of grid columns.
    pub columns: usize,
    /// Number of grid rows.
    pub rows: usize,
}

/// Choose the grid for `player_count` players.
///
/// One column of full-height panels for one to three players, and two columns
/// by `player_count / 2` rows for four or more. Two players is the two-column
/// case's degenerate sibling — one row — but the original drew it as two stacked
/// panels, so it stays on the one-column rule.
pub fn layout(player_count: u8) -> PlayerLayout {
    debug_assert!((MIN_PLAYERS..=MAX_PLAYERS).contains(&player_count));
    if player_count <= 3 {
        PlayerLayout {
            columns: 1,
            rows: player_count as usize,
        }
    } else {
        PlayerLayout {
            columns: 2,
            rows: player_count as usize / 2,
        }
    }
}

/// Whether player `index` is drawn upside down, as degrees.
///
/// The board is meant to be read from both sides of a table, so with two or
/// more players the panels in the second half of the grid are rotated 180°.
/// The rule is literally the original's: rotate when the player's index
/// doubled is still less than the player count, which is what "the bottom
/// half" means once the panels are in grid order.
///
/// A single player is never rotated — one person is sitting where they started
/// it — and `index` is taken modulo the count so the rule answers for any
/// index at all rather than trusting the caller.
pub fn rotation_for(index: usize, player_count: u8) -> u16 {
    if player_count >= 2 && 2 * (index % player_count as usize) < player_count as usize {
        180
    } else {
        0
    }
}

/// The colour of player `index`, cycling through [`COLORS`].
pub fn color_for(index: usize) -> &'static str {
    COLORS[index % COLORS.len()]
}

/// Read the player count out of the setup form.
///
/// The value comes from a `<select>` whose options are
/// [`PLAYER_COUNT_OPTIONS`], so anything else can only come from a tampered
/// DOM; clamping keeps one bad value from taking the whole board down, and
/// every count in range is a legitimate one, so there is nothing to reject.
pub fn parse_player_count(raw: &str) -> u8 {
    raw.trim()
        .parse::<i64>()
        .unwrap_or(MIN_PLAYERS as i64)
        .clamp(MIN_PLAYERS as i64, MAX_PLAYERS as i64) as u8
}

/// Read the starting life out of the setup form.
///
/// Anything unparseable or out of range falls back to the default the original
/// pre-filled the field with, so an empty or absurd number still starts a game
/// rather than refusing to.
pub fn parse_starting_life(raw: &str) -> i64 {
    let default = 20;
    let value = raw.trim().parse::<i64>().unwrap_or(default);
    value.clamp(MIN_STARTING_LIFE, MAX_STARTING_LIFE)
}

/// Format a snackbar total: a leading `+` for a gain, and `0` bare.
///
/// The original built this by string concatenation, and the commit "fix 0 total
/// change bug" is why a total that lands back on zero reads `0` and not `+0`.
pub fn snackbar_label(total: i64) -> String {
    if total > 0 {
        format!("+{total}")
    } else {
        format!("{total}")
    }
}

/// Whether a release changed anything, and what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Held for [`LONG_PRESS`] or longer: the long press already applied, so
    /// the tap must not also apply.
    LongPress,
    /// Released early: the tap has been applied.
    Tap,
    /// Nothing was held, so nothing changed.
    Nothing,
}

/// What one control area is doing while a finger is down on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Press {
    /// Held since `started`, with no long press applied yet.
    Holding {
        /// Which half of the panel was pressed, and so the sign.
        sign: i64,
        /// When the press began.
        started: i64,
    },
    /// The long press fired and has been applied; releasing adds nothing.
    Fired,
    Idle,
}

/// One player's running state: their life, and what their snackbar is summing.
///
/// Read through [`GameState::snapshot`]; nothing outside this module reaches
/// into the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Player {
    life: i64,
    snackbar: Snackbar,
    press: Press,
}

/// The snackbar's running total for one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Snackbar {
    /// Sum of the changes that have fallen inside the current window.
    total: i64,
    /// When the last change arrived, or `None` if nothing has changed yet.
    ///
    /// The original seeded this to `0`, which also works only because its clock
    /// epoch is below a game's; `None` states the "no streak yet" case without
    /// depending on the epoch.
    last: Option<i64>,
    /// When the snackbar should be hidden.
    hide_at: i64,
    /// Whether it is on screen.
    shown: bool,
}

impl Snackbar {
    fn new() -> Self {
        Self {
            total: 0,
            last: None,
            hide_at: 0,
            shown: false,
        }
    }

    /// Record `amount` at `now` and return the label to show, if any.
    fn push(&mut self, amount: i64, now: i64) -> String {
        // Changes within the window join the running total; anything later
        // starts a new one. `>=` rather than `>`, because the original tested
        // `now - last < SNACKBAR_WINDOW_MS` and 3000 ms of quiet is the point
        // at which the total stops being about the current streak.
        self.total = match self.last {
            Some(last) if now - last < SNACKBAR_WINDOW_MS => self.total + amount,
            _ => amount,
        };
        self.last = Some(now);
        self.shown = true;
        self.hide_at = now + SNACKBAR_HIDE_DELAY_MS;
        snackbar_label(self.total)
    }

    /// Whether the snackbar is due to be hidden at `now`.
    fn should_hide(&self, now: i64) -> bool {
        self.shown && now >= self.hide_at
    }
}

impl Player {
    fn new(starting_life: i64) -> Self {
        Self {
            life: starting_life,
            snackbar: Snackbar::new(),
            press: Press::Idle,
        }
    }

    /// Start a press at `now`, replacing any press already in progress.
    fn press_at(&mut self, sign: i64, now: i64) {
        self.press = Press::Holding { sign, started: now };
    }

    /// Advance the pending press to `now`.
    ///
    /// Returns whether the long press fired on this call, which is what lets
    /// the caller apply it exactly once.
    fn tick(&mut self, now: i64) -> bool {
        let Press::Holding { sign, started } = self.press else {
            return false;
        };
        if now - started < LONG_PRESS {
            return false;
        }
        self.life += sign * TAP_STEP * LONG_PRESS_MULTIPLIER;
        self.snackbar
            .push(sign * TAP_STEP * LONG_PRESS_MULTIPLIER, now);
        self.press = Press::Fired;
        true
    }

    /// End the press at `now`, applying the tap if the long press has not
    /// already replaced it.
    ///
    /// A release with nothing held applies nothing, rather than the original's
    /// unconditional ±1: the original had no press state to check — its
    /// `mouseup` handler always fired — so a `mouseup` with no `mousedown`, or
    /// a `touchend` the browser cancelled, moved a player's life by one. With a
    /// pointer-based input model that stray release is reachable, and a player's
    /// total has to be something they actually did. This is the one place the
    /// rewrite tightens the original.
    fn release(&mut self, now: i64) -> Release {
        let outcome = match self.press {
            Press::Fired => Release::LongPress,
            Press::Holding { .. } => Release::Tap,
            Press::Idle => Release::Nothing,
        };
        if let Press::Holding { sign, .. } = self.press {
            self.life += sign * TAP_STEP;
            self.snackbar.push(sign * TAP_STEP, now);
        }
        self.press = Press::Idle;
        outcome
    }

    /// Abandon the press without applying anything: the finger left the control
    /// area, or the browser cancelled the pointer.
    fn cancel(&mut self) {
        self.press = Press::Idle;
    }
}

/// One player's life total and snackbar, as the UI reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSnapshot {
    /// The index of the player whose snackbar this is, which is also their
    /// colour slot.
    pub index: usize,
    /// Current life.
    pub life: i64,
    /// The snackbar text, or `None` if it is not currently on screen.
    pub snackbar: Option<String>,
}

/// A game in progress, or the setup screen's defaults waiting to become one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    players: Vec<Player>,
    player_count: u8,
    starting_life: i64,
}

impl GameState {
    /// A board of `player_count` players, each starting on `starting_life`.
    ///
    /// A count of zero would build a grid with no rows and a division by zero
    /// in [`layout`], so it is raised to one player: the smallest game there
    /// is, rather than no game at all.
    pub fn new(player_count: u8, starting_life: i64) -> Self {
        let player_count = player_count.clamp(MIN_PLAYERS, MAX_PLAYERS);
        Self {
            players: (0..player_count as usize)
                .map(|_| Player::new(starting_life))
                .collect(),
            player_count,
            starting_life,
        }
    }

    /// A board built from the setup form's raw field values.
    pub fn from_form(player_count: &str, starting_life: &str) -> Self {
        Self::new(
            parse_player_count(player_count),
            parse_starting_life(starting_life),
        )
    }

    /// The number of players.
    pub fn player_count(&self) -> u8 {
        self.player_count
    }

    /// The life everyone started on.
    pub fn starting_life(&self) -> i64 {
        self.starting_life
    }

    /// The grid this board is laid out on.
    pub fn layout(&self) -> PlayerLayout {
        layout(self.player_count)
    }

    /// Whether player `index` is drawn upside down.
    pub fn rotation(&self, index: usize) -> u16 {
        rotation_for(index, self.player_count)
    }

    /// Player `index`'s panel colour.
    pub fn color(&self, index: usize) -> &'static str {
        color_for(index)
    }

    /// Everything the UI needs to draw one player.
    pub fn snapshot(&self, index: usize) -> Option<PlayerSnapshot> {
        self.players.get(index).map(|player| PlayerSnapshot {
            index,
            life: player.life,
            snackbar: if player.snackbar.shown {
                Some(snackbar_label(player.snackbar.total))
            } else {
                None
            },
        })
    }

    /// Every player, in grid order.
    pub fn snapshots(&self) -> Vec<PlayerSnapshot> {
        (0..self.players.len())
            .map(|index| self.snapshot(index).expect("index is in range"))
            .collect()
    }

    /// A finger went down on player `index`'s control area at `now`.
    ///
    /// `sign` is `1` for the plus half of the panel and `-1` for the minus
    /// half. Starting a press on a panel that is already held does nothing: the
    /// pending long press belongs to the finger that is down.
    pub fn press(&mut self, index: usize, sign: i64, now: i64) {
        if let Some(player) = self.players.get_mut(index) {
            if !matches!(player.press, Press::Holding { .. }) {
                player.press_at(sign, now);
            }
        }
    }

    /// Time passed: run any long press that has now matured, and hide any
    /// snackbar that has been up long enough. Returns the players that changed.
    pub fn tick(&mut self, now: i64) -> Vec<usize> {
        let mut changed = Vec::new();
        for (index, player) in self.players.iter_mut().enumerate() {
            let mut touched = player.tick(now);
            if player.snackbar.should_hide(now) {
                player.snackbar.shown = false;
                touched = true;
            }
            if touched {
                changed.push(index);
            }
        }
        changed
    }

    /// A finger came up at `now`. Returns the players whose life changed.
    ///
    /// A long press replaces the tap rather than adding to it, so a release
    /// after the long press has fired changes nothing.
    pub fn release(&mut self, index: usize, now: i64) -> Vec<usize> {
        let Some(player) = self.players.get_mut(index) else {
            return Vec::new();
        };
        match player.release(now) {
            Release::Tap => vec![index],
            Release::LongPress | Release::Nothing => Vec::new(),
        }
    }

    /// Abandon a pending press: the finger left the control area, or the
    /// browser cancelled the pointer.
    ///
    /// The original cleared the timer on `mouseleave` without applying the tap
    /// or the long press, so nothing happens here either — except that a press
    /// which had already matured into a long press has been applied and stays
    /// applied.
    pub fn cancel(&mut self, index: usize) {
        if let Some(player) = self.players.get_mut(index) {
            player.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tap player `index` at `now`: press and release in the same instant, so
    /// the long press never matures.
    fn tap(game: &mut GameState, index: usize, sign: i64, now: i64) -> Vec<usize> {
        game.press(index, sign, now);
        game.release(index, now)
    }

    /// Hold player `index`'s `sign` side from `now` for `held_ms`, letting the
    /// long press mature, then release. Returns what changed at each step.
    fn hold(
        game: &mut GameState,
        index: usize,
        sign: i64,
        now: i64,
        held_ms: i64,
    ) -> (Vec<usize>, Vec<usize>) {
        game.press(index, sign, now);
        let fired = game.tick(now + held_ms);
        let released = game.release(index, now + held_ms);
        (fired, released)
    }

    fn life(game: &GameState, index: usize) -> i64 {
        game.snapshot(index).expect("a player").life
    }

    fn snackbar(game: &GameState, index: usize) -> Option<String> {
        game.snapshot(index).expect("a player").snackbar
    }

    #[test]
    fn probe_plus_gives_more() {
        let mut g = GameState::new(2, 20);
        g.press(0, 1, 0);
        g.release(0, 10);
        assert_eq!(g.snapshot(0).unwrap().life, 21, "plus tap");
    }

    #[test]
    fn a_tap_moves_one_life() {
        let mut game = GameState::new(2, 20);
        assert_eq!(tap(&mut game, 0, 1, 0), vec![0]);
        assert_eq!(life(&game, 0), 21);

        assert_eq!(tap(&mut game, 0, -1, 10), vec![0]);
        assert_eq!(life(&game, 0), 20);
    }

    #[test]
    fn a_long_press_is_ten_and_replaces_the_tap() {
        let mut game = GameState::new(2, 20);
        // 499 ms is a tap; 500 ms is a long press that replaces it.
        let (fired, released) = hold(&mut game, 0, 1, 0, LONG_PRESS - 1);
        assert_eq!(fired, Vec::<usize>::new());
        assert_eq!(released, vec![0]);
        assert_eq!(life(&game, 0), 21);

        // 500 ms is a long press: ten points, and the release adds nothing.
        let (fired, released) = hold(&mut game, 0, 1, 0, LONG_PRESS);
        assert_eq!(fired, vec![0]);
        assert_eq!(released, Vec::<usize>::new());
        assert_eq!(life(&game, 0), 31);
    }

    #[test]
    fn a_long_press_works_in_both_directions() {
        let mut game = GameState::new(2, 20);
        let _ = hold(&mut game, 0, -1, 0, LONG_PRESS);
        assert_eq!(life(&game, 0), 10);

        let mut game = GameState::new(2, 20);
        let _ = hold(&mut game, 1, -1, 0, LONG_PRESS);
        assert_eq!(life(&game, 1), 10);
        assert_eq!(life(&game, 0), 20);
    }

    #[test]
    fn a_long_press_fires_once_and_only_once() {
        let mut game = GameState::new(2, 20);
        game.press(0, 1, 0);
        assert_eq!(game.tick(LONG_PRESS), vec![0]);
        // Time passing does not keep applying ten every tick.
        assert_eq!(game.tick(LONG_PRESS + 100), Vec::<usize>::new());
        assert_eq!(life(&game, 0), 30);
        // At 10 s the snackbar has also timed out, which is a change worth
        // redrawing — but still not another ten life.
        assert_eq!(game.tick(10_000), vec![0]);
        assert_eq!(life(&game, 0), 30);
    }

    #[test]
    fn a_cancelled_press_applies_nothing() {
        let mut game = GameState::new(2, 20);
        game.press(0, 1, 0);
        assert_eq!(game.tick(LONG_PRESS - 1), Vec::<usize>::new());
        game.cancel(0);
        // Leaving the area before the long press matured: no life change, and
        // the release that follows is not a tap either.
        assert_eq!(game.release(0, LONG_PRESS), Vec::<usize>::new());
        assert_eq!(life(&game, 0), 20);
        assert_eq!(snackbar(&game, 0), None);
    }

    #[test]
    fn a_cancelled_press_keeps_an_already_applied_long_press() {
        let mut game = GameState::new(2, 20);
        game.press(0, 1, 0);
        assert_eq!(game.tick(LONG_PRESS), vec![0]);
        game.cancel(0);
        assert_eq!(life(&game, 0), 30);
    }

    #[test]
    fn a_second_press_does_not_disturb_the_one_in_progress() {
        let mut game = GameState::new(2, 20);
        game.press(0, -1, 0);
        game.press(0, 1, 100);
        // The first press still owns the pending long press, and its sign.
        assert_eq!(game.tick(LONG_PRESS), vec![0]);
        assert_eq!(life(&game, 0), 10);
    }

    #[test]
    fn players_are_independent() {
        let mut game = GameState::new(3, 20);
        assert_eq!(tap(&mut game, 1, -1, 0), vec![1]);
        assert_eq!(
            game.snapshots().iter().map(|p| p.life).collect::<Vec<_>>(),
            vec![20, 19, 20]
        );
    }

    #[test]
    fn a_press_on_a_player_that_does_not_exist_is_ignored() {
        let mut game = GameState::new(2, 20);
        game.press(9, 1, 0);
        assert_eq!(game.tick(LONG_PRESS), Vec::<usize>::new());
        assert_eq!(game.release(9, LONG_PRESS), Vec::<usize>::new());
        game.cancel(9);
        assert_eq!(game.snapshots().len(), 2);
        assert_eq!(game.snapshot(9), None);
    }

    #[test]
    fn a_release_with_nothing_held_does_nothing() {
        // A release requires a press; see `Player::release` for why.
        let mut game = GameState::new(2, 20);
        assert_eq!(game.release(0, 0), Vec::<usize>::new());
        assert_eq!(life(&game, 0), 20);
        assert_eq!(snackbar(&game, 0), None);

        // A press and release still move the total.
        assert_eq!(tap(&mut game, 0, 1, 100), vec![0]);
        assert_eq!(life(&game, 0), 21);

        // A second release with nothing held again does nothing.
        assert_eq!(game.release(0, 200), Vec::<usize>::new());
        assert_eq!(life(&game, 0), 21);
    }

    #[test]
    fn a_press_makes_the_press_movable_again_after_a_release() {
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, -1, 0);
        tap(&mut game, 0, -1, 100);
        assert_eq!(life(&game, 0), 18);
    }

    #[test]
    fn the_snackbar_sums_changes_inside_the_window() {
        let mut game = GameState::new(2, 20);
        assert_eq!(tap(&mut game, 0, 1, 1_000), vec![0]);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("+1"));

        assert_eq!(tap(&mut game, 0, 1, 2_000), vec![0]);
        assert_eq!(tap(&mut game, 0, 1, 3_500), vec![0]);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("+3"));
    }

    #[test]
    fn a_change_after_the_window_starts_a_new_total() {
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, 1, 0);
        // Exactly 3000 ms later the window has closed, so this is a new total
        // rather than "+2".
        tap(&mut game, 0, -1, SNACKBAR_WINDOW_MS);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("-1"));

        // One millisecond earlier is still inside it.
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, 1, 0);
        tap(&mut game, 0, -1, SNACKBAR_WINDOW_MS - 1);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("0"));
    }

    #[test]
    fn the_first_change_of_a_game_does_not_join_an_empty_total() {
        // The original seeded `lastChangeTime` to 0; at any realistic clock a
        // first change begins a new streak. This pins that it still does.
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, -1, 1);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("-1"));
    }

    #[test]
    fn a_total_that_returns_to_zero_reads_zero_and_not_plus_zero() {
        assert_eq!(snackbar_label(0), "0");
        assert_eq!(snackbar_label(13), "+13");
        assert_eq!(snackbar_label(-7), "-7");

        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, 1, 0);
        tap(&mut game, 0, -1, 10);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("0"));
    }

    #[test]
    fn each_player_keeps_their_own_snackbar() {
        let mut game = GameState::new(3, 20);
        tap(&mut game, 0, 1, 0);
        tap(&mut game, 2, -1, 0);
        let all = game.snapshots();
        assert_eq!(all[0].snackbar.as_deref(), Some("+1"));
        assert_eq!(all[1].snackbar, None);
        assert_eq!(all[2].snackbar.as_deref(), Some("-1"));
    }

    #[test]
    fn a_long_press_shows_ten_in_the_snackbar() {
        let mut game = GameState::new(2, 20);
        game.press(0, 1, 0);
        game.tick(LONG_PRESS);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("+10"));
        // The tap it replaced must not add one to the total either.
        game.release(0, LONG_PRESS + 50);
        assert_eq!(snackbar(&game, 0).as_deref(), Some("+10"));
    }

    #[test]
    fn the_snackbar_hides_after_the_delay_and_the_hide_is_reported_once() {
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, 1, 0);
        assert_eq!(game.tick(SNACKBAR_HIDE_DELAY_MS - 1), Vec::<usize>::new());
        assert_eq!(game.tick(SNACKBAR_HIDE_DELAY_MS), vec![0]);
        assert_eq!(snackbar(&game, 0), None);
        assert_eq!(game.tick(SNACKBAR_HIDE_DELAY_MS + 1), Vec::<usize>::new());
    }

    #[test]
    fn a_change_resets_the_hide_delay() {
        let mut game = GameState::new(2, 20);
        tap(&mut game, 0, 1, 0);
        tap(&mut game, 0, 1, 2_000);
        // 2500 ms after the *first* change, but the second moved the deadline
        // to 4500.
        assert_eq!(game.tick(2_500), Vec::<usize>::new());
        assert_eq!(game.tick(2_000 + SNACKBAR_HIDE_DELAY_MS), vec![0]);
    }

    #[test]
    fn hiding_one_players_snackbar_leaves_the_others_alone() {
        let mut game = GameState::new(3, 20);
        tap(&mut game, 0, 1, 0);
        tap(&mut game, 1, -1, 0);
        // Player 2's first change is timed so it is still on screen when
        // players 0 and 1's snackbars time out.
        tap(&mut game, 2, 1, SNACKBAR_HIDE_DELAY_MS);
        // Players 0 and 1 became visible at 0 and time out at 2500; player 2
        // became visible at 2500 and does not.
        assert_eq!(game.tick(SNACKBAR_HIDE_DELAY_MS), vec![0, 1]);
        assert_eq!(snackbar(&game, 0), None);
        assert_eq!(snackbar(&game, 1), None);
        assert_eq!(snackbar(&game, 2).as_deref(), Some("+1"));
    }

    #[test]
    fn one_to_three_players_are_one_column() {
        assert_eq!(
            layout(1),
            PlayerLayout {
                columns: 1,
                rows: 1
            }
        );
        assert_eq!(
            layout(2),
            PlayerLayout {
                columns: 1,
                rows: 2
            }
        );
        assert_eq!(
            layout(3),
            PlayerLayout {
                columns: 1,
                rows: 3
            }
        );
    }

    #[test]
    fn four_or_more_players_are_two_columns_by_half_as_many_rows() {
        for count in 4..=MAX_PLAYERS {
            let grid = layout(count);
            assert_eq!(grid.columns, 2, "{count} players");
            assert_eq!(grid.rows, count as usize / 2, "{count} players");
        }
        // Every count the setup screen offers except the odd ones over three
        // fills its grid exactly. Five and seven do not: the original divides
        // by two and rounds down, so an odd board has one panel fewer than its
        // players and the last one wraps onto a row of its own below the grid.
        // That is the author's behaviour and it is kept, not "fixed".
        assert_eq!(layout(5).columns * layout(5).rows, 4);
        assert_eq!(layout(7).columns * layout(7).rows, 6);
    }

    #[test]
    fn a_lone_player_is_never_rotated() {
        assert_eq!(rotation_for(0, 1), 0);
    }

    #[test]
    fn the_bottom_half_of_the_board_is_rotated_for_two_or_more_players() {
        // Two players: the second panel is the one nearest the other player,
        // so index 0 — the top of the screen — is the upside-down one.
        assert_eq!(rotation_for(0, 2), 180);
        assert_eq!(rotation_for(1, 2), 0);
        // Five: `2 * index < 5`, so 0, 1 and 2 rotate and 3 and 4 do not.
        let rotated: Vec<u16> = (0..5).map(|index| rotation_for(index, 5)).collect();
        assert_eq!(rotated, vec![180, 180, 180, 0, 0]);
        // Twelve: exactly the first six.
        let rotated: Vec<u16> = (0..12).map(|index| rotation_for(index, 12)).collect();
        assert_eq!(rotated[..6], [180; 6]);
        assert_eq!(rotated[6..], [0; 6]);
    }

    #[test]
    fn the_rotation_rule_wraps_any_index() {
        assert_eq!(rotation_for(3, 3), rotation_for(0, 3));
        assert_eq!(rotation_for(13, 2), rotation_for(1, 2));
    }

    #[test]
    fn colours_cycle_through_nine_slots() {
        let all: Vec<&str> = (0..9).map(color_for).collect();
        assert_eq!(all.len(), 9);
        assert!(all.iter().all(|c| c.starts_with('#') && c.len() == 7));
        assert_eq!(color_for(9), color_for(0));
        assert_eq!(color_for(11), color_for(2));
    }

    #[test]
    fn the_player_count_is_read_from_the_form_and_clamped() {
        assert_eq!(parse_player_count("2"), 2);
        assert_eq!(parse_player_count("12"), 12);
        assert_eq!(parse_player_count(" 4 "), 4);
        assert_eq!(parse_player_count("0"), MIN_PLAYERS);
        assert_eq!(parse_player_count("99"), MAX_PLAYERS);
        assert_eq!(parse_player_count(""), MIN_PLAYERS);
        assert_eq!(parse_player_count("two"), MIN_PLAYERS);
        assert_eq!(parse_player_count("-5"), MIN_PLAYERS);
    }

    #[test]
    fn starting_life_is_read_from_the_form_and_clamped() {
        assert_eq!(parse_starting_life("20"), 20);
        assert_eq!(parse_starting_life(" 1 "), 1);
        assert_eq!(parse_starting_life("9999"), MAX_STARTING_LIFE);
        assert_eq!(parse_starting_life("10000"), MAX_STARTING_LIFE);
        assert_eq!(parse_starting_life("0"), MIN_STARTING_LIFE);
        assert_eq!(parse_starting_life("-20"), MIN_STARTING_LIFE);
        assert_eq!(parse_starting_life(""), 20);
        assert_eq!(parse_starting_life("lots"), 20);
    }

    #[test]
    fn a_game_can_be_built_from_the_raw_form_fields() {
        let game = GameState::from_form("3", "40");
        assert_eq!(game.player_count(), 3);
        assert_eq!(game.starting_life(), 40);
        assert_eq!(game.snapshots().len(), 3);
        assert_eq!(game.snapshots()[0].life, 40);
        assert_eq!(
            game.layout(),
            PlayerLayout {
                columns: 1,
                rows: 3
            }
        );
    }

    #[test]
    fn a_new_game_starts_everyone_on_the_starting_life_and_nothing_shown() {
        let game = GameState::new(12, 30);
        assert_eq!(game.player_count(), 12);
        for player in game.snapshots() {
            assert_eq!(player.life, 30);
            assert_eq!(player.snackbar, None);
        }
    }

    #[test]
    fn a_zero_player_game_is_raised_to_one() {
        // layout(0) would be a grid with no rows, so the count is clamped.
        let game = GameState::new(0, 20);
        assert_eq!(game.player_count(), 1);
        assert_eq!(
            game.layout(),
            PlayerLayout {
                columns: 1,
                rows: 1
            }
        );
    }

    #[test]
    fn the_game_exposes_its_layout_rotation_and_colour_per_player() {
        let game = GameState::from_form("5", "20");
        assert_eq!(
            game.layout(),
            PlayerLayout {
                columns: 2,
                rows: 2
            }
        );
        assert_eq!(game.rotation(0), 180);
        assert_eq!(game.rotation(4), 0);
        assert_eq!(game.color(0), COLORS[0]);
        assert_eq!(game.color(4), COLORS[4]);
    }
}
