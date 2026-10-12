//! The queue of messages for busy Claude sessions: one per turn end, in order, never twice.

use client_core::{SessionKey, Status};
use desktop::queue::{Queue, SendId};

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
fn a_message_goes_now_only_to_an_idle_session_with_nothing_before_it() {
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
    // Messages already waiting go first: a new one goes behind them.
    q.add(&k, "older".into());
    assert!(
        !q.goes_now(&k, Some(Status::Idle)),
        "the queue keeps its order"
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
