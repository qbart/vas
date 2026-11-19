//! Graphs and path search. Run with `cargo run --example pathfinding`.

use std::error::Error;

use vas::graph::{EdgeSlot, NodeSlot};
use vas::grid::Moves;
use vas::path::{Node, Open};
use vas::static_graph::Edge;
use vas::{FlowField, Graph, Grid, Memory, PathFinder, StaticGraph, Vector};

fn main() -> Result<(), Box<dyn Error>> {
    // A* on a tile grid: a cell is a node, its index is y * width + x.
    let mut tiles = Memory::<bool, 100>::new(); // true is a wall
    let mut level = Grid::new(&mut tiles, 10, 10, false)?;
    level.fill_rect(0, 5, 9, 1, true); // a wall with a gap on the right
    let walk = level.space(Moves::Four, |_, wall| (!wall).then_some(1.0));
    let mut nodes = Memory::<Node, 100>::new(); // one per tile
    let mut open = Memory::<Open, 256>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let start = level.index(0, 0).ok_or("outside")?;
    let goal = level.index(0, 9).ok_or("outside")?;
    // Over to the gap at x = 9 and back: 9 + 9 + 9.
    assert_eq!(finder.find(&walk, start, goal)?, 27.0);

    let mut memory = Memory::<u32, 64>::new();
    let mut path = Vector::new(&mut memory);
    finder.path_into(&mut path)?;
    assert_eq!(path.len(), 28);
    let through = path.get(14).and_then(|&node| level.position(node));
    assert_eq!(through, Some((9, 5))); // the gap

    // Flow field: one search, then every unit on the map knows its next step
    // towards the nearest exit.
    let mut field_nodes = Memory::<Node, 100>::new();
    let mut field_open = Memory::<Open, 256>::new();
    let mut field = FlowField::new(&mut field_nodes, &mut field_open);
    let exits = [
        level.index(0, 9).ok_or("outside")?,
        level.index(9, 9).ok_or("outside")?,
    ];
    field.build(&walk, &exits, f32::INFINITY)?;
    let unit = level.index(9, 0).ok_or("outside")?;
    assert_eq!(field.cost(unit), Some(9.0)); // straight down through the gap
    assert_eq!(
        field.next(unit).and_then(|node| level.position(node)),
        Some((9, 1))
    );

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
