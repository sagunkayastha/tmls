//! The poller against fake hosts: `sh -c …` commands made by an injected command factory.
//! Every wait has a timeout that fails the test.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use client_core::listing::{HostSnapshot, Status};
use client_core::poll::{Poller, SIGN_IN_REQUIRED, list_once, period_from};

/// The tests look at this process's threads by name, so they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fresh directory under cargo's target tmp dir (never /tmp).
fn dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("poll-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sh(script: impl AsRef<str>) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", script.as_ref()]);
    cmd
}

fn hosts(names: &[&str]) -> Vec<String> {
    names.iter().map(|h| h.to_string()).collect()
}

/// Starts a poller whose snapshots arrive, with their arrival time, on the returned channel.
fn start(
    names: &[&str],
    period: Duration,
    timeout: Duration,
    command: impl Fn(&str) -> Command + Send + Sync + 'static,
) -> (Poller, Receiver<(Instant, HostSnapshot)>) {
    let (tx, rx) = channel();
    let poller = Poller::start_with(
        &hosts(names),
        Box::new(command),
        period,
        timeout,
        Box::new(move |snap| {
            let _ = tx.send((Instant::now(), snap));
        }),
    );
    (poller, rx)
}

fn next(rx: &Receiver<(Instant, HostSnapshot)>, within: Duration) -> (Instant, HostSnapshot) {
    rx.recv_timeout(within)
        .unwrap_or_else(|e| panic!("no snapshot within {within:?}: {e}"))
}

fn poll_threads() -> Vec<String> {
    let mut names = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
        let name = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        if name.starts_with("tmls-poll-") {
            names.push(name.trim().to_string());
        }
    }
    names
}

/// Fails unless every polling thread is gone. A joined thread can stay in /proc for a moment
/// more (the join returns before the kernel has removed the task), hence the short wait.
fn assert_no_poll_threads() {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !poll_threads().is_empty() {
        assert!(
            Instant::now() < deadline,
            "polling threads left: {:?}",
            poll_threads()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Live processes in process group `pgid` (the poller starts each child in a group of its own).
fn group_members(pgid: u32) -> Vec<u32> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // "PID (COMM) STATE PPID PGRP …"; COMM may hold spaces and parentheses
        let Some((_, rest)) = stat.rsplit_once(") ") else {
            continue;
        };
        let mut fields = rest.split(' ');
        let (state, pgrp) = (
            fields.next(),
            fields.nth(1).and_then(|f| f.parse::<u32>().ok()),
        );
        if pgrp == Some(pgid) && state != Some("Z") {
            pids.push(pid);
        }
    }
    pids
}

/// Waits for `path` to hold a pid (a fake host writes its own there).
fn pid_in(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(pid) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "the fake host never wrote {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_group_gone(pgid: u32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !group_members(pgid).is_empty() {
        assert!(
            Instant::now() < deadline,
            "processes left in group {pgid}: {:?}",
            group_members(pgid)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn snapshots_arrive_every_period() {
    let _alone = alone();
    let d = dir("period");
    let counter = d.join("n");
    let period = Duration::from_millis(250);
    // a host whose one session has one more client attached at every listing
    let script = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{0}'; echo 1000; echo \"work:1:$n:990\"",
        counter.display()
    );
    let (poller, rx) = start(&["a"], period, Duration::from_secs(5), move |_| sh(&script));

    let begun = Instant::now();
    let arrivals: Vec<(Instant, HostSnapshot)> =
        (0..4).map(|_| next(&rx, Duration::from_secs(3))).collect();
    assert!(
        arrivals[0].0 - begun < period,
        "the first listing starts at once"
    );
    for (i, (_, snap)) in arrivals.iter().enumerate() {
        assert!(snap.online && snap.host == "a");
        assert_eq!(snap.sessions[0].attached, i as u32 + 1);
    }
    for pair in arrivals.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            gap >= period * 3 / 4 && gap < period * 3,
            "a period apart, not {gap:?}"
        );
    }
    drop(poller);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_snapshot_is_delivered_only_when_it_changes() {
    let _alone = alone();
    let d = dir("changes");
    let listing = d.join("listing");
    std::fs::write(&listing, "1000\nwork:1:0:990\n").unwrap();
    let period = Duration::from_millis(100);
    let script = format!("cat '{}'", listing.display());
    let (poller, rx) = start(&["a"], period, Duration::from_secs(5), move |_| sh(&script));

    let (_, first) = next(&rx, Duration::from_secs(3));
    assert_eq!(first.sessions[0].status, Status::Running);
    assert_eq!(
        rx.recv_timeout(period * 6).unwrap_err(),
        RecvTimeoutError::Timeout,
        "the same listing again is not news"
    );

    std::fs::write(&listing, "1000\nwork:1:0:990\nmore:1:0:5\n").unwrap();
    assert_eq!(next(&rx, Duration::from_secs(3)).1.sessions.len(), 2);

    // the host stops answering with a listing: offline, said once
    std::fs::write(&listing, "garbage\n").unwrap();
    let (_, gone) = next(&rx, Duration::from_secs(3));
    assert!(!gone.online && gone.sessions.is_empty());
    assert_eq!(
        rx.recv_timeout(period * 6).unwrap_err(),
        RecvTimeoutError::Timeout
    );

    std::fs::write(&listing, "1000\nwork:1:0:990\n").unwrap();
    assert_eq!(next(&rx, Duration::from_secs(3)).1, first);
    drop(poller);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn growing_output_age_alone_is_not_news() {
    let _alone = alone();
    let d = dir("age");
    let (counter, activity) = (d.join("n"), d.join("activity"));
    std::fs::write(&activity, "10").unwrap();
    let period = Duration::from_millis(100);
    // a real host: its clock moves on between listings, so an idle session's age grows each time
    let script = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); n=$((n+7)); echo $n > '{0}'; echo $((1000+n)); echo \"work:1:0:$(cat '{1}')\"",
        counter.display(),
        activity.display()
    );
    let (poller, rx) = start(&["a"], period, Duration::from_secs(5), move |_| sh(&script));

    let (_, first) = next(&rx, Duration::from_secs(3));
    assert_eq!((first.sessions[0].status, first.sessions[0].last_output_age_s), (Status::Idle, Some(997)));
    assert_eq!(rx.recv_timeout(period * 8).unwrap_err(), RecvTimeoutError::Timeout, "only the age changed");
    let listed: u64 = std::fs::read_to_string(&counter).unwrap().trim().parse().unwrap();
    assert!(listed >= 3 * 7, "the host was listed again meanwhile ({listed})");

    // the session prints again: its status changes, and that is delivered with the current age
    std::fs::write(&activity, "999999").unwrap();
    let (_, busy) = next(&rx, Duration::from_secs(3));
    assert_eq!((busy.sessions[0].status, busy.sessions[0].last_output_age_s), (Status::Running, Some(0)));
    assert_eq!(rx.recv_timeout(period * 4).unwrap_err(), RecvTimeoutError::Timeout);
    drop(poller);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn slow_host_does_not_block_the_others() {
    let _alone = alone();
    let d = dir("slow");
    let period = Duration::from_millis(500);
    let timeout = Duration::from_millis(1500);
    let runs = Arc::new(AtomicUsize::new(0));
    let (pids, counter) = (d.clone(), runs.clone());
    let (poller, rx) = start(&["a", "slow", "b"], period, timeout, move |host| {
        if host == "slow" {
            // like an ssh that hangs: says nothing for 60 s; `sleep` is a grandchild of the poller
            let n = counter.fetch_add(1, Ordering::SeqCst);
            sh(format!(
                "echo $$ > '{}/slow-{n}.pid'; sleep 60; true",
                pids.display()
            ))
        } else {
            sh(format!("echo 1000; echo '{host}-work:1:0:990'"))
        }
    });
    let begun = Instant::now();

    let mut quick = Vec::new();
    let slow = loop {
        let (at, snap) = next(&rx, Duration::from_secs(5));
        if snap.host == "slow" {
            break (at, snap);
        }
        assert!(
            at - begun < period,
            "{} delivered after {:?}, not within one period",
            snap.host,
            at - begun
        );
        assert!(snap.online);
        quick.push(snap.sessions[0].key.name.clone());
    };
    quick.sort();
    assert_eq!(
        quick,
        ["a-work", "b-work"],
        "both quick hosts delivered before the slow one gave up"
    );

    let (at, snap) = slow;
    let after = at - begun;
    assert!(
        after >= timeout && after < timeout + Duration::from_secs(1),
        "reported after {after:?}"
    );
    assert!(!snap.online && snap.sessions.is_empty());
    assert!(
        snap.error
            .as_deref()
            .is_some_and(|e| e.contains("no answer")),
        "{:?}",
        snap.error
    );
    assert_group_gone(pid_in(&d.join("slow-0.pid")));

    drop(poller);
    // later listings of the slow host were cut short by the drop (one may not have got to write its pid)
    for n in 0..runs.load(Ordering::SeqCst) {
        let pid = std::fs::read_to_string(d.join(format!("slow-{n}.pid")))
            .ok()
            .and_then(|text| text.trim().parse().ok());
        if let Some(pid) = pid {
            assert_group_gone(pid);
        }
    }
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn drop_stops_threads_and_children() {
    let _alone = alone();
    let d = dir("drop");
    let pidfile = d.join("hang.pid");
    let script = format!("echo $$ > '{}'; sleep 60; true", pidfile.display());
    let (poller, rx) = start(
        &["quick", "hang"],
        Duration::from_millis(100),
        Duration::from_secs(30),
        move |host| {
            if host == "hang" {
                sh(&script)
            } else {
                sh("echo 1000; echo 'work:1:0:990'")
            }
        },
    );
    assert_eq!(next(&rx, Duration::from_secs(3)).1.host, "quick");
    let hung = pid_in(&pidfile);
    assert!(
        !group_members(hung).is_empty(),
        "the hanging listing is running"
    );
    let mut threads = poll_threads();
    threads.sort();
    assert_eq!(threads, ["tmls-poll-hang", "tmls-poll-quick"]);

    let begun = Instant::now();
    drop(poller);
    assert!(
        begun.elapsed() < Duration::from_secs(2),
        "drop took {:?}",
        begun.elapsed()
    );
    assert_no_poll_threads();
    assert_group_gone(hung);
    // the callback went with the threads: nothing more can arrive
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap_err(),
        RecvTimeoutError::Disconnected
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn refresh_now_lists_at_once() {
    let _alone = alone();
    let d = dir("refresh");
    let counter = d.join("n");
    let script = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{0}'; echo 1000; echo \"work:1:$n:990\"",
        counter.display()
    );
    let (poller, rx) = start(
        &["a", "b"],
        Duration::from_secs(60),
        Duration::from_secs(5),
        move |host| {
            if host == "a" {
                sh(&script)
            } else {
                sh("echo 1000")
            }
        },
    );
    let mut first: Vec<String> = (0..2)
        .map(|_| next(&rx, Duration::from_secs(3)).1.host)
        .collect();
    first.sort();
    assert_eq!(first, ["a", "b"]);

    for _ in 0..2 {
        poller.refresh_now();
        let (_, snap) = next(&rx, Duration::from_secs(3)); // b lists again too, and has no news
        assert_eq!(snap.host, "a");
    }
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(300)).unwrap_err(),
        RecvTimeoutError::Timeout
    );
    drop(poller);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_failed_listing_is_offline_with_the_reason() {
    let _alone = alone();
    let t = Duration::from_secs(5);
    // test_hosts.py::test_list_host_sign_in_required
    for stderr in [
        "Permission denied (publickey,password).",
        "Host key verification failed.",
        "keyboard-interactive authentication required",
        "Password + OTP: ",
    ] {
        let snap = list_once("box", sh(format!("echo '{stderr}' >&2; exit 255")), t);
        assert_eq!(
            (snap.online, snap.error.as_deref()),
            (false, Some(SIGN_IN_REQUIRED)),
            "{stderr}"
        );
    }
    // test_hosts.py::test_list_host_offline_when_ssh_fails
    let snap = list_once(
        "box",
        sh(
            "echo 'banner' >&2; echo 'ssh: connect to host box port 22: Connection refused' >&2; exit 255",
        ),
        t,
    );
    assert_eq!(
        (snap.online, snap.error.as_deref()),
        (
            false,
            Some("ssh: connect to host box port 22: Connection refused")
        )
    );
    assert_eq!(snap.host, "box");
    // a listing cut short by a dropped connection is not shown as the host's sessions
    let snap = list_once("box", sh("echo 1000; echo 'work:1:0:990'; exit 255"), t);
    assert!(!snap.online && snap.sessions.is_empty() && snap.error.is_some());
    // test_hosts.py::test_list_host_offline_when_output_is_garbage
    assert!(!list_once("box", sh("echo garbage"), t).online);
    let snap = list_once(
        "box",
        Command::new("/nonexistent/tmlsrs-test-no-such-program"),
        t,
    );
    assert!(!snap.online && snap.error.is_some());
    assert_no_poll_threads(); // list_once runs on the caller's thread
}

#[test]
fn the_script_arrives_on_stdin_and_long_listings_are_read_whole() {
    let _alone = alone();
    // `sh -s` as on a real host, with a stub tmux; 5000 sessions is far more than a pipe holds
    let d = dir("stdin");
    std::fs::write(
        d.join("tmux"),
        "#!/bin/sh\ncase \"$*\" in *list-windows*) i=0; while [ $i -lt 5000 ]; do echo \"s$i:1:0:5\"; i=$((i+1)); done;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(
        d.join("tmux"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let mut cmd = Command::new("sh");
    cmd.arg("-s")
        .env("PATH", format!("{}:/usr/bin:/bin", d.display()))
        .env("HOME", &d);
    let snap = list_once("me", cmd, Duration::from_secs(20));
    assert!(snap.online, "{:?}", snap.error);
    assert_eq!(snap.sessions.len(), 5000);
    assert_eq!(snap.sessions[4999].key.name, "s4999");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_listing_that_differs_only_in_activity_is_not_delivered() {
    let _one = alone();
    // The window's output time changes at every listing; its status (running) does not.
    let (poller, rx) = start(&["a"], Duration::from_millis(100), Duration::from_secs(5), |_| {
        sh("cat >/dev/null; n=$(od -An -N2 -tu2 /dev/urandom | tr -d ' '); echo 100; echo \"w:1:0:$((100 + n))\"")
    });
    next(&rx, Duration::from_secs(5));
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "new output alone is no news for the poller");
    drop(poller);
}

#[test]
fn force_next_delivers_the_next_listing_even_when_unchanged() {
    let _one = alone();
    let (poller, rx) = start(&["a"], Duration::from_millis(100), Duration::from_secs(5), |_| sh("cat >/dev/null; echo 100"));
    next(&rx, Duration::from_secs(5));
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err(), "an unchanged listing is not delivered");
    poller.force_next();
    next(&rx, Duration::from_secs(2));
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err(), "only the next one");
    drop(poller);
}

#[test]
fn the_period_comes_from_tmls_rs_poll_s() {
    assert_eq!(period_from(None), Duration::from_secs(5));
    assert_eq!(period_from(Some("60")), Duration::from_secs(60));
    assert_eq!(period_from(Some(" 2 ")), Duration::from_secs(2));
    for bad in ["", "0", "-3", "x", "3601", "1.5"] {
        assert_eq!(period_from(Some(bad)), Duration::from_secs(5), "{bad:?}");
    }
}

#[test]
fn slow_down_gives_those_hosts_their_own_period_and_back_at_once() {
    let _one = alone();
    let d = dir("slow");
    let count = {
        let d = d.clone();
        move |host: &str| std::fs::read_to_string(d.join(host)).map_or(0, |text| text.lines().count())
    };
    let logs = d.clone();
    let (poller, _rx) = start(&["here", "far"], Duration::from_millis(100), Duration::from_secs(5), move |host| {
        sh(format!("echo x >>'{}'; cat >/dev/null; echo 100", logs.join(host).display()))
    });
    std::thread::sleep(Duration::from_millis(350));
    poller.slow_down(&hosts(&["far"]), Some(Duration::from_secs(60)));
    let (here, far) = (count("here"), count("far"));
    std::thread::sleep(Duration::from_millis(700));
    assert!(count("here") >= here + 3, "the other host keeps its period: {here} -> {}", count("here"));
    // A listing that had started already may end; no other one starts within the minute.
    assert!(count("far") <= far + 1, "far waits its minute: {far} -> {}", count("far"));
    let far = count("far");
    poller.slow_down(&hosts(&["far"]), None);
    std::thread::sleep(Duration::from_millis(450));
    assert!(count("far") >= far + 2, "back to the poller's period at once: {far} -> {}", count("far"));
    drop(poller);
    assert_no_poll_threads();
}

#[test]
fn a_host_that_wants_a_sign_in_says_so() {
    let _alone = alone();
    let refused = "cat >/dev/null; echo 'u@ph: Permission denied (publickey).' >&2; exit 255";
    assert!(list_once("ph", sh(refused), Duration::from_secs(5)).needs_sign_in());
    let unreachable = "cat >/dev/null; echo 'ssh: connect to host ph port 22: No route to host' >&2; exit 255";
    assert!(!list_once("ph", sh(unreachable), Duration::from_secs(5)).needs_sign_in());
    let up = list_once("ph", sh("cat >/dev/null; echo 100"), Duration::from_secs(5));
    assert!(up.online && !up.needs_sign_in());
}
