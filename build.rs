// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Write the static app shell to `dist/` while the crate compiles.
//!
//! Life Counter is a browser application, so publishing it is a file copy, not a
//! program run. Of the eight shell files, `app.js` and `app_bg.wasm` come from
//! `wasm-bindgen` and belong to that step alone. This script owns the other
//! six:
//!
//! * `index.html` is `src/ui.html`, byte for byte.
//! * `service-worker.js` is `src/service-worker.js` with `__VERSION__`
//!   substituted for a cache name derived from the bytes of every *other* file
//!   in `dist/`. Deriving it here rather than per request is what makes the
//!   cache name change exactly when the site does.
//! * `icon.svg` is `assets/icon.svg`, byte for byte: the shell links it
//!   directly and the service worker caches it, so the file that ships is the
//!   committed one.
//! * `icon-192.png` and `icon-512.png` are rasterized from that same SVG,
//!   which stays the source of truth and is never replaced by them.
//! * `manifest.webmanifest` is assembled here rather than committed, so its icon
//!   list cannot drift from what was actually rasterized.
//!
//! Everything lands in `dist/`, which is the whole site and the only copy, so
//! `cargo build --release` leaves a publishable site behind and there is no
//! `cargo run` step and no server.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// The committed icon, rasterized into `dist/` and never committed as a PNG.
const ICON: &str = "assets/icon.svg";

/// Shell files this script copies verbatim from a committed source.
///
/// `icon.svg` is one of them and is deliberately the *same file* the PNGs are
/// rasterized from: the committed icon is what ships, so the page's favicon and
/// the install icons cannot drift apart.
const SHELL: &[(&str, &str)] = &[("index.html", "src/ui.html"), ("icon.svg", ICON)];

/// The install PNGs, as `(name, size)`.
const ICON_SIZES: [(&str, u32); 2] = [("icon-192.png", 192), ("icon-512.png", 512)];

/// The app's own name and colours, from the original `manifest.json`.
///
/// The original declared `"name": "Life Counter"`, `"start_url": "."`,
/// `"display": "standalone"` and a single `any`-size SVG icon, and no
/// `short_name` (it was removed deliberately, in "remove short name"), so
/// `short_name` is invented here as "Life". It also declared neither a theme
/// nor a background colour, so both are taken from the original page: the
/// `bg-gray-800` body of the JavaScript version, resolved to a hex value in
/// `src/ui.html`.
const MANIFEST: (&str, &str, &str, &str) = ("Life Counter", "Life", "#1f2937", "#111827");

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());

    // The site is built only for the host target. This script also runs during
    // `cargo build --lib --target wasm32-unknown-unknown`, and at that moment
    // `dist/app_bg.wasm` and `dist/app.js` are the *output* of that build: they
    // do not exist yet, so writing the site there would fail on the very step
    // that produces them. The host build that follows the wasm-bindgen pass is
    // the one that publishes.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.starts_with("wasm") {
        return;
    }

    // Watch the SOURCE paths, not the output names: cargo compares these against
    // real files, so `src/ui.html` has to be named as the file it is. Watching
    // the destination name watches a file that never changes, and the script
    // then never re-runs.
    for (_, source) in SHELL {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=src/service-worker.js");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");

    let mut built = Vec::with_capacity(SHELL.len() + ICON_SIZES.len() + 2);
    for (name, source) in SHELL {
        let bytes = std::fs::read(root.join(source))
            .unwrap_or_else(|error| panic!("reading {source}: {error}"));
        built.push((*name, bytes));
    }

    for (name, size) in &ICON_SIZES {
        built.push((name, rasterize(&root, *size)));
    }

    built.push(("manifest.webmanifest", manifest().into_bytes()));

    // The worker's own template is hashed too, and is deliberately kept out of
    // `built` so it is hashed exactly once, in template form.
    let template = std::fs::read_to_string(root.join("src/service-worker.js"))
        .unwrap_or_else(|error| panic!("reading src/service-worker.js: {error}"));
    let version = cache_version(&built, &template);
    let worker = template.replace("__VERSION__", &version);
    assert!(
        !worker.contains("__VERSION__"),
        "the service worker still contains the version placeholder"
    );
    built.push(("service-worker.js", worker.into_bytes()));

    write_tree(&root.join("dist"), &built);
}

/// A cache name derived from the bytes of every shell file except the worker.
///
/// It deliberately covers the worker's own source: a change to the caching
/// logic must invalidate the cache too, or clients keep running the old logic
/// against new assets.
fn cache_version(built: &[(&str, Vec<u8>)], worker_template: &str) -> String {
    let mut hasher = DefaultHasher::new();
    for (name, bytes) in built {
        name.hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    worker_template.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Rasterize `assets/icon.svg` to a square PNG of `size` pixels.
///
/// The SVG is the source of truth and is never replaced by this: the PNGs are
/// build output and live only in `dist/`. Rendering at the requested size in
/// one pass keeps the outline sharp, which downscaling a larger render would
/// not.
fn rasterize(root: &Path, size: u32) -> Vec<u8> {
    let path = root.join(ICON);
    let svg =
        std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())
        .unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()));
    let mut pixmap = tiny_skia::Pixmap::new(size, size)
        .unwrap_or_else(|| panic!("a {size}x{size} canvas is not representable"));
    // f32, not f64: `Transform::from_scale` is f32, and `resvg` 0.48 has no f64
    // overload to fall back on.
    let scale = size as f32 / tree.size().width() as f32;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .unwrap_or_else(|error| panic!("encoding the {size}px icon: {error}"))
}

/// The web app manifest, assembled rather than committed.
///
/// Building it here means the icon list is exactly the set of PNGs this script
/// rasterized: a committed manifest can name a file the build no longer makes.
fn manifest() -> String {
    let (name, short_name, background_color, theme_color) = MANIFEST;
    let mut icons = String::new();
    for (index, (name, size)) in ICON_SIZES.iter().enumerate() {
        if index > 0 {
            icons.push(',');
        }
        let _ = write!(
            icons,
            r#"{{"src":"{name}","sizes":"{size}x{size}","type":"image/png","purpose":"any maskable"}}"#
        );
    }
    let mut out = String::new();
    let _ = write!(
        out,
        concat!(
            r#"{{"id":"./","name":"{}","short_name":"{}","start_url":"./","scope":"./","#,
            r#""display":"standalone","background_color":"{}","theme_color":"{}","#,
            r#""icons":[{}]}}"#
        ),
        name, short_name, background_color, theme_color, icons
    );
    // Fail the build rather than publish a manifest the browser will reject.
    serde_json::from_str::<serde_json::Value>(&out)
        .unwrap_or_else(|error| panic!("the assembled manifest is not valid JSON: {error}"));
    out
}

/// Write every file this script owns into `dir`, in place.
///
/// The wasm artefacts are written into `dist/` by the `wasm-bindgen` step, which
/// runs before this one, so the directory must not be replaced wholesale: an
/// earlier version of this script in a sibling project did exactly that and
/// deleted them, leaving a publishable-looking site with no app in it and no
/// error. Only the files named above are written; the rest is untouched.
///
/// Each is written under a scratch name and renamed over its target, so a host
/// serving the directory never observes a half-written file.
fn write_tree(dir: &Path, built: &[(&str, Vec<u8>)]) {
    std::fs::create_dir_all(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));

    for (name, bytes) in built {
        let path = dir.join(name);
        let scratch = dir.join(format!(".{name}.new"));
        std::fs::write(&scratch, bytes)
            .unwrap_or_else(|error| panic!("writing {}: {error}", scratch.display()));
        std::fs::rename(&scratch, &path)
            .unwrap_or_else(|error| panic!("publishing {}: {error}", path.display()));
    }
}
