//! Surface status → what the redraw does next; when the app may exit after the child has.

use std::time::Duration;

use s2_wgpu::surface::{Acquired, Next, OCCLUDED_RETRY, after_acquire, may_exit};

#[test]
fn a_texture_is_presented() {
    assert_eq!(
        after_acquire(Acquired::Ready),
        Next::Present { reconfigure: false }
    );
    // Still drawable; the surface is configured again once the frame is out.
    assert_eq!(
        after_acquire(Acquired::Suboptimal),
        Next::Present { reconfigure: true }
    );
}

#[test]
fn no_texture_always_schedules_another_redraw() {
    // The frames (and their keys) were taken before the texture was asked for. Without another
    // redraw the echo stays undrawn until some unrelated event arrives.
    for status in [Acquired::Outdated, Acquired::Lost] {
        assert_eq!(
            after_acquire(status),
            Next::Retry {
                reconfigure: true,
                after: Duration::ZERO
            },
            "{status:?}"
        );
    }
    assert_eq!(
        after_acquire(Acquired::Timeout),
        Next::Retry {
            reconfigure: false,
            after: Duration::ZERO
        }
    );
    // Occluded returns at once: retrying immediately would spin, so it waits a little.
    assert_eq!(
        after_acquire(Acquired::Occluded),
        Next::Retry {
            reconfigure: false,
            after: OCCLUDED_RETRY
        }
    );
    assert!(OCCLUDED_RETRY > Duration::ZERO && OCCLUDED_RETRY <= Duration::from_millis(250));
    assert_eq!(after_acquire(Acquired::Validation), Next::Fail);
}

#[test]
fn the_app_leaves_only_after_the_last_frame_was_presented() {
    let (yes, no) = (true, false);
    // (drained, shown, deadline passed)
    assert!(may_exit(yes, yes, no), "last output drawn and presented");
    assert!(
        !may_exit(yes, no, no),
        "drained but the present did not happen (no texture)"
    );
    assert!(
        !may_exit(no, yes, no),
        "what is on screen is not the last output yet"
    );
    assert!(!may_exit(no, no, no));
    // The deadline ends the wait whatever the state.
    for drained in [yes, no] {
        for shown in [yes, no] {
            assert!(may_exit(drained, shown, yes));
        }
    }
}
