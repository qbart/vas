use core::sync::atomic::{AtomicUsize, Ordering};
use std::error::Error;
use vas::arena::StaticBlock;
use vas::sync::{Consumer, Producer, Ring};
use vas::{Arena, Borrowed, Full, Inline, Memory};

type Result = core::result::Result<(), Box<dyn Error>>;

fn is_send<T: Send>(_: &T) {}

#[test]
fn first_in_first_out() {
    let mut ring = Ring::<u32, Inline<4>>::inline();
    assert_eq!(ring.capacity(), 4);
    assert!(ring.is_empty());
    let (producer, consumer) = ring.split();
    is_send(&producer);
    is_send(&consumer);
    assert_eq!(consumer.pop(), None);
    for value in 1..=4 {
        assert_eq!(producer.push(value), Ok(()));
    }
    assert_eq!((producer.len(), consumer.len()), (4, 4));
    assert_eq!(producer.push(5), Err(Full(5)));
    assert_eq!([consumer.pop(), consumer.pop()], [Some(1), Some(2)]);
    assert_eq!(producer.push(5), Ok(()));
    assert_eq!(producer.push(6), Ok(()));
    assert_eq!(producer.push(7), Err(Full(7)));
    assert_eq!(
        [
            consumer.pop(),
            consumer.pop(),
            consumer.pop(),
            consumer.pop()
        ],
        [Some(3), Some(4), Some(5), Some(6)]
    );
    assert_eq!(consumer.pop(), None);
    assert!(producer.is_empty() && consumer.is_empty());
    assert_eq!((producer.capacity(), consumer.capacity()), (4, 4));
}

#[test]
fn wraps_around_many_times() {
    let mut ring = Ring::<u64, Inline<8>>::inline();
    let (producer, consumer) = ring.split();
    let mut model = std::collections::VecDeque::new();
    let mut next = 0u64;
    for round in 0..3000u64 {
        for _ in 0..(round % 6 + 1) {
            match producer.push(next) {
                Ok(()) => model.push_back(next),
                Err(Full(back)) => assert_eq!((back, model.len()), (next, 8)),
            }
            next += 1;
        }
        for _ in 0..(round % 5) {
            assert_eq!(consumer.pop(), model.pop_front());
        }
        assert_eq!(consumer.len(), model.len());
    }
    assert!(next > 9000);
}

#[test]
fn capacity_is_rounded_down_to_a_power_of_two() -> Result {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut ring: Ring<u8, Borrowed> = Ring::new(arena.alloc::<u8>(12)?);
    assert_eq!(ring.capacity(), 8);
    let (producer, _) = ring.split();
    for value in 0..8 {
        producer.push(value)?;
    }
    assert_eq!(producer.push(8), Err(Full(8)));

    let mut one = Ring::<u8, Inline<1>>::inline();
    let (producer, consumer) = one.split();
    for round in 0..4 {
        assert_eq!(
            (producer.push(round), producer.push(9)),
            (Ok(()), Err(Full(9)))
        );
        assert_eq!(consumer.pop(), Some(round));
    }

    let mut none = Ring::<String, Inline<0>>::default();
    let (producer, consumer) = none.split();
    assert_eq!(producer.push("x".to_string()), Err(Full("x".to_string())));
    assert_eq!(consumer.pop(), None);
    Ok(())
}

#[test]
fn splitting_again_keeps_what_is_left() {
    let mut ring = Ring::<u32, Inline<4>>::inline();
    {
        let (producer, consumer) = ring.split();
        for value in 0..4 {
            let _ = producer.push(value);
        }
        assert_eq!(consumer.pop(), Some(0));
    }
    assert_eq!(ring.len(), 3);
    let (producer, consumer) = ring.split();
    assert_eq!(producer.push(4), Ok(()));
    assert_eq!(producer.push(5), Err(Full(5)));
    let all: Vec<_> = core::iter::from_fn(|| consumer.pop()).collect();
    assert_eq!(all, [1, 2, 3, 4]);
}

/// One thread pushes `0..count`, another pops: every value arrives once, in
/// order. Values are `Box`es so that a value read while being written is a
/// data race on its memory.
fn stream<S: vas::storage::Fixed<Box<u64>>>(ring: &mut Ring<Box<u64>, S>, count: u64) {
    let (producer, consumer) = ring.split();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            for value in 0..count {
                let mut boxed = Box::new(value);
                while let Err(Full(back)) = producer.push(boxed) {
                    boxed = back;
                    std::hint::spin_loop();
                }
            }
        });
        scope.spawn(move || {
            let mut expected = 0;
            while expected < count {
                match consumer.pop() {
                    Some(value) => {
                        assert_eq!(*value, expected);
                        expected += 1;
                    }
                    None => std::hint::spin_loop(),
                }
            }
            assert_eq!(consumer.pop(), None);
        });
    });
}

#[test]
fn every_value_arrives_once_in_order_across_threads() -> Result {
    let count = if cfg!(miri) { 200 } else { 200_000 };
    let mut small = Ring::<Box<u64>, Inline<2>>::inline();
    stream(&mut small, count);
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut large: Ring<Box<u64>, Borrowed> = Ring::new(arena.alloc::<Box<u64>>(1024)?);
    stream(&mut large, count);
    stream(&mut large, count);
    Ok(())
}

static MEMORY: StaticBlock<4096> = StaticBlock::new();

#[test]
fn a_ring_in_static_arena_memory_feeds_a_plain_thread() -> Result {
    let block = MEMORY.take().ok_or("taken")?;
    let arena = Arena::new(block);
    let slots = arena.alloc::<u32>(64)?;
    let ring: &'static mut Ring<u32, Borrowed<'static>> =
        arena.alloc_value(Ring::new(slots)).map_err(|_| "full")?;
    let (producer, consumer): (Producer<'static, u32>, Consumer<'static, u32>) = ring.split();
    let reader = std::thread::spawn(move || {
        let mut sum = 0;
        let mut seen = 0;
        while seen < 100 {
            if let Some(value) = consumer.pop() {
                sum += value;
                seen += 1;
            } else {
                std::thread::yield_now();
            }
        }
        sum
    });
    for value in 0..100 {
        let mut value = value;
        while let Err(Full(back)) = producer.push(value) {
            value = back;
            std::thread::yield_now();
        }
    }
    assert_eq!(reader.join().map_err(|_| "reader failed")?, 4950);
    Ok(())
}

static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Counts how many exist, to catch leaks and double drops.
#[derive(Debug, PartialEq)]
struct Counted;

impl Counted {
    fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn values_left_in_the_ring_are_dropped_once() {
    {
        let mut ring = Ring::<Counted, Inline<8>>::inline();
        let (producer, consumer) = ring.split();
        for _ in 0..5 {
            for _ in 0..5 {
                let _ = producer.push(Counted::new());
            }
            // What is left straddles the end of the ring.
            for _ in 0..3 {
                drop(consumer.pop());
            }
        }
        assert_eq!(LIVE.load(Ordering::SeqCst), consumer.len());
        while let Ok(()) = producer.push(Counted::new()) {}
        assert_eq!(LIVE.load(Ordering::SeqCst), 8);
    }
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}

#[test]
fn debug_shows_the_size() {
    let mut ring = Ring::<u8, Inline<2>>::inline();
    let (producer, consumer) = ring.split();
    let _ = producer.push(1);
    assert_eq!(format!("{producer:?}"), "Producer { len: 1, capacity: 2 }");
    assert_eq!(format!("{consumer:?}"), "Consumer { len: 1, capacity: 2 }");
    assert_eq!(format!("{ring:?}"), "Ring { len: 1, capacity: 2 }");
}
