use core::cell::Cell;
use vas::heap::Min;
use vas::{
    Aabb, BinaryTree, Bitset, Deque, Graph, Heap, Inline, Map, Memory, OrderedMap, PathFinder,
    Pool, Quadtree, SpatialGrid, StaticGraph, Tree, Vector,
};

/// Counts clones and drops, to check that a copy owns its own values.
struct Tracked<'a> {
    id: u32,
    clones: &'a Cell<u32>,
    drops: &'a Cell<u32>,
}

impl Clone for Tracked<'_> {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            id: self.id,
            clones: self.clones,
            drops: self.drops,
        }
    }
}

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn vector_copy_is_independent() {
    let mut v = Vector::<String, Inline<4>>::inline();
    assert!(v.push("a".to_string()).is_ok());
    assert!(v.push("b".to_string()).is_ok());
    let mut copy = v.clone();
    assert_eq!(copy.as_slice(), v.as_slice());
    assert_eq!(copy.capacity(), 4);
    assert!(copy.push("c".to_string()).is_ok());
    if let Some(first) = v.get_mut(0) {
        first.push('!');
    }
    assert_eq!(v.as_slice(), &["a!", "b"]);
    assert_eq!(copy.as_slice(), &["a", "b", "c"]);
}

#[test]
fn vector_clones_and_drops_each_element_once() {
    let clones = Cell::new(0);
    let drops = Cell::new(0);
    {
        let mut v = Vector::<Tracked<'_>, Inline<8>>::inline();
        for id in 0..5 {
            let value = Tracked {
                id,
                clones: &clones,
                drops: &drops,
            };
            assert!(v.push(value).is_ok());
        }
        let copy = v.clone();
        assert_eq!(clones.get(), 5);
        assert!(copy.iter().map(|t| t.id).eq(0..5));
        drop(copy);
        assert_eq!(drops.get(), 5);
    }
    assert_eq!(drops.get(), 10);
}

#[test]
fn empty_and_zero_capacity_containers() {
    let v = Vector::<u32, Inline<0>>::inline();
    assert!(v.clone().is_empty());
    let d = Deque::<String, Inline<3>>::inline();
    assert!(d.clone().is_empty());
    let p = Pool::<String, Inline<0>>::inline();
    assert!(p.clone().is_empty());
    let m = Map::<u32, String, Inline<0>>::inline();
    assert!(m.clone().is_empty());
    let b = Bitset::<Inline<0>>::inline();
    assert_eq!(b.clone().count(), 0);
}

#[test]
fn deque_keeps_the_order_when_wrapped() {
    let mut d = Deque::<String, Inline<4>>::inline();
    for i in 0..4 {
        assert!(d.push_back(i.to_string()).is_ok());
    }
    // Move the start away from slot 0 so the contents wrap around.
    assert_eq!(d.pop_front().as_deref(), Some("0"));
    assert_eq!(d.pop_front().as_deref(), Some("1"));
    assert!(d.push_back("4".to_string()).is_ok());
    let mut copy = d.clone();
    assert!(copy.iter().eq(d.iter()));
    assert!(copy.push_back("5".to_string()).is_ok());
    assert_eq!(copy.len(), 4);
    assert_eq!(d.len(), 3);
    assert_eq!(copy.pop_front().as_deref(), Some("2"));
    assert_eq!(copy.pop_back().as_deref(), Some("5"));
}

#[test]
fn pool_handles_work_on_the_copy() {
    let mut pool = Pool::<String, Inline<4>>::inline();
    let a = pool.insert("a".to_string()).ok();
    let b = pool.insert("b".to_string()).ok();
    let c = pool.insert("c".to_string()).ok();
    let (Some(a), Some(b), Some(c)) = (a, b, c) else {
        return;
    };
    assert_eq!(pool.remove(b).as_deref(), Some("b"));

    let mut copy = pool.clone();
    assert_eq!(copy.len(), 2);
    assert_eq!(copy.get(a).map(String::as_str), Some("a"));
    assert_eq!(copy.get(c).map(String::as_str), Some("c"));
    // Stale in the original, stale in the copy.
    assert!(!copy.contains(b));

    // Both reuse the freed slot, with the same new handle.
    let in_copy = copy.insert("d".to_string()).ok();
    let in_pool = pool.insert("e".to_string()).ok();
    assert_eq!(in_copy, in_pool);
    assert_eq!(in_copy.map(|h| h.index()), Some(b.index()));
    assert_eq!(
        in_copy.and_then(|h| copy.get(h)).map(String::as_str),
        Some("d")
    );
    assert_eq!(
        in_pool.and_then(|h| pool.get(h)).map(String::as_str),
        Some("e")
    );

    // Then both fill up at the same point.
    assert!(copy.insert("f".to_string()).is_ok());
    assert!(copy.is_full());
    assert_eq!(pool.len(), 3);
}

#[test]
fn pool_clones_and_drops_each_value_once() {
    let clones = Cell::new(0);
    let drops = Cell::new(0);
    {
        let mut pool = Pool::<Tracked<'_>, Inline<8>>::inline();
        let mut handles = Vector::<_, Inline<8>>::inline();
        for id in 0..6 {
            let value = Tracked {
                id,
                clones: &clones,
                drops: &drops,
            };
            if let Ok(handle) = pool.insert(value) {
                assert!(handles.push(handle).is_ok());
            }
        }
        for handle in handles.iter().step_by(2) {
            drop(pool.remove(*handle));
        }
        assert_eq!(drops.get(), 3);
        let copy = pool.clone();
        assert_eq!(clones.get(), 3);
        assert!(copy.iter().map(|(_, t)| t.id).eq([1, 3, 5]));
    }
    assert_eq!(drops.get(), 9);
}

#[test]
fn map_copy_finds_every_key() {
    let mut map = Map::<u32, String, Inline<64>>::inline();
    for key in 0..40u32 {
        assert!(map.insert(key * 7, key.to_string()).is_ok());
    }
    for key in 0..10u32 {
        assert!(map.remove(&(key * 21)).is_some());
    }
    let mut copy = map.clone();
    assert_eq!(copy.len(), map.len());
    for key in 0..300u32 {
        assert_eq!(copy.get(&key), map.get(&key));
    }
    // The copy keeps working as a map of its own.
    assert!(copy.insert(1, "one".to_string()).is_ok());
    assert!(copy.remove(&7).is_some());
    assert_eq!(copy.get(&1).map(String::as_str), Some("one"));
    assert!(map.get(&1).is_none());
    assert!(map.get(&7).is_some());
    assert_eq!(copy.len(), map.len());
}

#[test]
fn map_copy_fills_up_like_the_original() {
    let mut map = Map::<u32, u32, Inline<16>>::inline();
    for key in 0..5 {
        assert!(map.insert(key, key).is_ok());
    }
    let mut copy = map.clone();
    let mut a = 5;
    while map.insert(a, a).is_ok() {
        a += 1;
    }
    let mut b = 5;
    while copy.insert(b, b).is_ok() {
        b += 1;
    }
    assert_eq!(a, b);
    assert!((0..b).all(|key| copy.get(&key) == Some(&key)));
}

#[test]
fn heap_keeps_its_order() {
    let mut heap = Heap::<u32, Inline<8>, Min>::default();
    for value in [5, 1, 4, 2, 3] {
        assert!(heap.push(value).is_ok());
    }
    let mut copy = heap.clone();
    assert!(copy.push(0).is_ok());
    let mut drained = Vector::<u32, Inline<8>>::inline();
    while let Some(value) = copy.pop() {
        assert!(drained.push(value).is_ok());
    }
    assert_eq!(drained.as_slice(), &[0, 1, 2, 3, 4, 5]);
    assert_eq!(heap.len(), 5);
    assert_eq!(heap.peek(), Some(&1));
}

#[test]
fn ordered_map_copy_is_independent() {
    let mut map = OrderedMap::<u32, String, Inline<8>>::inline();
    for key in [3, 1, 2] {
        assert!(map.insert(key, key.to_string()).is_ok());
    }
    let mut copy = map.clone();
    assert!(copy.iter().eq(map.iter()));
    assert!(copy.remove(&2).is_some());
    assert!(map.get(&2).is_some());
    assert!(copy.iter().map(|(key, _)| *key).eq([1, 3]));
}

#[test]
fn bitset_copy_is_independent() {
    let mut bits = Bitset::<Inline<2>>::inline();
    assert!(bits.insert(3).is_ok());
    assert!(bits.insert(100).is_ok());
    let mut copy = bits.clone();
    assert!(copy.iter().eq([3, 100]));
    assert!(copy.insert(64).is_ok());
    assert!(bits.remove(3));
    assert!(copy.iter().eq([3, 64, 100]));
    assert!(bits.iter().eq([100]));
}

#[test]
fn tree_ids_work_on_the_copy() {
    let mut tree = Tree::<String, Inline<8>>::inline();
    let Ok(root) = tree.insert_root("root".to_string()) else {
        return;
    };
    let Ok(a) = tree.insert_child(root, "a".to_string()) else {
        return;
    };
    let Ok(b) = tree.insert_child(root, "b".to_string()) else {
        return;
    };
    assert!(tree.insert_child(a, "a1".to_string()).is_ok());

    let mut copy = tree.clone();
    assert_eq!(copy.len(), 4);
    assert!(copy.children(root).eq([a, b]));
    assert!(copy.descendants(root).eq(tree.descendants(root)));
    assert_eq!(copy.remove(a).as_deref(), Some("a"));
    assert_eq!(copy.len(), 2);
    assert_eq!(tree.len(), 4);
    assert_eq!(tree.get(a).map(String::as_str), Some("a"));
}

#[test]
fn binary_tree_ids_work_on_the_copy() {
    let mut tree = BinaryTree::<u32, Inline<8>>::inline();
    let Ok(root) = tree.set_root(1) else {
        return;
    };
    let Ok(left) = tree.insert_left(root, 2) else {
        return;
    };
    assert!(tree.insert_right(root, 3).is_ok());

    let mut copy = tree.clone();
    assert_eq!(copy.root(), Some(root));
    assert_eq!(copy.left(root), Some(left));
    assert_eq!(copy.right(root).and_then(|id| copy.get(id)), Some(&3));
    if let Some(value) = copy.get_mut(left) {
        *value = 20;
    }
    assert_eq!(tree.get(left), Some(&2));
    assert_eq!(copy.len(), 3);
}

#[test]
fn graph_ids_work_on_the_copy() {
    let mut graph = Graph::<&str, f32, Inline<4>, Inline<4>>::inline();
    let (Ok(a), Ok(b), Ok(c)) = (
        graph.add_node("a"),
        graph.add_node("b"),
        graph.add_node("c"),
    ) else {
        return;
    };
    let Ok(ab) = graph.add_edge(a, b, 1.0) else {
        return;
    };
    assert!(graph.add_edge(a, c, 2.0).is_ok());

    let mut copy = graph.clone();
    assert_eq!((copy.node_count(), copy.edge_count()), (3, 2));
    assert_eq!(copy.edge(ab), Some(&1.0));
    assert!(copy.outgoing(a).eq(graph.outgoing(a)));
    assert!(copy.remove_node(b).is_some());
    assert_eq!(copy.edge_count(), 1);
    assert_eq!(graph.edge_count(), 2);
    assert_eq!(graph.node(b), Some(&"b"));
}

#[test]
fn static_graph_copy_has_the_same_edges() {
    let graph: Result<StaticGraph<f32, Inline<4>, Inline<2>>, _> =
        StaticGraph::build(3, &[(0, 1, 1.5), (1, 2, 2.5)], Memory::new(), Memory::new());
    let Ok(graph) = graph else {
        return;
    };
    let copy = graph.clone();
    assert_eq!(copy.node_count(), 3);
    assert_eq!(copy.edge_count(), 2);
    for node in 0..3 {
        assert_eq!(copy.neighbors(node), graph.neighbors(node));
    }
}

#[test]
fn spatial_grid_copy_is_independent() {
    let bounds = Aabb::new([0.0, 0.0], [8.0, 8.0]);
    let grid: Result<SpatialGrid<String, 2, Inline<16>, Inline<8>>, _> =
        SpatialGrid::new(bounds, [4, 4], Memory::new(), Memory::new());
    let Ok(mut grid) = grid else {
        return;
    };
    let Ok(a) = grid.insert([1.0, 1.0], "a".to_string()) else {
        return;
    };
    assert!(grid.insert([7.0, 7.0], "b".to_string()).is_ok());

    let mut copy = grid.clone();
    assert_eq!(copy.len(), 2);
    assert_eq!(copy.get(a).map(String::as_str), Some("a"));
    assert!(copy.set_position(a, [7.0, 6.0]));
    let corner = Aabb::new([6.0, 6.0], [8.0, 8.0]);
    let (mut in_copy, mut in_grid) = (0, 0);
    copy.query(&corner, |_, _| in_copy += 1);
    grid.query(&corner, |_, _| in_grid += 1);
    assert_eq!((in_copy, in_grid), (2, 1));
    assert_eq!(grid.position(a), Some([1.0, 1.0]));
}

#[test]
fn quadtree_copy_is_independent() {
    let bounds = Aabb::new([0.0, 0.0], [8.0, 8.0]);
    let tree: Result<Quadtree<u32, Inline<32>, Inline<16>>, _> =
        Quadtree::with_limits(bounds, 2, 4, Memory::new(), Memory::new());
    let Ok(mut tree) = tree else {
        return;
    };
    let mut first = None;
    for i in 0..10u32 {
        let id = tree.insert([i as f32 * 0.8, 8.0 - i as f32 * 0.8], i).ok();
        assert!(id.is_some());
        first = first.or(id);
    }
    let mut copy = tree.clone();
    assert_eq!(copy.len(), 10);
    assert_eq!(copy.node_count(), tree.node_count());
    let (mut in_copy, mut in_tree) = (0, 0);
    copy.query(&bounds, |_, value| in_copy += *value);
    tree.query(&bounds, |_, value| in_tree += *value);
    assert_eq!(in_copy, in_tree);
    assert_eq!(first.and_then(|id| copy.remove(id)), Some(0));
    assert!(copy.insert([4.0, 4.0], 99).is_ok());
    assert_eq!((copy.len(), tree.len()), (10, 10));
    assert_eq!(first.and_then(|id| tree.get(id)), Some(&0));
}

#[test]
fn path_finder_copy_remembers_the_path() {
    let graph: Result<StaticGraph<f32, Inline<5>, Inline<4>>, _> = StaticGraph::build(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 10.0)],
        Memory::new(),
        Memory::new(),
    );
    let Ok(graph) = graph else {
        return;
    };
    let mut finder = PathFinder::<Inline<4>, Inline<8>>::new(Memory::new(), Memory::new());
    assert_eq!(finder.find(&graph, 0, 3), Ok(3.0));
    let mut copy = finder.clone();
    assert!(copy.path_rev().eq([3, 2, 1, 0]));
    assert_eq!(copy.cost(3), Some(3.0));
    assert_eq!(copy.find(&graph, 1, 3), Ok(2.0));
    assert!(finder.path_rev().eq([3, 2, 1, 0]));
}
