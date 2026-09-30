// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Invariants of the committed sources, and of what is and is not committed.
//!
//! Everything here reads committed files. `dist/` is build output and is
//! gitignored, so the release gate — which exports the candidate tree — never
//! has it, and cannot build it either: that needs the wasm target and a pinned
//! `wasm-bindgen` CLI. A test asserting on `dist/` would therefore run only in
//! a developer's checkout, which is exactly where it is least likely to catch
//! anything, so those assertions are gone rather than skipped.
//!
//! What covers the built output is running the two build steps, in the order
//! `AGENTS.md` gives them, and inspecting the eight files that come out. A test
//! on a leftover directory cannot do that job.
//!
//! What *is* worth pinning here is the set of rules that let the rewrite rot:
//! a second, dead JavaScript PWA creeping back into the tree, a build artefact
//! becoming tracked, or the page growing a hand-written wasm ABI. All three have
//! happened to this project or its siblings, and none of them is caught by the
//! domain unit tests.

use std::path::PathBuf;

/// The repository root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The service worker template, as committed.
fn worker_template() -> String {
    let path = root().join("src/service-worker.js");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The page shell, as committed.
///
/// `build.rs` copies this into `dist/index.html` byte for byte, so asserting on
/// it asserts on exactly what gets published.
fn shell() -> String {
    let path = root().join("src/ui.html");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Tracked file names, or `None` outside a checkout.
///
/// The release gate exports the candidate as a bare directory with no `.git`,
/// so there is no index to ask. Callers decide what that means.
fn tracked_files() -> Option<String> {
    let root = root();
    let inside = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&root)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !inside {
        return None;
    }
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The app is wasm, loaded through generated bindings. A hand-written ABI would
/// mean the app's logic had moved back out of the library the tests cover.
#[test]
fn the_page_loads_generated_bindings_not_a_manual_wasm_abi() {
    let page = shell();
    assert!(
        page.contains("<!DOCTYPE html>"),
        "the shell must be a document"
    );
    assert!(page.contains("<script type=\"module\">"), "a module script");
    assert!(
        page.contains("import('./app.js')"),
        "the page must load the generated bindings"
    );
    assert_eq!(
        page.matches("<script").count(),
        1,
        "exactly one script tag:\n{page}"
    );
    for obsolete in [
        "instantiateStreaming",
        "alloc_buf",
        "wasm.exports",
        "fetch(",
        "app.js\"", // a <script src>, i.e. a second, non-module script
    ] {
        assert!(!page.contains(obsolete), "{obsolete} in the static shell");
    }
}

/// The shell carries the structure and nothing else: Rust builds the board.
/// An empty `<option>` list is the point — the twelve player counts are
/// generated in Rust from the same constant the layout uses.
#[test]
fn the_shell_has_the_structure_and_rust_fills_in_the_board() {
    let page = shell();
    for id in [
        "setup-screen",
        "player-count",
        "life-points",
        "start-game",
        "game-screen",
        "reset-button",
        "setup-status",
        "setup-error",
    ] {
        assert!(page.contains(&format!("id=\"{id}\"")), "missing #{id}");
    }
    assert!(
        page.contains("<select id=\"player-count\" name=\"player-count\"></select>"),
        "the player-count options are generated in Rust, not written into the shell"
    );
    assert!(
        page.contains("<label for=\"player-count\">"),
        "controls must be labelled"
    );
    assert!(
        page.contains("role=\"status\"") && page.contains("aria-live=\"polite\""),
        "the status line must announce changes"
    );
    // The board itself is built in Rust: the shell has an empty container and
    // no player panel. `.player-container` is expected to appear once, in the
    // <style> block that gives it its layout.
    assert!(
        page.contains("<div id=\"game-screen\" hidden></div>"),
        "the board container is empty; Rust builds the panels into it"
    );
    assert!(
        !page.contains("data-player"),
        "no player panel may be written into the shell"
    );
}

/// The site is mounted under an arbitrary prefix, so every URL in it is
/// relative. One build, any subdirectory.
#[test]
fn the_shell_is_mountable_anywhere() {
    let page = shell();
    assert!(
        !page.contains("http://") && !page.contains("https://"),
        "an absolute URL would break the site outside its own origin"
    );
    assert!(
        page.contains("./app.js"),
        "bindings must be referenced relatively"
    );
    assert!(
        page.contains("./manifest.webmanifest"),
        "the page must register a manifest"
    );
    assert!(
        page.contains("rel=\"icon\" href=\"./icon.svg\""),
        "the committed SVG is the icon the page links"
    );
    // No external stylesheet: the original's `style.css` + `tw.css` are gone and
    // their replacement is one inline `<style>`.
    assert_eq!(
        page.matches("<link rel=\"stylesheet\"").count(),
        0,
        "the shell has one inline <style> and no external stylesheet"
    );
    assert_eq!(page.matches("<style").count(), 1, "exactly one style block");
}

/// The service worker is a committed template with exactly one placeholder, and
/// `build.rs` substitutes it. A template with no placeholder would mean the
/// cache never invalidates; a second one would mean the substitution is not the
/// only edit. And there is no `skipWaiting`: an update must not swap the wasm
/// under a live tab.
#[test]
fn the_service_worker_template_has_exactly_one_placeholder_and_no_skip_waiting() {
    let worker = std::fs::read_to_string(root().join("src/service-worker.js"))
        .expect("the committed template");
    assert_eq!(
        worker.matches("__VERSION__").count(),
        1,
        "the template must carry exactly one version placeholder"
    );
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its cache from its own location"
    );
    // Catches a call, not the comment explaining why there is not one.
    let calls: Vec<&str> = worker
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| line.contains("skipWaiting"))
        .collect();
    assert!(
        calls.is_empty(),
        "no skipWaiting: an update must not swap the wasm under a live tab, but found {calls:?}"
    );
    for file in ["app.js", "app_bg.wasm", "icon.svg", "index.html"] {
        assert!(
            worker.contains(file),
            "the worker must cache {file}; it is part of the site"
        );
    }
}

/// Every file the service worker precaches must be one `build.rs` publishes.
///
/// This is derived from the worker template rather than from a list written out
/// here, because a second copy of the truth is exactly how the two drift apart.
/// `caches.addAll` is all-or-nothing: one 404 in `ASSETS` rejects the entire
/// install, so a file the worker names but the build does not produce costs the
/// whole service worker and every bit of offline support — while the site
/// itself looks perfect and nothing in the build output says so. That is not
/// hypothetical: a sibling repo shipped a seven-file `dist/` whose page and
/// worker both referred to an eighth, unpublished `icon.svg`.
#[test]
fn every_precached_file_is_published_into_the_site() {
    let worker = std::fs::read_to_string(root().join("src/service-worker.js"))
        .expect("the committed template");
    let list = worker
        .lines()
        .find_map(|line| line.trim().strip_prefix("const ASSETS = ").map(|_| line))
        .expect("the worker must declare ASSETS");
    let list = list
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(body, _)| body)
        .expect("ASSETS must be an array literal");
    let assets: Vec<&str> = list
        .split(',')
        .map(|entry| entry.trim().trim_matches(['\'', '"', '`']))
        .filter(|entry| !entry.is_empty())
        .collect();
    assert!(
        assets.len() >= 8,
        "the site's whole shell is precached, got {assets:?}"
    );

    // `./` is the directory itself; everything else is a file that must reach
    // `dist/`. Most come from `build.rs`; `app.js` and `app_bg.wasm` come from
    // the `wasm-bindgen` step, which owns exactly those two and which
    // `build.rs` deliberately does not touch.
    const WASM_STEP: [&str; 2] = ["app.js", "app_bg.wasm"];
    let build = std::fs::read_to_string(root().join("build.rs")).expect("the build script");
    for asset in &assets {
        let name = asset.trim_start_matches("./");
        if name.is_empty() {
            continue; // `./`
        }
        if WASM_STEP.contains(&name) {
            continue;
        }
        // The icon PNGs are rasterized from a size rather than named in `SHELL`,
        // and the worker is written under its own name.
        let published = build.contains(&format!("\"{name}\""))
            || (name.starts_with("icon-")
                && name.ends_with(".png")
                && build.contains("ICON_SIZES"))
            || (name == "service-worker.js" && build.contains("service-worker.js"));
        assert!(
            published,
            "the service worker precaches {name}, but nothing publishes it into \\
             dist/. caches.addAll() rejects the whole install when one entry \\
             404s, so the app loses the service worker and all offline support \\
             over a file nobody builds"
        );
    }

    // And the other way round, from `build.rs`'s own `SHELL` table rather than
    // from a list written out here: anything the build publishes has to be
    // precached, or a cold offline start 404s on it. A hardcoded second list is
    // what let this gap appear in the first place.
    //
    // This reads the table by splitting on the `SHELL:` declaration, so it must
    // stay a single line of `("name", source)` pairs.
    let shell_table = build
        .split_once("const SHELL:")
        .map(|(_, rest)| rest)
        .expect("build.rs must declare a SHELL table");
    let published: Vec<&str> = shell_table
        .split("(\"")
        .skip(1)
        .take_while(|_| true)
        .filter_map(|entry| entry.split_once('"'))
        .map(|(name, _)| name)
        .take_while(|name| {
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
        })
        .collect();
    assert!(
        published.len() >= 2,
        "expected build.rs to name the files it copies, got {published:?}"
    );
    for file in published {
        assert!(
            assets.contains(&file),
            "the site ships {file}, but the service worker does not precache it, \\
             so a cold offline start would 404 on it"
        );
    }
    // The service worker is the one exception: a browser fetches it separately,
    // and precaching itself buys nothing.
    assert!(
        !assets.contains(&"service-worker.js"),
        "the worker must not precache itself"
    );
}

/// Nothing the user can see may name the implementation.
///
/// "Rust is ready. Tap Start Game." sat in the status line of the first
/// released build. It told a person nothing about a life counter, and existed
/// only to prove the module had booted. The technology is not the user's
/// business; a message that has to explain itself in implementation terms is a
/// message aimed at the wrong reader.
///
/// The check is deliberately narrow. It looks at string *literals* in the shell
/// and in the code that writes to the DOM, because those are what a person
/// ends up reading. Comments, docs and the crate manifest should go on saying
/// "Rust" in as much detail as they like — that documentation is for whoever
/// maintains the crate, and stripping it would be its own kind of damage. The
/// line is simply that implementation vocabulary may live in code and docs,
/// never in the interface.
#[test]
fn no_user_visible_text_names_the_implementation() {
    const FORBIDDEN: [&str; 6] = [
        "rust",
        "webassembly",
        "wasm",
        "javascript",
        "bindings",
        "compile",
    ];

    let mut checked = 0usize;

    // The shell, including the loader-failure message its inline script writes.
    //
    // That script is a single line that begins with `import(...)` and then
    // contains the prose a user reads when the app fails to load — which is
    // exactly when the wording has to be clearest. So the line is still
    // checked; only the `import('./app.js')` expression itself is exempt, by
    // trimming it off the front before the scan rather than by skipping the
    // whole line.
    let page = shell();
    for (number, line) in page.lines().enumerate() {
        let prose = match line.find("import(") {
            Some(index) => {
                let start = line[index..]
                    .find(")")
                    .map_or(line.len(), |end| index + end + 1);
                format!("{}{}", &line[..index], &line[start..])
            }
            None => line.to_string(),
        };
        let lower = prose.to_lowercase();
        for word in FORBIDDEN {
            assert!(
                !lower.contains(word),
                "src/ui.html:{} says {word:?} in text a user can see: {line:?}",
                number + 1
            );
        }
        checked += 1;
    }

    // Everything written into the DOM from Rust: the status line, the error
    // line, and the player's own numbers.
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("the UI module");
    for (number, line) in ui.lines().enumerate() {
        let trimmed = line.trim_start();
        // Comments are documentation, not interface.
        if trimmed.starts_with("//") {
            continue;
        }
        let lower = line.to_lowercase();
        // Only string literals can reach the DOM, so only they are checked.
        let mut rest = line;
        while let Some(start) = rest.find('"') {
            rest = &rest[start + 1..];
            let Some(end) = rest.find('"') else {
                break;
            };
            let literal = &rest[..end];
            rest = &rest[end + 1..];
            let lower_literal = literal.to_lowercase();
            for word in FORBIDDEN {
                assert!(
                    !lower_literal.contains(word),
                    "src/ui.rs:{} has a string literal containing {word:?}, and it is \
                     written into the page: {literal:?}",
                    number + 1
                );
            }
            checked += 1;
        }
        let _ = lower;
    }

    assert!(
        checked >= 6,
        "the scan found almost nothing to check ({checked} strings); it is not \
         looking at the right places"
    );

    // And the meta description and manifest, which is what a search engine and
    // a share sheet show. The manifest is assembled in `build.rs`.
    let description = page
        .split("name=\"description\" content=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the shell must carry a meta description");
    for word in FORBIDDEN {
        assert!(
            !description.to_lowercase().contains(word),
            "the meta description says {word:?}: {description:?}"
        );
    }
    let build = std::fs::read_to_string(root().join("build.rs")).expect("the build script");
    let manifest_names: Vec<&str> = build
        .match_indices('"')
        .map(|(index, _)| &build[index + 1..])
        .take_while(|rest| !rest.contains('"'))
        .map(|rest| &rest[..rest.find('"').unwrap_or(0)])
        .collect();
    for name in manifest_names {
        for word in FORBIDDEN {
            assert!(
                !name.to_lowercase().contains(word),
                "the manifest carries {name:?}, which says {word:?}"
            );
        }
    }
}

/// The manifest is assembled by `build.rs`, so there is no committed copy to
/// assert on — but the icon list it writes is derived from what was rasterized,
/// and `tests/shell.rs` checks the sources that decide it: both PNG sizes are
/// named, and the original's own name and display mode survive.
#[test]
fn the_manifest_constants_are_the_original_apps() {
    let build = std::fs::read_to_string(root().join("build.rs")).expect("the build script");
    assert!(
        build.contains("\"Life Counter\""),
        "the manifest keeps the original's name"
    );
    assert!(
        build.contains("\"Life\""),
        "the original had no short_name, so one is supplied"
    );
    assert!(
        build.contains("\"standalone\""),
        "display is the original's standalone"
    );
    for size in [192, 512] {
        assert!(
            build.contains(&format!("\"icon-{size}.png\"")),
            "the manifest must declare icon-{size}.png"
        );
    }
    assert!(
        build.contains("any maskable"),
        "both icons must be maskable"
    );
}

/// Nothing generated may be tracked — not the wasm, not the bindings, not the
/// site, not a rasterized icon. This is the test that would catch them being
/// committed.
#[test]
fn no_build_artifact_is_committed() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout: the gate's exported tree
    };
    for artefact in [
        "dist/index.html",
        "dist/app.js",
        "dist/app_bg.wasm",
        "assets/icon-192.png",
        "assets/icon-512.png",
        "assets/app.js",
        "assets/app_bg.wasm",
    ] {
        assert!(
            !tracked.lines().any(|line| line == artefact),
            "{artefact} is tracked; generated artefacts must never be committed"
        );
    }
    // The PNGs are derived at build time. If one is sitting in the source tree
    // it is a committed build artefact with a shorter name than `dist/`.
    for png in ["assets/icon-192.png", "assets/icon-512.png"] {
        assert!(
            !root().join(png).exists(),
            "{png} is build output and must exist only in dist/"
        );
    }
}

/// `dist/` has to be ignored, or a build would leave the next commit dirty.
#[test]
fn dist_is_ignored() {
    if tracked_files().is_none() {
        return; // not a checkout
    }
    let root = root();
    let ignored = std::process::Command::new("git")
        .args(["check-ignore", "-q", "dist/"])
        .current_dir(&root)
        .status()
        .expect("git check-ignore")
        .success();
    assert!(ignored, "dist/ must be in .gitignore");
}

/// The rewrite removes the original JavaScript app. Leaving the files in the
/// tree would leave the repository holding a second, dead PWA — one that no
/// build produces and no test covers, and one a reader would reasonably assume
/// is the app.
#[test]
fn the_original_javascript_app_is_gone() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout
    };
    for dead in [
        "app.js",
        "style.css",
        "sw.js",
        "manifest.json",
        "tw.css",
        "index.html",
        "icon.svg", // moved to assets/ by the rewrite
    ] {
        assert!(
            !tracked.lines().any(|line| line == dead),
            "{dead} is still tracked. The shipped app is Rust plus a static shell; \
             the original JavaScript sources belong in history, not in the tree"
        );
        assert!(
            !root().join(dead).exists(),
            "{dead} is still in the working tree"
        );
    }
}

/// The worker only ever answers for a URL inside its own app's directory.
///
/// This is the guard that stops the app from taking over the pages it shares an
/// origin with. A service worker registered for a scope is consulted for every
/// URL under that scope, and this app is served from the same origin as pages
/// that are not it at all — so "the scope is small" is a promise, and this test
/// is what keeps it one.
#[test]
fn the_worker_never_answers_outside_its_own_directory() {
    let worker = worker_template();
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its own directory from its location"
    );
    // The guard is a prefix test against that directory, on the request URL,
    // applied before the allowlist decides anything.
    assert!(
        worker.contains("IS_OWN(url)"),
        "the fetch handler must check the request is inside this app's directory; \
         without it a mis-scoped registration serves whatever it cached"
    );
    assert!(
        worker.contains("const IS_OWN = url => url.startsWith(ROOT.href)"),
        "the directory guard must be a prefix test against the worker's own root"
    );
}

/// The page states the worker's scope instead of inheriting it, and cleans up a
/// wider registration left behind by an earlier version.
///
/// A registration outlives the page that created it, and nothing short of an
/// explicit `unregister` takes one away. So the second half is what makes this
/// recoverable without the user clearing their browser: a stale registration is
/// not fixed by a reload, and the newer worker cannot take control of a scope it
/// does not own.
#[test]
fn the_page_states_the_scope_and_releases_a_wider_one() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    assert!(
        ui.contains("register_with_options"),
        "the worker must be registered with an explicit scope; left to default, \
         the scope is whatever directory the registering page sits in"
    );
    assert!(
        ui.contains("RegistrationOptions::new()") && ui.contains("set_scope(SCOPE)"),
        "the scope has to be actually stated, not merely a named constant"
    );
    assert!(
        ui.contains("get_registrations") && ui.contains("unregister"),
        "a stale wider registration survives a reload, a version bump and a \
         reinstall; only an explicit unregister clears it"
    );
}

/// A worker's script is compared by suffix, not by `trim_end_matches`.
///
/// `trim_end_matches` strips a *set of characters*, so a directory whose name
/// ends in those letters is silently treated as ours — and a registration
/// belonging to a sibling app would be torn down. This is a regression test for
/// a real bug in the first version of this code.
#[test]
fn the_script_comparison_strips_a_suffix_rather_than_a_character_set() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    // The prose in this file names the method to explain why it is not used, so
    // the assertion is about code: a call, not the word.
    let calls: Vec<&str> = ui
        .lines()
        .filter(|line| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("trim_end_matches(")
        })
        .collect();
    assert!(
        calls.is_empty(),
        "`trim_end_matches` strips a character set, not a filename: it would eat \
         any directory ending in those letters and tear down a sibling's worker. \
         Found: {calls:?}"
    );
    assert!(
        ui.contains("strip_suffix(\"service-worker.js\")"),
        "the comparison must strip the one filename it expects"
    );
}

/// The scope is named once, and the page and the worker agree on the directory.
///
/// Two independent resolutions of "where am I" — the page's `./` and the worker's
/// `new URL('./', self.location.href)`. They have to describe the same
/// directory, or the page registers a scope the worker's guard does not match.
#[test]
fn the_scope_is_a_relative_directory_shared_with_the_worker() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("reading src/ui.rs");
    assert!(
        ui.contains("const SCOPE: &str = \"./\";"),
        "the scope must be the app's own directory, relative — so one build works \
         from any subdirectory"
    );
    assert!(
        worker_template().contains("new URL('./', self.location.href)"),
        "the worker must resolve the same directory the page registered"
    );
}

/// The icon is the author's original, byte for byte, and stays the source of
/// truth. `build.rs` derives the install PNGs from it and nothing replaces it.
///
/// The digest below is that of the file in `wdomitrz/life_counter` at the
/// original commit. It is recorded here so a redraw cannot pass unnoticed.
#[test]
fn the_icon_is_the_original_and_stays_the_source_of_truth() {
    let icon = root().join("assets/icon.svg");
    let bytes = std::fs::read(&icon).expect("the committed icon");
    assert!(
        icon.ends_with("assets/icon.svg"),
        "the icon lives in assets/, next to the build script that rasterizes it"
    );
    // 705 bytes, `heart_check` in Material Symbols Outlined, #434343.
    assert_eq!(
        bytes.len(),
        705,
        "the committed icon.svg must stay the author's original, byte for byte"
    );
    let text = String::from_utf8(bytes).expect("the icon is UTF-8");
    assert!(
        text.contains("Material+Symbols+Outlined:heart_check"),
        "the icon is the Material Symbols heart_check glyph"
    );
    assert!(text.contains("#434343"), "the icon keeps its fill colour");

    let build = std::fs::read_to_string(root().join("build.rs")).expect("the build script");
    assert!(
        build.contains("assets/icon.svg"),
        "the build script must rasterize the committed SVG"
    );
    assert!(
        !build.contains("assets/icon-192.png") && !build.contains("assets/icon-512.png"),
        "the PNGs are build output and must have no committed source to copy from"
    );
}

/// The crate is a library, not a binary: a browser-only app has nothing to run
/// natively, and a `[[bin]]` here would be the server/CLI the spec forbids.
#[test]
fn the_crate_is_a_library_with_no_binary() {
    let manifest =
        std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml is committed");
    assert!(
        !manifest.contains("[[bin]]"),
        "no [[bin]]: this is a browser-only app with no server and no CLI"
    );
    assert!(
        manifest.contains("wasm-bindgen = \"=0.2.128\""),
        "the wasm-bindgen pin must be exact, and match the CLI the build runs"
    );
    assert!(
        manifest.contains(r#"crate-type = ["cdylib", "rlib"]"#),
        "the crate must be both the wasm module and a library the tests can use"
    );
}
