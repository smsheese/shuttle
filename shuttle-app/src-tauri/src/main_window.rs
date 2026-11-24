//! Lifecycle of the main window, plus the WebView unload that keeps a tray-hidden Shuttle cheap.
//!
//! Closing the window hides it to the tray, so the window and its WebView would otherwise stay
//! alive all day for a UI nobody is looking at. After `performance.unload_hidden_after_secs`
//! hidden, the window is destroyed and its renderer process exits; showing it again rebuilds it
//! from the app config. Everything the user relies on while hidden (notifications, sync,
//! reminders, scheduled messages, the tray tooltip) runs in Rust and keeps running.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, WebviewWindow, WebviewWindowBuilder,
};

use crate::connectors::AppEvent;
use crate::models::CallState;

/// Where the main window is right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// A window exists and the user may be looking at it.
    Visible,
    /// Hidden to the tray.
    Hidden { since: Instant },
    /// The window is gone and its renderer exited. Showing it rebuilds it.
    Unloaded,
}

/// Window size and position captured before the window is destroyed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// A maximized window reports the screen size, which would become its restored size.
    maximized: bool,
    size: Option<LogicalSize<f64>>,
    position: Option<LogicalPosition<f64>>,
}

/// The settings the unload decision reads.
#[derive(Clone, Copy, Debug)]
pub struct UnloadPolicy {
    pub enabled: bool,
    pub delay: Duration,
}

impl Default for UnloadPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            delay: Duration::from_secs(120),
        }
    }
}

/// Every input to the unload decision, so it can be decided without a running app.
#[derive(Clone, Copy, Debug)]
pub struct UnloadCheck {
    pub policy: UnloadPolicy,
    /// The phase the timer found when it fired.
    pub phase: Phase,
    /// The hide generation the timer was armed with.
    pub fired_generation: u64,
    /// The current generation.
    pub generation: u64,
    pub in_call: bool,
}

/// Whether the armed timer may still destroy the window.
///
/// A call is the one thing the user watches the app for while it is hidden, so it holds the
/// window open. A different generation means the window was shown again or hidden again since,
/// which leaves the armed timer stale.
pub fn should_unload(check: UnloadCheck) -> bool {
    check.policy.enabled
        && !check.in_call
        && matches!(check.phase, Phase::Hidden { .. })
        && check.generation == check.fired_generation
}

/// Whether an exit request is the last window closing rather than a quit the user asked for.
///
/// Tray Quit and `restart_app` go through `AppHandle::exit`/`request_restart` and carry a code.
/// Only the last window going away arrives with none, and that must not end the app.
pub fn keeps_app_alive(code: Option<i32>) -> bool {
    code.is_none()
}

struct State {
    phase: Phase,
    /// Bumped on every show and hide, so an armed timer can tell it went stale.
    generation: u64,
    geometry: Option<Geometry>,
    /// Last unread total, so a rebuilt window's badge agrees with what the tray showed meanwhile.
    unread: i64,
    call: Option<CallState>,
}

impl State {
    const fn new() -> Self {
        Self {
            phase: Phase::Visible,
            generation: 0,
            geometry: None,
            unread: 0,
            call: None,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

fn policy<R: Runtime>(app: &AppHandle<R>) -> UnloadPolicy {
    let performance = crate::commands::performance_config(app);
    UnloadPolicy {
        enabled: performance.unload_hidden_window,
        delay: Duration::from_secs(u64::from(performance.unload_hidden_after_secs.max(5))),
    }
}

/// Show the main window, rebuilding its webview if it was unloaded.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let (needs_rebuild, geometry) = {
        let mut state = STATE.lock();
        state.generation += 1;
        let needs_rebuild = matches!(state.phase, Phase::Unloaded);
        state.phase = Phase::Visible;
        (needs_rebuild, state.geometry)
    };
    if needs_rebuild {
        if rebuild(app, geometry).is_none() {
            STATE.lock().phase = Phase::Unloaded;
            return;
        }
        // The window was not there to carry the badge while it was gone.
        let unread = STATE.lock().unread;
        crate::tray::set_unread(app, unread);
        return;
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Hide the main window to the tray and arm the unload timer.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    let unload = policy(app);
    let generation = {
        let mut state = STATE.lock();
        if matches!(state.phase, Phase::Unloaded) {
            return;
        }
        state.generation += 1;
        state.phase = Phase::Hidden {
            since: Instant::now(),
        };
        state.generation
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    arm_unload(app, unload, generation);
}

fn arm_unload<R: Runtime>(app: &AppHandle<R>, unload: UnloadPolicy, generation: u64) {
    if !unload.enabled {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(unload.delay).await;
        try_unload(&handle, generation);
    });
}

/// A call that held a hidden window open has ended: start its grace period again, since the
/// timer armed at hide time already fired and gave up.
fn rearm_after_call<R: Runtime>(app: &AppHandle<R>) {
    let generation = {
        let mut state = STATE.lock();
        if !matches!(state.phase, Phase::Hidden { .. }) {
            return;
        }
        state.generation += 1;
        state.generation
    };
    arm_unload(app, policy(app), generation);
}

fn try_unload<R: Runtime>(app: &AppHandle<R>, fired_generation: u64) {
    let policy = policy(app);
    let check = {
        let state = STATE.lock();
        UnloadCheck {
            policy,
            phase: state.phase,
            fired_generation,
            generation: state.generation,
            in_call: state.call.is_some(),
        }
    };
    if !should_unload(check) {
        return;
    }
    let Some(win) = app.get_webview_window("main") else {
        STATE.lock().phase = Phase::Unloaded;
        return;
    };
    let geometry = read_geometry(&win);
    {
        let mut state = STATE.lock();
        state.geometry = geometry.or(state.geometry);
        state.phase = Phase::Unloaded;
        state.generation += 1;
    }
    let hidden_for = match check.phase {
        Phase::Hidden { since } => since.elapsed(),
        _ => Duration::ZERO,
    };
    // WebKitGTK leaves the WebProcess running once its window is destroyed, which would keep the
    // memory this feature exists to free. Ending the process is the only way to get it back.
    #[cfg(target_os = "linux")]
    let _ = win.with_webview(|platform| {
        use webkit2gtk::WebViewExt;
        platform.inner().terminate_web_process();
    });
    // Skips the close handler, so this destroys the window instead of hiding it again.
    if let Err(e) = win.destroy() {
        tracing::warn!("could not unload the hidden main window: {e}");
        STATE.lock().phase = Phase::Hidden {
            since: Instant::now() - hidden_for,
        };
        return;
    }
    tracing::debug!(
        "unloaded the main window after {}s hidden",
        hidden_for.as_secs()
    );
}

fn read_geometry<R: Runtime>(win: &WebviewWindow<R>) -> Option<Geometry> {
    let maximized = win.is_maximized().unwrap_or(false);
    if maximized {
        return Some(Geometry {
            maximized,
            size: None,
            position: None,
        });
    }
    let scale = win.scale_factor().unwrap_or(1.0);
    Some(Geometry {
        maximized,
        size: win
            .inner_size()
            .ok()
            .map(|size| size.to_logical::<f64>(scale)),
        position: win
            .outer_position()
            .ok()
            .map(|position| position.to_logical::<f64>(scale)),
    })
}

/// Build the main window again from the app config and run the per-window setup once more.
fn rebuild<R: Runtime>(app: &AppHandle<R>, geometry: Option<Geometry>) -> Option<WebviewWindow<R>> {
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()?;
    let mut builder = match WebviewWindowBuilder::from_config(app, &window_config) {
        Ok(builder) => builder,
        Err(e) => {
            tracing::error!("could not rebuild the main window: {e}");
            return None;
        }
    };
    if let Some(Geometry {
        size: Some(size), ..
    }) = geometry
    {
        builder = builder.inner_size(size.width, size.height);
    }
    if let Some(Geometry {
        position: Some(position),
        ..
    }) = geometry
    {
        builder = builder.position(position.x, position.y);
    }
    let win = match builder.build() {
        Ok(win) => win,
        Err(e) => {
            tracing::error!("could not rebuild the main window: {e}");
            return None;
        }
    };
    #[cfg(target_os = "linux")]
    let _ = crate::prepare_main_window(&win)
        .inspect_err(|e| tracing::warn!("could not finish the rebuilt window: {e}"));
    #[cfg(not(target_os = "linux"))]
    if let Err(e) = crate::prepare_main_window(&win) {
        tracing::warn!("could not finish the rebuilt window: {e}");
    }
    if geometry.is_some_and(|geometry| geometry.maximized) {
        let _ = win.maximize();
    }
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
    // X11 can drop an icon set before the window is mapped, as at startup.
    #[cfg(not(target_os = "macos"))]
    let _ = crate::apply_window_icon(&win);
    Some(win)
}

/// Fold one connector event into the lifecycle: the unread total the tray shows while the window
/// is unloaded, and the call that holds the window open.
pub fn note_event<R: Runtime>(app: &AppHandle<R>, event: &AppEvent) {
    if let Some(total) = event.payload.get("unread_total").and_then(|v| v.as_i64()) {
        STATE.lock().unread = total;
        crate::tray::set_unread(app, total);
    }
    match event.kind.as_str() {
        "call.ringing" | "call.connected" => {
            if let Ok(call) = serde_json::from_value::<CallState>(event.payload.clone()) {
                STATE.lock().call = Some(call);
            }
        }
        "call.ended" | "call.error" => {
            let was_in_call = STATE.lock().call.take().is_some();
            if was_in_call {
                rearm_after_call(app);
            }
        }
        _ => {}
    }
}

/// The call in progress, so a rebuilt window puts its call panel back.
pub fn current_call() -> Option<CallState> {
    STATE.lock().call.clone()
}

#[cfg(test)]
mod tests {
    use super::{keeps_app_alive, should_unload, Phase, UnloadCheck, UnloadPolicy};
    use std::time::{Duration, Instant};

    fn policy(enabled: bool) -> UnloadPolicy {
        UnloadPolicy {
            enabled,
            delay: Duration::from_secs(120),
        }
    }

    fn hidden() -> Phase {
        Phase::Hidden {
            since: Instant::now(),
        }
    }

    fn check(phase: Phase, generation: u64) -> UnloadCheck {
        UnloadCheck {
            policy: policy(true),
            phase,
            fired_generation: generation,
            generation,
            in_call: false,
        }
    }

    #[test]
    fn a_hidden_window_past_its_grace_period_is_unloaded() {
        assert!(should_unload(check(hidden(), 3)));
    }

    #[test]
    fn showing_the_window_again_cancels_the_armed_timer() {
        let mut shown = check(hidden(), 3);
        shown.generation = 4;
        assert!(!should_unload(shown));
        assert!(!should_unload(check(Phase::Visible, 3)));
    }

    #[test]
    fn a_rebuilt_window_is_not_unloaded_again() {
        assert!(!should_unload(check(Phase::Unloaded, 5)));
    }

    #[test]
    fn a_call_holds_the_window_open() {
        let mut in_call = check(hidden(), 3);
        in_call.in_call = true;
        assert!(!should_unload(in_call));
    }

    #[test]
    fn the_setting_off_never_unloads() {
        let mut off = check(hidden(), 3);
        off.policy = policy(false);
        assert!(!should_unload(off));
    }

    #[test]
    fn only_a_quit_the_user_asked_for_exits() {
        assert!(keeps_app_alive(None));
        assert!(!keeps_app_alive(Some(0)));
        assert!(!keeps_app_alive(Some(tauri::RESTART_EXIT_CODE)));
    }
}
