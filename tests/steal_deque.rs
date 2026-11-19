use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::error::Error;
use std::sync::Mutex;
use vas::arena::StaticBlock;
use vas::sync::{Slot, Steal, StealDeque, Stealer};
use vas::{Arena, Borrowed, Full, Inline, Memory};

type Result = core::result::Result<(), Box<dyn Error>>;

fn is_send<T: Send>(_: &T) {}
fn is_send_sync<T: Send + Sync>(_: &T) {}
fn is_copy<T: Copy>(_: &T) {}

#[test]
fn the_owner_works_last_in_first_out() {
    let mut deque = StealDeque::<u32, Inline<4>>::inline();
    assert_eq!(deque.capacity(), 4);
    let (worker, stealer) = deque.split();
    is_send(&worker);
    is_send_sync(&stealer);
    is_copy(&stealer);
    assert!(worker.is_empty());
    assert_eq!(worker.pop(), None);
    for job in 1..=4 {
        assert_eq!(worker.push(job), Ok(()));
    }
    assert_eq!(worker.len(), 4);
    assert_eq!(worker.push(5), Err(Full(5)));
    assert_eq!([worker.pop(), worker.pop()], [Some(4), Some(3)]);
    assert_eq!(worker.push(6), Ok(()));
    assert_eq!(
        [worker.pop(), worker.pop(), worker.pop(), worker.pop()],
        [Some(6), Some(2), Some(1), None]
    );
    assert!(stealer.is_empty());
}

#[test]
fn thieves_take_the_oldest_first() {
    let mut deque = StealDeque::<u32, Inline<8>>::inline();
    let (worker, stealer) = deque.split();
    assert_eq!(stealer.steal(), Steal::Empty);
    for job in 1..=5 {
        assert_eq!(worker.push(job), Ok(()));
    }
    let thief = stealer;
    assert_eq!(stealer.steal(), Steal::Success(1));
    assert_eq!(thief.steal().success(), Some(2));
    assert_eq!(stealer.len(), 3);
    assert_eq!(worker.pop(), Some(5));
    assert_eq!(stealer.steal(), Steal::Success(3));
    // The last one, which owner and thieves compete for.
    assert_eq!(worker.pop(), Some(4));
    assert_eq!(stealer.steal(), Steal::Empty);
    assert_eq!(worker.stealer().steal(), Steal::Empty);
    assert!(!Steal::<u8>::Empty.is_retry() && Steal::<u8>::Retry.is_retry());
    assert!(Steal::<u8>::Empty.is_empty() && Steal::Success(1).is_success());
}

#[test]
fn wraps_around_many_times() {
    let mut deque = StealDeque::<u64, Inline<8>>::inline();
    let (worker, stealer) = deque.split();
    let mut model = std::collections::VecDeque::new();
    let mut next = 0u64;
    for round in 0..2000u64 {
        for _ in 0..(round % 5 + 1) {
            match worker.push(next) {
                Ok(()) => model.push_back(next),
                Err(Full(back)) => assert_eq!((back, model.len()), (next, 8)),
            }
            next += 1;
        }
        if round % 3 == 0 {
            assert_eq!(worker.pop(), model.pop_back());
        }
        for _ in 0..(round % 4) {
            assert_eq!(stealer.steal().success(), model.pop_front());
        }
        assert_eq!(worker.len(), model.len());
    }
    assert!(next > 5000);
}

#[test]
fn capacity_is_rounded_down_to_a_power_of_two() -> Result {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut deque: StealDeque<u8, Borrowed> = StealDeque::new(arena.alloc::<Slot<u8>>(12)?);
    assert_eq!(deque.capacity(), 8);
    let (worker, _) = deque.split();
    for job in 0..8 {
        worker.push(job)?;
    }
    assert_eq!(worker.push(8), Err(Full(8)));

    let mut one = StealDeque::<u8, Inline<1>>::inline();
    assert_eq!(one.capacity(), 1);
    let (worker, stealer) = one.split();
    for round in 0..4 {
        assert_eq!((worker.push(round), worker.push(9)), (Ok(()), Err(Full(9))));
        if round % 2 == 0 {
            assert_eq!(worker.pop(), Some(round));
        } else {
            assert_eq!(stealer.steal(), Steal::Success(round));
        }
    }
    Ok(())
}

#[test]
fn zero_capacity_is_always_full_and_empty() {
    let mut deque = StealDeque::<String, Inline<0>>::inline();
    let (worker, stealer) = deque.split();
    assert_eq!(worker.push("job".to_string()), Err(Full("job".to_string())));
    assert_eq!(worker.pop(), None);
    assert_eq!(stealer.steal(), Steal::Empty);
}

/// The owner pushes `0..count`, popping one of every three itself, while
/// `thieves` threads steal. Every value must be taken exactly once.
fn work<S: vas::storage::Fixed<Slot<u64>>>(
    deque: &mut StealDeque<u64, S>,
    thieves: usize,
    count: u64,
) {
    let (worker, stealer) = deque.split();
    let done = AtomicBool::new(false);
    let logs = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..thieves {
            scope.spawn(|| {
                let mut log = Vec::new();
                loop {
                    match stealer.steal() {
                        Steal::Success(value) => log.push(value),
                        Steal::Retry => {}
                        Steal::Empty if done.load(Ordering::Acquire) => break,
                        Steal::Empty => std::thread::yield_now(),
                    }
                }
                if let Ok(mut logs) = logs.lock() {
                    logs.push(log);
                }
            });
        }
        let mut mine = Vec::new();
        for value in 0..count {
            let mut value = value;
            while let Err(Full(back)) = worker.push(value) {
                value = back;
                mine.extend(worker.pop());
            }
            if value % 3 == 0 {
                mine.extend(worker.pop());
            }
        }
        while let Some(value) = worker.pop() {
            mine.push(value);
        }
        done.store(true, Ordering::Release);
        if let Ok(mut logs) = logs.lock() {
            logs.push(mine);
        }
    });
    let mut all: Vec<u64> = logs.into_inner().unwrap_or_default().concat();
    all.sort_unstable();
    assert_eq!(all, (0..count).collect::<Vec<_>>());
}

#[test]
fn every_job_is_taken_exactly_once_under_stealing() -> Result {
    let count = if cfg!(miri) { 60 } else { 100_000 };
    let mut small = StealDeque::<u64, Inline<4>>::inline();
    work(&mut small, 3, count);
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut large: StealDeque<u64, Borrowed> = StealDeque::new(arena.alloc::<Slot<u64>>(1024)?);
    work(&mut large, 4, count);
    work(&mut large, 1, count);
    Ok(())
}

static MEMORY: StaticBlock<8192> = StaticBlock::new();

#[test]
fn a_deque_in_static_arena_memory_feeds_plain_threads() -> Result {
    let block = MEMORY.take().ok_or("taken")?;
    let arena = Arena::new(block);
    let slots = arena.alloc::<Slot<u32>>(64)?;
    let deque: &'static mut StealDeque<u32, Borrowed<'static>> = arena
        .alloc_value(StealDeque::new(slots))
        .map_err(|_| "full")?;
    let (worker, stealer): (_, Stealer<'static, u32, Borrowed<'static>>) = deque.split();
    for job in 0..10 {
        worker.push(job)?;
    }
    // A `'static` stealer goes to a thread that is not scoped.
    let thief = std::thread::spawn(move || {
        let mut sum = 0;
        while let Steal::Success(job) = stealer.steal() {
            sum += job;
        }
        sum
    });
    let stolen = thief.join().map_err(|_| "thief failed")?;
    let mut left = 0;
    while let Some(job) = worker.pop() {
        left += job;
    }
    assert_eq!(stolen + left, 45);
    Ok(())
}

static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Counts how many exist, to catch leaks and double drops.
#[derive(Debug, PartialEq)]
struct Counted(u32);

impl Counted {
    fn new(value: u32) -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Self(value)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn values_left_in_the_deque_are_dropped_once() {
    {
        let mut deque = StealDeque::<Counted, Inline<8>>::inline();
        let (worker, stealer) = deque.split();
        for round in 0..6 {
            for i in 0..5 {
                let _ = worker.push(Counted::new(i));
            }
            // Thieves move the start, so what is left straddles the end of
            // the ring.
            for _ in 0..3 {
                drop(stealer.steal());
            }
            if round % 2 == 0 {
                drop(worker.pop());
            }
        }
        assert_eq!(LIVE.load(Ordering::SeqCst), worker.len());
        while let Ok(()) = worker.push(Counted::new(99)) {}
        assert_eq!(LIVE.load(Ordering::SeqCst), 8);
    }
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}

#[test]
fn debug_shows_the_size() {
    let mut deque = StealDeque::<u8, Inline<2>>::inline();
    let (worker, stealer) = deque.split();
    let _ = worker.push(1);
    assert_eq!(format!("{worker:?}"), "Worker { len: 1, capacity: 2 }");
    assert_eq!(format!("{stealer:?}"), "Stealer { len: 1, capacity: 2 }");
    assert_eq!(format!("{deque:?}"), "StealDeque { len: 1, capacity: 2 }");
}
