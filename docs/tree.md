# Tree

`Tree<T, S>` is a fixed-capacity hierarchy with a [storage parameter](storage.md#storage-parameter): every
node has an optional parent and an ordered list of children. See
[storage](storage.md) and [errors](errors.md).

Use it for scene graphs, UI trees and transform hierarchies.

## Layout

Index-based, built on [`Pool`](pool.md). Each node stores its value plus five
links (parent, first child, last child, previous sibling, next sibling) as
slot indices: 20 bytes of links per node, no pointers, no allocation.

- Nodes without a parent are roots. A tree may have several roots; they form
  an ordered list like any other siblings.
- `NodeId<T>` is a generational handle: `Copy`, 8 bytes, and stale once the
  node is removed.
- Values never move.

## Storage

```rust
use vas::tree::Slot;
use vas::{Memory, Tree};

let mut memory = Memory::<Slot<Transform>, 1024>::new();
let mut tree = Tree::new(&mut memory);
```

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `len`, `capacity`, `is_empty`, `is_full` | O(1) |
| add root | `insert_root(T) -> Result<NodeId<T>, Full<T>>` | appended after existing roots |
| add child | `insert_child(parent, T) -> Result<NodeId<T>, InsertError<T>>` | appended as last child; `Full(T)` or `Stale(T)` |
| remove | `remove(id) -> Option<T>` | removes the node **and its whole subtree**; returns the node's value |
| move | `set_parent(id, Option<NodeId<T>>) -> Result<(), MoveError>` | moves the subtree; `None` makes it a root; `Stale` or `Cycle` |
| value | `get`, `get_mut`, `contains` | `None` / `false` for a stale id |
| links | `parent`, `first_child`, `last_child`, `next_sibling`, `prev_sibling` | O(1) |
| depth | `depth(id) -> Option<usize>` | 0 for a root; O(depth) |
| walk | `roots()`, `children(id)`, `ancestors(id)`, `descendants(id)` | iterators of ids; `descendants` is depth first, parent before children, excluding `id` |
| all nodes | `iter`, `iter_mut` | `(id, value)` in no particular order |
| clear | `clear()` | drops all values, all ids go stale |

All walks need no extra memory: they follow the links. Iterators given a
stale id are empty.

## Guarantees

- No allocation, no panics, no `unsafe` of its own.
- A node can never become its own ancestor: `set_parent` rejects it with
  `MoveError::Cycle`.
- Values still in the tree are dropped when it is dropped.

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Tree<T, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Tree<T, Borrowed<'a>>` | provided by the caller, sized in `Slot<T>`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.
