//! The queue of messages for busy Claude sessions: one per turn end, in order, never twice.

use client_core::{SessionKey, Status, Tool};
use desktop::queue::{self, Queue, SendId, Turn};

fn key(name: &str) -> SessionKey {
    SessionKey {
        host: "nas".to_string(),
        name: name.to_string(),
    }
}

/// The text of a send that started.
fn text(started: Option<(SendId, String)>) -> Option<String> {
    started.map(|(_, text)| text)
}

#[test]
fn one_message_per_turn_end_in_order() {
    let (mut q, k) = (Queue::default(), key("w"));
    assert_eq!(q.add(&k, "one".into()), 1);
    assert_eq!(q.add(&k, "two".into()), 2);
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Running)),
        None
    );
    let (id, one) = q
        .due(&k, Some(Status::Running), Some(Status::Idle))
        .unwrap();
    assert_eq!(one, "one");
    assert_eq!(q.sent(id, true), Some(k.clone()));
    assert_eq!(q.count(&k), 1);
    assert_eq!(
        q.due(&k, Some(Status::Idle), Some(Status::Idle)),
        None,
        "no turn ended"
    );
    let (id, two) = q
        .due(&k, Some(Status::Waiting), Some(Status::Unseen))
        .unwrap();
    assert_eq!(two, "two");
    q.sent(id, true);
    assert_eq!((q.count(&k), q.keys().count()), (0, 0));
}

#[test]
fn flapping_failing_or_unlisted_sends_nothing() {
    let (mut q, k) = (Queue::default(), key("w"));
    q.add(&k, "one".into());
    for (before, after) in [
        (Some(Status::Running), Some(Status::Waiting)),
        (Some(Status::Waiting), Some(Status::Running)),
        (Some(Status::Running), Some(Status::Error)),
        (Some(Status::Running), None), // the host went away
        (None, Some(Status::Idle)),    // first seen idle: no turn ended here
    ] {
        assert_eq!(q.due(&k, before, after), None, "{before:?} -> {after:?}");
    }
    assert_eq!(q.count(&k), 1, "kept");
}

#[test]
fn a_message_in_flight_is_not_sent_twice() {
    let (mut q, k) = (Queue::default(), key("w"));
    q.add(&k, "one".into());
    let (id, _) = q
        .due(&k, Some(Status::Running), Some(Status::Idle))
        .unwrap();
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Idle)),
        None,
        "still on its way"
    );
    q.sent(id, false);
    assert_eq!(q.count(&k), 1, "a failed send keeps it");
    assert_eq!(
        text(q.due(&k, Some(Status::Running), Some(Status::Idle))),
        Some("one".to_string()),
        "tried at the next turn end"
    );
}

#[test]
fn rename_moves_the_queue_kill_drops_it_clear_empties_it() {
    let (mut q, old, new) = (Queue::default(), key("old"), key("new"));
    q.add(&old, "a".into());
    q.renamed(&old, &new);
    assert_eq!((q.count(&old), q.count(&new)), (0, 1));
    q.forget(&new);
    assert_eq!(q.count(&new), 0);
    q.add(&new, "b".into());
    q.clear(&new);
    assert_eq!(q.keys().count(), 0);
}

#[test]
fn a_rename_while_a_message_is_on_its_way_loses_nothing() {
    let (mut q, old, new) = (Queue::default(), key("old"), key("new"));
    q.add(&old, "a".into());
    q.add(&old, "b".into());
    let (id, _) = q
        .due(&old, Some(Status::Running), Some(Status::Idle))
        .unwrap();
    q.renamed(&old, &new);
    assert_eq!(
        q.sent(id, true),
        Some(new.clone()),
        "the answer finds the session by its new name"
    );
    assert_eq!(q.count(&new), 1, "the delivered head left the queue");
    assert_eq!(
        text(q.due(&new, Some(Status::Running), Some(Status::Idle))),
        Some("b".to_string()),
        "the session is free for its next turn end"
    );
}

#[test]
fn clear_and_kill_free_a_session_whose_message_is_on_its_way() {
    for kill in [false, true] {
        let (mut q, k) = (Queue::default(), key("w"));
        q.add(&k, "a".into());
        let (id, _) = q
            .due(&k, Some(Status::Running), Some(Status::Idle))
            .unwrap();
        if kill {
            q.forget(&k);
        } else {
            q.clear(&k);
        }
        // Its answer belongs to nothing now and takes nothing new.
        q.add(&k, "a".into());
        assert_eq!(q.sent(id, true), None, "kill {kill}");
        assert_eq!(q.count(&k), 1, "kill {kill}");
        assert_eq!(
            text(q.due(&k, Some(Status::Running), Some(Status::Idle))),
            Some("a".to_string()),
            "kill {kill}"
        );
    }
}

#[test]
fn a_message_goes_now_only_to_an_idle_session_with_nothing_on_its_way() {
    let (mut q, k) = (Queue::default(), key("w"));
    assert!(q.goes_now(&k, Some(Status::Idle)));
    assert!(q.goes_now(&k, Some(Status::Unseen)));
    assert!(!q.goes_now(&k, Some(Status::Running)), "busy");
    assert!(!q.goes_now(&k, Some(Status::Waiting)), "busy");
    // Sent now: it goes through the queue, so it is on its way until it is answered.
    q.add(&k, "now".into());
    let (id, now) = q.start(&k).unwrap();
    assert_eq!(now, "now");
    assert_eq!(q.start(&k), None, "one at a time");
    assert!(
        !q.goes_now(&k, Some(Status::Idle)),
        "one on its way: the next one waits"
    );
    q.sent(id, true);
    assert!(q.goes_now(&k, Some(Status::Idle)));
}

#[test]
fn a_head_left_behind_goes_with_the_next_message_to_an_idle_session() {
    let (mut q, k) = (Queue::default(), key("w"));
    // The head stayed: its send failed while the session was idle (or the host was away when
    // the turn ended), and no turn ends any more to send it.
    q.add(&k, "first".into());
    let (id, _) = q.start(&k).unwrap();
    q.sent(id, false);
    assert_eq!(
        q.turn_ended(&k, Some(Status::Idle), Some(Status::Idle), Some("%1")),
        Turn::Wait
    );
    // A new message for the idle session starts the queue: its head first, the order kept.
    let id = match q.submit(&k, Some(Status::Idle), Some("%1"), "second".into()) {
        Turn::Send { id, text, .. } => {
            assert_eq!(text, "first");
            id
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(q.count(&k), 2);
    q.sent(id, true);
    assert_eq!(q.count(&k), 1);
    assert_eq!(
        text(q.due(&k, Some(Status::Running), Some(Status::Idle))),
        Some("second".to_string())
    );
    // Busy, or one on its way: the new one only waits.
    let (mut q, k) = (Queue::default(), key("v"));
    q.add(&k, "first".into());
    assert_eq!(
        q.submit(&k, Some(Status::Running), Some("%1"), "x".into()),
        Turn::Wait
    );
    q.start(&k).unwrap();
    assert_eq!(
        q.submit(&k, Some(Status::Idle), Some("%1"), "y".into()),
        Turn::Wait
    );
}

/// The listing of host `nas` at clock 1000: the session `w`, its last output at `activity`, and
/// with `claude` the status file of a busy Claude in its pane `%1`.
fn listing(claude: bool, activity: u64) -> client_core::HostSnapshot {
    let mut out = format!("1000\nw:1:0:{activity}\n");
    out += match claude {
        true => {
            "tool w:claude\n---\n{\"pid\":4242,\"sessionId\":\"aaaa\",\"status\":\"busy\",\"statusUpdatedAt\":900000,\"tmux\":\"w:@1.%1\"}\n"
        }
        false => "tool w:bash\n---\n",
    };
    client_core::parse_listing("nas", &out)
}

#[test]
fn a_message_goes_only_to_claude_s_own_pane() {
    let snap = listing(true, 990);
    assert_eq!(queue::claude_pane(snap.sessions.first()), Some("%1"));
    assert_eq!(queue::claude_pane(None), None, "not listed");
    let mut info = snap.sessions[0].clone();
    info.pane = None;
    assert_eq!(
        queue::claude_pane(Some(&info)),
        None,
        "Claude without a pane"
    );
    info.pane = Some("w:".to_string());
    assert_eq!(queue::claude_pane(Some(&info)), None, "not a pane id");
    info.pane = Some("%1".to_string());
    info.tool = Tool::Other;
    assert_eq!(
        queue::claude_pane(Some(&info)),
        None,
        "not Claude's any more"
    );
    info.tool = Tool::Claude;
    info.key.host = client_core::KITTY.to_string();
    assert_eq!(queue::claude_pane(Some(&info)), None, "outside tmux");
}

#[test]
fn claude_gone_from_the_listing_sends_nothing_and_keeps_the_queue() {
    let (mut q, k) = (Queue::default(), key("w"));
    q.add(&k, "echo $(rm -rf ~)".into());
    let busy = listing(true, 990);
    assert_eq!(busy.sessions[0].status, Status::Running);
    // Claude exits: its status file is dropped, the row is the shell it leaves, quiet now.
    let shell = listing(false, 900);
    assert_eq!(
        (shell.sessions[0].tool, shell.sessions[0].status),
        (Tool::Other, Status::Idle)
    );
    let pane = queue::claude_pane(shell.sessions.first());
    assert_eq!(
        q.turn_ended(&k, Some(Status::Running), Some(Status::Idle), pane),
        Turn::Gone(1)
    );
    assert_eq!(q.count(&k), 1, "kept");
    assert_eq!(
        queue::gone_note("w", 1),
        "Claude is gone from w; 1 kept".to_string()
    );
    // Nothing was started: when Claude is back in its pane, the next turn end sends it.
    let back = listing(true, 990);
    match q.turn_ended(
        &k,
        Some(Status::Running),
        Some(Status::Idle),
        queue::claude_pane(back.sessions.first()),
    ) {
        Turn::Send { text, pane, .. } => {
            assert_eq!((text.as_str(), pane.as_str()), ("echo $(rm -rf ~)", "%1"))
        }
        other => panic!("{other:?}"),
    }
    // No turn ended, or nothing queued: nothing to say.
    let other = key("x");
    assert_eq!(
        q.turn_ended(&other, Some(Status::Running), Some(Status::Idle), None),
        Turn::Wait
    );
    q.add(&other, "a".into());
    assert_eq!(
        q.turn_ended(&other, Some(Status::Idle), Some(Status::Idle), None),
        Turn::Wait
    );
}

#[test]
fn a_message_submitted_for_a_session_claude_left_is_kept() {
    let (mut q, k) = (Queue::default(), key("w"));
    assert_eq!(
        q.submit(&k, Some(Status::Idle), None, "hi".into()),
        Turn::Gone(1)
    );
    assert_eq!(q.count(&k), 1);
    // With Claude's pane, an idle session gets it at once.
    match q.submit(&key("v"), Some(Status::Idle), Some("%2"), "hi".into()) {
        Turn::Send { text, pane, .. } => assert_eq!((text.as_str(), pane.as_str()), ("hi", "%2")),
        other => panic!("{other:?}"),
    }
    // A busy one: it waits.
    assert_eq!(
        q.submit(&key("u"), Some(Status::Running), Some("%3"), "hi".into()),
        Turn::Wait
    );
}

#[test]
fn busy_is_running_or_waiting() {
    assert!(Queue::busy(Some(Status::Running)) && Queue::busy(Some(Status::Waiting)));
    for idle in [
        Some(Status::Idle),
        Some(Status::Unseen),
        Some(Status::Error),
        None,
    ] {
        assert!(!Queue::busy(idle), "{idle:?}");
    }
}
