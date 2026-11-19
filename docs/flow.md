# Flow field

`FlowField<SN, SO = SN>` holds, for every node of a [`Space`](path.md#the-space),
the cheapest way to the nearest of a set of goals: a flow field, or
Dijkstra map. It has [storage parameters](storage.md#storage-parameter); see
[storage](storage.md) and [errors](errors.md).

Use it when many units head for the same place: an army to a rally point,
every monster to the player, workers to the nearest stockpile. One build
serves them all; each unit reads the step for the node it stands on. For
one unit going somewhere of its own, [`PathFinder`](path.md) is cheaper.

## Memory

The same two blocks as a `PathFinder`:

| Block | Size |
|---|---|
| `Memory::<path::Node, N>` | one per node of the space (12 bytes each) |
| `Memory::<path::Open, M>` | the open list; the number of nodes is usually plenty |

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(nodes, open)` | O(nodes) |
| build | `build(&space, &goals, max_cost) -> Result<(), FlowError>` | replaces the field; `f32::INFINITY` for the whole space |
| cost | `cost(node) -> Option<f32>` | cheapest route to the nearest goal; 0 at a goal, `None` when not reached |
| step | `next(node) -> Option<u32>` | next node towards the nearest goal; `None` at a goal or when not reached |
| route | `route(node)` | iterator: `node` first, the goal last |
| memory | `node_capacity()`, `reserve(nodes)` | |

```rust
let walk = map.space(Moves::Eight, |_, tile| tile.cost());
field.build(&walk, &[map.index(player.x, player.y)?], 40.0)?;
for monster in &mut monsters {
    if let Some(step) = field.next(map.index(monster.x, monster.y)?) {
        (monster.x, monster.y) = map.position(step)?;
    }
}
```

## How it works

Dijkstra's algorithm from all goals at once, run backwards: from a node it
follows the steps *into* that node, [`Space::predecessors`](path.md#the-space),
costed from the node they leave. Each node remembers the node it was
reached from, which is its next step towards the goal.

- `Grid::space` lists predecessors exactly, so costs that differ by
  direction (each cell costs what entering it costs, diagonals √2) give
  the same routes and costs as A* from each node.
- Other spaces list their neighbours by default, which is right when every
  step can be taken back at the same cost. For a `Graph` or `StaticGraph`,
  give each edge a twin going back, or implement `predecessors`.
- `max_cost` stops the search: nodes further than that from every goal are
  not reached, and the build only costs what it covers.
- Building again replaces the field in O(1) plus the search: each build is
  stamped, as in `PathFinder`.

Cost: O((nodes + edges) log nodes) for the nodes within `max_cost`. On a
128x128 grid with 20% walls a whole-map build takes about 0.7 ms.

## Errors

`FlowError::Full` when the open list runs out, `FlowError::OutOfBounds`
when a goal or a node reached is not below the node capacity of fixed
storage. A failed build leaves no field: every node reads as not reached.

## Storage

| Type | Memory |
|---|---|
| `FlowField<Inline<NODES>, Inline<OPEN>>` | inside the field: no lifetime; can be cloned |
| `FlowField<Borrowed<'a>>` | provided by the caller |
| `FlowField<Dynamic<A>>` | node memory grows to the highest node reached; the open list never fills |

## Not included

- Fleeing maps (scale the field by a negative factor and rescan) and
  blending several fields: read `cost` from two fields and combine them in
  the game.
- Directions as vectors for steering: `next` gives the node; turn it into
  a direction with `Grid::position`.
