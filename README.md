# Life Counter

A 1–12 player life tracker for tabletop games, as a static PWA. All behaviour
is Rust compiled to WebAssembly; the page is a shell and the board is built at
runtime.

Tap a panel's left or right half for ±1, or press and hold for 500 ms for ±10.
The bottom half of the board is drawn upside down, so a table of players can
each read their own number. It works offline after one visit and asks the
browser to keep the screen awake while a game is running.

## Building

```
rustup target add wasm32-unknown-unknown
cargo build --locked --lib --target wasm32-unknown-unknown --release
~/.cargo/bin/wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/life_counter.wasm
touch build.rs && cargo build --release --locked
```

That writes the whole site to `dist/`. Serve that directory with any file host
and it is done — there is no server and nothing to run.

## Testing

```
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

No browser tests and no Node; the game's rules are ordinary Rust and are tested
on the host. See `AGENTS.md` for the architecture, the design decisions the
rewrite inherited from the original JavaScript app, and the service-worker trap
worth knowing about before you debug a stale build.

AGPL-3.0-only. Original app by Witalis Domitrz.