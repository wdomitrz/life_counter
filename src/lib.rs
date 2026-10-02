// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Life Counter, a 1–12 player life tracker for tabletop games.
//!
//! Everything the app decides — the life arithmetic, the long-press state
//! machine, the change snackbar's running total, the grid layout and which
//! panels are upside down — lives in [`counter`], which knows nothing about the
//! DOM and is unit-tested on the host. [`ui`] is the thin layer that draws it
//! and turns pointer and visibility events into calls on it.
//!
//! The app is a static PWA. There is no server, no binary and no run step:
//! `cargo build` writes `dist/`, and that directory is the publishable site.

mod counter;

#[cfg(target_arch = "wasm32")]
mod ui;

pub use counter::{
    layout, parse_player_count, parse_starting_life, rotation_for, snackbar_label, GameState,
    PlayerLayout, COLORS, LONG_PRESS, LONG_PRESS_MULTIPLIER, MAX_PLAYERS, MAX_STARTING_LIFE,
    MIN_PLAYERS, MIN_STARTING_LIFE, PLAYER_COUNT_OPTIONS, SNACKBAR_HIDE_DELAY_MS,
    SNACKBAR_WINDOW_MS, TAP_STEP,
};
