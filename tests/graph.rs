use core::cell::Cell;
use std::vec::Vec;
use vas::graph::{EdgeId, EdgeSlot, InsertError, NodeId, NodeSlot};
use vas::path::{Node, Open, PathError};
use vas::{Full, Graph, Memory, PathFinder, Rng, Vector};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut nodes = Memory::<NodeSlot<u32>, 4>::new();
    let mut edges = Memory::<EdgeSlot<u32>, 8>::new();
    let graph = Graph::new(&mut nodes, &mut edges);
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
    assert_eq!(graph.node_capacity(), 4);
    assert_eq!(graph.edge_capacity(), 8);
    assert_eq!(graph.nodes().count(), 0);
    assert_eq!(graph.edges().count(), 0);
}

#[test]
fn nodes_and_edges() {
    let mut nodes = Memory::<NodeSlot<&str>, 4>::new();
    let mut edges = Memory::<EdgeSlot<u32>, 8>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let a = graph.add_node("a").ok();
    let b = graph.add_node("b").ok();
    let c = graph.add_node("c").ok();
    assert!(a.is_some() && b.is_some() && c.is_some());
    if let (Some(a), Some(b), Some(c)) = (a, b, c) {
        let ab = graph.add_edge(a, b, 1).ok();
        let ac = graph.add_edge(a, c, 2).ok();
        let cb = graph.add_edge(c, b, 3).ok();
        assert!(ab.is_some() && ac.is_some() && cb.is_some());
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 3);
        assert_eq!(graph.node(a), Some(&"a"));
        if let (Some(ab), Some(ac), Some(cb)) = (ab, ac, cb) {
            assert_eq!(graph.edge(ab), Some(&1));
            assert_eq!(graph.endpoints(ab), Some((a, b)));
            assert_eq!(graph.endpoints(cb), Some((c, b)));
            // Most recently added first.
            assert!(graph.outgoing(a).eq([(ac, c), (ab, b)]));
            assert!(graph.incoming(b).eq([(cb, c), (ab, a)]));
            assert_eq!(graph.outgoing(b).count(), 0);
            assert_eq!(graph.incoming(a).count(), 0);
            assert_eq!(graph.find_edge(a, b), Some(ab));
            assert_eq!(graph.find_edge(b, a), None);

            if let Some(weight) = graph.edge_mut(ab) {
                *weight = 10;
            }
            if let Some(name) = graph.node_mut(b) {
                *name = "B";
            }
            assert_eq!(graph.edge(ab), Some(&10));
            assert_eq!(graph.node(b), Some(&"B"));

            assert_eq!(graph.remove_edge(ac), Some(2));
            assert_eq!(graph.remove_edge(ac), None);
            assert!(graph.outgoing(a).eq([(ab, b)]));
            assert_eq!(graph.incoming(c).count(), 0);
            assert_eq!(graph.edge_count(), 2);
        }
        assert_eq!(graph.node_at(a.index()), Some(a));
        assert_eq!(graph.node_at(99), None);
    }
}

#[test]
fn removing_a_node_removes_its_edges() {
    let drops = Cell::new(0);
    let mut nodes = Memory::<NodeSlot<u32>, 4>::new();
    let mut edges = Memory::<EdgeSlot<Counted>, 8>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let a = graph.add_node(1).ok();
    let b = graph.add_node(2).ok();
    let c = graph.add_node(3).ok();
    if let (Some(a), Some(b), Some(c)) = (a, b, c) {
        assert!(graph.add_edge(a, b, Counted(&drops)).is_ok());
        assert!(graph.add_edge(b, a, Counted(&drops)).is_ok());
        assert!(graph.add_edge(b, c, Counted(&drops)).is_ok());
        assert!(graph.add_edge(b, b, Counted(&drops)).is_ok());
        let ac = graph.add_edge(a, c, Counted(&drops)).ok();
        assert_eq!(graph.remove_node(b), Some(2));
        assert_eq!(drops.get(), 4);
        assert_eq!(graph.remove_node(b), None);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        assert!(!graph.contains_node(b));
        assert_eq!(graph.outgoing(a).map(|(edge, _)| edge).next(), ac);
        assert_eq!(graph.outgoing(a).count(), 1);
        assert_eq!(graph.incoming(a).count(), 0);
        assert_eq!(graph.incoming(c).count(), 1);
        assert_eq!(graph.outgoing(b).count(), 0);
    }
    drop(graph);
    assert_eq!(drops.get(), 5);
}

#[test]
fn self_loops_and_parallel_edges() {
    let mut nodes = Memory::<NodeSlot<u32>, 2>::new();
    let mut edges = Memory::<EdgeSlot<u32>, 8>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let a = graph.add_node(1).ok();
    let b = graph.add_node(2).ok();
    if let (Some(a), Some(b)) = (a, b) {
        let loop_edge = graph.add_edge(a, a, 0).ok();
        assert!(graph.add_edge(a, b, 1).is_ok());
        assert!(graph.add_edge(a, b, 2).is_ok());
        assert_eq!(graph.outgoing(a).count(), 3);
        assert_eq!(graph.incoming(a).count(), 1);
        assert_eq!(graph.incoming(b).count(), 2);
        if let Some(loop_edge) = loop_edge {
            assert_eq!(graph.endpoints(loop_edge), Some((a, a)));
            assert_eq!(graph.remove_edge(loop_edge), Some(0));
        }
        assert_eq!(graph.outgoing(a).count(), 2);
        assert_eq!(graph.incoming(a).count(), 0);
    }
}

#[test]
fn errors_return_the_value() {
    let mut nodes = Memory::<NodeSlot<u32>, 2>::new();
    let mut edges = Memory::<EdgeSlot<u32>, 1>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let a = graph.add_node(1).ok();
    let b = graph.add_node(2).ok();
    assert_eq!(graph.add_node(3), Err(Full(3)));
    if let (Some(a), Some(b)) = (a, b) {
        assert!(graph.add_edge(a, b, 7).is_ok());
        assert_eq!(graph.add_edge(b, a, 8), Err(InsertError::Full(8)));
        assert_eq!(graph.remove_node(b), Some(2));
        assert_eq!(graph.add_edge(a, b, 9), Err(InsertError::Stale(9)));
        assert_eq!(graph.add_edge(b, a, 9), Err(InsertError::Stale(9)));
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(graph.node(b), None);
        assert_eq!(graph.outgoing(b).count(), 0);
    }
}

#[test]
fn clear_empties_everything() {
    let mut nodes = Memory::<NodeSlot<u32>, 2>::new();
    let mut edges = Memory::<EdgeSlot<u32>, 2>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let a = graph.add_node(1).ok();
    if let Some(a) = a {
        assert!(graph.add_edge(a, a, 1).is_ok());
        graph.clear();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert!(!graph.contains_node(a));
    }
    assert!(graph.add_node(5).is_ok());
    assert_eq!(graph.nodes().map(|(_, value)| *value).sum::<u32>(), 5);
}

#[test]
fn shortest_path_through_a_graph() {
    let mut nodes = Memory::<NodeSlot<&str>, 8>::new();
    let mut edges = Memory::<EdgeSlot<f32>, 16>::new();
    let mut graph = Graph::new(&mut nodes, &mut edges);
    let town = graph.add_node("town").ok();
    let forest = graph.add_node("forest").ok();
    let bridge = graph.add_node("bridge").ok();
    let castle = graph.add_node("castle").ok();
    let island = graph.add_node("island").ok();
    if let (Some(town), Some(forest), Some(bridge), Some(castle), Some(island)) =
        (town, forest, bridge, castle, island)
    {
        for (from, to, cost) in [
            (town, forest, 2.0),
            (forest, castle, 9.0),
            (town, bridge, 4.0),
            (bridge, castle, 3.0),
        ] {
            assert!(graph.add_edge(from, to, cost).is_ok());
        }
        let mut search = Memory::<Node, 8>::new();
        let mut open = Memory::<Open, 16>::new();
        let mut finder = PathFinder::new(&mut search, &mut open);
        assert_eq!(finder.find(&graph, town.index(), castle.index()), Ok(7.0));

        let mut memory = Memory::<u32, 8>::new();
        let mut route = Vector::new(&mut memory);
        assert_eq!(finder.path_into(&mut route), Ok(3));
        let names: Vec<&str> = route
            .iter()
            .filter_map(|index| graph.node_at(*index))
            .filter_map(|id| graph.node(id).copied())
            .collect();
        assert_eq!(names, ["town", "bridge", "castle"]);
        assert_eq!(
            finder.find(&graph, town.index(), island.index()),
            Err(PathError::NoPath)
        );
    }
}

#[test]
fn matches_a_reference_model() {
    const NODES: usize = 10;
    const EDGES: usize = 40;
    let mut node_memory = Memory::<NodeSlot<usize>, NODES>::new();
    let mut edge_memory = Memory::<EdgeSlot<usize>, EDGES>::new();
    let mut graph = Graph::new(&mut node_memory, &mut edge_memory);
    // Model: nodes[key] = (id, alive); edges[key] = (id, from, to, alive).
    let mut nodes: Vec<(NodeId<usize>, bool)> = Vec::new();
    let mut edges: Vec<(EdgeId<usize>, usize, usize, bool)> = Vec::new();
    let mut rng = Rng::new(1234);
    let steps = if cfg!(miri) { 150 } else { 4000 };
    for _ in 0..steps {
        let live_nodes: Vec<usize> = (0..nodes.len())
            .filter(|key| nodes.get(*key).is_some_and(|node| node.1))
            .collect();
        let live_edges: Vec<usize> = (0..edges.len())
            .filter(|key| edges.get(*key).is_some_and(|edge| edge.3))
            .collect();
        match rng.u32_below(8) {
            0 | 1 => match graph.add_node(nodes.len()) {
                Ok(id) => nodes.push((id, true)),
                Err(Full(_)) => assert_eq!(live_nodes.len(), NODES),
            },
            2..=5 => {
                let (Some(from), Some(to)) = (rng.pick(&live_nodes), rng.pick(&live_nodes)) else {
                    continue;
                };
                let (Some(from_id), Some(to_id)) = (nodes.get(*from), nodes.get(*to)) else {
                    continue;
                };
                match graph.add_edge(from_id.0, to_id.0, edges.len()) {
                    Ok(id) => edges.push((id, *from, *to, true)),
                    Err(error) => {
                        assert_eq!(error, InsertError::Full(edges.len()));
                        assert_eq!(live_edges.len(), EDGES);
                    }
                }
            }
            6 => {
                let Some(key) = rng.pick(&live_edges).copied() else {
                    continue;
                };
                if let Some(edge) = edges.get_mut(key) {
                    assert_eq!(graph.remove_edge(edge.0), Some(key));
                    edge.3 = false;
                }
            }
            _ => {
                let Some(key) = rng.pick(&live_nodes).copied() else {
                    continue;
                };
                if let Some(node) = nodes.get_mut(key) {
                    assert_eq!(graph.remove_node(node.0), Some(key));
                    node.1 = false;
                }
                for edge in edges.iter_mut() {
                    if edge.1 == key || edge.2 == key {
                        edge.3 = false;
                    }
                }
            }
        }

        assert_eq!(
            graph.node_count(),
            nodes.iter().filter(|node| node.1).count()
        );
        assert_eq!(
            graph.edge_count(),
            edges.iter().filter(|edge| edge.3).count()
        );
        for (key, (id, alive)) in nodes.iter().enumerate() {
            assert_eq!(graph.contains_node(*id), *alive);
            // Newest first, so compare against the model in reverse.
            let out: Vec<usize> = graph
                .outgoing(*id)
                .filter_map(|(edge, _)| graph.edge(edge).copied())
                .collect();
            let expected: Vec<usize> = (0..edges.len())
                .rev()
                .filter(|e| edges.get(*e).is_some_and(|edge| edge.3 && edge.1 == key))
                .collect();
            assert_eq!(out, expected);
            let into: Vec<usize> = graph
                .incoming(*id)
                .filter_map(|(edge, _)| graph.edge(edge).copied())
                .collect();
            let expected: Vec<usize> = (0..edges.len())
                .rev()
                .filter(|e| edges.get(*e).is_some_and(|edge| edge.3 && edge.2 == key))
                .collect();
            assert_eq!(into, expected);
        }
        for (id, from, to, alive) in &edges {
            let endpoints = graph.endpoints(*id);
            assert_eq!(endpoints.is_some(), *alive);
            if let (Some((a, b)), Some(from), Some(to)) =
                (endpoints, nodes.get(*from), nodes.get(*to))
            {
                assert_eq!((a, b), (from.0, to.0));
            }
        }
    }
}
