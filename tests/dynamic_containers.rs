use core::cell::Cell;
use vas::path::Space;
use vas::{
    Aabb, Arena, BinaryTree, Bitset, Deque, Dynamic, Full, Graph, Heap, Map, Memory, Octree,
    OrderedMap, OutOfBounds, PathFinder, Pool, Quadtree, SpatialGrid, StaticGraph, Tree, Vector,
};

/// Counts drops, to check that elements are dropped exactly once.
struct Tracked<'a>(&'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// A straight line of nodes: `0 - 1 - 2 - ...`.
struct Row(u32);

impl Space for Row {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        if node + 1 < self.0 {
            visit(node + 1, 1.0);
        }
        if node > 0 {
            visit(node - 1, 1.0);
        }
    }
}

#[test]
fn pool_grows_and_handles_stay_valid() {
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    let mut pool = Pool::new(Dynamic(&arena));
    assert_eq!(pool.capacity(), 0);
    let first = pool.insert(0u32);
    assert!(first.is_ok());
    let mut last = first;
    for i in 1..500u32 {
        last = pool.insert(i);
        assert!(last.is_ok());
    }
    assert_eq!(pool.len(), 500);
    if let (Ok(first), Ok(last)) = (first, last) {
        assert_eq!(pool.get(first), Some(&0));
        assert_eq!(pool.get(last), Some(&499));
        // Freed slots are reused before the pool grows again.
        let capacity = pool.capacity();
        assert_eq!(pool.remove(first), Some(0));
        assert!(pool.insert(7).is_ok());
        assert_eq!(pool.capacity(), capacity);
        assert_eq!(pool.get(first), None);
    }
    assert_eq!(pool.iter().count(), 500);
}

#[test]
fn pool_reports_full_when_the_allocator_refuses() {
    let mut block = Memory::<u8, 256>::new();
    let arena = Arena::new(&mut block);
    let mut pool = Pool::new(Dynamic(&arena));
    let mut stored = 0u64;
    let rejected = loop {
        match pool.insert(stored) {
            Ok(_) => stored += 1,
            Err(Full(value)) => break value,
        }
    };
    assert_eq!(rejected, stored);
    assert!(stored >= 4);
    assert_eq!(pool.len() as u64, stored);
    assert_eq!(pool.iter().map(|(_, value)| *value).sum::<u64>(), {
        (0..stored).sum::<u64>()
    });
}

#[test]
fn pool_reserve() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut pool: Pool<u8, _> = Pool::new(Dynamic(&arena));
    assert_eq!(pool.reserve(100), Ok(()));
    assert!(pool.capacity() >= 100);
    assert_eq!(pool.reserve(1 << 20), Err(Full(())));

    let mut fixed = Pool::<u8, vas::Inline<2>>::inline();
    assert_eq!(fixed.reserve(2), Ok(()));
    assert!(fixed.insert(1).is_ok());
    assert_eq!(fixed.reserve(2), Err(Full(())));
}

#[test]
fn map_grows_and_keeps_every_entry() {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut map = Map::new(Dynamic(&arena));
    assert_eq!((map.len(), map.capacity()), (0, 0));
    assert_eq!(map.get(&1), None);
    for key in 0..1000u32 {
        assert_eq!(map.insert(key, key * 2), Ok(None));
    }
    assert_eq!(map.len(), 1000);
    assert!(map.capacity() > 1000);
    for key in 0..1000u32 {
        assert_eq!(map.get(&key), Some(&(key * 2)));
    }
    assert_eq!(map.insert(5, 0), Ok(Some(10)));
    for key in (0..1000u32).step_by(2) {
        assert!(map.remove(&key).is_some());
    }
    assert_eq!(map.len(), 500);
    assert_eq!(map.get(&4), None);
    assert_eq!(map.get(&999), Some(&1998));
    assert_eq!(map.iter().count(), 500);
}

#[test]
fn map_reports_full_when_the_allocator_refuses() {
    let mut block = Memory::<u8, 512>::new();
    let arena = Arena::new(&mut block);
    let mut map = Map::new(Dynamic(&arena));
    let mut stored = 0u32;
    let rejected = loop {
        match map.insert(stored, stored) {
            Ok(_) => stored += 1,
            Err(Full(entry)) => break entry,
        }
    };
    assert_eq!(rejected, (stored, stored));
    assert!(stored >= 4);
    // A map that cannot grow fills up completely, like a fixed one.
    assert_eq!(map.len(), map.capacity());
    for key in 0..stored {
        assert_eq!(map.get(&key), Some(&key));
    }
    // Replacing a value needs no room.
    assert_eq!(map.insert(0, 9), Ok(Some(0)));
}

#[test]
fn map_reserve_and_drop() {
    let drops = Cell::new(0);
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    {
        let mut map = Map::new(Dynamic(&arena));
        assert_eq!(map.reserve(100), Ok(()));
        let capacity = map.capacity();
        assert!(capacity >= 100);
        for key in 0..100u32 {
            assert!(map.insert(key, Tracked(&drops)).is_ok());
        }
        assert_eq!(map.capacity(), capacity);
        for key in 100..300u32 {
            assert!(map.insert(key, Tracked(&drops)).is_ok());
        }
        assert_eq!(drops.get(), 0);
        drop(map.remove(&7));
        assert_eq!(drops.get(), 1);
        assert_eq!(map.reserve(1 << 24), Err(Full(())));
    }
    assert_eq!(drops.get(), 300);
}

#[test]
fn deque_grows_at_both_ends() {
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    let mut deque = Deque::new(Dynamic(&arena));
    for i in 0..100i32 {
        assert!(deque.push_back(i).is_ok());
        assert!(deque.push_front(-i - 1).is_ok());
    }
    assert_eq!(deque.len(), 200);
    assert!(deque.iter().copied().eq(-100..100));
    assert_eq!(deque.pop_front(), Some(-100));
    assert_eq!(deque.pop_back(), Some(99));
}

#[test]
fn deque_grows_while_wrapped() {
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    let mut deque = Deque::new(Dynamic(&arena));
    let mut next = 0u32;
    let mut front = 0u32;
    // Popping between pushes moves the front around, so the contents wrap
    // at many different places when the deque has to grow.
    for round in 0..200 {
        for _ in 0..(round % 7) + 1 {
            assert!(deque.push_back(next).is_ok());
            next += 1;
        }
        if round % 3 != 0 {
            assert_eq!(deque.pop_front(), Some(front));
            front += 1;
        }
    }
    assert_eq!(deque.len() as u32, next - front);
    assert!(deque.iter().copied().eq(front..next));
    assert_eq!(deque.reserve(1000), Ok(()));
    assert!(deque.capacity() >= deque.len() + 1000);
    assert!(deque.iter().copied().eq(front..next));
}

#[test]
fn deque_drops_each_element_once() {
    let drops = Cell::new(0);
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    {
        let mut deque = Deque::new(Dynamic(&arena));
        for i in 0..50 {
            assert!(deque.push_back(Tracked(&drops)).is_ok());
            if i % 5 == 0 {
                drop(deque.pop_front());
            }
            assert!(deque.push_front(Tracked(&drops)).is_ok());
        }
        assert_eq!(drops.get(), 10);
    }
    assert_eq!(drops.get(), 100);
}

#[test]
fn deque_push_overwrite_never_grows() {
    let mut block = Memory::<u8, 1024>::new();
    let arena = Arena::new(&mut block);
    let mut history = Deque::new(Dynamic(&arena));
    assert_eq!(history.push_overwrite(1u8), Some(1));
    assert_eq!(history.reserve(3), Ok(()));
    let capacity = history.capacity();
    let evicted = (0..100u8).filter_map(|i| history.push_overwrite(i)).count();
    assert_eq!(history.capacity(), capacity);
    assert_eq!(evicted, 100 - capacity);
}

#[test]
fn heap_and_ordered_map_grow() {
    let mut block = Memory::<u8, 32768>::new();
    let arena = Arena::new(&mut block);
    let mut heap = Heap::new(Dynamic(&arena));
    let mut sorted = OrderedMap::new(Dynamic(&arena));
    for i in 0..500u32 {
        let value = (i * 7919) % 1009;
        assert!(heap.push(value).is_ok());
        assert_eq!(sorted.insert(value, i), Ok(None));
    }
    let greatest = (0..500u32).map(|i| (i * 7919) % 1009).max();
    assert_eq!(heap.peek(), greatest.as_ref());
    assert_eq!(sorted.len(), 500);
    assert!(sorted.as_slice().is_sorted_by_key(|(key, _)| *key));
    let mut previous = u32::MAX;
    while let Some(value) = heap.pop() {
        assert!(value <= previous);
        previous = value;
    }
    assert_eq!(heap.reserve(10), Ok(()));
    assert_eq!(sorted.reserve(10), Ok(()));
    assert!(sorted.remaining() >= 10);
}

#[test]
fn bitset_grows_to_the_highest_bit() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut bits = Bitset::new(Dynamic(&arena));
    assert_eq!(bits.capacity(), 0);
    assert!(!bits.contains(1000));
    assert_eq!(bits.insert(1000), Ok(true));
    assert!(bits.capacity() > 1000);
    assert!(bits.contains(1000));
    assert_eq!(bits.insert(3), Ok(true));
    assert_eq!(bits.insert(1000), Ok(false));
    assert_eq!(bits.count(), 2);
    assert!(bits.iter().eq([3, 1000]));

    // Clearing a bit that was never there needs no memory.
    let capacity = bits.capacity();
    assert_eq!(bits.set(1 << 20, false), Ok(()));
    assert!(!bits.remove(1 << 20));
    assert_eq!(bits.capacity(), capacity);

    assert_eq!(bits.toggle(2000), Ok(true));
    assert_eq!(bits.set(2001, true), Ok(()));
    assert_eq!(bits.count(), 4);

    // More than the arena can hold.
    assert_eq!(bits.insert(1 << 30), Err(OutOfBounds));
    assert_eq!(bits.insert(usize::MAX), Err(OutOfBounds));
    assert_eq!(bits.count(), 4);
}

#[test]
fn bitset_union_grows_to_fit_the_other_set() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut small = Bitset::new(Dynamic(&arena));
    let mut large = Bitset::new(Dynamic(&arena));
    assert_eq!(small.insert(1), Ok(true));
    assert_eq!(large.insert(500), Ok(true));
    small.union_with(&large);
    assert!(small.contains(1) && small.contains(500));
}

#[test]
fn trees_grow() {
    let mut block = Memory::<u8, 32768>::new();
    let arena = Arena::new(&mut block);

    let mut tree = Tree::new(Dynamic(&arena));
    let root = tree.insert_root(0u32);
    assert!(root.is_ok());
    if let Ok(root) = root {
        for i in 1..200 {
            assert!(tree.insert_child(root, i).is_ok());
        }
        assert_eq!(tree.children(root).count(), 199);
        assert_eq!(tree.get(root), Some(&0));
    }

    let mut binary = BinaryTree::new(Dynamic(&arena));
    let top = binary.set_root(0u32);
    assert!(top.is_ok());
    if let Ok(top) = top {
        let mut current = top;
        for i in 1..100 {
            let next = binary.insert_left(current, i);
            assert!(next.is_ok());
            if let Ok(next) = next {
                current = next;
            }
        }
        assert_eq!(binary.depth(current), Some(99));
        assert_eq!(binary.pre_order(top).count(), 100);
    }
}

#[test]
fn graph_grows_nodes_and_edges() {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut graph = Graph::new(Dynamic(&arena), Dynamic(&arena));
    let mut previous = None;
    let mut first = None;
    for i in 0..200u32 {
        let node = graph.add_node(i);
        assert!(node.is_ok());
        if let Ok(node) = node {
            if let Some(previous) = previous {
                assert!(graph.add_edge(previous, node, 1.0f32).is_ok());
            }
            first = first.or(Some(node));
            previous = Some(node);
        }
    }
    assert_eq!((graph.node_count(), graph.edge_count()), (200, 199));
    if let Some(first) = first {
        assert_eq!(graph.outgoing(first).count(), 1);
        assert_eq!(graph.node(first), Some(&0));
    }
}

#[test]
fn static_graph_takes_what_it_needs() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let edges: Vec<(u32, u32, f32)> = (0..99).map(|i| (i, i + 1, 1.0)).collect();
    let graph = StaticGraph::build(100, &edges, Dynamic(&arena), Dynamic(&arena));
    assert!(graph.is_ok());
    if let Ok(graph) = graph {
        assert_eq!((graph.node_count(), graph.edge_count()), (100, 99));
        assert_eq!(graph.neighbors(42).first().map(|edge| edge.to), Some(43));
        let mut finder = PathFinder::new(Dynamic(&arena), Dynamic(&arena));
        assert_eq!(finder.find(&graph, 0, 99), Ok(99.0));
    }

    let mut tiny = Memory::<u8, 64>::new();
    let arena = Arena::new(&mut tiny);
    let refused = StaticGraph::build(100, &edges, Dynamic(&arena), Dynamic(&arena));
    assert!(refused.is_err());
}

#[test]
fn spatial_structures_grow() {
    let mut block = Memory::<u8, 262144>::new();
    let arena = Arena::new(&mut block);
    let bounds = Aabb::new([0.0, 0.0], [100.0, 100.0]);
    let everything = Aabb::new([0.0, 0.0], [100.0, 100.0]);

    let grid = SpatialGrid::new(bounds, [10, 10], Dynamic(&arena), Dynamic(&arena));
    let tree = Quadtree::new(bounds, Dynamic(&arena), Dynamic(&arena));
    assert!(grid.is_ok() && tree.is_ok());
    if let (Ok(mut grid), Ok(mut tree)) = (grid, tree) {
        for i in 0..1000u32 {
            let position = [(i % 100) as f32 + 0.5, (i / 10) as f32 + 0.25];
            assert!(grid.insert(position, i).is_ok());
            assert!(tree.insert(position, i).is_ok());
        }
        assert_eq!((grid.len(), tree.len()), (1000, 1000));
        let (mut in_grid, mut in_tree) = (0, 0);
        grid.query(&everything, |_, _| in_grid += 1);
        tree.query(&everything, |_, _| in_tree += 1);
        assert_eq!((in_grid, in_tree), (1000, 1000));

        let corner = Aabb::new([0.0, 0.0], [10.0, 10.0]);
        let (mut in_grid, mut in_tree) = (0, 0);
        grid.query(&corner, |_, _| in_grid += 1);
        tree.query(&corner, |_, _| in_tree += 1);
        assert_eq!(in_grid, in_tree);
        assert!(in_grid > 0);
    }

    let space = Aabb::new([0.0; 3], [8.0; 3]);
    let octree = Octree::new(space, Dynamic(&arena), Dynamic(&arena));
    assert!(octree.is_ok());
    if let Ok(mut octree) = octree {
        for i in 0..200u32 {
            let position = [
                (i % 8) as f32 + 0.5,
                (i / 8 % 8) as f32 + 0.5,
                (i / 64) as f32,
            ];
            assert!(octree.insert(position, i).is_ok());
        }
        assert_eq!(octree.len(), 200);
    }
}

#[test]
fn spatial_structures_report_a_refusing_allocator() {
    let mut block = Memory::<u8, 8>::new();
    let arena = Arena::new(&mut block);
    let bounds = Aabb::new([0.0, 0.0], [100.0, 100.0]);
    let grid: Result<SpatialGrid<u32, 2, _, _>, _> =
        SpatialGrid::new(bounds, [10, 10], Dynamic(&arena), Dynamic(&arena));
    assert!(grid.is_err());
    let tree: Result<Quadtree<u32, _, _>, _> =
        Quadtree::new(bounds, Dynamic(&arena), Dynamic(&arena));
    assert!(tree.is_err());
}

#[test]
fn path_finder_grows_with_the_space() {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut finder = PathFinder::new(Dynamic(&arena), Dynamic(&arena));
    assert_eq!(finder.node_capacity(), 0);
    assert_eq!(finder.find(&Row(500), 0, 499), Ok(499.0));
    assert!(finder.node_capacity() >= 500);
    let mut path = Vector::new(Dynamic(&arena));
    assert_eq!(finder.path_into(&mut path), Ok(500));
    assert!(path.iter().copied().eq(0..500));
    // Searching again reuses the memory.
    let capacity = finder.node_capacity();
    assert_eq!(finder.find(&Row(500), 499, 0), Ok(499.0));
    assert_eq!(finder.node_capacity(), capacity);
    assert_eq!(finder.reserve(1000), Ok(()));
    assert!(finder.node_capacity() >= 1000);
}

#[test]
fn path_finder_reports_a_refusing_allocator() {
    let mut block = Memory::<u8, 256>::new();
    let arena = Arena::new(&mut block);
    let mut finder = PathFinder::new(Dynamic(&arena), Dynamic(&arena));
    assert!(finder.find(&Row(5000), 0, 4999).is_err());
    assert_eq!(finder.reserve(1 << 20), Err(Full(())));
}

#[cfg(feature = "alloc")]
mod global {
    use vas::{
        BinaryTree, Bitset, Deque, Dynamic, Graph, Heap, Map, OrderedMap, PathFinder, Pool, Tree,
    };

    /// Everything owned, nothing sized, no lifetime.
    #[derive(Default)]
    struct World {
        enemies: Pool<String, Dynamic>,
        names: Map<u32, String, Dynamic>,
        events: Deque<u32, Dynamic>,
        queue: Heap<u32, Dynamic>,
        scores: OrderedMap<u32, u32, Dynamic>,
        seen: Bitset<Dynamic>,
        scene: Tree<u32, Dynamic>,
        decisions: BinaryTree<u32, Dynamic>,
        roads: Graph<u32, f32, Dynamic>,
        finder: PathFinder<Dynamic>,
    }

    #[test]
    fn a_world_of_growing_containers() {
        let mut world = World::default();
        let mut previous = None;
        for i in 0..2000u32 {
            assert!(world.enemies.insert(format!("enemy {i}")).is_ok());
            assert!(world.names.insert(i, format!("name {i}")).is_ok());
            assert!(world.events.push_back(i).is_ok());
            assert!(world.queue.push(i).is_ok());
            assert!(world.scores.insert(i, i).is_ok());
            assert!(world.seen.insert(i as usize * 3).is_ok());
            assert!(world.scene.insert_root(i).is_ok());
            let node = world.roads.add_node(i);
            assert!(node.is_ok());
            if let (Some(previous), Ok(node)) = (previous, node) {
                assert!(world.roads.add_edge(previous, node, 1.0).is_ok());
            }
            previous = node.ok();
        }
        assert!(world.decisions.set_root(1).is_ok());
        assert_eq!(world.enemies.len(), 2000);
        assert_eq!(
            world.names.get(&1999).map(String::as_str),
            Some("name 1999")
        );
        assert_eq!(world.events.front(), Some(&0));
        assert_eq!(world.queue.peek(), Some(&1999));
        assert_eq!(world.scores.last(), Some((&1999, &1999)));
        assert_eq!(world.seen.count(), 2000);
        assert_eq!(world.scene.roots().count(), 2000);
        assert_eq!(world.roads.edge_count(), 1999);
        assert_eq!(world.finder.find(&super::Row(100), 0, 99), Ok(99.0));
    }

    #[test]
    fn containers_move_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<World>();
    }
}
