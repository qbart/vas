use vas::graph::{EdgeSlot, NodeSlot};
use vas::path::{Node, Open};
use vas::static_graph::{BuildError, Edge};
use vas::{Graph, Memory, PathFinder, Rng, StaticGraph};

#[test]
fn builds_from_an_edge_list() {
    let mut offsets = Memory::<u32, 5>::new();
    let mut edges = Memory::<Edge<f32>, 8>::new();
    let input = [
        (2, 0, 5.0),
        (0, 1, 1.0),
        (0, 2, 2.0),
        (2, 3, 3.0),
        (0, 3, 9.0),
    ];
    let graph = StaticGraph::build(4, &input, &mut offsets, &mut edges);
    assert!(graph.is_ok());
    if let Ok(graph) = graph {
        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.edge_count(), 5);
        // Edges of a node keep the order they had in the input.
        let targets = |node| {
            graph
                .neighbors(node)
                .iter()
                .map(|edge| edge.to)
                .collect::<std::vec::Vec<_>>()
        };
        assert_eq!(targets(0), [1, 2, 3]);
        assert_eq!(targets(1), []);
        assert_eq!(targets(2), [0, 3]);
        assert_eq!(targets(3), []);
        assert_eq!(targets(4), []);
        assert_eq!(graph.neighbors(0).first().map(|edge| edge.value), Some(1.0));
        assert_eq!(graph.out_degree(0), 3);
        assert_eq!(graph.out_degree(1), 0);
        assert_eq!(graph.out_degree(99), 0);
    }
}

#[test]
fn empty_graphs() {
    let mut offsets = Memory::<u32, 1>::new();
    let mut edges = Memory::<Edge<f32>, 0>::new();
    let graph = StaticGraph::build(0, &[], &mut offsets, &mut edges);
    assert!(graph.is_ok_and(|graph| graph.node_count() == 0 && graph.edge_count() == 0));

    let mut offsets = Memory::<u32, 4>::new();
    let mut edges = Memory::<Edge<f32>, 0>::new();
    let graph = StaticGraph::build(3, &[], &mut offsets, &mut edges);
    assert!(graph.is_ok_and(|graph| graph.node_count() == 3 && graph.neighbors(1).is_empty()));
}

#[test]
fn build_errors() {
    let input = [(0, 1, 1.0), (1, 0, 1.0)];
    // Offsets need node_count + 1 entries.
    let mut offsets = Memory::<u32, 2>::new();
    let mut edges = Memory::<Edge<f32>, 2>::new();
    assert_eq!(
        StaticGraph::build(2, &input, &mut offsets, &mut edges).err(),
        Some(BuildError::Full)
    );
    let mut offsets = Memory::<u32, 3>::new();
    let mut edges = Memory::<Edge<f32>, 1>::new();
    assert_eq!(
        StaticGraph::build(2, &input, &mut offsets, &mut edges).err(),
        Some(BuildError::Full)
    );
    let mut offsets = Memory::<u32, 3>::new();
    let mut edges = Memory::<Edge<f32>, 2>::new();
    assert_eq!(
        StaticGraph::build(2, &[(0, 2, 1.0)], &mut offsets, &mut edges).err(),
        Some(BuildError::OutOfBounds)
    );
    assert_eq!(
        StaticGraph::build(2, &[(2, 0, 1.0)], &mut offsets, &mut edges).err(),
        Some(BuildError::OutOfBounds)
    );
}

#[test]
fn path_search_agrees_with_the_dynamic_graph() {
    const NODES: usize = 30;
    let mut rng = Rng::new(555);
    let rounds = if cfg!(miri) { 2 } else { 40 };
    for _ in 0..rounds {
        let mut input = [(0u32, 0u32, 0.0f32); 90];
        for edge in input.iter_mut() {
            *edge = (
                rng.u32_below(30),
                rng.u32_below(30),
                rng.f32_range(1.0, 10.0),
            );
        }

        let mut offsets = Memory::<u32, { NODES + 1 }>::new();
        let mut edges = Memory::<Edge<f32>, 90>::new();
        let fixed = StaticGraph::build(NODES, &input, &mut offsets, &mut edges);

        let mut node_memory = Memory::<NodeSlot<()>, NODES>::new();
        let mut edge_memory = Memory::<EdgeSlot<f32>, 90>::new();
        let mut dynamic = Graph::new(&mut node_memory, &mut edge_memory);
        for _ in 0..NODES {
            assert!(dynamic.add_node(()).is_ok());
        }
        for (from, to, cost) in input {
            if let (Some(from), Some(to)) = (dynamic.node_at(from), dynamic.node_at(to)) {
                assert!(dynamic.add_edge(from, to, cost).is_ok());
            }
        }

        let mut search = Memory::<Node, NODES>::new();
        let mut open = Memory::<Open, 256>::new();
        let mut finder = PathFinder::new(&mut search, &mut open);
        assert!(fixed.is_ok());
        if let Ok(fixed) = fixed {
            for _ in 0..10 {
                let (start, goal) = (rng.u32_below(30), rng.u32_below(30));
                let expected = finder.find(&dynamic, start, goal);
                assert_eq!(finder.find(&fixed, start, goal), expected);
            }
        }
    }
}
