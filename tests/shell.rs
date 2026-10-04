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
    // The table is read by splitting on the `SHELL:` declaration, so it must
    // stay a single line of `("name", source)` pairs.
    let shell_table = build
        .split_once("const SHELL:")
        .map(|(_, rest)| rest)
        .expect("build.rs must declare a SHELL table");
    let published: Vec<&str> = shell_table
        .split("(\"")
        .skip(1)
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
    // That script is one line that holds both the `import('./app.js')` call and
    // the prose a user reads when the app fails to load — so the line is
    // checked, with only the import expression itself trimmed off before the
    // scan.
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

/// A workflow file, as committed.
///
/// Nonexistent is not a reason to fail. The Pages workflow is the only one here
/// that a fresh clone of a *release branch* need not have, and a test that
/// hard-failed on its absence would make a partial export red for a reason that
/// says nothing about the app. Callers say which they want.
fn workflow(name: &str) -> Option<String> {
    let path = root().join(".github/workflows").join(name);
    std::fs::read_to_string(&path).ok()
}

/// A workflow with its comments removed.
///
/// A `#` inside a quoted string is not a comment, and neither is a `#` in a
/// value -- but these workflows quote nothing in the keys they are read for and
/// carry no `#` in any value any assertion depends on, so a line-wise cut at
/// the first `#` is enough and a YAML parser is not worth a dependency.
///
/// Stripping is not tidiness, it is the point: commenting a line out instead of
/// deleting it is the easiest way to "fix" one of these, and an assertion that
/// reads the raw text is satisfied by the comment. That mistake shipped in the
/// sibling repository and it is what these tests exist to make impossible.
fn strip_yaml_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find('#') {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Pages deployment cannot drift from the crate it publishes.
///
/// The deploy job installs its own `wasm-bindgen`, pinned to a literal in the
/// YAML, and compares its digest against a second literal in the same file.
/// `Cargo.toml` pins the same version the crate compiles against. Move the
/// dependency and the workflow keeps building happily: it generates bindings for
/// a runtime the page does not have, and the only symptom is a live site that
/// fails at startup with "Life Counter could not start" -- for every visitor, and
/// only in the browser. So the two are asserted equal here rather than trusted
/// to be edited together.
///
/// The `env:` block itself is required, and that is not decoration: with no
/// declaration `version="$WASM_BINDGEN_VERSION"` expands to nothing, the
/// generator step installs no generator, and every other check in the job still
/// passes.
#[test]
fn the_pages_build_generates_bindings_for_the_pinned_runtime() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");

    // Read the pin as Cargo writes it: `wasm-bindgen = "=0.2.128"`, an exact
    // requirement. A looser form ("0.2.128", "^0.2") would resolve to whatever
    // is newest in the lockfile, and the workflow's literal would then be
    // naming one arbitrary version of several -- so the exact form is required.
    let pinned = manifest
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("wasm-bindgen")?;
            let rest = rest.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
        .expect("Cargo.toml pins no wasm-bindgen version");
    assert!(
        pinned.starts_with('='),
        "Cargo.toml must pin wasm-bindgen exactly ({pinned:?}), so there is one version for the \
         crate and one for the bindings generator"
    );
    let version = pinned.trim_start_matches('=').trim_matches('"');

    // Read the workflow with its comments stripped. An assertion over raw text
    // is satisfied by commenting a line out instead of deleting it -- which is
    // what someone "tidying" this file would do, and what shipped in the sibling
    // repository: a commented-out `WASM_BINDGEN_VERSION:` looked like a declared
    // version, and the job installed no generator at all.
    let pages = strip_yaml_comments(&pages);
    let env = pages
        .split("\nenv:")
        .nth(1)
        .and_then(|after| after.split("\npermissions:").next())
        .expect("pages.yml must have a top-level `env:` block");
    assert!(
        env.contains(&format!("WASM_BINDGEN_VERSION: {version}\n")),
        "pages.yml's top-level `env:` must declare WASM_BINDGEN_VERSION: {version}, matching the \
         Cargo.toml pin; an undeclared variable expands to nothing and the generator step installs \
         no generator (found: {})",
        env.trim()
    );

    // The declared version has to reach the step that installs it, as the
    // `version=` it unrolls -- not merely sit in the block, where every other
    // check would still pass.
    assert!(
        pages.contains("version=\"$WASM_BINDGEN_VERSION\""),
        "pages.yml must install $WASM_BINDGEN_VERSION rather than a second, independent literal"
    );
}

/// This repository carries the wake-lock cfg in `.cargo/config.toml`, so the
/// Pages workflow must NOT carry it in its environment.
///
/// The siblings (`chess_clock`) have no config file and so *must* set
/// `RUSTFLAGS`; copying that workflow across verbatim ships a line that is at
/// best redundant here. It is worse than redundant, though. `RUSTFLAGS` in the
/// environment does not merge with `[target.*.rustflags]` in a config file:
/// cargo takes one or the other, and the environment wins. So declaring
/// `--cfg=web_sys_unstable_apis` in `env:` silently *replaces* the config's
/// per-target scoping, and the cfg is then applied to the host build and to
/// every dependency as well -- the opposite of what the config file chose, and
/// verified here rather than assumed.
///
/// Meanwhile deleting the config file to "tidy up" the duplication is not
/// reversible by editing the workflow: without either mechanism the wasm build
/// dies with `cannot find WakeLockSentinel in crate web_sys` and `no method
/// named wake_lock`, naming no feature to add. So the invariant asserted is the
/// pair: the config file carries the cfg, and the workflow stays out of it.
#[test]
fn the_wake_lock_cfg_comes_from_the_cargo_config_not_the_environment() {
    let config = std::fs::read_to_string(root().join(".cargo/config.toml")).expect(
        ".cargo/config.toml must be committed: it is this repository's only mechanism for \
                the wake-lock cfg",
    );

    let config = strip_yaml_comments(&config);
    let scoped = config.contains("[target.wasm32-unknown-unknown]")
        && config.contains("--cfg=web_sys_unstable_apis");
    assert!(
        scoped,
        ".cargo/config.toml must set rustflags = [\"--cfg=web_sys_unstable_apis\"] under \
         [target.wasm32-unknown-unknown]; without it the wasm build cannot compile the Wake Lock \
         API at all"
    );
    // Per-target, not global: only the wasm build needs the flag. `src/ui.rs` is
    // `#[cfg(target_arch = "wasm32")]`, so a global flag would apply it to
    // every dependency of the host build for no reason.
    assert!(
        !config.contains("[build]"),
        ".cargo/config.toml must scope the cfg to the wasm target rather than setting a global \
         `[build] rustflags`, which would apply it to the host build and all its dependencies"
    );

    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    // Comments stripped for the same reason as everywhere else: a commented-out
    // `RUSTFLAGS:` is not a setting, and a workflow asserting over raw text
    // cannot tell the two apart. The file does discuss RUSTFLAGS in its
    // comments -- explaining why it sets none -- and only the stripped text
    // separates that discussion from a setting.
    let live = strip_yaml_comments(&pages);
    assert!(
        !live.contains("RUSTFLAGS"),
        "pages.yml must not set RUSTFLAGS; .cargo/config.toml already scopes --cfg=\
         web_sys_unstable_apis to the wasm target, and a RUSTFLAGS in the environment does not \
         merge with it -- it replaces it, so the cfg would reach the host build too"
    );
}

/// A deploy that can run from any branch is a deploy a stranger can run.
///
/// `pages: write` and `id-token: write` are the two permissions that let a
/// GitHub Actions job overwrite the live site, and the token behind them is
/// minted for the repository however the workflow was reached. This project
/// *wants* an automatic deploy on every merge to master -- that is the point,
/// and it is why nobody has to remember to publish. What it does not want is
/// that same power on every other ref, so the invariant asserted here is the
/// narrow one that survives the convenience: master is the only ref that can
/// reach the live site, and the publishing permissions live in the one job that
/// is gated on it.
#[test]
fn only_master_can_reach_the_live_site() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };

    // The trigger must be the named branch, not a bare `push:`. A bare `push:`
    // deploys from every branch that exists, including a contributor's feature
    // branch, and it also changes what `on:` means for pull requests -- the
    // opposite of the intent.
    assert!(
        pages.contains("branches: [master]"),
        "pages.yml must trigger on `branches: [master]`, not a bare `push:`; a bare push \
         deploys from every branch, including other people's"
    );
    // A tag trigger alongside the branch trigger would publish a version that
    // was never on master.
    assert!(
        !pages.contains("tags:"),
        "pages.yml must not also deploy on tags; a tagged commit that never reached master \
         would be published to the live site"
    );
    // No manual trigger either: it needs admin on the repository, which this
    // account does not have, so it reads as a working escape hatch and is not
    // one.
    assert!(
        !pages.contains("workflow_dispatch"),
        "pages.yml must not offer a workflow_dispatch deploy; it requires admin rights this \
         account does not have, so it is an escape hatch that cannot be used"
    );

    // The deploy job's gate, named so the assertion cannot be satisfied by a
    // gate on some other job. This is the check that actually holds if the
    // trigger is ever widened by accident.
    let deploy_job = pages
        .split("\n  deploy:")
        .nth(1)
        .expect("pages.yml must have a `deploy:` job");
    assert!(
        deploy_job.contains("if:") && deploy_job.contains("github.ref == 'refs/heads/master'"),
        "the `deploy` job must be gated on the build being for master"
    );

    // And it must ALSO be gated on not being a fork. This file is
    // byte-identical in `wdomitrz/life_counter` and in its fork
    // `bot-git-ai/life_counter`, so a gate that tests only the branch name
    // cannot tell the two repositories apart: both have a `master`, and a push
    // to the fork's master would try to publish.
    //
    // `github.event.repository.fork` is the discriminator because it needs no
    // configuration: the event supplies it, false upstream and true in the
    // fork. The obvious alternative, a repository Actions variable, would have
    // to be set on the *upstream* repository to publish, and no account but the
    // user's can do that — so the gate would ship silently off on the one
    // repository where it matters.
    //
    // Read out of the comment-stripped text, or the comment block above the
    // `if:` — which names both halves of the gate while explaining it — would
    // satisfy this on its own. That is not hypothetical: it is the mistake the
    // `RUSTFLAGS` assertion in this same file shipped with, and it shipped.
    let live = strip_yaml_comments(&pages);
    let live_gate = live
        .split("\n  deploy:")
        .nth(1)
        .and_then(|job| job.split_once("if:").map(|(_, after)| after))
        .expect("the `deploy` job must have an `if:` gate");
    assert!(
        live_gate.contains("!github.event.repository.fork"),
        "the `deploy` job must be gated on `!github.event.repository.fork`; this workflow is \
         byte-identical in the fork `bot-git-ai/life_counter`, so a branch-name-only gate \
         publishes from the fork too -- failing with 'Ensure GitHub Pages has been enabled' \
         until Pages is enabled there, and serving a divergent copy afterwards",
    );
    // The two halves are one condition, not two jobs: an `if:` per job would
    // be an AND across two independent gates, and a `build`-job gate would
    // silently stop the *build* from running on the fork rather than just its
    // publish, which is the opposite of what this is for -- the fork is where
    // the site is checked before it is ever proposed upstream.
    assert!(
        live_gate.contains("github.ref == 'refs/heads/master'"),
        "the fork rule must extend the master gate, not replace it: `deploy` must be one `if:` \
         testing both `github.ref` and `github.event.repository.fork`",
    );

    // And the permissions that can actually publish must be scoped to that job
    // rather than granted workflow-wide, so a build step or a third-party
    // action added later cannot spend them.
    let build_job = pages
        .split("\n  build:")
        .nth(1)
        .and_then(|after| after.split("\n  deploy:").next())
        .expect("pages.yml must have a `build:` job");
    assert!(
        !build_job.contains("pages: write") && !build_job.contains("id-token: write"),
        "the `build` job must not hold pages: write or id-token: write; those belong to `deploy`"
    );
    let permissions = deploy_job
        .split("\n    permissions:")
        .nth(1)
        .and_then(|after| after.split("\n    steps:").next())
        .expect("the `deploy` job must declare its own permissions block");
    assert!(
        permissions.contains("pages: write") && permissions.contains("id-token: write"),
        "the `deploy` job must hold both publishing permissions (found: {})",
        permissions.trim()
    );
    // The workflow-level block stays read-only. A deploy job cannot widen a
    // token the workflow never asked for, so if `pages: write` is also granted
    // here then some *other* job can be added that publishes without being
    // gated on master.
    let workflow_permissions = pages
        .split("\npermissions:")
        .nth(1)
        .and_then(|after| after.split("\njobs:").next())
        .expect("pages.yml must have a top-level `permissions:` block");
    assert!(
        workflow_permissions.contains("contents: read")
            && !workflow_permissions.contains("pages: write")
            && !workflow_permissions.contains("id-token: write"),
        "the top-level `permissions:` must stay read-only; publishing rights belong to the \
         `deploy` job alone (found: {})",
        workflow_permissions.trim()
    );
}

/// The deploy has to be handed something the upload actually produced.
///
/// `deploy-pages` v5 takes `artifact_name`. There is no `artifact_id` input:
/// passing one is reported as `Unexpected input(s) 'artifact_id'` and the action
/// falls back to its own default, which is only the right answer while the
/// upload side also defaults to the same string. Change one side and the deploy
/// finds no artifact and fails with a bare `HttpError: Not Found` -- which says
/// nothing about the artifact, so the cause has to be read out of the workflow,
/// not out of the error.
#[test]
fn the_deploy_is_handed_the_artifact_the_build_uploaded() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // The input that v5 does not have. Its presence is a warning at run time
    // and never an error, so nothing else would ever report it.
    assert!(
        !live.contains("artifact_id:"),
        "pages.yml passes `artifact_id` to deploy-pages v5, which has no such input; it is \
         warned about and ignored, leaving the deploy to guess the artifact name"
    );

    // Both sides name the artifact the same way. Read the two keys out of the
    // live text rather than asserting on a fixed string, so the invariant is the
    // agreement and not the particular name.
    //
    // Only the two are read, and only where they are the artifact's own keys:
    // the file also carries the workflow's `name:` and every step's `name:`, and
    // a plain prefix match picks up whichever of those comes first.
    // `artifact_name:` is unique, and the upload's `name:` is the one indented
    // ten spaces -- a step's own `name:` is eight.
    let name_of = |key: &str, indent: usize| {
        live.lines().find_map(|line| {
            let prefix = format!("{}{key}: ", " ".repeat(indent));
            let rest = line.strip_prefix(prefix.as_str())?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
    };
    let uploaded = name_of("name", 10)
        .unwrap_or_else(|| panic!("pages.yml must state the upload step's artifact `name:`"));
    let deployed = name_of("artifact_name", 10).unwrap_or_else(|| {
        panic!(
            "pages.yml must pass `artifact_name:` to deploy-pages, or it uses a default that can \
                drift from the upload"
        )
    });
    assert_eq!(
        uploaded, deployed,
        "the artifact the build uploads ({uploaded:?}) and the one the deploy asks for \
         ({deployed:?}) must be the same name"
    );
    // And it must be the Pages upload action, not the generic one. A plain
    // `upload-artifact` produces a green build and then a deploy that cannot
    // find what it was given: the Pages artifact is a single tarball the deploy
    // action looks up by name, not a file in the run's artifact list.
    assert!(
        live.contains("actions/upload-pages-artifact@"),
        "pages.yml must use actions/upload-pages-artifact; a plain upload-artifact leaves the \
         deploy unable to find the artifact by name"
    );
    assert!(
        !live.contains("actions/upload-artifact@"),
        "pages.yml must not upload the Pages artifact with the generic upload-artifact action"
    );
}

/// The site is built, not committed, so the deploy job has to build it.
///
/// `dist/` is gitignored and `app.js` and `app_bg.wasm` are written by
/// `wasm-bindgen` into it: they exist nowhere else in the tree. A Pages
/// deployment that served committed files would publish a page that loads and
/// never starts, with nothing anywhere reporting an error. So the workflow has to
/// run both of the project's build steps itself, and has to assert the eight
/// files that come out -- a green job that produced seven of the eight is
/// exactly the failure this shape has had.
#[test]
fn the_deploy_job_builds_and_inspects_the_site_it_publishes() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // Two targets, so two builds, and in the documented order: the bindings
    // before `build.rs`, which hashes the worker's cache version from the bytes
    // of everything else in `dist/`.
    let bind = live
        .find("wasm-bindgen --target web")
        .expect("pages.yml must generate the web bindings");
    let rest = live
        .find("cargo build --release --locked")
        .expect("pages.yml must run the host build that writes the rest of the site");
    assert!(
        bind < rest,
        "the bindings must be generated before the host build: build.rs hashes the service \
         worker's cache version from what the bindings step wrote"
    );
    // And the generated crate name, which is `life_counter` -- not the package
    // name `life-counter`, and not a sibling's. The wasm-bindgen step fails on
    // a wrong path, so this is cheap to assert and expensive to get wrong.
    assert!(
        live.contains("release/life_counter.wasm"),
        "pages.yml must generate bindings from life_counter.wasm, the crate's own wasm artefact"
    );
    // The `touch` that makes the host build actually re-run `build.rs`. Without
    // it a fresh CI runner leaves `dist/` holding the two wasm artefacts and
    // none of the six shell files -- a publishable-looking site with no app.
    assert!(
        live.contains("touch build.rs"),
        "pages.yml must touch build.rs before the host build; build.rs writes into the source \
         tree rather than OUT_DIR, so cargo will not re-run it otherwise"
    );
    // The bindings step itself, as a step: renaming or deleting it while leaving
    // the `wasm-bindgen` word somewhere in a comment is exactly the half-built
    // site the other assertions here cannot see.
    assert!(
        live.contains("- name: Generate the web bindings"),
        "pages.yml must have a step that generates the web bindings; dist/app.js and \
         dist/app_bg.wasm exist nowhere else in the tree"
    );

    // The eight files, asserted as the one list the check loops over. Asserting
    // that each name appears *somewhere* in the file is not enough -- `app.js`
    // occurs in three error messages here, so dropping it from the checked list
    // changes nothing an occurrence-based assertion can see.
    let expected = live
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("expected=\"")?;
            Some(rest.trim_end_matches('"').to_owned())
        })
        .expect("pages.yml must name the expected site files in an `expected=` list");
    let mut checked: Vec<&str> = expected.split_whitespace().collect();
    checked.sort_unstable();
    assert_eq!(
        checked,
        [
            "app.js",
            "app_bg.wasm",
            "icon-192.png",
            "icon-512.png",
            "icon.svg",
            "index.html",
            "manifest.webmanifest",
            "service-worker.js",
        ],
        "the site's expected-file list must name all eight files and nothing else"
    );
    // And the properties that distinguish a whole site from a plausible one.
    assert!(
        live.contains("grep -q '__VERSION__' dist/service-worker.js"),
        "pages.yml must fail on an unsubstituted __VERSION__ placeholder in the service worker"
    );
    assert!(
        live.contains("__wbindgen_start"),
        "pages.yml must check that the generated bindings carry the start function the page's \
         m.default() call runs"
    );
    assert!(
        live.contains("m.default()"),
        "pages.yml must check that the published page initialises the bindings"
    );
    assert!(
        live.contains("89504e470d0a1a0a"),
        "pages.yml must check the install icons are real PNGs and not truncated writes"
    );
}

/// Every `run:` block in the workflow has to be valid bash.
///
/// This is not a style preference. A `run:` body that does not parse is
/// diagnosed by the runner *before the first command executes*, so the step dies
/// at `line 35: syntax error near unexpected token '}'` having checked nothing
/// at all -- and, because the failure is in the step that was supposed to verify
/// the site, the deployment that follows it never runs. That is exactly what
/// happened on 2026-10-02: the `__VERSION__` guard closed its `if` with the
/// `}` belonging to the `|| { ...; }` one-liner idiom used on the lines around
/// it, and `wdomitrz.github.io/life_counter` stopped publishing.
///
/// Nothing else in this suite can catch it. No test runs the workflow, the
/// release gate does not build wasm, and `tests/shell.rs` otherwise asserts on
/// source *text* -- where `exit 1; }` and `exit 1` differ by one character and
/// every other assertion still passes. So the check is `bash -n`, over every
/// block, here.
///
/// The runner's own de-indentation is reproduced rather than assumed: a YAML
/// block scalar keeps the common indentation, so the body is handed to `bash`
/// with that indentation removed.
#[test]
fn every_run_block_in_the_workflow_is_valid_bash() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };

    // Pull the `run:` bodies out of the committed YAML without a dependency on
    // a YAML crate. There are two forms: a one-liner (`run: cargo test
    // --locked`) and a block scalar (`run: |`) whose following lines are
    // indented deeper than the key. Both are valid bash and both are checked.
    let mut checked = 0usize;
    let mut lines = pages.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(rest) = line.trim_start().strip_prefix("run:") else {
            continue;
        };
        let inline = rest.trim();
        let mut body: Vec<String> = Vec::new();
        if inline.ends_with('|') || inline.ends_with('>') {
            let indent = line.len() - line.trim_start().len();
            while let Some(next) = lines.peek() {
                let blank = next.trim().is_empty();
                let deeper = next.len() - next.trim_start().len() > indent;
                if !blank && !deeper {
                    break;
                }
                body.push(next.get(indent..).unwrap_or("").to_owned());
                lines.next();
            }
            // Trailing blank lines are a block-scalar artefact, not content.
            while body.last().is_some_and(|line| line.trim().is_empty()) {
                body.pop();
            }
        } else {
            body.push(inline.to_owned());
        }
        let script = body.join("\n");
        assert!(
            !script.trim().is_empty(),
            "a `run:` block in the workflow is empty; the step would do nothing and pass",
        );
        if inline.ends_with('|') || inline.ends_with('>') {
            assert!(
                !script.trim().is_empty(),
                "a `run:` block scalar in the workflow is empty; the step would do nothing and \
                 pass",
            );
        }

        let mut bash = std::process::Command::new("bash");
        bash.arg("-n").arg("-c").arg(&script);
        let output = bash
            .output()
            .expect("bash -n is available; CI runs ubuntu-latest");
        assert!(
            output.status.success(),
            "a `run:` block is not valid bash, so the runner rejects the step before executing \
             any of it:\n{}",
            String::from_utf8_lossy(&output.stderr),
        );
        checked += 1;
    }

    assert!(
        checked >= 5,
        "expected to check every step of the workflow, found only {checked} `run:` blocks",
    );
}
