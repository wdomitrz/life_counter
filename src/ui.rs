// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The browser application: draw the board, and turn pointers into calls on
//! [`crate::counter`].
//!
//! JavaScript in this app is the generated wasm-bindgen platform glue, the
//! ~6-line loader in `ui.html` and the service worker. There is no handwritten
//! application script and no pointer ABI: a `PointerEvent` is received here,
//! where its `client_x` is read once, and only the player's index and the sign
//! cross into the domain layer.
//!
//! Two timers replace the original's `setTimeout` calls, and both are driven
//! from one interval: one to mature a long press, and one to expire a snackbar.
//! Rust owns them, so a game in progress cannot leak a timer it forgot about,
//! and both stop the moment the round resets.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::{closure::Closure, prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Document, Element, Event, EventTarget, HtmlElement, HtmlInputElement, HtmlSelectElement,
    PointerEvent, VisibilityState, WakeLockSentinel, WakeLockType, Window,
};

use crate::counter::{GameState, PLAYER_COUNT_OPTIONS};

/// How often the domain layer is polled, in ms.
///
/// The long press matures at 500 ms and the snackbar hides 2500 ms after a
/// change; nothing in the app needs either to the millisecond. 16 ms is one
/// animation frame, which bounds the gap between the deadline and the change
/// at about a frame.
const TICK_MS: i64 = 16;

/// The scope this app's worker is registered for.
///
/// Stated rather than inherited, and it is the one string that has to agree
/// with `src/service-worker.js`: the worker resolves its own directory the same
/// way, from `self.location`. See [`register_service_worker`].
const SCOPE: &str = "./";

/// The app's mutable state, shared with every listener.
type Shared = Rc<RefCell<App>>;

/// Everything the DOM layer holds between events.
#[derive(Default)]
struct App {
    /// The game on screen, or `None` on the setup screen.
    game: Option<GameState>,
    /// The handle of the running tick interval, so it can be cancelled.
    ticker: Option<i32>,
    /// The wake lock held while a game is in progress.
    wake_lock: Option<WakeLockSentinel>,
    /// The control area a press started on: `(player, sign)`.
    ///
    /// `pointerup` must not re-derive this from the event. Once the pointer is
    /// captured by a panel, every later event is retargeted to that panel and
    /// the half of it that was pressed is gone — so the sign has to be kept
    /// from the `pointerdown` that had it.
    press: Option<(usize, i64)>,
}

/// Show `message` on the setup screen's error line.
fn document() -> Result<Document, JsValue> {
    web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("Browser document unavailable"))
}

fn element(id: &str) -> Result<Element, JsValue> {
    document()?
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("Missing interface element: {id}")))
}

/// Show `message` on the setup screen's error line.
fn show_error(message: &str) {
    let Ok(box_) = element("setup-error") else {
        return;
    };
    box_.set_text_content(Some(message));
    let _ = box_.remove_attribute("hidden");
}

/// The milliseconds since the page loaded, which is the clock the domain layer
/// is driven with.
///
/// Monotonic by construction — `Performance::now` does not move when the system
/// clock does — which is what the snackbar's window and the long press need.
/// It starts near zero on every load, so the first change of a game is always a
/// new streak rather than a continuation of one.
fn now_ms() -> i64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or_default() as i64
}

/// Attach a listener that is never removed, because there are a fixed number of
/// them and they all live as long as the page does.
fn listen(
    target: &Element,
    event: &str,
    handler: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let callback = Closure::<dyn FnMut(Event)>::new(handler);
    target.add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())?;
    callback.forget();
    Ok(())
}

/// Stop the tick interval, if one is running.
fn stop_ticker(state: &Shared) {
    let tick_id = state.borrow_mut().ticker.take();
    if let Some(tick_id) = tick_id {
        if let Some(window) = web_sys::window() {
            window.clear_interval_with_handle(tick_id);
        }
    }
}

/// Start the tick interval, replacing any existing one.
fn start_ticker(state: &Shared) {
    stop_ticker(state);
    let handle = state.clone();
    let callback = Closure::<dyn FnMut()>::new(move || tick(&handle));
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(tick_id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        TICK_MS as i32,
    ) else {
        return;
    };
    callback.forget();
    state.borrow_mut().ticker = Some(tick_id);
}

/// One tick: mature long presses, expire snackbars, redraw what changed.
fn tick(state: &Shared) {
    let now = now_ms();
    let changed = {
        let mut app = state.borrow_mut();
        match app.game.as_mut() {
            Some(game) => game.tick(now),
            None => Vec::new(),
        }
    };
    for index in changed {
        let _ = redraw_player(state, index);
    }
}

/// Redraw one player's life total and snackbar.
fn redraw_player(state: &Shared, index: usize) -> Result<(), JsValue> {
    let Some(snapshot) = state
        .borrow()
        .game
        .as_ref()
        .and_then(|game| game.snapshot(index))
    else {
        return Ok(());
    };
    if let Ok(Some(total)) =
        player_panel(index).and_then(|panel| panel.query_selector(".life-total"))
    {
        total.set_text_content(Some(&snapshot.life.to_string()));
    }
    // Addressed by player rather than by grid position: the original reached
    // into `gameScreen.children[i]`, which is the same thing written more
    // fragile.
    let selector = format!("[data-player='{index}'] .change-snackbar");
    if let Some(snackbar) = document()?.query_selector(&selector)? {
        match &snapshot.snackbar {
            Some(text) => {
                snackbar.set_text_content(Some(text));
                let _ = snackbar.class_list().add_1("show");
            }
            None => {
                let _ = snackbar.class_list().remove_1("show");
            }
        }
    }
    Ok(())
}

/// One player's panel element.
fn player_panel(index: usize) -> Result<Element, JsValue> {
    document()?
        .query_selector(&format!("[data-player='{index}']"))?
        .ok_or_else(|| JsValue::from_str(&format!("No panel for player {index}")))
}

/// A panel's inline style.
fn style_of(element: &Element) -> Result<web_sys::CssStyleDeclaration, JsValue> {
    Ok(element.clone().dyn_into::<HtmlElement>()?.style())
}

/// The index of the player a pointer landed on, if it landed on one.
///
/// `closest` finds the panel from anywhere inside it, so a press on the life
/// total — which is deliberately not clickable — still resolves to its player.
fn player_index_at(target: Option<EventTarget>) -> Option<usize> {
    let target: Element = target?.dyn_into().ok()?;
    let panel = target.closest("[data-player]").ok()??;
    panel.get_attribute("data-player")?.parse().ok()
}

/// The sign a pointer means: `1` for the plus half of a panel, `-1` for the
/// minus half.
///
/// Read from the control area's own `data-sign`, walking up to the area first.
/// The original tested which of two class names the target was, but this uses
/// the attribute that is written alongside the class, so the sign is a property
/// of the control rather than an inference from the markup.
///
/// A press that does not land on a control area at all — on the panel's middle,
/// or on the life total over it — has no sign and is ignored, rather than
/// being counted as a plus.
fn sign_at(target: Option<EventTarget>) -> Option<i64> {
    let target: Option<Element> = target.and_then(|target| target.dyn_into().ok());
    let area = target?.closest(".control-area").ok()??;
    area.get_attribute("data-sign")?.parse().ok()
}

/// A pointer went down on a control area: start a press, so a long press can
/// mature under the finger.
fn on_press(state: &Shared, event: Event) {
    let (Some(index), Some(sign)) = (player_index_at(event.target()), sign_at(event.target()))
    else {
        return;
    };
    // Capture the pointer so the release still arrives here if the finger slides
    // off the panel, which is what stops a drag off a control area from
    // stranding a pending long press.
    let _ = pointer_capture(&event, index);
    let now = now_ms();
    let mut app = state.borrow_mut();
    app.press = Some((index, sign));
    if let Some(game) = app.game.as_mut() {
        game.press(index, sign, now);
    }
}

/// A pointer came up: apply the tap, unless a long press already replaced it.
fn on_release(state: &Shared, event: Event) {
    // The press belongs to whichever control it started on. Falling back to the
    // event's own target is right only when no capture is in effect, and
    // capture is exactly the case that matters.
    let pressed = state.borrow().press;
    let index = pressed
        .map(|(index, _)| index)
        .or_else(|| player_index_at(event.target()));
    let Some(index) = index else {
        return;
    };
    let now = now_ms();
    let changed = {
        let mut app = state.borrow_mut();
        app.press = None;
        app.game
            .as_mut()
            .map(|game| game.release(index, now))
            .unwrap_or_default()
    };
    for index in changed {
        let _ = redraw_player(state, index);
    }
}

/// The browser cancelled the pointer, or the finger left the control area: drop
/// the pending press without applying anything.
fn on_cancel(state: &Shared, _event: Event) {
    let pressed = state.borrow().press;
    let Some(index) = pressed.map(|(index, _)| index) else {
        return;
    };
    let mut app = state.borrow_mut();
    app.press = None;
    if let Some(game) = app.game.as_mut() {
        game.cancel(index);
    }
}

/// Hold a pointer capture on the panel that owns `event`'s pointer, so its
/// later `pointerup` and `pointercancel` still reach us.
fn pointer_capture(event: &Event, index: usize) -> Result<(), JsValue> {
    let Some(pointer) = event.dyn_ref::<PointerEvent>() else {
        return Ok(());
    };
    let Ok(panel) = player_panel(index) else {
        return Ok(());
    };
    panel.set_pointer_capture(pointer.pointer_id())
}

/// Build the setup screen's player-count options from the domain constant, so
/// the two cannot disagree about how many players there can be.
fn build_player_options(select: &HtmlSelectElement) -> Result<(), JsValue> {
    let document = document()?;
    for count in PLAYER_COUNT_OPTIONS {
        let option = document.create_element("option")?;
        option.set_attribute("value", &count.to_string())?;
        option.set_text_content(Some(&count.to_string()));
        if count == 2 {
            // The original pre-selected two players; it is the default the
            // README's screenshot shows and the most common table size.
            let _ = option.set_attribute("selected", "");
        }
        select.append_child(&option)?;
    }
    Ok(())
}

/// Ask the browser to keep the screen awake, and remember the sentinel.
///
/// A refusal is expected on some browsers, over plain HTTP, and in a background
/// tab: the Wake Lock API is not universally available and the original
/// swallowed the failure too. Log it and carry on — a life counter that cannot
/// keep the screen on is still a life counter.
fn acquire_wake_lock(state: &Shared) {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return;
    };
    let promise = navigator.wake_lock().request(WakeLockType::Screen);
    let handle = state.clone();
    spawn_local(async move {
        let sentinel = wasm_bindgen_futures::JsFuture::from(promise).await;
        match sentinel.map(|value| value.dyn_into::<WakeLockSentinel>()) {
            Ok(Ok(sentinel)) => handle.borrow_mut().wake_lock = Some(sentinel),
            other => {
                web_sys::console::warn_1(&JsValue::from_str("screen wake lock unavailable"));
                if let Err(error) = other {
                    web_sys::console::warn_1(&error);
                }
            }
        }
    });
}

/// Release the wake lock, if one is held.
///
/// `release` returns a promise, and the sentinel has to be forgotten either way:
/// the sentinel's `release` event fires when the page stops being visible on its
/// own, and holding a stale one would leave the app thinking it still holds a
/// lock it does not.
fn release_wake_lock(state: &Shared) {
    let sentinel = state.borrow_mut().wake_lock.take();
    if let Some(sentinel) = sentinel {
        let _ = sentinel.release();
    }
}

/// Build one player's panel: the life total, the snackbar and the two halves.
fn build_panel(document: &Document, index: usize, game: &GameState) -> Result<Element, JsValue> {
    let panel = document.create_element("div")?;
    let snapshot = game
        .snapshot(index)
        .ok_or_else(|| JsValue::from_str(&format!("No player {index}")))?;

    panel.set_class_name("player-container");
    panel.set_attribute("data-player", &index.to_string())?;
    let rotation = game.rotation(index);
    if rotation != 0 {
        style_of(&panel)?.set_property("transform", &format!("rotate({rotation}deg)"))?;
    }

    let total = document.create_element("div")?;
    total.set_class_name("life-total");
    total.set_text_content(Some(&snapshot.life.to_string()));
    panel.append_child(&total)?;

    let snackbar = document.create_element("div")?;
    snackbar.set_class_name("change-snackbar");
    snackbar.set_attribute("role", "status")?;
    snackbar.set_attribute("aria-live", "polite")?;
    panel.append_child(&snackbar)?;

    for (class, sign, label, glyph) in [
        ("minus-area", -1, "Decrease life", "\u{2212}"),
        ("plus-area", 1, "Increase life", "+"),
    ] {
        let area = document.create_element("div")?;
        area.set_class_name(&format!("control-area {class}"));
        area.set_attribute("aria-label", label)?;
        area.set_attribute("data-sign", &sign.to_string())?;
        // Not a button, as in the original: these are half-panel targets meant
        // for thumbs, and a `div` takes the whole half rather than shrinking to
        // its glyph. The label is still there for a screen reader.
        area.set_attribute("role", "button")?;
        area.set_attribute("tabindex", "0")?;
        area.set_text_content(Some(glyph));
        panel.append_child(&area)?;
    }

    // The panel's own colour, set inline so nothing has to know how long the
    // list is. The original got it from a `bg-*-500` utility class cycling
    // through nine.
    style_of(&panel)?.set_property("background-color", game.color(index))?;

    Ok(panel)
}

/// Start a game from the setup form's values and show it.
fn start_game(state: &Shared) -> Result<(), JsValue> {
    let document = document()?;
    let player_count = element("player-count")?
        .dyn_into::<HtmlSelectElement>()?
        .value();
    let starting_life = element("life-points")?
        .dyn_into::<HtmlInputElement>()?
        .value();

    let game = GameState::from_form(&player_count, &starting_life);
    let layout = game.layout();

    let screen = element("game-screen")?;
    while let Some(child) = screen.first_child() {
        let _ = screen.remove_child(&child);
    }
    // The track sizes stay in CSS; this only decides how many of them there are.
    let style = style_of(&screen)?;
    style.set_property(
        "grid-template-columns",
        &format!("repeat({}, 1fr)", layout.columns),
    )?;
    style.set_property(
        "grid-template-rows",
        &format!("repeat({}, 1fr)", layout.rows),
    )?;

    for index in 0..game.player_count() as usize {
        screen.append_child(&build_panel(&document, index, &game)?.into())?;
    }

    // The board is built before the setup screen is hidden, so the first frame
    // already shows it rather than flashing an empty screen.
    element("setup-screen")?.set_attribute("hidden", "")?;
    // The board is one big group of controls; a screen reader should be able
    // to skip straight over it to the setup form or the reset button.
    screen.set_attribute("aria-label", "Game board")?;
    screen.remove_attribute("hidden")?;
    element("reset-button")?.remove_attribute("hidden")?;

    state.borrow_mut().game = Some(game);

    // The wake lock is requested once a game is actually running, exactly as
    // the original did — asking for it on the setup screen would hold the screen
    // on before the player had started anything.
    acquire_wake_lock(state);
    start_ticker(state);
    Ok(())
}

/// Return to the setup screen, ending the round.
fn reset_game(state: &Shared) -> Result<(), JsValue> {
    stop_ticker(state);
    release_wake_lock(state);
    let mut app = state.borrow_mut();
    app.game = None;
    app.press = None;
    drop(app);

    let screen = element("game-screen")?;
    while let Some(child) = screen.first_child() {
        let _ = screen.remove_child(&child);
    }
    screen.set_attribute("hidden", "")?;
    element("reset-button")?.set_attribute("hidden", "")?;
    element("setup-screen")?.remove_attribute("hidden")?;
    Ok(())
}

/// Register the service worker, so the app works offline after one visit.
///
/// Fire and forget, as it always was: the app is usable whether or not the
/// browser accepts the registration, and nothing the user does depends on the
/// promise, so there is nothing to await before the game can be played. The
/// scope is stated, and the wider registration cleanup is chained after it,
/// so the two are not two code paths.
fn register_service_worker() {
    let Some(window) = web_sys::window() else {
        return;
    };
    // The scope is stated rather than inherited. Left to itself, a
    // registration's scope is the directory of the page that registered it,
    // which is right today and silently wrong the moment the app is published
    // somewhere else, or opened through a path that resolves higher up the
    // origin. A worker registered for the whole origin does not serve just its
    // own pages — it answers for every page on that origin, including the ones
    // that have nothing to do with it. Naming the scope keeps that claim as
    // small as the app.
    let options = web_sys::RegistrationOptions::new();
    options.set_scope(SCOPE);
    let promise = window
        .navigator()
        .service_worker()
        .register_with_options("./service-worker.js", &options);
    spawn_local(async move {
        // Awaited, but only so the cleanup below is sequenced after the
        // registration this app's own worker made; nothing waits on it.
        let _ = JsFuture::from(promise).await;
        release_stale(&window).await;
    });
}

/// Hand this app's own URLs back to the current worker.
///
/// A service worker is a registration, and a registration outlives the page
/// that made it: it is kept by the browser, not by the tab, and it keeps
/// answering for its scope until something explicitly unregisters it. That is
/// how a page on this origin can come to be served by a worker installed for a
/// *different* page, long after the app that installed it was closed. A stale
/// registration is not corrected by a reload, by a newer version of the app, or
/// by a newer worker installing itself — the newer worker only takes control
/// where its own scope reaches, and a wider stale one is still in the way.
///
/// So the repair is explicit: find any registration whose scope covers this
/// app's directory but is not this app's directory, and unregister it. This
/// app's own registration is left alone, and so is every other app on the
/// origin — each is scoped to its own directory, and a sibling that never
/// covered us is not ours to remove.
///
/// Failures are ignored on purpose. This is best-effort cleanup of state this
/// app did not create, and a browser that refuses leaves the user no worse
/// off: the app still runs and still caches its own assets.
async fn release_stale(window: &Window) {
    let container = window.navigator().service_worker();
    let Ok(registrations) = JsFuture::from(container.get_registrations()).await else {
        return;
    };
    let Ok(array) = registrations.dyn_into::<js_sys::Array>() else {
        return;
    };

    // This app's own directory, as an absolute URL with a trailing slash. The
    // app is served from a subdirectory and every URL of ours is inside it.
    let Ok(home) = window.location().href() else {
        return;
    };
    let Ok(ours) = web_sys::Url::new_with_base(&home, "./") else {
        return;
    };
    let ours = ours.href();

    for entry in array.iter() {
        let Ok(registration) = entry.dyn_into::<web_sys::ServiceWorkerRegistration>() else {
            continue;
        };
        let scope = registration.scope();
        // Leave alone any scope that is this app's own, or narrower: a sibling
        // app mounted inside this directory is legitimate and separate, and
        // nothing there can intercept us. One test, because the two cases are
        // the same one: `ours` begins with `scope`.
        if ours.starts_with(&scope) {
            continue;
        }
        // What is left is a scope that is a *strict* prefix of ours: a worker
        // that would be consulted for this app's URLs while being registered
        // for more of the origin. A worker is consulted for a URL exactly when
        // its scope is a prefix of that URL, which is the test above inverted.
        //
        // Of those, only our own worker qualifies: a different app's worker
        // lives in a different directory, so unregistering it would break the
        // app it belongs to.
        let script = registration
            .active()
            .map(|worker| worker.script_url())
            .unwrap_or_default();
        if script_belongs_to_app(&script, &ours) {
            match registration.unregister() {
                Ok(promise) => {
                    let _ = JsFuture::from(promise).await;
                }
                Err(error) => web_sys::console::warn_1(&error),
            }
        }
    }
}

/// Whether a worker script at `script` is this app's own worker, registered for
/// more of the origin than this app's directory.
///
/// A wider scope means the script sits at the root of this app's own directory
/// rather than anywhere below it: a sibling app's worker is in a sibling
/// directory and does not match. `strip_suffix`, not `trim_end_matches` — the
/// latter strips a *set of characters*, so it would happily eat a directory
/// named `...e-worker.js` and call it ours.
fn script_belongs_to_app(script: &str, ours: &str) -> bool {
    match web_sys::Url::new(script) {
        Ok(url) => url.href().strip_suffix("service-worker.js") == Some(ours),
        Err(_) => false,
    }
}

/// The page became visible again: the browser has released the screen wake lock
/// whenever the page is hidden, so ask for a new one.
fn on_visibility_change(state: &Shared) {
    let visible = document()
        .map(|document| document.visibility_state() == VisibilityState::Visible)
        .unwrap_or(false);
    let in_game = state.borrow().game.is_some();
    if visible && in_game {
        acquire_wake_lock(state);
    }
}

/// Install the app. Called once, by the generated bindings' default export.
#[wasm_bindgen(start)]
pub fn main() {
    if let Err(error) = start() {
        web_sys::console::error_1(&error);
        show_error("Life Counter could not start in this browser.");
    }
}

fn start() -> Result<(), JsValue> {
    let state: Shared = Rc::new(RefCell::new(App::default()));

    let select = element("player-count")?.dyn_into::<HtmlSelectElement>()?;
    build_player_options(&select)?;

    {
        let handle = state.clone();
        let start_button = element("start-game")?;
        listen(&start_button, "click", move |_| {
            if let Err(error) = start_game(&handle) {
                // The cause is ours to debug, not the user's: what is shown is
                // what they can do about it and nothing else.
                if error
                    .as_string()
                    .is_none_or(|message| message.trim().is_empty())
                {
                    web_sys::console::error_1(&error);
                }
                show_error("The game could not start. Try reloading the page.");
            }
        })?;
    }

    {
        let handle = state.clone();
        let reset_button = element("reset-button")?;
        listen(&reset_button, "click", move |_| {
            let _ = reset_game(&handle);
        })?;
    }

    // The three pointer events cover mouse, touch and pen in one path, which
    // is what the original needed two sets of handlers to do. `touch-action:
    // manipulation` in the shell stops a press from becoming a scroll or a
    // double-tap zoom.
    {
        let handle = state.clone();
        let screen = element("game-screen")?;
        listen(&screen, "pointerdown", move |event| {
            on_press(&handle, event)
        })?;
    }
    {
        let handle = state.clone();
        let screen = element("game-screen")?;
        listen(&screen, "pointerup", move |event| {
            on_release(&handle, event)
        })?;
    }
    {
        let screen = element("game-screen")?;
        // Only `pointercancel`: the browser taking the pointer back (a system
        // gesture, a scroll, a second touch). `pointerleave` is deliberately
        // NOT handled — with the pointer captured by the panel it fires as soon
        // as the pointer moves at all, which would abandon every press.
        let cancel = state.clone();
        listen(&screen, "pointercancel", move |event| {
            on_cancel(&cancel, event)
        })?;
    }

    {
        let handle = state.clone();
        let body = document()?
            .body()
            .ok_or_else(|| JsValue::from_str("The page has no body"))?;
        listen(&body, "visibilitychange", move |_| {
            on_visibility_change(&handle)
        })?;
    }

    register_service_worker();

    // Nothing the user can see may name the implementation. "Rust is ready"
    // tells them nothing about a life counter and only exists to prove the
    // module booted; the technology is not the user's business.
    element("setup-status")?.set_text_content(Some(""));
    Ok(())
}
