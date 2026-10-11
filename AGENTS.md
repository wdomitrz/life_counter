# life_counter

Life Counter: a private, browser-side 1–12 player life tracker for tabletop
games. All behaviour is Rust. `cargo build` generates the **static PWA** into
`./dist` — a front-end-only site any file host can serve, with no server and no
runtime dependency on the binary.

AGPL-3.0-only. See `LICENSE`.

The original was a plain JavaScript PWA (`wdomitrz/life_counter`, which this
repository carries the history of). This is a rewrite of it, not a redesign: the
same setup screen, the same tap and press-and-hold behaviour, the same nine
colours, the same layout, and the same upside-down bottom half of the board.

## Build and run

Two builds, because there are two targets. Nothing generated is committed.

```
# 1. the site: compile the crate to wasm and run the bindings generator
rustup target add wasm32-unknown-unknown
cargo build --locked --lib --target wasm32-unknown-unknown --release
~/.cargo/bin/wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/life_counter.wasm

# 2. the rest of the site
touch build.rs
cargo build --release --locked
```

Step 1 writes `dist/app.js` and `dist/app_bg.wasm`; step 2 adds the six files
`build.rs` publishes and derives the service worker's cache version. The order
matters: the version is hashed from the bytes of every other file in `dist/`,
including the wasm, so running step 2 first would pin it to whatever the
previous build left behind. The `touch` is not redundant either — `build.rs`
writes into the source tree rather than `OUT_DIR`, so cargo cannot see that
anything changed and will not re-run it for a second identical invocation,
leaving a half-built site.

There is **no run step and no server**: the build is the whole story. To look at
it, serve the directory (`python3 -m http.server`) and open it.

`wasm-bindgen` is pinned to `=0.2.128` in `Cargo.toml` and must match the CLI
exactly; a mismatched generator produces bindings the runtime will not load,
and the page then fails to start with "Life Counter could not start".

**`dist/` is where a stale service worker bites.** The app registers one, and
`caches.addAll` fails if any listed URL 404s — so a missing file in `dist/`
costs you the whole service worker and every bit of offline support, with
nothing in the build output pointing at it. If the app behaves like an old
build after you change something, unregister it before debugging your Rust:

```js
navigator.serviceWorker.getRegistrations().then(rs => Promise.all(rs.map(r => r.unregister())))
caches.keys().then(ks => Promise.all(ks.map(k => caches.delete(k))))
```

## The site

Eight files, all relative (`./app.js`, `new URL('./', self.location.href)`,
`start_url: "./"`), so one build works from any subdirectory.

| file | from |
|---|---|
| `app.js`, `app_bg.wasm` | `wasm-bindgen`, step 1 |
| `index.html` | `src/ui.html`, byte for byte |
| `icon.svg` | `assets/icon.svg`, byte for byte |
| `icon-192.png`, `icon-512.png` | rasterized from `assets/icon.svg` |
| `service-worker.js` | `src/service-worker.js`, `__VERSION__` substituted |
| `manifest.webmanifest` | assembled in `build.rs` |

`assets/icon.svg` is the author's original Material Symbols `heart_check`,
committed byte for byte, and it is the **source of truth**: `build.rs`
rasterizes the install PNGs from it and `tests/shell.rs` pins its size and
contents. The PNGs are build output and exist only in `dist/`.

The manifest keeps the original's `name` ("Life Counter") and `display`
(`standalone`). The original had no `short_name` — it was removed deliberately —
so this supplies `"Life"`. It declared no theme or background colour either, so
both are taken from the original page's dark gray body: `theme_color` `#111827`
and `background_color` `#1f2937`.

For the same reason the worker **never serves the manifest from its cache**.
The manifest decides what an install *is*; a cached copy would let a fresh
install read a manifest older than the last change to it and re-derive a stale
identity — and the origin's Cache Storage survives an uninstall/reinstall, so
the stale copy would outlive the app it came from. The manifest stays in the
worker's precache list so a broken one still fails the install loudly, but its
fetch handler passes it to the network unconditionally.

## Behaviour

- **Setup**: 1–12 players (default 2) and a starting life (default 20).
- **Board**: one full-height panel per player, each with a big total, a minus
  half and a plus half sized for thumbs.
- **Tap** is ±1. **Press and hold** for 500 ms is ±10, and the long press
  *replaces* the tap rather than adding to it.
- **Snackbar**: shows the accumulated change per player (`+13`, `-7`).
  Changes within 3000 ms of each other sum into one total; it hides 2500 ms
  after the last change.
- **Colours**: nine, cycling — red, blue, green, yellow, purple, pink, cyan,
  orange, lime (the Tailwind `-500` values the original used).
- **Layout**: one column for 1–3 players; two columns by `n / 2` rows for 4 or
  more.
- **Rotation**: with 2 or more players, every player whose index doubled is
  still less than the player count is drawn rotated 180°. The bottom half of the
  board is upside down so the panel nearest each player reads correctly for
  them. This is the original's deliberate quirk and it is preserved exactly,
  odd player counts included.
- **Round reset** is a fixed button in the bottom-right corner.
- **Screen Wake Lock** is requested when a game starts and re-requested on
  `visibilitychange`, because the browser releases it whenever the page is
  hidden.

## Tests

```
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

There are no browser tests and no Node. `src/counter.rs` holds the entire
domain — the life arithmetic, the long-press state machine, the snackbar's
running total, the layout and the rotation rule — as pure functions over an
explicit clock, so the boundaries (499 ms vs 500 ms, 2999 ms vs 3000 ms) are
tested directly and the whole thing runs on the host with no web-sys.

`tests/shell.rs` asserts the invariants of the committed sources: that the page
loads generated bindings rather than a hand-written ABI, that every URL is
relative, that the service worker carries exactly one `__VERSION__` and no
`skipWaiting`, that no build artefact is tracked, that the original JavaScript
app is gone from the tree, and that `assets/icon.svg` is still the author's
original.

## Code map

- `counter.rs`: all of it. Player state, the press/long-press/cancel machine,
  the snackbar window, `layout()`, `rotation_for()`, `color_for()`, the form
  parsers, and `snackbar_label()`.
- `ui.rs`: wasm-only. Builds the panels, turns pointer and visibility events
  into calls on `counter`, drives one interval for the long press and the
  snackbar expiry, and holds the screen wake lock.
- `ui.html`: the static shell. Structure, one inline `<style>`, and the ~6-line
  module script that dynamically imports the bindings.
- `service-worker.js`: cache lifecycle only, `__VERSION__` substituted at build
  time, no `skipWaiting`.
- `build.rs`: writes the six files it owns into `dist/`, rasterizing the icon
  and assembling the manifest.