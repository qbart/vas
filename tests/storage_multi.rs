//! Containers with two blocks of memory: each block has its own storage
//! parameter, and the second defaults to the first.

use vas::path::Space;
use vas::static_graph::Edge;
use vas::{
    Aabb, Borrowed, Graph, Inline, Memory, Octree, PathFinder, Quadtree, SpatialGrid, StaticGraph,
};

/// A row of `len` nodes, each connected to the next.
struct Row(u32);

impl Space for Row {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        if node + 1 < self.0 {
            visit(node + 1, 1.0);
        }
    }
}

/// A struct that owns everything, with no lifetime.
struct Level {
    roads: Graph<&'static str, f32, Inline<8>, Inline<16>>,
    units: SpatialGrid<u32, 2, Inline<16>, Inline<8>>,
    props: Quadtree<u32, Inline<16>, Inline<8>>,
    finder: PathFinder<Inline<8>, Inline<16>>,
    links: StaticGraph<f32, Inline<4>, Inline<3>>,
}

fn level() -> Option<Level> {
    let bounds = Aabb::new([0.0, 0.0], [8.0, 8.0]);
    Some(Level {
        roads: Graph::inline(),
        units: SpatialGrid::new(bounds, [4, 4], Memory::new(), Memory::new()).ok()?,
        props: Quadtree::new(bounds, Memory::new(), Memory::new()).ok()?,
        finder: PathFinder::new(Memory::new(), Memory::new()),
        links: StaticGraph::build(
            3,
            &[(0, 1, 1.5), (0, 2, 4.0), (1, 2, 1.0)],
            Memory::new(),
            Memory::new(),
        )
        .ok()?,
    })
}

#[test]
fn a_struct_can_own_two_buffer_containers() {
    let level = level();
    assert!(level.is_some());
    if let Some(mut level) = level {
        let a = level.roads.add_node("a");
        let b = level.roads.add_node("b");
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(a), Ok(b)) = (a, b) {
            assert!(level.roads.add_edge(a, b, 2.0).is_ok());
            assert_eq!(level.roads.outgoing(a).count(), 1);
        }
        assert_eq!(
            (level.roads.node_capacity(), level.roads.edge_capacity()),
            (8, 16)
        );

        assert!(level.units.insert([1.0, 1.0], 7).is_ok());
        assert!(level.props.insert([2.0, 2.0], 9).is_ok());
        assert_eq!((level.units.capacity(), level.props.capacity()), (8, 8));

        assert_eq!(level.finder.find(&Row(8), 0, 7), Ok(7.0));
        assert_eq!(level.finder.find(&level.links, 0, 2), Ok(2.5));
        assert_eq!(level.links.neighbors(0).len(), 2);

        // Moving the struct moves everything in it.
        let mut moved = core::hint::black_box(level);
        let mut seen = 0;
        moved
            .units
            .query(&Aabb::new([0.0, 0.0], [8.0, 8.0]), |_, value| {
                seen += *value
            });
        moved
            .props
            .query(&Aabb::new([0.0, 0.0], [8.0, 8.0]), |_, value| {
                seen += *value
            });
        assert_eq!(seen, 16);
        assert_eq!(moved.roads.node_count(), 2);
        assert_eq!(moved.finder.find(&Row(8), 0, 3), Ok(3.0));
        assert_eq!(
            moved.links.neighbors(1).first().map(|edge| edge.to),
            Some(2)
        );
    }
}

#[test]
fn one_parameter_is_enough_for_borrowed_storage() {
    let mut nodes = Memory::<vas::graph::NodeSlot<u8>, 4>::new();
    let mut edges = Memory::<vas::graph::EdgeSlot<u8>, 4>::new();
    let mut graph: Graph<u8, u8, Borrowed<'_>> = Graph::new(&mut nodes, &mut edges);
    assert!(graph.add_node(1).is_ok());

    let mut offsets = Memory::<u32, 4>::new();
    let mut edges = Memory::<Edge<f32>, 3>::new();
    let built: Result<StaticGraph<f32, Borrowed<'_>>, _> =
        StaticGraph::build(3, &[(0, 1, 1.0)], &mut offsets, &mut edges);
    assert!(built.is_ok());

    let mut nodes = Memory::<vas::path::Node, 4>::new();
    let mut open = Memory::<vas::path::Open, 8>::new();
    let mut finder: PathFinder<Borrowed<'_>> = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&Row(4), 0, 3), Ok(3.0));
}

#[test]
fn the_two_blocks_can_use_different_storage() {
    // Cells inline, entries borrowed.
    let mut entries = Memory::<vas::spatial_grid::Slot<u32, 2>, 4>::new();
    let bounds = Aabb::new([0.0, 0.0], [4.0, 4.0]);
    let grid: Result<SpatialGrid<u32, 2, Inline<4>, Borrowed<'_>>, _> =
        SpatialGrid::new(bounds, [2, 2], Memory::new(), &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        assert!(grid.insert([1.0, 1.0], 5).is_ok());
        assert_eq!(grid.len(), 1);
    }
}

#[test]
fn inline_blocks_that_are_too_small_are_reported() {
    let bounds = Aabb::new([0.0, 0.0], [4.0, 4.0]);
    // 16 cells do not fit in 4.
    let grid: Result<SpatialGrid<u32, 2, Inline<4>, Inline<4>>, _> =
        SpatialGrid::new(bounds, [4, 4], Memory::new(), Memory::new());
    assert!(grid.is_err());
    // Two edges do not fit in one.
    let graph: Result<StaticGraph<f32, Inline<4>, Inline<1>>, _> =
        StaticGraph::build(3, &[(0, 1, 1.0), (1, 2, 1.0)], Memory::new(), Memory::new());
    assert!(graph.is_err());
}

#[test]
fn octree_takes_the_same_parameters() {
    let bounds = Aabb::new([0.0; 3], [8.0; 3]);
    let tree: Result<Octree<u32, Inline<32>, Inline<8>>, _> =
        Octree::new(bounds, Memory::new(), Memory::new());
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        assert!(tree.insert([1.0, 2.0, 3.0], 1).is_ok());
        assert_eq!(tree.len(), 1);
    }
}

#[test]
fn graph_default_is_empty_and_inline() {
    let graph = Graph::<u8, u8, Inline<4>, Inline<4>>::default();
    assert_eq!((graph.node_count(), graph.edge_count()), (0, 0));
    static ROADS: Graph<u8, u8, Inline<4>> = Graph::inline();
    assert_eq!(ROADS.node_capacity(), 4);
}
