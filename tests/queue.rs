use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::error::Error;
use std::sync::Mutex;
use vas::sync::{Queue, Slot};
use vas::{Arena, Borrowed, Full, Inline, Memory};

type Result = core::result::Result<(), Box<dyn Error>>;

fn is_send_sync<T: Send + Sync>(_: &T) {}

#[test]
fn first_in_first_out() {
    let queue = Queue::<u32, Inline<4>>::inline();
    is_send_sync(&queue);
    assert_eq!(queue.capacity(), 4);
    assert!(queue.is_empty());
    assert_eq!(queue.pop(), None);
    for value in 1..=4 {
        assert_eq!(queue.push(value), Ok(()));
    }
    assert_eq!(queue.len(), 4);
    assert_eq!(queue.push(5), Err(Full(5)));
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.push(5), Ok(()));
    assert_eq!(
        [queue.pop(), queue.pop(), queue.pop(), queue.pop()],
        [Some(2), Some(3), Some(4), Some(5)]
    );
    assert_eq!(queue.pop(), None);
    assert!(queue.is_empty());
}

#[test]
fn wraps_around_many_times() {
    let queue = Queue::<u64, Inline<8>>::inline();
    let mut next_in = 0u64;
    let mut next_out = 0u64;
    for round in 0..1000 {
        // An uneven number in and out, so the positions drift around the ring.
        for _ in 0..(round % 7 + 1) {
            if queue.push(next_in).is_ok() {
                next_in += 1;
            }
        }
        for _ in 0..(round % 5 + 1) {
            if let Some(value) = queue.pop() {
                assert_eq!(value, next_out);
                next_out += 1;
            }
        }
        assert_eq!(queue.len() as u64, next_in - next_out);
    }
    assert!(next_out > 1000);
}

#[test]
fn capacity_is_rounded_down_to_a_power_of_two() -> Result {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let queue: Queue<u8, Borrowed> = Queue::new(arena.alloc::<Slot<u8>>(10)?);
    assert_eq!(queue.capacity(), 8);
    for value in 0..8 {
        queue.push(value)?;
    }
    assert_eq!(queue.push(8), Err(Full(8)));

    let odd = Queue::<u8, Inline<5>>::inline();
    assert_eq!(odd.capacity(), 4);
    let one = Queue::<u8, Inline<1>>::inline();
    assert_eq!(one.capacity(), 1);
    assert_eq!(
        (one.push(1), one.push(2), one.pop(), one.pop()),
        (Ok(()), Err(Full(2)), Some(1), None)
    );
    Ok(())
}

#[test]
fn zero_capacity_is_always_full_and_empty() {
    let mut slots: [MaybeUninit<Slot<String>>; 0] = [];
    let queue = Queue::new(&mut slots);
    assert_eq!(queue.capacity(), 0);
    assert_eq!(queue.push("job".to_string()), Err(Full("job".to_string())));
    assert_eq!(queue.pop(), None);
    assert_eq!(queue.len(), 0);
    let inline = Queue::<u8, Inline<0>>::inline();
    assert_eq!(inline.push(1), Err(Full(1)));
}

static INJECTOR: Queue<u32, Inline<64>> = Queue::inline();

#[test]
fn a_static_queue_needs_no_setup() {
    std::thread::scope(|scope| {
        scope.spawn(|| INJECTOR.push(42));
    });
    assert_eq!(INJECTOR.pop(), Some(42));
}

/// Values from `producers` threads, each sending `0..count` tagged with its
/// number, received by `consumers` threads. Checks that every value arrives
/// exactly once and in order per producer.
fn exchange<S: vas::storage::Fixed<Slot<u64>>>(
    queue: &Queue<u64, S>,
    producers: u64,
    consumers: usize,
    count: u64,
) {
    let total = (producers * count) as usize;
    let received = AtomicUsize::new(0);
    let logs = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for producer in 0..producers {
            scope.spawn(move || {
                for i in 0..count {
                    let mut value = producer << 32 | i;
                    while let Err(Full(back)) = queue.push(value) {
                        value = back;
                        std::thread::yield_now();
                    }
                }
            });
        }
        for _ in 0..consumers {
            scope.spawn(|| {
                let mut log = Vec::new();
                while received.load(Ordering::Relaxed) < total {
                    match queue.pop() {
                        Some(value) => {
                            log.push(value);
                            received.fetch_add(1, Ordering::Relaxed);
                        }
                        None => std::thread::yield_now(),
                    }
                }
                if let Ok(mut logs) = logs.lock() {
                    logs.push(log);
                }
            });
        }
    });
    let logs = logs.into_inner().unwrap_or_default();
    // In order per producer, as each consumer saw them.
    for log in &logs {
        let mut last = vec![None; producers as usize];
        for value in log {
            let (producer, i) = ((value >> 32) as usize, value & 0xffff_ffff);
            assert!(last[producer] < Some(i));
            last[producer] = Some(i);
        }
    }
    // Every value exactly once.
    let mut all: Vec<u64> = logs.concat();
    all.sort_unstable();
    let expected: Vec<u64> = (0..producers)
        .flat_map(|p| (0..count).map(move |i| p << 32 | i))
        .collect();
    assert_eq!(all, expected);
    assert!(queue.is_empty());
}

#[test]
fn many_producers_and_consumers_exchange_every_value_once() -> Result {
    let count = if cfg!(miri) { 30 } else { 50_000 };
    let small = Queue::<u64, Inline<4>>::inline();
    exchange(&small, 4, 4, count);
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let large: Queue<u64, Borrowed> = Queue::new(arena.alloc::<Slot<u64>>(1024)?);
    exchange(&large, 3, 5, count);
    exchange(&large, 1, 1, count);
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
fn values_left_in_the_queue_are_dropped_once() {
    {
        let queue = Queue::<Counted, Inline<8>>::inline();
        for round in 0..5 {
            for i in 0..6 {
                let _ = queue.push(Counted::new(i));
            }
            // Wrap around so the values left straddle the end of the ring.
            for _ in 0..(4 + round % 2) {
                drop(queue.pop());
            }
        }
        assert_eq!(LIVE.load(Ordering::SeqCst), queue.len());
        // Fill it up; the value that does not fit comes back to its owner.
        while let Ok(()) = queue.push(Counted::new(99)) {}
        assert_eq!(LIVE.load(Ordering::SeqCst), 8);
    }
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}

#[test]
fn debug_shows_the_size() {
    let queue = Queue::<u8, Inline<2>>::inline();
    let _ = queue.push(1);
    assert_eq!(format!("{queue:?}"), "Queue { len: 1, capacity: 2 }");
}
