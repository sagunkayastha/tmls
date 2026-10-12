//! The queue of messages for busy Claude sessions: one per turn end, in order, never twice.

use client_core::{SessionKey, Status};
use desktop::queue::Queue;

fn key(name: &str) -> SessionKey {
    SessionKey {
        host: "nas".to_string(),
        name: name.to_string(),
    }
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
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Idle)),
        Some("one".to_string())
    );
    q.sent(&k, "one", true);
    assert_eq!(q.count(&k), 1);
    assert_eq!(
        q.due(&k, Some(Status::Idle), Some(Status::Idle)),
        None,
        "no turn ended"
    );
    assert_eq!(
        q.due(&k, Some(Status::Waiting), Some(Status::Unseen)),
        Some("two".to_string())
    );
    q.sent(&k, "two", true);
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
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Idle)),
        Some("one".to_string())
    );
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Idle)),
        None,
        "still on its way"
    );
    q.sent(&k, "one", false);
    assert_eq!(q.count(&k), 1, "a failed send keeps it");
    assert_eq!(
        q.due(&k, Some(Status::Running), Some(Status::Idle)),
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
