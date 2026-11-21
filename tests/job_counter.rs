#![allow(unsafe_code)]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::error::Error;
use vas::sync::{JobCounter, Queue};
use vas::{Full, Inline};

type Result = core::result::Result<(), Box<dyn Error>>;

#[test]
fn counts_jobs_down_to_done() {
    let counter = JobCounter::new();
    assert!(counter.is_done());
    assert_eq!(counter.add(2), Ok(()));
    assert_eq!(counter.pending(), 2);
    assert!(!counter.is_done());
    assert_eq!(counter.add(3), Ok(()));
    assert_eq!(counter.pending(), 5);
    for _ in 0..4 {
        assert!(!counter.done());
    }
    assert!(!counter.is_done());
    // Only the call that finishes the last job says so.
    assert!(counter.done());
    assert!(counter.is_done());
    assert_eq!(counter.pending(), 0);
    // Reusable: adding again starts a new round.
    assert_eq!(counter.add(1), Ok(()));
    assert!(!counter.is_done());
    assert!(counter.done());
    assert!(JobCounter::default().is_done());
}

#[test]
fn done_on_a_finished_counter_is_ignored() {
    let counter = JobCounter::new();
    assert!(!counter.done());
    assert!(!counter.done());
    assert!(counter.is_done());
    assert_eq!(counter.pending(), 0);
    assert_eq!(counter.add(2), Ok(()));
    assert_eq!(counter.pending(), 2);
}

#[test]
fn adding_past_the_limit_is_refused() {
    assert_eq!(JobCounter::LIMIT, isize::MAX as usize);
    let counter = JobCounter::new();
    assert_eq!(counter.add(JobCounter::LIMIT - 1), Ok(()));
    assert_eq!(counter.add(2), Err(Full(2)));
    assert_eq!(counter.pending(), JobCounter::LIMIT - 1);
    assert_eq!(counter.add(1), Ok(()));
    assert_eq!(counter.add(1), Err(Full(1)));
    assert_eq!(counter.add(0), Ok(()));
    assert_eq!(counter.pending(), JobCounter::LIMIT);
    let empty = JobCounter::new();
    assert_eq!(empty.add(usize::MAX), Err(Full(usize::MAX)));
    assert_eq!(empty.add(JobCounter::LIMIT), Ok(()));
    assert!(!empty.done());
    assert_eq!(empty.pending(), JobCounter::LIMIT - 1);
}

#[test]
fn wait_helps_until_done() {
    let counter = JobCounter::new();
    assert_eq!(counter.add(3), Ok(()));
    let mut helped = 0;
    counter.wait(|| {
        helped += 1;
        counter.done();
    });
    assert_eq!(helped, 3);
    // Already done: `help` is not called.
    counter.wait(|| helped += 1);
    assert_eq!(helped, 3);
}

#[test]
fn debug_shows_the_pending_jobs() {
    let counter = JobCounter::new();
    assert_eq!(counter.add(4), Ok(()));
    assert_eq!(format!("{counter:?}"), "JobCounter { pending: 4 }");
}

/// Results written by jobs without atomics, read once the counter is done:
/// the counter's ordering is all that makes this sound, so a missing
/// `Release`/`Acquire` is a data race that Miri and ThreadSanitizer report.
struct Results(Vec<UnsafeCell<u64>>);

// SAFETY: each cell is written by exactly one job and read only after the
// counter says every job is done.
unsafe impl Sync for Results {}

#[test]
fn every_result_is_visible_once_the_counter_is_done() {
    let jobs = if cfg!(miri) { 24 } else { 20_000 };
    let workers = if cfg!(miri) { 3 } else { 4 };
    for _ in 0..if cfg!(miri) { 1 } else { 20 } {
        let results = Results((0..jobs).map(|_| UnsafeCell::new(0)).collect());
        let queue = Queue::<usize, Inline<64>>::inline();
        let counter = JobCounter::new();
        assert_eq!(counter.add(jobs), Ok(()));
        let lasts = AtomicUsize::new(0);
        let shared = &results;
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    while !counter.is_done() {
                        if let Some(job) = queue.pop() {
                            if let Some(cell) = shared.0.get(job) {
                                // SAFETY: only this job writes this cell.
                                unsafe { *cell.get() = job as u64 * 3 + 1 };
                            }
                            if counter.done() {
                                lasts.fetch_add(1, Ordering::Relaxed);
                            }
                        } else {
                            std::thread::yield_now();
                        }
                    }
                });
            }
            let mut next = 0;
            counter.wait(|| {
                if next < jobs && queue.push(next).is_ok() {
                    next += 1;
                } else {
                    std::thread::yield_now();
                }
            });
            // The workers may still be running; the results are final.
            for (job, cell) in results.0.iter().enumerate() {
                // SAFETY: every job is done, and the counter's `Acquire`
                // makes their writes visible.
                assert_eq!(unsafe { *cell.get() }, job as u64 * 3 + 1);
            }
        });
        assert_eq!(lasts.load(Ordering::Relaxed), 1);
    }
}

static FRAME: JobCounter = JobCounter::new();

#[test]
fn a_static_counter_tracks_plain_threads() -> Result {
    FRAME.add(4)?;
    let threads: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(|| FRAME.done()))
        .collect();
    FRAME.wait(std::thread::yield_now);
    let mut lasts = 0;
    for thread in threads {
        lasts += usize::from(thread.join().map_err(|_| "thread failed")?);
    }
    assert_eq!(lasts, 1);
    Ok(())
}

#[test]
fn extra_done_calls_racing_with_adds_leave_the_count_right() {
    let rounds = if cfg!(miri) { 30 } else { 50_000 };
    let counter = JobCounter::new();
    let refused = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..rounds {
                    match counter.add(1) {
                        Ok(()) => {
                            counter.done();
                        }
                        Err(Full(_)) => {
                            refused.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    // A mistake: one `done` too many.
                    counter.done();
                }
            });
        }
    });
    assert_eq!(counter.pending(), 0);
    assert!(counter.is_done());
    assert!(refused.load(Ordering::Relaxed) <= 4 * rounds);
}
