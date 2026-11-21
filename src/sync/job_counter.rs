//! Counts unfinished jobs, so a thread can wait for a group of them.

use core::fmt;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};

use crate::Full;

/// The number of jobs of a group that are not finished yet.
///
/// The thread that starts the jobs [`add`](Self::add)s them, each job calls
/// [`done`](Self::done) when it finishes, and whoever needs the results
/// checks [`is_done`](Self::is_done) or [`wait`](Self::wait)s. Whatever a
/// job wrote before `done` is visible to a thread that then sees the
/// counter done, without further locking.
///
/// ```
/// use vas::Inline;
/// use vas::sync::{JobCounter, Queue};
///
/// let jobs = Queue::<u32, Inline<64>>::inline();
/// let counter = JobCounter::new();
/// counter.add(40)?;
/// for job in 0..40 {
///     jobs.push(job)?;
/// }
/// std::thread::scope(|scope| {
///     for _ in 0..3 {
///         scope.spawn(|| {
///             while let Some(_job) = jobs.pop() {
///                 // ... run the job ...
///                 counter.done();
///             }
///         });
///     }
///     // Help with the jobs instead of idling.
///     counter.wait(|| {
///         if jobs.pop().is_some() {
///             counter.done();
///         }
///     });
/// });
/// assert!(counter.is_done());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// It never blocks: [`wait`](Self::wait) calls the caller's `help` in a
/// loop, which can run other jobs, spin or yield. `done` returns `true` to
/// the one job that finishes the group, which is the place to start a job
/// that depends on the whole group.
///
/// A counter shared by many threads that finish jobs often can be put in a
/// [`CachePadded`](super::CachePadded) to keep it off other data's cache
/// line.
#[derive(Default)]
pub struct JobCounter {
    pending: AtomicUsize,
}

impl JobCounter {
    /// A counter with no jobs, which is done. `const`, so it can
    /// initialise a `static`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            pending: AtomicUsize::new(0),
        }
    }

    /// The most jobs the counter holds. The upper half of the range is
    /// left free, so that a [`done`](Self::done) too many can notice it went
    /// below zero and undo itself.
    pub const LIMIT: usize = isize::MAX as usize;

    /// Adds `count` jobs, or refuses with [`Full`] (and adds nothing) when
    /// the counter would then hold more than [`LIMIT`](Self::LIMIT). Add
    /// the jobs before handing them to other threads.
    #[inline]
    pub fn add(&self, count: usize) -> Result<(), Full<usize>> {
        let mut pending = self.pending.load(Relaxed);
        loop {
            // A compare-and-swap, not `fetch_add`, so that the count never
            // passes `LIMIT`, even for a moment.
            if pending > Self::LIMIT || Self::LIMIT - pending < count {
                return Err(Full(count));
            }
            // `Relaxed`: whoever runs the jobs receives them through
            // something that synchronises (a queue, a thread spawn) after
            // this add.
            match self
                .pending
                .compare_exchange_weak(pending, pending + count, Relaxed, Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(current) => pending = current,
            }
        }
    }

    /// Marks one job finished. Returns `true` to the call that finishes the
    /// last job, and `false` to every other.
    ///
    /// Calling it on a counter that is already done is a mistake, which it
    /// undoes: the counter stays done and `false` is returned, though for a
    /// moment the counter reads as unfinished, and an [`add`](Self::add)
    /// racing with it may be refused.
    #[inline]
    pub fn done(&self) -> bool {
        // `Release`: this job's writes happen before the `Acquire` of any
        // thread that sees the count it leaves, or a later one (every
        // change to the count is a read-modify-write, so they form one
        // release sequence). `Acquire`: the call that finishes the last job
        // sees every other job's writes.
        let before = self.pending.fetch_sub(1, AcqRel);
        if before == 0 || before > Self::LIMIT {
            // It was done already (or another call too many is under way,
            // which left the count past `LIMIT`): undo. While any such undo
            // is pending the count stays above `LIMIT`, so it never reads 0
            // early, and `add` refuses rather than build on it.
            self.pending.fetch_add(1, Relaxed);
            return false;
        }
        before == 1
    }

    /// `true` when every job added has finished. Once it returns `true`,
    /// whatever those jobs wrote before [`done`](Self::done) is visible.
    #[inline]
    pub fn is_done(&self) -> bool {
        // `Acquire`: pairs with the `Release` of every `done`.
        self.pending.load(Acquire) == 0
    }

    /// How many jobs are unfinished. Only a snapshot: other threads may
    /// change it at once.
    #[inline]
    pub fn pending(&self) -> usize {
        self.pending.load(Relaxed)
    }

    /// Calls `help` until every job has finished; returns at once when
    /// they already have. `help` decides how to wait: run another job,
    /// [`spin_loop`](core::hint::spin_loop), or yield the thread.
    #[inline]
    pub fn wait<F: FnMut()>(&self, mut help: F) {
        while !self.is_done() {
            help();
        }
    }
}

impl fmt::Debug for JobCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobCounter")
            .field("pending", &self.pending())
            .finish()
    }
}
