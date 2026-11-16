# Binary tree

`BinaryTree<T, S>` is a fixed-capacity binary tree with a [storage parameter](storage.md#storage-parameter): one root, and every node has at most a left and a right child. See
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

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `BinaryTree<T, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `BinaryTree<T, Borrowed<'a>>` | provided by the caller, sized in `Slot<T>`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.

## Growing

With `BinaryTree<T, Dynamic<A>>`, inserting a node takes more memory from
the allocator `A` when the tree is full. Node ids stay valid.
`BinaryTree::<T, Dynamic<A>>::default()` exists when `A: Default`.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
