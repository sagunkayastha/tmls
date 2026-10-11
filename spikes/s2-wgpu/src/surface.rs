//! Decisions of the window code that can go wrong silently, kept free of winit and wgpu types so
//! they are tested: what to do with the result of acquiring the surface texture, and when the
//! app may leave after the child has exited.

use std::time::Duration;

/// `wgpu::CurrentSurfaceTexture` without the texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquired {
    Ready,
    Suboptimal,
    Outdated,
    Lost,
    Timeout,
    Occluded,
    Validation,
}

/// What the redraw does after asking for the surface texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Draw into the texture and present it; then configure the surface again if asked.
    Present { reconfigure: bool },
    /// No texture, nothing presented. The frames taken for this redraw are not on screen, so
    /// another redraw must be scheduled — `after` this long, after configuring the surface
    /// again if asked.
    Retry { reconfigure: bool, after: Duration },
    /// The surface is unusable: stop with an error.
    Fail,
}

/// How long to wait before trying again when the window is occluded. That status comes back
/// at once, so an immediate retry would spin.
pub const OCCLUDED_RETRY: Duration = Duration::from_millis(100);

pub fn after_acquire(acquired: Acquired) -> Next {
    let retry = |reconfigure, after| Next::Retry { reconfigure, after };
    match acquired {
        Acquired::Ready => Next::Present { reconfigure: false },
        Acquired::Suboptimal => Next::Present { reconfigure: true },
        Acquired::Outdated | Acquired::Lost => retry(true, Duration::ZERO),
        // The compositor did not hand out a buffer in time; the call itself waited.
        Acquired::Timeout => retry(false, Duration::ZERO),
        Acquired::Occluded => retry(false, OCCLUDED_RETRY),
        Acquired::Validation => Next::Fail,
    }
}

/// After the child has exited: may the app leave now?
/// `drained`: the newest frame taken has the child's last output (`Session::output_drained`).
/// `shown`: that frame has been presented — a redraw can end without a present (`Next::Retry`).
/// `deadline_passed`: the safety deadline, which ends the wait whatever the state.
pub fn may_exit(drained: bool, shown: bool, deadline_passed: bool) -> bool {
    deadline_passed || (drained && shown)
}
