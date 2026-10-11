//! Latency marks through a fake present path: what is stamped when, and when a `frame` line is
//! written.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use s2_wgpu::marks::{FrameLine, Marks, PresentPath, WORK_DONE_WAIT_NS, WorkDone};

/// A clock that moves 10 ns every time it is read.
#[derive(Clone, Default)]
struct Clock(Rc<Cell<u128>>);

impl Clock {
    fn tick(&self) -> u128 {
        self.0.set(self.0.get() + 10);
        self.0.get()
    }
    fn advance(&self, ns: u128) {
        self.0.set(self.0.get() + ns);
    }
}

/// Records when `submit` and `present` ran and keeps the work-done handles.
#[derive(Default)]
struct FakeGpu {
    clock: Clock,
    calls: RefCell<Vec<(&'static str, u128)>>,
    done: Vec<WorkDone>,
}

impl PresentPath for FakeGpu {
    fn submit(&mut self, done: WorkDone) {
        self.calls.borrow_mut().push(("submit", self.clock.tick()));
        self.done.push(done);
    }
    fn present(&mut self) {
        self.calls.borrow_mut().push(("present", self.clock.tick()));
    }
}

fn setup() -> (Marks<Vec<FrameLine>>, FakeGpu, Clock) {
    let clock = Clock::default();
    let reader = clock.clone();
    let marks = Marks::with_clock(Vec::new(), Box::new(move || reader.tick()));
    let gpu = FakeGpu {
        clock: clock.clone(),
        ..FakeGpu::default()
    };
    (marks, gpu, clock)
}

#[test]
fn submit_is_stamped_before_the_call_and_present_after_it_returns() {
    let (mut marks, mut gpu, _clock) = setup();
    marks.present_frame(7, &mut gpu);
    let calls = gpu.calls.borrow().clone();
    assert_eq!(
        calls.iter().map(|c| c.0).collect::<Vec<_>>(),
        ["submit", "present"]
    );
    let (in_submit, in_present) = (calls[0].1, calls[1].1);

    // The line waits for the GPU's work-done callback.
    assert!(marks.sink().is_empty());
    assert_eq!(marks.settle(), 1);
    assert!(marks.sink().is_empty());
    assert_eq!(gpu.done[0].get(), None);

    gpu.done[0].mark(5_000);
    assert_eq!(gpu.done[0].get(), Some(5_000));
    assert_eq!(marks.settle(), 0);
    let line = marks.sink()[0];
    assert_eq!((line.seq, line.t_work_done_ns), (7, Some(5_000)));
    assert!(
        line.t_submit_ns < in_submit && in_submit < in_present && in_present < line.t_present_ns,
        "{line:?} around submit at {in_submit} and present at {in_present}"
    );
    assert_eq!(marks.sink().len(), 1, "one line per presented frame");
    assert_eq!(marks.settle(), 0);
    assert_eq!(marks.sink().len(), 1);
}

#[test]
fn lines_are_written_in_frame_order() {
    // `Log::frame(seq)` takes the keys of every frame up to `seq`: a later frame written first
    // would take an earlier frame's keys and give them its own, later, present time.
    let (mut marks, mut gpu, _clock) = setup();
    for seq in [1, 2, 3] {
        marks.present_frame(seq, &mut gpu);
    }
    gpu.done[1].mark(900);
    gpu.done[2].mark(950);
    assert_eq!(
        marks.settle(),
        3,
        "frame 1 is not done: nothing may pass it"
    );
    assert!(marks.sink().is_empty());
    gpu.done[0].mark(990);
    assert_eq!(marks.settle(), 0);
    let lines = marks.sink();
    assert_eq!(lines.iter().map(|l| l.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(
        lines.iter().map(|l| l.t_work_done_ns).collect::<Vec<_>>(),
        [Some(990), Some(900), Some(950)]
    );
    assert!(
        lines
            .windows(2)
            .all(|w| w[0].t_present_ns < w[1].t_submit_ns)
    );
}

#[test]
fn a_callback_that_never_comes_does_not_hold_the_log_back() {
    let (mut marks, mut gpu, clock) = setup();
    marks.present_frame(1, &mut gpu);
    marks.present_frame(2, &mut gpu);
    gpu.done[1].mark(700);
    clock.advance(WORK_DONE_WAIT_NS - 1_000);
    assert_eq!(marks.settle(), 2, "still inside the wait");
    clock.advance(2_000);
    assert_eq!(marks.settle(), 0);
    let lines = marks.sink();
    assert_eq!((lines[0].seq, lines[0].t_work_done_ns), (1, None));
    assert_eq!((lines[1].seq, lines[1].t_work_done_ns), (2, Some(700)));
}

#[test]
fn finish_writes_what_is_pending() {
    let (mut marks, mut gpu, _clock) = setup();
    marks.present_frame(1, &mut gpu);
    marks.present_frame(2, &mut gpu);
    gpu.done[1].mark(700);
    marks.finish();
    let lines = marks.sink();
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].seq, lines[0].t_work_done_ns), (1, None));
    assert_eq!((lines[1].seq, lines[1].t_work_done_ns), (2, Some(700)));
    marks.finish();
    assert_eq!(marks.sink().len(), 2);
}
