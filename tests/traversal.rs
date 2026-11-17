use vas::path::Space;
use vas::traversal::{Mark, TraversalError};
use vas::{Arena, Dynamic, Graph, Inline, Memory, StaticGraph, Traversal};

/// A tile grid where `#` is a wall; steps go to the four neighbours.
struct Tiles {
    width: u32,
    cells: &'static [u8],
}

impl Space for Tiles {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        let (x, y) = (node % self.width, node / self.width);
        let height = self.cells.len() as u32 / self.width;
        let open = |n: u32| self.cells.get(n as usize).is_some_and(|c| *c != b'#');
        if x > 0 && open(node - 1) {
            visit(node - 1, 1.0);
        }
        if x + 1 < self.width && open(node + 1) {
            visit(node + 1, 1.0);
        }
        if y > 0 && open(node - self.width) {
            visit(node - self.width, 1.0);
        }
        if y + 1 < height && open(node + self.width) {
            visit(node + self.width, 1.0);
        }
    }
}

const ROOM: Tiles = Tiles {
    width: 5,
    cells: b"\
.....\
.###.\
.#.#.\
.###.\
.....",
};

fn graph(
    nodes: usize,
    edges: &[(u32, u32, f32)],
) -> Option<StaticGraph<f32, Inline<16>, Inline<32>>> {
    StaticGraph::build(nodes, edges, Memory::new(), Memory::new()).ok()
}

#[test]
fn breadth_first_order_and_steps() {
    // 0 -> 1 -> 3
    // 0 -> 2 -> 3 -> 4
    let Some(g) = graph(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 3, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
        ],
    ) else {
        return;
    };
    let mut t = Traversal::<Inline<8>, Inline<8>>::new(Memory::new(), Memory::new());
    assert_eq!(t.breadth_first(&g, 0, u32::MAX), Ok(&[0, 1, 2, 3, 4][..]));
    assert_eq!(t.steps(0), Some(0));
    assert_eq!(t.steps(3), Some(2));
    assert_eq!(t.steps(4), Some(3));
    assert_eq!(t.steps(5), None);
    assert!(t.reached(4));
    assert!(!t.reached(5));
    assert_eq!(t.breadth_first(&g, 3, u32::MAX), Ok(&[3, 4][..]));
    // The previous search is forgotten.
    assert_eq!(t.steps(0), None);
    assert_eq!(t.breadth_first(&g, 0, 1), Ok(&[0, 1, 2][..]));
    assert_eq!(t.breadth_first(&g, 0, 0), Ok(&[0][..]));
    assert_eq!(t.order(), &[0]);
}

#[test]
fn flood_fill_on_a_grid() {
    let mut marks = Memory::<Mark, 25>::new();
    let mut list = Memory::<u32, 25>::new();
    let mut t = Traversal::new(&mut marks, &mut list);
    // The enclosed centre tile (12) is not reachable from the corner.
    let reached = t.breadth_first(&ROOM, 0, u32::MAX).map(<[u32]>::len);
    assert_eq!(reached, Ok(16));
    assert!(!t.reached(12));
    assert_eq!(t.steps(24), Some(8));
    // Tiles within three moves of the corner.
    let near = t.breadth_first(&ROOM, 0, 3).map(<[u32]>::to_vec);
    assert_eq!(
        near.map(|mut v| {
            v.sort_unstable();
            v
        }),
        Ok(vec![0, 1, 2, 3, 5, 10, 15])
    );
    assert_eq!(t.breadth_first(&ROOM, 12, u32::MAX), Ok(&[12][..]));
}

#[test]
fn depth_first_preorder_like_recursion() {
    //      0
    //    / | \
    //   1  2  5
    //  / \    |
    // 3   4   2
    let Some(g) = graph(
        7,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (0, 5, 1.0),
            (1, 3, 1.0),
            (1, 4, 1.0),
            (5, 2, 1.0),
            (4, 0, 1.0),
        ],
    ) else {
        return;
    };
    let mut t = Traversal::<Inline<8>, Inline<8>>::new(Memory::new(), Memory::new());
    assert_eq!(t.depth_first(&g, 0), Ok(&[0, 1, 3, 4, 2, 5][..]));
    assert_eq!(t.depth_first(&g, 5), Ok(&[5, 2][..]));
    assert!(!t.reached(0));
    assert_eq!(t.depth_first(&g, 6), Ok(&[6][..]));
}

#[test]
fn depth_first_matches_a_recursive_search() {
    fn recurse(g: &impl Space, node: u32, seen: &mut [bool], out: &mut Vec<u32>) {
        if seen[node as usize] {
            return;
        }
        seen[node as usize] = true;
        out.push(node);
        let mut next = Vec::new();
        g.neighbors(node, |n, _| next.push(n));
        for n in next {
            recurse(g, n, seen, out);
        }
    }
    let mut rng = vas::Rng::new(5);
    let mut edges = Vec::new();
    for _ in 0..30 {
        edges.push((rng.u32_below(16), rng.u32_below(16), 1.0f32));
    }
    let Some(g) = graph(16, &edges) else {
        return;
    };
    let mut t = Traversal::<Inline<16>, Inline<16>>::new(Memory::new(), Memory::new());
    for start in 0..16 {
        let mut seen = [false; 16];
        let mut expected = Vec::new();
        recurse(&g, start, &mut seen, &mut expected);
        assert_eq!(t.depth_first(&g, start), Ok(&expected[..]), "from {start}");
    }
}

#[test]
fn topological_order() {
    // Build order of a small tech tree: an edge means "needed before".
    let Some(g) = graph(
        6,
        &[
            (5, 2, 1.0),
            (5, 0, 1.0),
            (4, 0, 1.0),
            (4, 1, 1.0),
            (2, 3, 1.0),
            (3, 1, 1.0),
        ],
    ) else {
        return;
    };
    let mut t = Traversal::<Inline<8>, Inline<8>>::new(Memory::new(), Memory::new());
    let order = t.topological_order(&g, 0..6).map(<[u32]>::to_vec);
    assert!(order.is_ok(), "{order:?}");
    let Ok(order) = order else {
        return;
    };
    assert_eq!(order.len(), 6);
    let position = |n: u32| order.iter().position(|x| *x == n);
    for (from, to) in [(5, 2), (5, 0), (4, 0), (4, 1), (2, 3), (3, 1)] {
        assert!(position(from) < position(to), "{from} before {to}");
    }
    // Only the listed nodes count; edges to others are ignored.
    assert_eq!(t.topological_order(&g, [2, 3]), Ok(&[2, 3][..]));

    let Some(cyclic) = graph(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 1, 1.0), (2, 3, 1.0)]) else {
        return;
    };
    assert_eq!(
        t.topological_order(&cyclic, 0..4),
        Err(TraversalError::Cycle)
    );
    // Nodes listed twice are not a cycle.
    assert_eq!(t.topological_order(&g, [4, 4, 1]), Ok(&[4, 1][..]));
}

#[test]
fn errors() {
    let Some(g) = graph(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]) else {
        return;
    };
    let mut t = Traversal::<Inline<3>, Inline<8>>::new(Memory::new(), Memory::new());
    assert_eq!(
        t.breadth_first(&g, 9, u32::MAX),
        Err(TraversalError::OutOfBounds)
    );
    // Node 3 is beyond the marks.
    assert_eq!(
        t.breadth_first(&g, 0, u32::MAX),
        Err(TraversalError::OutOfBounds)
    );
    assert_eq!(t.depth_first(&g, 0), Err(TraversalError::OutOfBounds));
    assert_eq!(
        t.topological_order(&g, 0..4),
        Err(TraversalError::OutOfBounds)
    );

    let mut t = Traversal::<Inline<8>, Inline<2>>::new(Memory::new(), Memory::new());
    assert_eq!(t.breadth_first(&g, 0, u32::MAX), Err(TraversalError::Full));
    assert_eq!(t.depth_first(&g, 0), Err(TraversalError::Full));
    assert_eq!(t.topological_order(&g, 0..4), Err(TraversalError::Full));
    assert_eq!(t.breadth_first(&g, 0, 1), Ok(&[0, 1][..]));
}

#[test]
fn works_on_an_editable_graph() {
    let mut g = Graph::<&str, f32, Inline<8>, Inline<16>>::inline();
    let (Ok(a), Ok(b), Ok(c), Ok(d)) = (
        g.add_node("a"),
        g.add_node("b"),
        g.add_node("c"),
        g.add_node("d"),
    ) else {
        return;
    };
    for (from, to) in [(a, b), (b, c), (a, d), (d, c)] {
        assert!(g.add_edge(from, to, 1.0).is_ok());
    }
    assert!(g.remove_node(b).is_some());
    let mut t = Traversal::<Inline<8>, Inline<8>>::new(Memory::new(), Memory::new());
    let nodes = g.nodes().map(|(id, _)| id.index());
    let order = t.topological_order(&g, nodes).map(<[u32]>::to_vec);
    assert_eq!(order, Ok(vec![a.index(), d.index(), c.index()]));
    let reached = t
        .breadth_first(&g, a.index(), u32::MAX)
        .map(<[u32]>::to_vec);
    assert_eq!(reached, Ok(vec![a.index(), d.index(), c.index()]));
    assert_eq!(
        g.node_at(reached.ok().and_then(|r| r.last().copied()).unwrap_or(0)),
        Some(c)
    );
}

#[test]
fn grows_with_dynamic_storage() {
    let mut block = Memory::<u8, { 256 * 1024 }>::new();
    let arena = Arena::new(&mut block);
    let mut t = Traversal::new(Dynamic(&arena), Dynamic(&arena));
    assert_eq!(t.node_capacity(), 0);
    // Smaller under Miri, which is slow.
    let side: u32 = if cfg!(miri) { 12 } else { 50 };
    let count = (side * side) as usize;
    let long = Tiles {
        width: side,
        cells: &[b'.'; 2500][..count],
    };
    let last = side * side - 1;
    assert_eq!(
        t.breadth_first(&long, 0, u32::MAX).map(<[u32]>::len),
        Ok(count)
    );
    assert_eq!(t.steps(last), Some(2 * (side - 1)));
    assert!(t.node_capacity() >= count);
    assert_eq!(t.depth_first(&long, last).map(<[u32]>::len), Ok(count));
    assert_eq!(t.reserve(1 << 30), Err(vas::Full(())));
}

#[test]
fn stamps_survive_many_searches() {
    let Some(g) = graph(3, &[(0, 1, 1.0), (1, 2, 1.0)]) else {
        return;
    };
    let mut t = Traversal::<Inline<3>, Inline<3>>::new(Memory::new(), Memory::new());
    let searches = if cfg!(miri) { 300 } else { 10_000u32 };
    for i in 0..searches {
        let start = i % 3;
        assert_eq!(
            t.breadth_first(&g, start, u32::MAX).map(<[u32]>::len),
            Ok(3 - start as usize)
        );
        assert_eq!(
            t.depth_first(&g, start).map(<[u32]>::len),
            Ok(3 - start as usize)
        );
    }
}
