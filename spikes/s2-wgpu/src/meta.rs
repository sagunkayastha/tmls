//! The parts of the `meta` line that have to be found out at run time: the monitor's refresh
//! rate (and where the value came from), when the line may be written, and whether the UI is
//! on. Pure decisions; `app` asks winit and the environment and passes the answers in.
//!
//! Refresh rate. winit's `Window::current_monitor()` is the monitor the window is *on*. On
//! Wayland that is the first output the surface has entered, and a compositor sends the enter
//! event only after the surface is mapped — after the first present. Asked before that, it
//! is `None` on every Wayland desktop, which is why `hz` was `0.0` there. So:
//! 1. the window's current monitor, if it reports a rate → `hz_source: "current_monitor"`;
//! 2. else, if exactly one monitor is connected and it reports a rate → `"only_monitor"`;
//! 3. else unknown (`hz: 0.0`, `"unknown"`). With several monitors and no current one there
//!    is no safe guess — not the first, not the fastest.
//!
//! When `meta` is written: `app` tries before each present and after each batch of events.
//! The line is written at the first try that has a rate; if none has one within `META_WAIT`
//! of the first try, or the app exits, it is written as unknown. It is written exactly once,
//! so it is not necessarily the first line of the log, nor before the first `frame` line.
//!
//! UI switch: `S2_WGPU_NO_UI=1` in the environment runs the window without sidebar, form and
//! menu (terminal at the window's corner, focus fixed on it) — the A/B variant for measuring
//! what the UI costs per frame. Not a command-line flag: the flag list is contract.

use std::time::Duration;

use s2_core::latency::{Event, Log, MetaExtra};

/// Where `meta.hz` came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HzSource {
    /// The monitor the window is on.
    CurrentMonitor,
    /// The window is on no monitor yet (or its monitor reports no rate), and exactly one
    /// monitor is connected.
    OnlyMonitor,
    Unknown,
}

impl HzSource {
    pub fn as_str(self) -> &'static str {
        match self {
            HzSource::CurrentMonitor => "current_monitor",
            HzSource::OnlyMonitor => "only_monitor",
            HzSource::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refresh {
    /// `None`: unknown.
    pub millihertz: Option<u32>,
    pub source: HzSource,
}

impl Refresh {
    /// The value of `meta.hz`; `0.0` when unknown (contract: never divide by it).
    pub fn hz(&self) -> f64 {
        self.millihertz.map_or(0.0, |mhz| mhz as f64 / 1000.0)
    }
}

/// How long `meta` waits for a refresh rate after the first try.
pub const META_WAIT: Duration = Duration::from_secs(1);

/// Picks the refresh rate. `current`: `Window::current_monitor()` mapped to its
/// `refresh_rate_millihertz()` (`None` = the window is on no monitor); `available`: the same
/// for every monitor of `Window::available_monitors()`. A rate of zero counts as none.
pub fn refresh(current: Option<Option<u32>>, available: &[Option<u32>]) -> Refresh {
    let rate = |mhz: Option<u32>| mhz.filter(|&mhz| mhz > 0);
    if let Some(mhz) = rate(current.flatten()) {
        return Refresh {
            millihertz: Some(mhz),
            source: HzSource::CurrentMonitor,
        };
    }
    if let [only] = available
        && let Some(mhz) = rate(*only)
    {
        return Refresh {
            millihertz: Some(mhz),
            source: HzSource::OnlyMonitor,
        };
    }
    Refresh {
        millihertz: None,
        source: HzSource::Unknown,
    }
}

/// May `meta` be written with this answer? `waited`: time since the first try.
pub fn write_now(refresh: &Refresh, waited: Duration, exiting: bool) -> bool {
    refresh.source != HzSource::Unknown || waited >= META_WAIT || exiting
}

/// The environment variable that switches the UI off.
pub const NO_UI_ENV: &str = "S2_WGPU_NO_UI";

/// Is the UI on, given the value of `NO_UI_ENV` (`None` = not set)? Off for any value except
/// an empty one, `0` and `false`.
pub fn ui_enabled(env: Option<&str>) -> bool {
    matches!(env.map(str::trim), None | Some("" | "0" | "false"))
}

/// The values of the `meta` line that do not come from this module.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixed<'a> {
    pub git: &'a str,
    pub profile: &'a str,
    pub scale: f64,
    pub present_mode: &'a str,
    pub load: &'a str,
}

/// Writes the `meta` line: the contract's fixed fields, then the two optional ones this
/// front-end knows — `ui` (false under `S2_WGPU_NO_UI`) and `hz_source`.
pub fn write(log: &Log, fixed: &Fixed, refresh: &Refresh, ui: bool) {
    let meta = Event::Meta {
        app: "wgpu".to_string(),
        git: fixed.git.to_string(),
        profile: fixed.profile.to_string(),
        hz: refresh.hz(),
        scale: fixed.scale,
        present_mode: fixed.present_mode.to_string(),
        load: fixed.load.to_string(),
    };
    let extra = MetaExtra {
        ui: Some(ui),
        hz_source: Some(refresh.source.as_str().to_string()),
    };
    log.meta_with(&meta, &extra);
}
