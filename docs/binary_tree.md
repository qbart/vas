# Binary tree

`BinaryTree<'a, T>` is a fixed-capacity binary tree over user-provided
memory: one root, and every node has at most a left and a right child. See
[storage](storage.md) and [errors](errors.md).

It is the plain structure: the caller decides where each node goes, and
nothing is kept sorted or balanced. Use it for space partitions, decision
trees and expression trees. For sorted keys use an ordered map.

## Layout

Index-based, built on [`Pool`](pool.md). Each node stores its value plus
three links (parent, left, right) as slot indices: 12 bytes of links per
node. `NodeId<T>` is a generational handle that goes stale when the node is
removed. Values never move.

## Storage

```rust
use vas::binary_tree::Slot;
use vas::{BinaryTree, Memory};

let mut memory = Memory::<Slot<Split>, 256>::new();
let mut tree = BinaryTree::new(&mut memory);
```

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `len`, `capacity`, `is_empty`, `is_full` | O(1) |
| root | `root() -> Option<NodeId<T>>`, `set_root(T)` | `set_root` only on an empty tree |
| add | `insert_left(parent, T)`, `insert_right(parent, T)` | `Result<NodeId<T>, InsertError<T>>` |
| remove | `remove(id) -> Option<T>` | removes the node **and its whole subtree**; returns the node's value |
| value | `get`, `get_mut`, `contains` | `None` / `false` for a stale id |
| links | `parent`, `left`, `right` | O(1) |
| depth | `depth(id) -> Option<usize>` | 0 for the root; O(depth) |
| walk | `pre_order(id)`, `in_order(id)`, `post_order(id)` | iterators over the subtree of `id`, including `id` |
| all nodes | `iter`, `iter_mut` | `(id, value)` in no particular order |
| clear | `clear()` | drops all values, all ids go stale |

`InsertError<T>` is `Full(T)`, `Stale(T)` (the parent was removed) or
`Occupied(T)` (the position is taken); the value always comes back.

The walks need no extra memory and no recursion: they follow the parent
links. A walk started from a stale id is empty.

## Guarantees

- No allocation, no panics, no `unsafe` of its own.
- Values still in the tree are dropped when it is dropped.
