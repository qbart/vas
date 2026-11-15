use core::cell::Cell;
use std::collections::BinaryHeap;
use vas::heap::{Max, Min};
use vas::{Full, Heap, Memory};

struct Counted<'a>(&'a Cell<usize>, u32);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<u32, 4>::new();
    let heap = Heap::new(&mut memory);
    assert_eq!(heap.len(), 0);
    assert_eq!(heap.capacity(), 4);
    assert_eq!(heap.remaining(), 4);
    assert!(heap.is_empty());
    assert!(!heap.is_full());
    assert_eq!(heap.peek(), None);
}

#[test]
fn pops_largest_first_by_default() {
    let mut memory = Memory::<u32, 8>::new();
    let mut heap = Heap::new(&mut memory);
    for value in [3, 1, 4, 1, 5, 9, 2, 6] {
        assert_eq!(heap.push(value), Ok(()));
    }
    assert_eq!(heap.peek(), Some(&9));
    for expected in [9, 6, 5, 4, 3, 2, 1, 1] {
        assert_eq!(heap.pop(), Some(expected));
    }
    assert_eq!(heap.pop(), None);
}

#[test]
fn min_pops_smallest_first() {
    let mut memory = Memory::<u32, 8>::new();
    let mut heap = Heap::min(&mut memory);
    for value in [3, 1, 4, 1, 5, 9, 2, 6] {
        assert_eq!(heap.push(value), Ok(()));
    }
    assert_eq!(heap.peek(), Some(&1));
    for expected in [1, 1, 2, 3, 4, 5, 6, 9] {
        assert_eq!(heap.pop(), Some(expected));
    }
}

#[test]
fn custom_order_for_float_costs() {
    #[derive(Debug, PartialEq, Clone, Copy)]
    struct Node {
        cost: f32,
        id: u32,
    }
    let mut memory = Memory::<Node, 4>::new();
    // Lowest cost first, as a pathfinder wants it.
    let mut open = Heap::with_order(&mut memory, |a: &Node, b: &Node| b.cost.total_cmp(&a.cost));
    assert!(open.push(Node { cost: 2.5, id: 1 }).is_ok());
    assert!(open.push(Node { cost: 0.5, id: 2 }).is_ok());
    assert!(open.push(Node { cost: 1.5, id: 3 }).is_ok());
    assert_eq!(open.pop().map(|n| n.id), Some(2));
    assert_eq!(open.pop().map(|n| n.id), Some(3));
    assert_eq!(open.pop().map(|n| n.id), Some(1));
}

#[test]
fn push_when_full_returns_value() {
    let mut memory = Memory::<u32, 2>::new();
    let mut heap = Heap::new(&mut memory);
    assert_eq!(heap.push(1), Ok(()));
    assert_eq!(heap.push(2), Ok(()));
    assert!(heap.is_full());
    assert_eq!(heap.push(3), Err(Full(3)));
    assert_eq!(heap.len(), 2);
    assert_eq!(heap.pop(), Some(2));
}

#[test]
fn zero_capacity() {
    let mut memory = Memory::<u32, 0>::new();
    let mut heap = Heap::new(&mut memory);
    assert!(heap.is_full());
    assert_eq!(heap.push(1), Err(Full(1)));
    assert_eq!(heap.pop(), None);
    assert_eq!(heap.peek(), None);
}

#[test]
fn as_slice_holds_every_element() {
    let mut memory = Memory::<u32, 8>::new();
    let mut heap = Heap::new(&mut memory);
    for value in [5, 3, 8, 1] {
        assert_eq!(heap.push(value), Ok(()));
    }
    let mut seen = [0u32; 4];
    seen.copy_from_slice(heap.as_slice());
    seen.sort_unstable();
    assert_eq!(seen, [1, 3, 5, 8]);
    // The first element is always the one `pop` would return.
    assert_eq!(heap.as_slice().first(), heap.peek());
}

#[test]
fn drops_elements_and_clear() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Counted, 4>::new();
    {
        let mut heap = Heap::with_order(&mut memory, |a: &Counted, b: &Counted| a.1.cmp(&b.1));
        for priority in [2, 7, 4, 1] {
            assert!(heap.push(Counted(&drops, priority)).is_ok());
        }
        assert_eq!(heap.pop().map(|c| c.1), Some(7));
        assert_eq!(drops.get(), 1);
        heap.clear();
        assert_eq!(drops.get(), 4);
        assert!(heap.is_empty());
        assert!(heap.push(Counted(&drops, 9)).is_ok());
    }
    assert_eq!(drops.get(), 5);
}

#[test]
fn matches_std_binary_heap() {
    let mut memory = Memory::<u32, 33>::new();
    let mut heap = Heap::new(&mut memory);
    let mut model: BinaryHeap<u32> = BinaryHeap::new();
    let mut seed: u32 = 0xdead_beef;
    let steps: u32 = if cfg!(miri) { 600 } else { 20_000 };
    for _ in 0..steps {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let value = (seed >> 20) & 63;
        if (seed >> 12) & 3 == 0 {
            assert_eq!(heap.pop(), model.pop());
        } else if model.len() == 33 {
            assert_eq!(heap.push(value), Err(Full(value)));
        } else {
            model.push(value);
            assert_eq!(heap.push(value), Ok(()));
        }
        assert_eq!(heap.len(), model.len());
        assert_eq!(heap.peek(), model.peek());
    }
    while let Some(expected) = model.pop() {
        assert_eq!(heap.pop(), Some(expected));
    }
    assert!(heap.is_empty());
}

#[test]
fn orders_are_zero_sized() {
    assert_eq!(size_of::<Max>(), 0);
    assert_eq!(size_of::<Min>(), 0);
}
