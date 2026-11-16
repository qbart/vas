//! Graphs and path search. Run with `cargo run --example pathfinding`.

use std::error::Error;

use vas::graph::{EdgeSlot, NodeSlot};
use vas::path::{Node, Open, Space};
use vas::static_graph::Edge;
use vas::{Graph, Memory, PathFinder, StaticGraph, Vector};

/// A tile grid: node index = y * width + x.
struct Level {
    width: u32,
    height: u32,
}

impl Space for Level {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        let (x, y) = (node % self.width, node / self.width);
        if x > 0 {
            visit(node - 1, 1.0);
        }
        if x + 1 < self.width {
            visit(node + 1, 1.0);
        }
        if y > 0 {
            visit(node - self.width, 1.0);
        }
        if y + 1 < self.height {
            visit(node + self.width, 1.0);
        }
    }

    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        let dx = (from % self.width).abs_diff(goal % self.width);
        let dy = (from / self.width).abs_diff(goal / self.width);
        (dx + dy) as f32
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // A* on a grid: no graph structure needed, only the neighbours.
    let level = Level {
        width: 10,
        height: 10,
    };
    let mut nodes = Memory::<Node, 100>::new(); // one per tile
    let mut open = Memory::<Open, 256>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&level, 0, 99)?, 18.0);

    let mut memory = Memory::<u32, 64>::new();
    let mut path = Vector::new(&mut memory);
    finder.path_into(&mut path)?;
    assert_eq!(path.len(), 19);
    assert_eq!((path.first(), path.last()), (Some(&0), Some(&99)));

    // Graph: waypoints that can change at run time.
    let mut node_memory = Memory::<NodeSlot<&str>, 8>::new();
    let mut edge_memory = Memory::<EdgeSlot<f32>, 16>::new();
    let mut graph = Graph::new(&mut node_memory, &mut edge_memory);
    let town = graph.add_node("town")?;
    let bridge = graph.add_node("bridge")?;
    let castle = graph.add_node("castle")?;
    graph.add_edge(town, bridge, 4.0)?;
    graph.add_edge(bridge, castle, 3.0)?;
    assert_eq!(graph.outgoing(town).count(), 1);

    let mut search = Memory::<Node, 8>::new(); // one per node capacity
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut search, &mut open);
    assert_eq!(finder.find(&graph, town.index(), castle.index())?, 7.0);
    let route: Vec<&str> = finder
        .path_rev()
        .filter_map(|index| graph.node_at(index))
        .filter_map(|id| graph.node(id).copied())
        .collect();
    assert_eq!(route, ["castle", "bridge", "town"]);

    // Removing a node removes its edges, so the castle is cut off.
    graph.remove_node(bridge);
    assert!(finder.find(&graph, town.index(), castle.index()).is_err());

    // StaticGraph: built once from an edge list, then read-only and compact.
    let mut offsets = Memory::<u32, 4>::new(); // node count + 1
    let mut edges = Memory::<Edge<f32>, 3>::new();
    let input = [(0, 1, 1.5), (0, 2, 4.0), (1, 2, 1.0)]; // (from, to, cost)
    let fixed = StaticGraph::build(3, &input, &mut offsets, &mut edges)?;
    assert_eq!(fixed.neighbors(0).len(), 2);
    assert_eq!(finder.find(&fixed, 0, 2)?, 2.5);

    println!("pathfinding: ok");
    Ok(())
}
