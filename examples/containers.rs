//! The general-purpose containers. Run with `cargo run --example containers`.

use std::error::Error;

use vas::bitset::words;
use vas::pool::Slot;
use vas::{Bitset, Deque, Heap, Inline, Map, Memory, OrderedMap, Pool, Rng, SparseSet, Vector};

fn main() -> Result<(), Box<dyn Error>> {
    // Vector: a contiguous block, e.g. vertices to hand to a renderer.
    let mut memory = Memory::<f32, 1024>::new();
    let mut vertices = Vector::new(&mut memory);
    vertices.push(1.0)?;
    vertices.extend_from_slice(&[2.0, 3.0, 4.0])?;
    assert_eq!(vertices.as_slice(), &[1.0, 2.0, 3.0, 4.0]);

    // Growing: get a bigger block and move into it.
    let mut bigger = Memory::<f32, 4096>::new();
    let vertices = vertices
        .move_into(&mut bigger)
        .map_err(|_| "block too small")?;
    assert_eq!(vertices.capacity(), 4096);

    // Inline: the vector holds its own memory, so it is a plain struct
    // field with no lifetime.
    struct Trail {
        points: Vector<[f32; 2], Inline<16>>,
    }
    let mut trail = Trail {
        points: Vector::inline(),
    };
    trail.points.push([0.0, 1.0])?;
    assert_eq!(trail.points.capacity(), 16);

    // The same for the other containers: a struct that owns everything it
    // needs, created in one line.
    #[derive(Default)]
    struct Inventory {
        items: Pool<&'static str, Inline<32>>,
        by_id: Map<u32, &'static str, Inline<64>>,
        recent: Deque<u32, Inline<8>>,
    }
    let mut inventory = Inventory::default();
    inventory.items.insert("potion")?;
    inventory.by_id.insert(1, "potion")?;
    inventory.recent.push_back(1)?;
    assert_eq!(inventory.by_id.get(&1), Some(&"potion"));

    // Pool: values with stable handles that detect removal.
    let mut memory = Memory::<Slot<&str>, 64>::new();
    let mut enemies = Pool::new(&mut memory);
    let goblin = enemies.insert("goblin")?;
    assert_eq!(enemies.get(goblin), Some(&"goblin"));
    enemies.remove(goblin);
    assert_eq!(enemies.get(goblin), None); // stale handle

    // Map: unordered key -> value.
    let mut memory = Memory::<vas::map::Slot<u32, &str>, 64>::new();
    let mut tags = Memory::<u8, 64>::new();
    let mut items = Map::new(&mut memory, &mut tags);
    items.insert(7, "sword")?;
    assert_eq!(items.get(&7), Some(&"sword"));

    // SparseSet: components by entity id, packed for fast iteration.
    let mut sparse = Memory::<u32, 1024>::new(); // entity ids below 1024
    let mut ids = Memory::<u32, 64>::new();
    let mut values = Memory::<f32, 64>::new();
    let mut health = SparseSet::new(&mut sparse, &mut ids, &mut values);
    health.insert(900, 100.0)?;
    health.insert(3, 40.0)?;
    for hp in health.values_mut() {
        *hp -= 10.0; // a tight loop over 2 values
    }
    assert_eq!(health.get(3), Some(&30.0));

    // OrderedMap: sorted keys, nearest-key lookups.
    let mut memory = Memory::<(u32, &str), 16>::new();
    let mut keyframes = OrderedMap::new(&mut memory);
    keyframes.insert(20, "jump")?;
    keyframes.insert(0, "idle")?;
    keyframes.insert(10, "step")?;
    assert_eq!(keyframes.at_or_before(&15), Some((&10, &"step")));

    // Deque: a queue, or a "keep the last N" history.
    let mut memory = Memory::<u32, 3>::new();
    let mut frame_times = Deque::new(&mut memory);
    for time in [16, 17, 15, 33] {
        frame_times.push_overwrite(time);
    }
    assert!(frame_times.iter().copied().eq([17, 15, 33]));

    // Heap: the most urgent item first.
    let mut memory = Memory::<u32, 16>::new();
    let mut timers = Heap::min(&mut memory);
    for due in [50, 10, 30] {
        timers.push(due)?;
    }
    assert_eq!(timers.pop(), Some(10));

    // Bitset: a compact set of small integers.
    let mut memory = Memory::<u64, { words(1000) }>::new();
    let mut visited = Bitset::new(&mut memory);
    visited.insert(42)?;
    assert!(visited.contains(42));
    assert_eq!(visited.first_clear(), Some(0));

    // Rng: reproducible randomness.
    let mut rng = Rng::new(42);
    let roll = rng.u32_below(6) + 1;
    assert!((1..=6).contains(&roll));
    assert_eq!(Rng::new(42).u32_below(6) + 1, roll);

    println!("containers: ok");
    Ok(())
}
