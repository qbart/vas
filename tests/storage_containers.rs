//! Every single-buffer container with inline storage: no lifetime, no
//! separate memory, usable as a plain struct field.

use vas::storage::Storage;
use vas::{
    BinaryTree, Bitset, Borrowed, Deque, Full, Heap, Inline, Map, Memory, OrderedMap, Pool, Tree,
};

/// A struct that owns all of its containers.
#[derive(Default)]
struct World {
    enemies: Pool<&'static str, Inline<8>>,
    names: Map<u32, &'static str, Inline<16>>,
    events: Deque<u32, Inline<4>>,
    queue: Heap<u32, Inline<8>>,
    scores: OrderedMap<u32, &'static str, Inline<8>>,
    alive: Bitset<Inline<2>>,
    scene: Tree<&'static str, Inline<8>>,
    decisions: BinaryTree<&'static str, Inline<8>>,
}

static EVENTS: Deque<u8, Inline<4>> = Deque::inline();
static ENEMIES: Pool<u8, Inline<4>> = Pool::inline();

#[test]
fn a_struct_can_own_all_of_its_containers() {
    let mut world = World::default();

    let goblin = world.enemies.insert("goblin");
    assert_eq!(
        goblin.ok().and_then(|id| world.enemies.get(id)),
        Some(&"goblin")
    );

    assert!(world.names.insert(7, "sword").is_ok());
    assert_eq!(world.names.get(&7), Some(&"sword"));

    assert!(world.events.push_back(1).is_ok());
    assert!(world.events.push_front(0).is_ok());
    assert_eq!(world.events.iter().copied().collect::<Vec<_>>(), [0, 1]);

    for value in [3, 9, 1] {
        assert!(world.queue.push(value).is_ok());
    }
    assert_eq!(world.queue.pop(), Some(9));

    assert!(world.scores.insert(20, "b").is_ok());
    assert!(world.scores.insert(10, "a").is_ok());
    assert_eq!(world.scores.first(), Some((&10, &"a")));

    assert_eq!(world.alive.capacity(), 128);
    assert_eq!(world.alive.insert(100), Ok(true));
    assert!(world.alive.contains(100));

    let root = world.scene.insert_root("root");
    assert!(root.is_ok());
    if let Ok(root) = root {
        assert!(world.scene.insert_child(root, "child").is_ok());
        assert_eq!(world.scene.children(root).count(), 1);
    }

    let top = world.decisions.set_root("top");
    assert!(top.is_ok());
    if let Ok(top) = top {
        assert!(world.decisions.insert_left(top, "yes").is_ok());
        assert_eq!(world.decisions.pre_order(top).count(), 2);
    }

    // Moving the struct moves everything in it.
    let moved = core::hint::black_box(world);
    assert_eq!(moved.names.get(&7), Some(&"sword"));
    assert_eq!(moved.enemies.len(), 1);
    assert_eq!(moved.scene.len(), 2);
    assert_eq!(moved.decisions.len(), 2);
    assert_eq!(moved.queue.peek(), Some(&3));
    assert!(moved.alive.contains(100));
}

#[test]
fn inline_capacity_is_the_parameter() {
    assert_eq!(Pool::<u8, Inline<8>>::inline().capacity(), 8);
    assert_eq!(Deque::<u8, Inline<4>>::inline().capacity(), 4);
    assert_eq!(Heap::<u8, Inline<8>>::inline().capacity(), 8);
    assert_eq!(OrderedMap::<u8, u8, Inline<8>>::inline().capacity(), 8);
    assert_eq!(Bitset::<Inline<3>>::inline().capacity(), 192);
    assert_eq!(Tree::<u8, Inline<8>>::inline().capacity(), 8);
    assert_eq!(BinaryTree::<u8, Inline<8>>::inline().capacity(), 8);
    assert_eq!(Map::<u8, u8, Inline<16>>::inline().capacity(), 16);
}

#[test]
fn inline_containers_report_full() {
    let mut pool = Pool::<u8, Inline<2>>::inline();
    assert!(pool.insert(1).is_ok() && pool.insert(2).is_ok());
    assert_eq!(pool.insert(3).err(), Some(Full(3)));

    let mut deque = Deque::<u8, Inline<1>>::inline();
    assert!(deque.push_back(1).is_ok());
    assert_eq!(deque.push_back(2), Err(Full(2)));

    let mut heap = Heap::<u8, Inline<1>>::inline();
    assert!(heap.push(1).is_ok());
    assert_eq!(heap.push(2), Err(Full(2)));

    let mut map = Map::<u8, u8, Inline<2>>::inline();
    assert!(map.insert(1, 1).is_ok() && map.insert(2, 2).is_ok());
    assert_eq!(map.insert(3, 3).err(), Some(Full((3, 3))));
    assert_eq!(map.insert(2, 9).ok(), Some(Some(2)));
}

#[test]
fn inline_containers_work_in_statics() {
    assert_eq!((EVENTS.len(), EVENTS.capacity()), (0, 4));
    assert_eq!((ENEMIES.len(), ENEMIES.capacity()), (0, 4));
}

#[test]
fn memory_by_value_makes_inline_containers() {
    let mut pool = Pool::new(Memory::<vas::pool::Slot<u8>, 4>::new());
    assert!(pool.insert(1).is_ok());
    let mut deque = Deque::new(Memory::<u8, 4>::new());
    assert!(deque.push_back(1).is_ok());
    let mut heap = Heap::min(Memory::<u8, 4>::new());
    assert!(heap.push(2).is_ok() && heap.push(1).is_ok());
    assert_eq!(heap.pop(), Some(1));
}

#[test]
fn borrowed_spelling_still_works() {
    let mut memory = Memory::<vas::pool::Slot<u8>, 4>::new();
    let mut pool: Pool<u8, Borrowed<'_>> = Pool::new(&mut memory);
    assert!(pool.insert(1).is_ok());

    let mut memory = Memory::<vas::map::Slot<u8, u8>, 4>::new();
    let mut map: Map<u8, u8, Borrowed<'_>> = Map::new(&mut memory);
    assert!(map.insert(1, 1).is_ok());

    let mut memory = Memory::<u64, 1>::new();
    let bits: Bitset<Borrowed<'_>> = Bitset::new(&mut memory);
    assert_eq!(bits.capacity(), 64);
}

/// Generic over the storage of each container.
fn drain<S: Storage<u32>>(deque: &mut Deque<u32, S>) -> u32 {
    let mut total = 0;
    while let Some(value) = deque.pop_front() {
        total += value;
    }
    total
}

#[test]
fn functions_can_take_any_storage() {
    let mut inline = Deque::<u32, Inline<4>>::inline();
    let mut memory = Memory::<u32, 4>::new();
    let mut borrowed = Deque::new(&mut memory);
    for value in 1..=4 {
        assert!(inline.push_back(value).is_ok());
        assert!(borrowed.push_back(value).is_ok());
    }
    assert_eq!((drain(&mut inline), drain(&mut borrowed)), (10, 10));
}

#[test]
fn bitsets_of_different_storage_combine() {
    let mut a = Bitset::<Inline<1>>::inline();
    let mut memory = Memory::<u64, 1>::new();
    let mut b = Bitset::new(&mut memory);
    assert_eq!(a.insert(1), Ok(true));
    assert_eq!(b.insert(2), Ok(true));
    a.union_with(&b);
    assert_eq!(a.iter().collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn inline_containers_drop_their_contents() {
    use std::rc::Rc;
    let marker = Rc::new(());
    {
        let mut pool = Pool::<Rc<()>, Inline<4>>::inline();
        let mut map = Map::<u8, Rc<()>, Inline<4>>::inline();
        let mut deque = Deque::<Rc<()>, Inline<4>>::inline();
        let mut tree = Tree::<Rc<()>, Inline<4>>::inline();
        assert!(pool.insert(marker.clone()).is_ok());
        assert!(map.insert(1, marker.clone()).is_ok());
        assert!(deque.push_back(marker.clone()).is_ok());
        assert!(tree.insert_root(marker.clone()).is_ok());
        assert_eq!(Rc::strong_count(&marker), 5);
        let moved = core::hint::black_box((pool, map, deque, tree));
        assert_eq!(Rc::strong_count(&marker), 5);
        assert_eq!(moved.0.len(), 1);
    }
    assert_eq!(Rc::strong_count(&marker), 1);
}
