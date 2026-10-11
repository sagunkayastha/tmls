//! What goes into the `meta` line besides the fixed values: the refresh rate and where it came
//! from, when the line may be written, and whether the UI is on.

use std::time::Duration;

use s2_core::latency::{Event, Log, MetaExtra};
use s2_wgpu::meta::{Fixed, HzSource, META_WAIT, Refresh, refresh, ui_enabled, write, write_now};

#[test]
fn refresh_rate_comes_from_the_windows_monitor_first() {
    // The monitor the window is on, whatever else is connected.
    let r = refresh(Some(Some(165_000)), &[Some(60_000), Some(165_000)]);
    assert_eq!(
        r,
        Refresh {
            millihertz: Some(165_000),
            source: HzSource::CurrentMonitor
        }
    );
    assert_eq!(r.hz(), 165.0);
    assert_eq!(refresh(Some(Some(59_951)), &[]).hz(), 59.951);
    assert_eq!(
        refresh(Some(Some(144_000)), &[Some(60_000)]).source,
        HzSource::CurrentMonitor
    );
}

#[test]
fn the_only_monitor_stands_in_when_the_window_has_none_yet() {
    // Wayland before the surface is mapped: the window is on no monitor.
    let r = refresh(None, &[Some(165_000)]);
    assert_eq!(
        r,
        Refresh {
            millihertz: Some(165_000),
            source: HzSource::OnlyMonitor
        }
    );
    assert_eq!(r.hz(), 165.0);
    // Also when the window's monitor is known but reports no rate.
    assert_eq!(
        refresh(Some(None), &[Some(75_000)]).source,
        HzSource::OnlyMonitor
    );
    assert_eq!(
        refresh(Some(Some(0)), &[Some(75_000)]).millihertz,
        Some(75_000)
    );
}

#[test]
fn several_monitors_are_never_guessed_from() {
    let unknown = Refresh {
        millihertz: None,
        source: HzSource::Unknown,
    };
    // Two monitors and no idea which one the window is on: not the highest, not the first.
    assert_eq!(refresh(None, &[Some(60_000), Some(165_000)]), unknown);
    assert_eq!(
        refresh(None, &[Some(165_000), Some(165_000)]),
        unknown,
        "even if equal"
    );
    assert_eq!(refresh(Some(None), &[Some(60_000), Some(165_000)]), unknown);
    // Nothing to ask, or monitors that report no rate (a headless output) or zero.
    assert_eq!(refresh(None, &[]), unknown);
    assert_eq!(refresh(None, &[None]), unknown);
    assert_eq!(refresh(None, &[Some(0)]), unknown);
    assert_eq!(refresh(Some(Some(0)), &[]), unknown);
    assert_eq!(refresh(Some(None), &[None]), unknown);
    // Unknown is written as 0.0 (contract: "treat as unknown; never divide by it").
    assert_eq!(unknown.hz(), 0.0);
    assert_eq!(
        [
            HzSource::CurrentMonitor,
            HzSource::OnlyMonitor,
            HzSource::Unknown
        ]
        .map(HzSource::as_str),
        ["current_monitor", "only_monitor", "unknown"]
    );
}

#[test]
fn meta_waits_for_the_rate_but_not_for_ever() {
    let known = refresh(Some(Some(165_000)), &[]);
    let only = refresh(None, &[Some(60_000)]);
    let unknown = refresh(None, &[Some(60_000), Some(165_000)]);
    let zero = Duration::ZERO;
    // A known rate is written at once.
    assert!(write_now(&known, zero, false));
    assert!(write_now(&only, zero, false));
    // An unknown one waits: the window may enter its monitor after the first present.
    assert!(!write_now(&unknown, zero, false));
    assert!(!write_now(
        &unknown,
        META_WAIT - Duration::from_millis(1),
        false
    ));
    // …until the wait is over, or the app exits: then it is written as unknown.
    assert!(write_now(&unknown, META_WAIT, false));
    assert!(write_now(&unknown, zero, true));
    assert!(META_WAIT >= Duration::from_millis(200) && META_WAIT <= Duration::from_secs(5));
}

#[test]
fn the_ui_is_on_unless_the_switch_says_otherwise() {
    assert!(ui_enabled(None), "not set");
    assert!(ui_enabled(Some("")), "set but empty");
    assert!(ui_enabled(Some("0")));
    assert!(ui_enabled(Some("false")));
    assert!(!ui_enabled(Some("1")));
    assert!(!ui_enabled(Some("true")));
    assert!(!ui_enabled(Some(" 1 ")));
    assert!(!ui_enabled(Some("yes")));
    assert_eq!(s2_wgpu::meta::NO_UI_ENV, "S2_WGPU_NO_UI");
}

#[test]
fn the_meta_line_says_whether_the_ui_was_on_and_where_hz_came_from() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("meta_line");
    let log = Log::create(&dir).unwrap();
    let fixed = Fixed {
        git: "abc1234",
        profile: "release",
        scale: 1.15,
        present_mode: "fifo",
        load: "quiet",
    };
    write(&log, &fixed, &refresh(Some(Some(165_000)), &[]), true);
    write(&log, &fixed, &refresh(None, &[Some(60_000)]), false);
    write(&log, &fixed, &refresh(None, &[]), false);
    log.flush();
    let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        r#"{"ev":"meta","app":"wgpu","git":"abc1234","profile":"release","hz":165.0,"scale":1.15,"present_mode":"fifo","load":"quiet","ui":true,"hz_source":"current_monitor"}"#
    );
    let expected = [
        (165.0, true, "current_monitor"),
        (60.0, false, "only_monitor"),
        (0.0, false, "unknown"),
    ];
    assert_eq!(lines.len(), expected.len());
    for (line, (hz, ui, source)) in lines.iter().zip(expected) {
        // The contract's fixed fields, readable as before…
        assert_eq!(
            serde_json::from_str::<Event>(line).unwrap(),
            Event::Meta {
                app: "wgpu".to_string(),
                git: "abc1234".to_string(),
                profile: "release".to_string(),
                hz,
                scale: 1.15,
                present_mode: "fifo".to_string(),
                load: "quiet".to_string(),
            }
        );
        // …and both extras, always (this front-end knows both).
        assert_eq!(
            serde_json::from_str::<MetaExtra>(line).unwrap(),
            MetaExtra {
                ui: Some(ui),
                hz_source: Some(source.to_string()),
            }
        );
    }
}
